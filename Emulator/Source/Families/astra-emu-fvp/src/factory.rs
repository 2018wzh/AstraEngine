use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Cursor, Read},
    path::{Component, Path},
    sync::{Arc, Mutex},
};

use astra_byte_source::{
    audit_source, AccessedResourceLedger, BoundedByteSource, ByteRange, FileByteSource,
    DEFAULT_MAX_RANGE_BYTES,
};
use astra_core::Hash256;
use astra_emu_family_core::{
    validate_legacy_vfs_uri, LegacyCoreError, LegacyMountedVfs, LegacyPackManifest, LegacyVfsEntry,
    LegacyVfsFamilyFactory, LegacyVfsMountContext, LegacyVfsNode, LegacyVfsNodeKind,
    LegacyVfsReadResult, LegacyVfsSource, LegacyVfsStat, LegacyVfsStream,
    LEGACY_PACK_MANIFEST_SCHEMA, LEGACY_VFS_MAX_READ_BYTES,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{archive::classify, vfs_audit::audit_archive, FvpArchive, FvpNls};

pub const FVP_FAMILY_OPTIONS_SCHEMA: &str = "astra.emu.fvp_vfs_options.v1";
pub const FVP_READER_ID: &str = "astra.emu.fvp.bin.v2";
pub const FVP_DECRYPT_PROVIDER_ID: &str = "astra.emu.fvp.raw.v1";
const MAX_ARCHIVES: usize = 4096;
const MAX_ENTRIES: usize = 10_000_000;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FvpVfsFamilyOptions {
    pub nls: FvpNls,
    pub archives: Vec<String>,
    pub max_entries_per_archive: usize,
}

#[derive(Debug, Default)]
pub struct FvpVfsFamilyFactory;

impl LegacyVfsFamilyFactory for FvpVfsFamilyFactory {
    fn family_id(&self) -> &str {
        "fvp"
    }

    fn mount_profile_schema_id(&self) -> &str {
        FVP_FAMILY_OPTIONS_SCHEMA
    }

    fn mount_profile_schema_hash(&self) -> Hash256 {
        Hash256::from_sha256(FVP_FAMILY_OPTIONS_SCHEMA.as_bytes())
    }

    fn decrypt_provider_id(&self) -> &str {
        FVP_DECRYPT_PROVIDER_ID
    }

    fn mount(
        &self,
        context: &LegacyVfsMountContext,
    ) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError> {
        if context.family_config.schema_id != FVP_FAMILY_OPTIONS_SCHEMA
            || context.family_config.schema_hash != self.mount_profile_schema_hash()
            || context.prefix != "fvp:/"
            || context.private_patch.is_some()
        {
            return Err(invalid(
                "ASTRA_EMU_FVP_MOUNT_PROFILE",
                "FVP mount identity, options schema, or no-patch policy is invalid",
            ));
        }
        let options: FvpVfsFamilyOptions = serde_json::from_slice(&context.family_config.payload)
            .map_err(|_| {
            invalid(
                "ASTRA_EMU_FVP_MOUNT_OPTIONS",
                "FVP family options are invalid",
            )
        })?;
        validate_options(&options)?;
        Ok(Arc::new(FvpMountedVfs::mount(context, &options)?))
    }
}

struct MountedArchive {
    role: String,
    source_hash: Hash256,
    archive: Arc<FvpArchive>,
}

#[derive(Debug, Clone)]
struct MountedEntry {
    archive_index: usize,
    entry_index: usize,
    uri: String,
    entry_id: String,
    role: String,
    size: u64,
    content_hash: Hash256,
    media_kind: String,
}

pub struct FvpMountedVfs {
    mount_id: String,
    prefix: String,
    manifest: LegacyPackManifest,
    archives: Vec<MountedArchive>,
    entries: BTreeMap<String, MountedEntry>,
    ledger: Arc<Mutex<AccessedResourceLedger>>,
}

impl FvpMountedVfs {
    fn mount(
        context: &LegacyVfsMountContext,
        options: &FvpVfsFamilyOptions,
    ) -> Result<Self, LegacyCoreError> {
        let mut archives = Vec::with_capacity(options.archives.len());
        let mut entries = BTreeMap::new();
        let mut entry_ids = BTreeSet::new();

        for archive_name in &options.archives {
            let role = archive_role(archive_name)?;
            let path = resolve_archive(&context.game_root, archive_name)?;
            let source: Arc<dyn BoundedByteSource> =
                Arc::new(FileByteSource::open(path).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_FVP_ARCHIVE_OPEN",
                        "FVP archive could not be opened",
                    )
                })?);
            let archive = Arc::new(
                FvpArchive::open_source(
                    Arc::clone(&source),
                    options.nls,
                    options.max_entries_per_archive,
                )
                .map_err(|error| {
                    invalid(
                        "ASTRA_EMU_FVP_ARCHIVE_PARSE",
                        format!("FVP archive metadata is invalid: {}", error.code()),
                    )
                })?,
            );
            let (source_hash, entry_audits) = audit_archive(archive.as_ref())?;
            let archive_index = archives.len();
            for (entry_index, descriptor) in archive.entries().iter().enumerate() {
                let uri = format!("{}{role}/{}", context.prefix, descriptor.name);
                validate_legacy_vfs_uri(&context.prefix, &uri)?;
                let entry_id = format!("{role}:{}", descriptor.name);
                if !entry_ids.insert(entry_id.clone()) || entries.contains_key(&uri) {
                    return Err(invalid(
                        "ASTRA_EMU_FVP_ENTRY_DUPLICATE",
                        "FVP archive set contains a duplicate URI or entry id",
                    ));
                }
                let (content_hash, magic) = &entry_audits[entry_index];
                let media_kind = classify(magic).to_owned();
                entries.insert(
                    uri.clone(),
                    MountedEntry {
                        archive_index,
                        entry_index,
                        uri,
                        entry_id,
                        role: role.clone(),
                        size: descriptor.size,
                        content_hash: *content_hash,
                        media_kind,
                    },
                );
            }
            tracing::info!(
                event = "astra_emu_fvp_archive_mounted",
                archive_role = %role,
                entry_count = archive.entries().len(),
                archive_hash = %source_hash
            );
            archives.push(MountedArchive {
                role,
                source_hash,
                archive,
            });
        }

        let mut reader_material = Vec::with_capacity(archives.len() * 32);
        for archive in &archives {
            reader_material.extend_from_slice(archive.source_hash.as_bytes());
        }
        let reader_hash = Hash256::from_sha256(&reader_material);
        let manifest = LegacyPackManifest {
            schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
            family_id: "fvp".into(),
            mount_id: context.mount_id.clone(),
            prefix: context.prefix.clone(),
            reader_id: FVP_READER_ID.into(),
            reader_hash,
            decrypt_provider_id: FVP_DECRYPT_PROVIDER_ID.into(),
            private_profile_hash: Hash256::from_sha256(b"astra.emu.fvp.no_private_profile"),
            mount_profile_hash: context.profile_hash,
            sources: archives
                .iter()
                .map(|archive| LegacyVfsSource {
                    source_id: archive.role.clone(),
                    archive_role: Some(archive.role.clone()),
                    byte_size: archive.archive.stat().len,
                    part_count: 1,
                    source_hash: archive.source_hash,
                })
                .collect(),
            entries: entries
                .values()
                .map(|entry| {
                    let descriptor = archives[entry.archive_index]
                        .archive
                        .entry(entry.entry_index)
                        .expect("mounted entry index was validated");
                    LegacyVfsEntry {
                        uri: entry.uri.clone(),
                        entry_id: entry.entry_id.clone(),
                        source_id: entry.role.clone(),
                        source_offset: descriptor.offset,
                        stored_size: entry.size,
                        decoded_size: entry.size,
                        source_hash: entry.content_hash,
                        content_hash: Some(entry.content_hash),
                        method: "raw".into(),
                        media_kind: entry.media_kind.clone(),
                    }
                })
                .collect(),
        };
        manifest.validate(MAX_ENTRIES)?;
        tracing::info!(
            event = "astra_emu_fvp_vfs_mounted",
            mount_id = %context.mount_id,
            archive_count = archives.len(),
            entry_count = entries.len(),
            reader_hash = %manifest.reader_hash
        );
        Ok(Self {
            mount_id: context.mount_id.clone(),
            prefix: context.prefix.clone(),
            manifest,
            archives,
            entries,
            ledger: Arc::new(Mutex::new(AccessedResourceLedger::default())),
        })
    }

    fn entry(&self, uri: &str) -> Result<&MountedEntry, LegacyCoreError> {
        validate_legacy_vfs_uri(&self.prefix, uri)?;
        self.entries
            .get(uri)
            .ok_or_else(|| invalid("ASTRA_EMU_VFS_NOT_FOUND", "FVP VFS entry was not found"))
    }

    fn read_entry(
        &self,
        entry: &MountedEntry,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, LegacyCoreError> {
        let descriptor = self.archives[entry.archive_index]
            .archive
            .entry(entry.entry_index)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_FVP_ENTRY_INDEX",
                    "mounted FVP entry index is invalid",
                )
            })?;
        let source = self.archives[entry.archive_index]
            .archive
            .source()
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_FVP_SOURCE_MISSING",
                    "mounted FVP archive source is unavailable",
                )
            })?;
        let source_offset = descriptor.offset.checked_add(offset).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_READ_OVERFLOW",
                "FVP entry source range overflowed",
            )
        })?;
        read_bounded(
            source.as_ref(),
            self.archives[entry.archive_index].archive.stat(),
            &entry.uri,
            source_offset,
            length,
            &self.ledger,
        )
    }
}

impl LegacyMountedVfs for FvpMountedVfs {
    fn mount_id(&self) -> &str {
        &self.mount_id
    }

    fn manifest(&self) -> &LegacyPackManifest {
        &self.manifest
    }

    fn validate_sources(&self) -> Result<(), LegacyCoreError> {
        for archive in &self.archives {
            let source = archive.archive.source().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_FVP_SOURCE_MISSING",
                    "mounted FVP archive source is unavailable",
                )
            })?;
            if source.stat().map_err(byte_source_error)? != archive.archive.stat()
                || audit_source(source.as_ref()).map_err(byte_source_error)? != archive.source_hash
            {
                return Err(invalid(
                    "ASTRA_EMU_VFS_SOURCE_CHANGED",
                    "FVP archive changed after mount",
                ));
            }
        }
        Ok(())
    }

    fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyCoreError> {
        if uri != self.prefix {
            validate_legacy_vfs_uri(&self.prefix, uri)?;
        }
        let base = if uri.ends_with('/') {
            uri.to_owned()
        } else {
            format!("{uri}/")
        };
        let mut children = BTreeMap::new();
        for entry_uri in self
            .entries
            .keys()
            .filter(|candidate| candidate.starts_with(&base))
        {
            let suffix = &entry_uri[base.len()..];
            let Some(name) = suffix.split('/').next() else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let directory = suffix.contains('/');
            children
                .entry(name.to_owned())
                .or_insert_with(|| LegacyVfsNode {
                    uri: format!("{base}{name}"),
                    name: name.to_owned(),
                    kind: if directory {
                        LegacyVfsNodeKind::Directory
                    } else {
                        LegacyVfsNodeKind::File
                    },
                });
        }
        if children.is_empty() && uri != self.prefix {
            return Err(invalid(
                "ASTRA_EMU_VFS_NOT_FOUND",
                "FVP VFS directory was not found",
            ));
        }
        Ok(children.into_values().collect())
    }

    fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyCoreError> {
        if uri == self.prefix
            || self
                .entries
                .keys()
                .any(|candidate| candidate.starts_with(&format!("{}/", uri.trim_end_matches('/'))))
        {
            return Ok(LegacyVfsStat {
                uri: uri.into(),
                entry_id: None,
                kind: LegacyVfsNodeKind::Directory,
                size: 0,
                content_hash: None,
                archive_role: None,
                method: None,
            });
        }
        let entry = self.entry(uri)?;
        Ok(LegacyVfsStat {
            uri: uri.into(),
            entry_id: Some(entry.entry_id.clone()),
            kind: LegacyVfsNodeKind::File,
            size: entry.size,
            content_hash: Some(entry.content_hash),
            archive_role: Some(entry.role.clone()),
            method: Some("raw".into()),
        })
    }

    fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<LegacyVfsReadResult, LegacyCoreError> {
        if length > LEGACY_VFS_MAX_READ_BYTES {
            return Err(invalid(
                "ASTRA_EMU_VFS_READ_LIMIT",
                "FVP range read exceeds the configured limit",
            ));
        }
        let entry = self.entry(uri)?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| invalid("ASTRA_EMU_VFS_READ_OVERFLOW", "FVP range read overflowed"))?;
        if end > entry.size {
            return Err(invalid(
                "ASTRA_EMU_VFS_READ_BOUNDS",
                "FVP range read is outside the entry",
            ));
        }
        let bytes = self.read_entry(entry, offset, length)?;
        Ok(LegacyVfsReadResult {
            uri: uri.into(),
            offset,
            bytes,
            eof: end == entry.size,
            cache_hit: false,
        })
    }

    fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, LegacyCoreError> {
        let entry = self.entry(uri)?;
        if entry.size <= DEFAULT_MAX_RANGE_BYTES {
            return Ok(Box::new(Cursor::new(
                self.read_entry(entry, 0, entry.size)?,
            )));
        }
        let archive = &self.archives[entry.archive_index];
        let descriptor = archive.archive.entry(entry.entry_index).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_FVP_ENTRY_INDEX",
                "mounted FVP entry index is invalid",
            )
        })?;
        Ok(Box::new(FvpEntryStream {
            source: Arc::clone(archive.archive.source().ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_FVP_SOURCE_MISSING",
                    "mounted FVP archive source is unavailable",
                )
            })?),
            stat: archive.archive.stat(),
            resource_id: entry.uri.clone(),
            source_offset: descriptor.offset,
            length: entry.size,
            position: 0,
            ledger: Arc::clone(&self.ledger),
        }))
    }
}

struct FvpEntryStream {
    source: Arc<dyn BoundedByteSource>,
    stat: astra_byte_source::ByteSourceStat,
    resource_id: String,
    source_offset: u64,
    length: u64,
    position: u64,
    ledger: Arc<Mutex<AccessedResourceLedger>>,
}

impl Read for FvpEntryStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.position == self.length {
            return Ok(0);
        }
        let length = (self.length - self.position)
            .min(buffer.len() as u64)
            .min(DEFAULT_MAX_RANGE_BYTES);
        let offset = self
            .source_offset
            .checked_add(self.position)
            .ok_or_else(|| io::Error::other("ASTRA_EMU_VFS_READ_OVERFLOW"))?;
        let result = self
            .source
            .read_range(
                self.stat.revision,
                ByteRange {
                    offset,
                    len: length,
                },
                DEFAULT_MAX_RANGE_BYTES,
            )
            .map_err(|_| io::Error::other("ASTRA_EMU_VFS_SOURCE_READ"))?;
        self.ledger
            .lock()
            .map_err(|_| io::Error::other("ASTRA_EMU_VFS_LEDGER_POISONED"))?
            .record(&self.resource_id, &result)
            .map_err(|_| io::Error::other("ASTRA_EMU_VFS_REPEAT_READ_CHANGED"))?;
        buffer[..result.bytes.len()].copy_from_slice(&result.bytes);
        self.position += result.bytes.len() as u64;
        Ok(result.bytes.len())
    }
}

fn read_bounded(
    source: &dyn BoundedByteSource,
    stat: astra_byte_source::ByteSourceStat,
    resource_id: &str,
    offset: u64,
    length: u64,
    ledger: &Mutex<AccessedResourceLedger>,
) -> Result<Vec<u8>, LegacyCoreError> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("ASTRA_EMU_VFS_READ_OVERFLOW", "FVP source range overflowed"))?;
    if end > stat.len {
        return Err(invalid(
            "ASTRA_EMU_VFS_READ_BOUNDS",
            "FVP source range is outside the archive",
        ));
    }
    let capacity = usize::try_from(length).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_READ_LIMIT",
            "FVP source range does not fit memory",
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut cursor = offset;
    while cursor < end {
        let chunk_length = (end - cursor).min(DEFAULT_MAX_RANGE_BYTES);
        let result = source
            .read_range(
                stat.revision,
                ByteRange {
                    offset: cursor,
                    len: chunk_length,
                },
                DEFAULT_MAX_RANGE_BYTES,
            )
            .map_err(byte_source_error)?;
        ledger
            .lock()
            .map_err(|_| {
                invalid(
                    "ASTRA_EMU_VFS_LEDGER_POISONED",
                    "FVP resource ledger is poisoned",
                )
            })?
            .record(resource_id, &result)
            .map_err(byte_source_error)?;
        bytes.extend_from_slice(&result.bytes);
        cursor = cursor.checked_add(chunk_length).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_READ_OVERFLOW",
                "FVP source cursor overflowed",
            )
        })?;
    }
    Ok(bytes)
}

fn validate_options(options: &FvpVfsFamilyOptions) -> Result<(), LegacyCoreError> {
    if options.archives.is_empty()
        || options.archives.len() > MAX_ARCHIVES
        || options.max_entries_per_archive == 0
        || options.max_entries_per_archive > MAX_ENTRIES
    {
        return Err(invalid(
            "ASTRA_EMU_FVP_MOUNT_OPTIONS",
            "FVP archive count or entry budget is outside the supported bounds",
        ));
    }
    let mut roles = BTreeSet::new();
    for archive in &options.archives {
        let role = archive_role(archive)?;
        if !roles.insert(role) {
            return Err(invalid(
                "ASTRA_EMU_FVP_ARCHIVE_DUPLICATE",
                "FVP archive role is duplicated",
            ));
        }
    }
    Ok(())
}

fn archive_role(archive: &str) -> Result<String, LegacyCoreError> {
    let path = Path::new(archive);
    if archive.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
        || !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("bin"))
    {
        return Err(invalid(
            "ASTRA_EMU_FVP_ARCHIVE_PATH",
            "FVP archive must be a normalized root-relative .bin file",
        ));
    }
    let role = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if role.is_empty()
        || role.len() > 128
        || !role
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid(
            "ASTRA_EMU_FVP_ARCHIVE_ROLE",
            "FVP archive role is invalid",
        ));
    }
    Ok(role)
}

fn resolve_archive(root: &Path, archive: &str) -> Result<std::path::PathBuf, LegacyCoreError> {
    let candidate = root.join(archive);
    let resolved = candidate.canonicalize().map_err(|_| {
        invalid(
            "ASTRA_EMU_FVP_ARCHIVE_MISSING",
            "FVP archive does not exist",
        )
    })?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(invalid(
            "ASTRA_EMU_FVP_ARCHIVE_PATH",
            "FVP archive resolves outside the game root or is not a file",
        ));
    }
    Ok(resolved)
}

fn byte_source_error(error: astra_byte_source::ByteSourceError) -> LegacyCoreError {
    let code = match error {
        astra_byte_source::ByteSourceError::RangeLimit => "ASTRA_EMU_VFS_READ_LIMIT",
        astra_byte_source::ByteSourceError::RangeOverflow => "ASTRA_EMU_VFS_READ_OVERFLOW",
        astra_byte_source::ByteSourceError::RangeBounds => "ASTRA_EMU_VFS_READ_BOUNDS",
        astra_byte_source::ByteSourceError::RevisionMismatch => "ASTRA_EMU_VFS_SOURCE_CHANGED",
        astra_byte_source::ByteSourceError::ShortRead => "ASTRA_EMU_VFS_SHORT_READ",
        astra_byte_source::ByteSourceError::Poisoned => "ASTRA_EMU_VFS_SOURCE_POISONED",
        astra_byte_source::ByteSourceError::Io(_) => "ASTRA_EMU_VFS_SOURCE_IO",
    };
    invalid(code, "FVP bounded archive source operation failed")
}

fn invalid(code: &'static str, message: impl Into<String>) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
#[path = "factory_tests.rs"]
mod tests;
