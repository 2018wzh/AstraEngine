use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use astra_core::Hash256;
use astra_emu_family_core::{
    validate_decrypt_output, validate_decrypt_request, validate_legacy_vfs_uri, LegacyCoreError,
    LegacyDecryptPhase, LegacyDecryptProvider, LegacyDecryptRequest, LegacyDecryptTransport,
    LegacyMountedVfs, LegacyOpaqueDescriptor, LegacyPackManifest, LegacyVfsEntry, LegacyVfsNode,
    LegacyVfsNodeKind, LegacyVfsReadResult, LegacyVfsSource, LegacyVfsStat, LegacyVfsStream,
    LEGACY_DECRYPT_CHUNK_BYTES, LEGACY_DECRYPT_MAX_BATCH_BYTES, LEGACY_PACK_MANIFEST_SCHEMA,
    LEGACY_VFS_MAX_READ_BYTES,
};
use blowfish::cipher::{BlockCipherDecrypt, KeyInit};
use blowfish::Blowfish;
use encoding_rs::SHIFT_JIS;
use flate2::read::ZlibDecoder;
use rc4::{Rc4, StreamCipher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use astra_emu_family_support::{CacheIdentity, PlaintextCache};

use crate::{MINORI_DECRYPT_DESCRIPTOR_SCHEMA, MINORI_DECRYPT_PROVIDER_ID, MINORI_READER_ID};

type PazError = LegacyCoreError;

pub const REQUIRED_ARCHIVE_ROLES: [&str; 6] = ["scr", "st", "sys", "se", "voice", "mov"];
pub const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PazArchiveConfig {
    pub role: String,
    pub path: PathBuf,
    pub game_root: PathBuf,
    pub version: u8,
    pub index_size_xor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PazEntryDescriptor {
    pub archive_role: String,
    pub entry_id: String,
    pub name: String,
    pub offset: u64,
    pub unpacked_size: u64,
    pub stored_size: u64,
    pub aligned_size: u64,
    pub packed: bool,
    pub video_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct PazRoleScheme {
    pub index_key: Vec<u8>,
    pub data_key: Vec<u8>,
    pub type_passwords: BTreeMap<String, String>,
    pub archive_xor: Option<u32>,
    pub video_key: Option<[u8; 256]>,
}

/// The only production decrypt provider for Minori PAZ archives.
/// Key material remains process-local and is never serializable.
pub struct MinoriPazDecryptProvider {
    private_profile_hash: Hash256,
    roles: BTreeMap<String, PazRoleScheme>,
}

impl MinoriPazDecryptProvider {
    pub fn new(
        private_profile_hash: Hash256,
        roles: BTreeMap<String, PazRoleScheme>,
    ) -> Result<Self, PazError> {
        if roles
            .keys()
            .any(|role| !REQUIRED_ARCHIVE_ROLES.contains(&role.as_str()))
        {
            return Err(error(
                "ASTRA_EMU_MINORI_DECODER_CONFIG",
                "decoder id or archive role is invalid",
            ));
        }
        for role in REQUIRED_ARCHIVE_ROLES {
            let scheme = roles.get(role).ok_or_else(|| {
                error(
                    "ASTRA_EMU_MINORI_DECODER_ROLE",
                    "decoder is missing a required archive role",
                )
            })?;
            validate_blowfish_key(&scheme.index_key)?;
            if role != "mov" {
                validate_blowfish_key(&scheme.data_key)?;
            } else if scheme.data_key.len() > 56 {
                return Err(error(
                    "ASTRA_EMU_MINORI_BLOWFISH_KEY",
                    "movie data key exceeds the supported bound",
                ));
            }
        }
        Ok(Self {
            private_profile_hash,
            roles,
        })
    }

    fn scheme(&self, role: &str) -> Result<&PazRoleScheme, PazError> {
        self.roles.get(role).ok_or_else(|| {
            error(
                "ASTRA_EMU_MINORI_DECODER_ROLE",
                "decoder has no scheme for the archive role",
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum MinoriDecryptDescriptor {
    Index {
        role: String,
        version: u8,
        stream_offset: u64,
    },
    Entry {
        version: u8,
        entry: PazEntryDescriptor,
        stream_offset: u64,
    },
}

impl LegacyDecryptProvider for MinoriPazDecryptProvider {
    fn provider_id(&self) -> &str {
        MINORI_DECRYPT_PROVIDER_ID
    }
    fn private_profile_hash(&self) -> Hash256 {
        self.private_profile_hash
    }
    fn descriptor_schema_id(&self) -> &str {
        MINORI_DECRYPT_DESCRIPTOR_SCHEMA
    }
    fn descriptor_schema_hash(&self) -> Hash256 {
        Hash256::from_sha256(MINORI_DECRYPT_DESCRIPTOR_SCHEMA.as_bytes())
    }

    fn decrypt(&self, request: LegacyDecryptRequest<'_>) -> Result<Vec<u8>, PazError> {
        validate_decrypt_request(self, &request)?;
        if request.descriptors.len() != 1 {
            return Err(error(
                "ASTRA_EMU_MINORI_DESCRIPTOR_BATCH",
                "Minori decrypt batches require exactly one descriptor",
            ));
        }
        let descriptor: MinoriDecryptDescriptor =
            serde_json::from_slice(&request.descriptors[0].payload).map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_DESCRIPTOR",
                    "Minori decrypt descriptor is invalid",
                )
            })?;
        if !matches!(
            (&request.phase, &descriptor),
            (
                LegacyDecryptPhase::Index,
                MinoriDecryptDescriptor::Index { .. }
            ) | (
                LegacyDecryptPhase::Entry,
                MinoriDecryptDescriptor::Entry { .. }
            )
        ) {
            return Err(error(
                "ASTRA_EMU_MINORI_DESCRIPTOR_PHASE",
                "Minori decrypt descriptor phase does not match the request",
            ));
        }
        let absolute_offset = match &descriptor {
            MinoriDecryptDescriptor::Index { stream_offset, .. }
            | MinoriDecryptDescriptor::Entry { stream_offset, .. } => stream_offset
                .checked_add(request.transport.chunk_offset)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MINORI_DECRYPT_OFFSET",
                        "decrypt stream offset overflowed",
                    )
                })?,
        };
        let output = match descriptor {
            MinoriDecryptDescriptor::Index { role, .. } => {
                blowfish_decrypt(self.scheme(&role)?.index_key.as_slice(), request.bytes)?
            }
            MinoriDecryptDescriptor::Entry { version, entry, .. } => {
                self.decrypt_entry_chunk(version, &entry, absolute_offset, request.bytes)?
            }
        };
        validate_decrypt_output(&request, &output)?;
        if output.len() != request.bytes.len() {
            return Err(error(
                "ASTRA_EMU_MINORI_DECRYPT_SIZE",
                "Minori transform changed the chunk size",
            ));
        }
        Ok(output)
    }
}

impl MinoriPazDecryptProvider {
    fn decrypt_entry_chunk(
        &self,
        version: u8,
        entry: &PazEntryDescriptor,
        absolute_offset: u64,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, PazError> {
        let scheme = self.scheme(&entry.archive_role)?;
        let mut bytes = encrypted.to_vec();
        if entry.archive_role == "mov" {
            let video_key = entry.video_key.as_ref().ok_or_else(|| {
                error(
                    "ASTRA_EMU_MINORI_VIDEO_KEY",
                    "video entry is missing its index key",
                )
            })?;
            if version == 0 {
                let mut table = [0u8; 256];
                for (index, value) in video_key.iter().enumerate() {
                    table[*value as usize] = index as u8;
                }
                for byte in &mut bytes {
                    *byte = table[*byte as usize];
                }
                return Ok(bytes);
            }
            let material = format!("{} {:08X} ", entry.name.to_lowercase(), entry.unpacked_size);
            let (entry_key, _, malformed) = SHIFT_JIS.encode(&material);
            if malformed || entry_key.is_empty() {
                return Err(error(
                    "ASTRA_EMU_MINORI_RC4_KEY",
                    "video RC4 key cannot be encoded as CP932",
                ));
            }
            let key = (0..256)
                .map(|index| video_key[index] ^ entry_key[index % entry_key.len()])
                .collect::<Vec<_>>();
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_RC4_KEY",
                    "video RC4 key length is invalid",
                )
            })?;
            let block_len = usize::try_from(entry.aligned_size.min(0x10000)).map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_VIDEO_SIZE",
                    "movie transform size exceeds the platform bound",
                )
            })?;
            if block_len == 0 {
                return Ok(bytes);
            }
            let mut block = vec![0; block_len];
            cipher.apply_keystream(&mut block);
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte ^= block[(absolute_offset as usize + index) % block.len()];
            }
            return Ok(bytes);
        }
        bytes = blowfish_decrypt(&scheme.data_key, &bytes)?;
        if version > 0 && password_for_entry(entry, scheme).is_some() {
            let password = password_for_entry(entry, scheme).unwrap_or_default();
            let material = format!(
                "{} {:08X} {}",
                entry.name.to_lowercase(),
                entry.unpacked_size,
                password
            );
            let (key, _, malformed) = SHIFT_JIS.encode(&material);
            if malformed || key.is_empty() {
                return Err(error(
                    "ASTRA_EMU_MINORI_RC4_KEY",
                    "entry RC4 key cannot be encoded as CP932",
                ));
            }
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_RC4_KEY",
                    "entry RC4 key length is invalid",
                )
            })?;
            let version_skip = if version >= 2 {
                ((crc32fast::hash(&key) >> 12) & 0xff) as u64
            } else {
                0
            };
            let skip = version_skip
                .checked_add(absolute_offset)
                .ok_or_else(|| error("ASTRA_EMU_MINORI_RC4_SKIP", "entry RC4 offset overflowed"))?;
            if skip > 0 {
                let mut remaining = skip;
                let mut discarded = vec![0; LEGACY_DECRYPT_CHUNK_BYTES];
                while remaining > 0 {
                    let length =
                        usize::try_from(remaining.min(discarded.len() as u64)).map_err(|_| {
                            error(
                                "ASTRA_EMU_MINORI_RC4_SKIP",
                                "entry RC4 offset exceeds the platform bound",
                            )
                        })?;
                    cipher.apply_keystream(&mut discarded[..length]);
                    discarded[..length].fill(0);
                    remaining -= length as u64;
                }
            }
            cipher.apply_keystream(&mut bytes);
        }
        Ok(bytes)
    }
}

#[derive(Clone)]
struct ArchiveSource {
    role: String,
    parts: Vec<ArchivePart>,
    version: u8,
    length: u64,
    hash: Hash256,
    xor_key: u8,
}

#[derive(Clone)]
struct ArchivePart {
    path: PathBuf,
    length: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone)]
struct MountedEntry {
    descriptor: PazEntryDescriptor,
    uri: String,
    archive: usize,
    encrypted_hash: Hash256,
}

pub struct MinoriMountedVfs {
    mount_id: String,
    prefix: String,
    manifest: LegacyPackManifest,
    archives: Vec<ArchiveSource>,
    entries: BTreeMap<String, MountedEntry>,
    decrypt_provider: Arc<MinoriPazDecryptProvider>,
    cache: Option<PlaintextCache>,
}

impl MinoriMountedVfs {
    pub fn mount(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decrypt_provider: Arc<MinoriPazDecryptProvider>,
        mount_profile_hash: Hash256,
    ) -> Result<Self, PazError> {
        Self::mount_with_cache(
            mount_id,
            prefix,
            configs,
            decrypt_provider,
            mount_profile_hash,
            None,
        )
    }

    pub fn mount_with_cache(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decrypt_provider: Arc<MinoriPazDecryptProvider>,
        mount_profile_hash: Hash256,
        cache: Option<PlaintextCache>,
    ) -> Result<Self, PazError> {
        let mount_id = mount_id.into();
        let prefix = prefix.into();
        if prefix != "minori:/" {
            return Err(error(
                "ASTRA_EMU_MINORI_PREFIX",
                "Minori mounts require the stable minori:/ prefix",
            ));
        }
        validate_role_set(&configs)?;
        let mut archives = Vec::with_capacity(configs.len());
        let mut entries = BTreeMap::new();
        let mut entry_ids = BTreeSet::new();
        let mut prepared = Vec::with_capacity(configs.len());
        for config in configs {
            tracing::info!(
                event = "astra_emu_minori_archive_mount_started",
                archive_role = %config.role,
                version = config.version
            );
            let parts = discover_parts(&config.path, &config.game_root)?;
            let total_length = parts
                .iter()
                .try_fold(0u64, |total, part| total.checked_add(part.length))
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MINORI_ARCHIVE_SIZE",
                        "multipart PAZ size overflowed",
                    )
                })?;
            if parts[0].length == 0 {
                return Err(error(
                    "ASTRA_EMU_MINORI_ARCHIVE_EMPTY",
                    format!("required archive role {} is empty", config.role),
                ));
            }
            if config.version > 2 {
                return Err(error(
                    "ASTRA_EMU_MINORI_VERSION",
                    "only PAZ versions 0 through 2 are supported",
                ));
            }
            let mut source = ArchiveSource {
                role: config.role.clone(),
                parts,
                version: config.version,
                length: total_length,
                hash: Hash256::from_sha256(&[]),
                xor_key: 0,
            };
            let parsed = parse_archive_index(
                &mut source,
                config.index_size_xor,
                decrypt_provider.as_ref(),
            )?;
            tracing::info!(
                event = "astra_emu_minori_archive_index_decoded",
                archive_role = %config.role,
                entry_count = parsed.len()
            );
            prepared.push((config, source, parsed));
        }
        for (config, mut source, parsed) in prepared {
            source.hash = hash_parts(&source.parts)?;
            tracing::info!(
                event = "astra_emu_minori_archive_hashed",
                archive_role = %config.role,
                archive_hash = %source.hash
            );
            let archive_index = archives.len();
            for entry in parsed {
                let uri = format!(
                    "{}{}/{}",
                    prefix,
                    config.role,
                    normalize_entry_name(&entry.name)?
                );
                validate_legacy_vfs_uri(&prefix, &uri)?;
                if !entry_ids.insert(entry.entry_id.clone()) || entries.contains_key(&uri) {
                    return Err(error(
                        "ASTRA_EMU_MINORI_ENTRY_DUPLICATE",
                        "PAZ set contains a duplicate URI or entry id",
                    ));
                }
                let encrypted = read_source_range(&source, entry.offset, entry.aligned_size)?;
                let encrypted_hash = Hash256::from_sha256(&encrypted);
                entries.insert(
                    uri.clone(),
                    MountedEntry {
                        descriptor: entry,
                        uri,
                        archive: archive_index,
                        encrypted_hash,
                    },
                );
            }
            archives.push(source);
            tracing::info!(
                event = "astra_emu_minori_archive_mount_completed",
                archive_role = %config.role
            );
        }
        let reader_material = archives
            .iter()
            .flat_map(|archive| archive.hash.as_bytes().iter().copied())
            .collect::<Vec<_>>();
        let reader_hash = Hash256::from_sha256(&reader_material);
        let manifest_entries = entries
            .values()
            .map(|entry| LegacyVfsEntry {
                uri: entry.uri.clone(),
                entry_id: entry.descriptor.entry_id.clone(),
                source_id: entry.descriptor.archive_role.clone(),
                source_offset: entry.descriptor.offset,
                stored_size: entry.descriptor.aligned_size,
                decoded_size: entry.descriptor.unpacked_size,
                source_hash: entry.encrypted_hash,
                content_hash: None,
                method: entry_method(&entry.descriptor).into(),
                media_kind: media_kind(&entry.descriptor.name).into(),
            })
            .collect();
        let manifest = LegacyPackManifest {
            schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
            family_id: "minori".into(),
            mount_id: mount_id.clone(),
            prefix: prefix.clone(),
            reader_id: MINORI_READER_ID.into(),
            reader_hash,
            decrypt_provider_id: decrypt_provider.provider_id().into(),
            private_profile_hash: decrypt_provider.private_profile_hash(),
            mount_profile_hash,
            sources: archives
                .iter()
                .map(|archive| LegacyVfsSource {
                    source_id: archive.role.clone(),
                    archive_role: Some(archive.role.clone()),
                    byte_size: archive.length,
                    part_count: archive.parts.len() as u32,
                    source_hash: archive.hash,
                })
                .collect(),
            entries: manifest_entries,
        };
        manifest.validate(10_000_000)?;
        Ok(Self {
            mount_id,
            prefix,
            manifest,
            archives,
            entries,
            decrypt_provider,
            cache,
        })
    }

    fn decoded_entry(&self, entry: &MountedEntry) -> Result<(Vec<u8>, bool), PazError> {
        let archive = &self.archives[entry.archive];
        verify_source_unchanged(archive)?;
        let identity = CacheIdentity {
            family_id: "minori".into(),
            source_hash: entry.encrypted_hash,
            entry_id: entry.descriptor.entry_id.clone(),
            private_profile_hash: self.decrypt_provider.private_profile_hash(),
            decrypt_provider_id: self.decrypt_provider.provider_id().into(),
            descriptor_schema_hash: self.decrypt_provider.descriptor_schema_hash(),
            codec_identity: if entry.descriptor.packed {
                "paz-zlib-v1"
            } else {
                "paz-raw-v1"
            }
            .into(),
        };
        if let Some(bytes) = self
            .cache
            .as_ref()
            .map(|cache| cache.get(&identity))
            .transpose()
            .map_err(|_| error("ASTRA_EMU_MINORI_CACHE_READ", "plaintext cache read failed"))?
            .flatten()
        {
            if bytes.len() as u64 != entry.descriptor.unpacked_size {
                return Err(error(
                    "ASTRA_EMU_MINORI_CACHE_SIZE",
                    "cached plaintext size does not match the entry descriptor",
                ));
            }
            return Ok((bytes, true));
        }
        let mut encrypted = read_source_range(
            archive,
            entry.descriptor.offset,
            entry.descriptor.aligned_size,
        )?;
        if Hash256::from_sha256(&encrypted) != entry.encrypted_hash {
            return Err(error(
                "ASTRA_EMU_MINORI_SOURCE_CHANGED",
                "archive bytes changed after mount",
            ));
        }
        xor_byte(&mut encrypted, archive.xor_key);
        let mut decoded = decrypt_bytes(
            self.decrypt_provider.as_ref(),
            MinoriDecryptDescriptor::Entry {
                version: archive.version,
                entry: entry.descriptor.clone(),
                stream_offset: 0,
            },
            &encrypted,
        )?;
        decoded.truncate(entry.descriptor.stored_size as usize);
        if entry.descriptor.packed {
            let mut unpacked = Vec::with_capacity(entry.descriptor.unpacked_size as usize);
            ZlibDecoder::new(decoded.as_slice())
                .read_to_end(&mut unpacked)
                .map_err(|_| error("ASTRA_EMU_MINORI_ZLIB", "entry zlib stream is invalid"))?;
            decoded = unpacked;
        }
        let expected_size = entry.descriptor.unpacked_size as usize;
        if decoded.len() > expected_size
            && decoded.len() - expected_size <= 16
            && decoded[expected_size..].iter().all(|byte| *byte == 0)
        {
            tracing::debug!(
                event = "astra_emu_minori_entry_zero_padding_removed",
                archive_role = %entry.descriptor.archive_role,
                entry_id = %entry.descriptor.entry_id,
                padding_size = decoded.len() - expected_size
            );
            decoded.truncate(expected_size);
        }
        if decoded.len() != expected_size {
            tracing::error!(
                event = "astra_emu_minori_entry_size_mismatch",
                archive_role = %entry.descriptor.archive_role,
                entry_id = %entry.descriptor.entry_id,
                packed = entry.descriptor.packed,
                stored_size = entry.descriptor.stored_size,
                unpacked_size = entry.descriptor.unpacked_size,
                decoded_size = decoded.len()
            );
            return Err(error(
                "ASTRA_EMU_MINORI_ENTRY_SIZE",
                "decoded entry size does not match its index descriptor",
            ));
        }
        if let Some(cache) = &self.cache {
            cache.put(&identity, &decoded).map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_CACHE_WRITE",
                    "plaintext cache write failed",
                )
            })?;
        }
        Ok((decoded, false))
    }

    fn entry(&self, uri: &str) -> Result<&MountedEntry, PazError> {
        validate_legacy_vfs_uri(&self.prefix, uri)?;
        self.entries
            .get(uri)
            .ok_or_else(|| error("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry was not found"))
    }
}

impl LegacyMountedVfs for MinoriMountedVfs {
    fn mount_id(&self) -> &str {
        &self.mount_id
    }
    fn manifest(&self) -> &LegacyPackManifest {
        &self.manifest
    }

    fn validate_sources(&self) -> Result<(), PazError> {
        for archive in &self.archives {
            verify_source_unchanged(archive)?;
            if hash_parts(&archive.parts)? != archive.hash {
                return Err(error(
                    "ASTRA_EMU_MINORI_SOURCE_CHANGED",
                    "archive content changed after mount",
                ));
            }
        }
        Ok(())
    }

    fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, PazError> {
        validate_legacy_vfs_uri(&self.prefix, uri)?;
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
            return Err(error(
                "ASTRA_EMU_VFS_NOT_FOUND",
                "VFS directory was not found",
            ));
        }
        Ok(children.into_values().collect())
    }

    fn stat(&self, uri: &str) -> Result<LegacyVfsStat, PazError> {
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
            entry_id: Some(entry.descriptor.entry_id.clone()),
            kind: LegacyVfsNodeKind::File,
            size: entry.descriptor.unpacked_size,
            content_hash: None,
            archive_role: Some(entry.descriptor.archive_role.clone()),
            method: Some(entry_method(&entry.descriptor).into()),
        })
    }

    fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<LegacyVfsReadResult, PazError> {
        if length > LEGACY_VFS_MAX_READ_BYTES {
            return Err(error(
                "ASTRA_EMU_VFS_READ_LIMIT",
                "range read exceeds the configured limit",
            ));
        }
        let entry = self.entry(uri)?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| error("ASTRA_EMU_VFS_READ_OVERFLOW", "range read overflowed"))?;
        if offset > entry.descriptor.unpacked_size || end > entry.descriptor.unpacked_size {
            return Err(error(
                "ASTRA_EMU_VFS_READ_BOUNDS",
                "range read is outside the entry",
            ));
        }
        let (decoded, cache_hit) = self.decoded_entry(entry)?;
        Ok(LegacyVfsReadResult {
            uri: uri.into(),
            offset,
            bytes: decoded[offset as usize..end as usize].to_vec(),
            eof: end == entry.descriptor.unpacked_size,
            cache_hit,
        })
    }

    fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, PazError> {
        Ok(Box::new(Cursor::new(
            self.decoded_entry(self.entry(uri)?)?.0,
        )))
    }
}

fn validate_role_set(configs: &[PazArchiveConfig]) -> Result<(), PazError> {
    let mut roles = BTreeSet::new();
    for config in configs {
        if !REQUIRED_ARCHIVE_ROLES.contains(&config.role.as_str())
            || !roles.insert(config.role.as_str())
        {
            return Err(error(
                "ASTRA_EMU_MINORI_ARCHIVE_ROLE",
                "archive role is unknown or duplicated",
            ));
        }
        let expected = format!("{}.paz", config.role);
        if !config
            .path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&expected))
        {
            return Err(error(
                "ASTRA_EMU_MINORI_ARCHIVE_NAME",
                "archive file name does not match its declared role",
            ));
        }
    }
    if REQUIRED_ARCHIVE_ROLES
        .iter()
        .any(|role| !roles.contains(role))
    {
        return Err(error(
            "ASTRA_EMU_MINORI_ARCHIVE_MISSING",
            "all six required PAZ roles must be supplied",
        ));
    }
    Ok(())
}

fn parse_archive_index(
    source: &mut ArchiveSource,
    expected_index_xor: u32,
    decrypt_provider: &MinoriPazDecryptProvider,
) -> Result<Vec<PazEntryDescriptor>, PazError> {
    let (index_offset, encrypted_size) = if source.version == 0 {
        let bytes = read_source_range(source, 0, 4)?;
        let mut size = [0u8; 4];
        size.copy_from_slice(&bytes);
        (4u64, u32::from_le_bytes(size) as u64)
    } else {
        let raw = read_source_range(source, 0x20, 4)?;
        let raw_size = u32::from_le_bytes(raw.try_into().unwrap());
        source.xor_key = (raw_size >> 24) as u8;
        let derived = u32::from_le_bytes([source.xor_key; 4]);
        if expected_index_xor != 0 && expected_index_xor != derived {
            return Err(error(
                "ASTRA_EMU_MINORI_INDEX_XOR",
                "configured index XOR does not match the archive header",
            ));
        }
        (0x24u64, (raw_size ^ derived) as u64)
    };
    if encrypted_size == 0
        || encrypted_size > MAX_INDEX_BYTES
        || !encrypted_size.is_multiple_of(8)
        || index_offset + encrypted_size > source.length
    {
        return Err(error(
            "ASTRA_EMU_MINORI_INDEX_SIZE",
            "PAZ index size is empty, unaligned, or out of bounds",
        ));
    }
    let mut encrypted = read_source_range(source, index_offset, encrypted_size)?;
    xor_byte(&mut encrypted, source.xor_key);
    let decoded = decrypt_bytes(
        decrypt_provider,
        MinoriDecryptDescriptor::Index {
            role: source.role.clone(),
            version: source.version,
            stream_offset: 0,
        },
        &encrypted,
    )?;
    let mut cursor = Cursor::new(decoded.as_slice());
    let count = read_u32(&mut cursor)? as usize;
    if count > 1_000_000 {
        return Err(error(
            "ASTRA_EMU_MINORI_ENTRY_COUNT",
            "PAZ entry count exceeds the configured limit",
        ));
    }
    let video_key = if source.role == "mov" {
        let mut key = vec![0u8; 256];
        cursor
            .read_exact(&mut key)
            .map_err(|_| error("ASTRA_EMU_MINORI_VIDEO_KEY", "PAZ video key is truncated"))?;
        Some(key)
    } else {
        None
    };
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let name = read_c_string(&mut cursor)?;
        let offset = read_u64(&mut cursor)?;
        let unpacked_size = read_u32(&mut cursor)? as u64;
        let stored_size = read_u32(&mut cursor)? as u64;
        let aligned_size = read_u32(&mut cursor)? as u64;
        let packed = read_i32(&mut cursor)? != 0;
        if unpacked_size > MAX_ENTRY_BYTES
            || stored_size > aligned_size
            || (source.role != "mov" && !aligned_size.is_multiple_of(8))
            || offset
                .checked_add(aligned_size)
                .is_none_or(|end| end > source.length)
        {
            tracing::error!(
                event = "astra_emu_minori_entry_bounds_invalid",
                archive_role = %source.role,
                entry_index = index,
                offset,
                unpacked_size,
                stored_size,
                aligned_size,
                archive_size = source.length
            );
            return Err(error(
                "ASTRA_EMU_MINORI_ENTRY_BOUNDS",
                "PAZ entry descriptor is oversized, unaligned, or out of bounds",
            ));
        }
        entries.push(PazEntryDescriptor {
            archive_role: source.role.clone(),
            entry_id: format!("{}:{index}", source.role),
            name,
            offset,
            unpacked_size,
            stored_size,
            aligned_size,
            packed,
            video_key: video_key.clone(),
        });
    }
    if cursor.position() as usize > decoded.len() {
        return Err(error(
            "ASTRA_EMU_MINORI_INDEX_SHORT",
            "PAZ index is truncated",
        ));
    }
    Ok(entries)
}

fn normalize_entry_name(name: &str) -> Result<String, PazError> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(error(
            "ASTRA_EMU_MINORI_ENTRY_PATH",
            "PAZ entry name is absolute or traverses",
        ));
    }
    Ok(normalized)
}

fn read_c_string(cursor: &mut Cursor<&[u8]>) -> Result<String, PazError> {
    let start = cursor.position() as usize;
    let bytes = cursor.get_ref();
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| start + relative)
        .ok_or_else(|| {
            error(
                "ASTRA_EMU_MINORI_INDEX_STRING",
                "PAZ entry name is not terminated",
            )
        })?;
    if end - start > 4096 {
        return Err(error(
            "ASTRA_EMU_MINORI_INDEX_STRING",
            "PAZ entry name exceeds the configured limit",
        ));
    }
    let (text, _, malformed) = SHIFT_JIS.decode(&bytes[start..end]);
    if malformed {
        return Err(error(
            "ASTRA_EMU_MINORI_INDEX_ENCODING",
            "PAZ entry name is not valid CP932",
        ));
    }
    cursor.set_position((end + 1) as u64);
    Ok(text.into_owned())
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, PazError> {
    let mut bytes = [0; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| error("ASTRA_EMU_MINORI_INDEX_SHORT", "PAZ index is truncated"))?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_i32(cursor: &mut Cursor<&[u8]>) -> Result<i32, PazError> {
    Ok(read_u32(cursor)? as i32)
}
fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, PazError> {
    let mut bytes = [0; 8];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| error("ASTRA_EMU_MINORI_INDEX_SHORT", "PAZ index is truncated"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_exact_range(
    path: &Path,
    offset: u64,
    length: u64,
    source_length: u64,
) -> Result<Vec<u8>, PazError> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > source_length)
        || length > usize::MAX as u64
    {
        return Err(error(
            "ASTRA_EMU_MINORI_SOURCE_BOUNDS",
            "archive range is out of bounds",
        ));
    }
    let mut file = File::open(path).map_err(|_| {
        error(
            "ASTRA_EMU_MINORI_ARCHIVE_OPEN",
            "PAZ archive cannot be opened",
        )
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| error("ASTRA_EMU_MINORI_ARCHIVE_SEEK", "PAZ archive seek failed"))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes).map_err(|_| {
        error(
            "ASTRA_EMU_MINORI_ARCHIVE_SHORT_READ",
            "PAZ archive returned a short read",
        )
    })?;
    Ok(bytes)
}

fn read_source_range(
    source: &ArchiveSource,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>, PazError> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > source.length)
        || length > usize::MAX as u64
    {
        return Err(error(
            "ASTRA_EMU_MINORI_SOURCE_BOUNDS",
            "archive range is out of bounds",
        ));
    }
    let mut remaining = length;
    let mut logical = 0u64;
    let mut start = offset;
    let mut output = Vec::with_capacity(length as usize);
    for part in &source.parts {
        let part_end = logical + part.length;
        if start >= part_end {
            logical = part_end;
            continue;
        }
        let local = start.saturating_sub(logical);
        let count = remaining.min(part.length - local);
        let bytes = read_exact_range(&part.path, local, count, part.length)?;
        output.extend_from_slice(&bytes);
        remaining -= count;
        start += count;
        logical = part_end;
        if remaining == 0 {
            break;
        }
    }
    if remaining != 0 {
        return Err(error(
            "ASTRA_EMU_MINORI_ARCHIVE_SHORT_READ",
            "multipart PAZ returned a short read",
        ));
    }
    Ok(output)
}

fn hash_parts(parts: &[ArchivePart]) -> Result<Hash256, PazError> {
    let mut hasher = Sha256::new();
    // Windows reserves a relatively small main-thread stack. Archive hashing is a
    // normal host operation, so its MiB-sized scratch area belongs on the heap.
    let mut buffer = vec![0u8; 1024 * 1024];
    for part in parts {
        let mut file = File::open(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_ARCHIVE_OPEN",
                "PAZ archive cannot be opened",
            )
        })?;
        loop {
            let count = file.read(&mut buffer).map_err(|_| {
                error(
                    "ASTRA_EMU_MINORI_ARCHIVE_READ",
                    "PAZ archive hash read failed",
                )
            })?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
    }
    Ok(Hash256::from_bytes(hasher.finalize().into()))
}

fn verify_source_unchanged(source: &ArchiveSource) -> Result<(), PazError> {
    for part in &source.parts {
        let metadata = std::fs::metadata(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_SOURCE_CHANGED",
                "archive part disappeared after mount",
            )
        })?;
        if metadata.len() != part.length || metadata.modified().ok() != part.modified {
            return Err(error(
                "ASTRA_EMU_MINORI_SOURCE_CHANGED",
                "archive part metadata changed after mount",
            ));
        }
    }
    Ok(())
}

fn discover_parts(base: &Path, game_root: &Path) -> Result<Vec<ArchivePart>, PazError> {
    let game_root = game_root
        .canonicalize()
        .map_err(|_| error("ASTRA_EMU_MINORI_GAME_ROOT", "game root cannot be resolved"))?;
    let base = base.canonicalize().map_err(|_| {
        error(
            "ASTRA_EMU_MINORI_ARCHIVE_OPEN",
            "required PAZ archive cannot be resolved",
        )
    })?;
    if !base.starts_with(&game_root) {
        return Err(error(
            "ASTRA_EMU_MINORI_ARCHIVE_PATH",
            "PAZ archive resolves outside the game root",
        ));
    }
    let metadata = std::fs::metadata(&base).map_err(|_| {
        error(
            "ASTRA_EMU_MINORI_ARCHIVE_OPEN",
            "required PAZ archive cannot be opened",
        )
    })?;
    let mut parts = vec![ArchivePart {
        path: base.clone(),
        length: metadata.len(),
        modified: metadata.modified().ok(),
    }];
    let base_text = base.as_os_str().to_string_lossy();
    let mut missing_seen = false;
    for suffix in b'A'..=b'Z' {
        let path = PathBuf::from(format!("{base_text}{}", suffix as char));
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                if missing_seen {
                    return Err(error(
                        "ASTRA_EMU_MINORI_MULTIPART_GAP",
                        "PAZ multipart suffixes must be contiguous",
                    ));
                }
                if metadata.len() == 0 {
                    return Err(error(
                        "ASTRA_EMU_MINORI_MULTIPART_EMPTY",
                        "PAZ multipart volume is empty",
                    ));
                }
                let path = path.canonicalize().map_err(|_| {
                    error(
                        "ASTRA_EMU_MINORI_MULTIPART_OPEN",
                        "PAZ multipart volume cannot be resolved",
                    )
                })?;
                if !path.starts_with(&game_root) {
                    return Err(error(
                        "ASTRA_EMU_MINORI_ARCHIVE_PATH",
                        "PAZ multipart volume resolves outside the game root",
                    ));
                }
                parts.push(ArchivePart {
                    path,
                    length: metadata.len(),
                    modified: metadata.modified().ok(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing_seen = true,
            Err(_) => {
                return Err(error(
                    "ASTRA_EMU_MINORI_MULTIPART_OPEN",
                    "PAZ multipart volume cannot be inspected",
                ))
            }
        }
    }
    Ok(parts)
}

fn media_kind(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "sc" => "script",
        "png" | "jpg" | "bmp" => "image",
        "ogg" | "wav" => "audio",
        "mpg" | "avi" | "wmv" => "video",
        _ => "binary",
    }
}

fn entry_method(entry: &PazEntryDescriptor) -> &'static str {
    match (entry.archive_role.as_str(), entry.packed) {
        ("mov", false) => "movie-transform",
        ("mov", true) => "movie-transform+zlib",
        (_, false) => "blowfish+rc4",
        (_, true) => "blowfish+rc4+zlib",
    }
}

fn password_for_entry<'a>(
    entry: &PazEntryDescriptor,
    scheme: &'a PazRoleScheme,
) -> Option<&'a str> {
    if entry.packed {
        return None;
    }
    let lower = entry.name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        scheme.type_passwords.get("png").map(String::as_str)
    } else if lower.ends_with(".ogg") || matches!(entry.archive_role.as_str(), "se" | "voice") {
        scheme.type_passwords.get("ogg").map(String::as_str)
    } else if lower.ends_with(".sc") {
        scheme.type_passwords.get("sc").map(String::as_str)
    } else if lower.ends_with(".avi") || lower.ends_with(".mpg") || lower.ends_with(".mpeg") {
        scheme.type_passwords.get("avi").map(String::as_str)
    } else {
        None
    }
}

fn decrypt_bytes(
    provider: &MinoriPazDecryptProvider,
    descriptor: MinoriDecryptDescriptor,
    bytes: &[u8],
) -> Result<Vec<u8>, PazError> {
    if bytes.is_empty() {
        return Err(error(
            "ASTRA_EMU_MINORI_DECRYPT_EMPTY",
            "Minori decrypt input is empty",
        ));
    }
    let mut output = Vec::with_capacity(bytes.len());
    for (batch_index, batch) in bytes.chunks(LEGACY_DECRYPT_MAX_BATCH_BYTES).enumerate() {
        let batch_offset = u64::try_from(batch_index)
            .ok()
            .and_then(|index| index.checked_mul(LEGACY_DECRYPT_MAX_BATCH_BYTES as u64))
            .ok_or_else(|| {
                error(
                    "ASTRA_EMU_MINORI_DECRYPT_OFFSET",
                    "decrypt batch offset overflowed",
                )
            })?;
        let batch_descriptor = descriptor.with_stream_offset(batch_offset);
        let payload = serde_json::to_vec(&batch_descriptor).map_err(|_| {
            error(
                "ASTRA_EMU_MINORI_DESCRIPTOR",
                "Minori decrypt descriptor could not be encoded",
            )
        })?;
        let opaque = LegacyOpaqueDescriptor {
            schema_id: MINORI_DECRYPT_DESCRIPTOR_SCHEMA.into(),
            schema_hash: provider.descriptor_schema_hash(),
            payload,
        };
        let phase = match batch_descriptor {
            MinoriDecryptDescriptor::Index { .. } => LegacyDecryptPhase::Index,
            MinoriDecryptDescriptor::Entry { .. } => LegacyDecryptPhase::Entry,
        };
        for (chunk_index, chunk) in batch.chunks(LEGACY_DECRYPT_CHUNK_BYTES).enumerate() {
            let chunk_offset = (chunk_index * LEGACY_DECRYPT_CHUNK_BYTES) as u64;
            output.extend_from_slice(&provider.decrypt(LegacyDecryptRequest {
                phase,
                descriptors: std::slice::from_ref(&opaque),
                transport: LegacyDecryptTransport {
                    chunk_offset,
                    total_size: batch.len() as u64,
                    batch_index: batch_index as u32,
                    input_bound: batch.len() as u64,
                    output_bound: chunk.len() as u64,
                },
                bytes: chunk,
            })?);
        }
    }
    if output.len() != bytes.len() {
        return Err(error(
            "ASTRA_EMU_MINORI_DECRYPT_SIZE",
            "Minori decrypt output size is inconsistent",
        ));
    }
    Ok(output)
}

impl MinoriDecryptDescriptor {
    fn with_stream_offset(&self, stream_offset: u64) -> Self {
        match self {
            Self::Index { role, version, .. } => Self::Index {
                role: role.clone(),
                version: *version,
                stream_offset,
            },
            Self::Entry { version, entry, .. } => Self::Entry {
                version: *version,
                entry: entry.clone(),
                stream_offset,
            },
        }
    }
}

fn error(code: &'static str, message: impl Into<String>) -> PazError {
    PazError::invalid(code, message)
}

fn validate_blowfish_key(key: &[u8]) -> Result<(), PazError> {
    if !(4..=56).contains(&key.len()) {
        return Err(error(
            "ASTRA_EMU_MINORI_BLOWFISH_KEY",
            "Blowfish key length must be between 4 and 56 bytes",
        ));
    }
    Ok(())
}

fn blowfish_decrypt(key: &[u8], encrypted: &[u8]) -> Result<Vec<u8>, PazError> {
    validate_blowfish_key(key)?;
    if !encrypted.len().is_multiple_of(8) {
        return Err(error(
            "ASTRA_EMU_MINORI_BLOWFISH_ALIGNMENT",
            "Blowfish input is not block aligned",
        ));
    }
    let cipher: Blowfish = Blowfish::new_from_slice(key)
        .map_err(|_| error("ASTRA_EMU_MINORI_BLOWFISH_KEY", "Blowfish key is invalid"))?;
    let mut bytes = encrypted.to_vec();
    for chunk in bytes.chunks_exact_mut(8) {
        chunk[..4].reverse();
        chunk[4..].reverse();
        let block: &mut [u8; 8] = chunk.try_into().expect("chunks_exact_mut yields 8 bytes");
        cipher.decrypt_block(block.into());
        block[..4].reverse();
        block[4..].reverse();
    }
    Ok(bytes)
}

fn xor_byte(bytes: &mut [u8], key: u8) {
    if key != 0 {
        for byte in bytes {
            *byte ^= key;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blowfish::cipher::BlockCipherEncrypt;
    use std::{fs, io::Write};

    const FIXTURE_KEY: &[u8] = b"fixture-key";

    fn fixture_archive(role: &str, version: u8) -> Vec<u8> {
        let payload = b"fixture\0";
        let mut index = Vec::new();
        index.extend_from_slice(&1u32.to_le_bytes());
        if role == "mov" {
            index.extend(0u8..=255);
        }
        index.extend_from_slice(format!("{role}.bin\0").as_bytes());
        let descriptor_offset = index.len();
        index.extend_from_slice(&0u64.to_le_bytes());
        index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        index.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        index.extend_from_slice(&0i32.to_le_bytes());
        while !index.len().is_multiple_of(8) {
            index.push(0);
        }
        let header_size = if version == 0 { 4 } else { 0x24 };
        let payload_offset = header_size + index.len() as u64;
        index[descriptor_offset..descriptor_offset + 8]
            .copy_from_slice(&payload_offset.to_le_bytes());

        let index = blowfish_encrypt(FIXTURE_KEY, &index);
        let payload = if role == "mov" {
            payload.to_vec()
        } else {
            blowfish_encrypt(FIXTURE_KEY, payload)
        };
        let mut archive = Vec::new();
        if version == 0 {
            archive.extend_from_slice(&(index.len() as u32).to_le_bytes());
            archive.extend_from_slice(&index);
            archive.extend_from_slice(&payload);
        } else {
            let xor_key = 0x5au8;
            let derived = u32::from_le_bytes([xor_key; 4]);
            archive.resize(0x20, 0);
            archive.extend_from_slice(&((index.len() as u32) ^ derived).to_le_bytes());
            archive.extend(index.into_iter().map(|byte| byte ^ xor_key));
            archive.extend(payload.iter().map(|byte| *byte ^ xor_key));
        }
        archive
    }

    fn mount_fixture(root: &Path, version: u8) -> MinoriMountedVfs {
        let configs = REQUIRED_ARCHIVE_ROLES
            .iter()
            .map(|role| PazArchiveConfig {
                role: (*role).into(),
                path: root.join(format!("{role}.paz")),
                game_root: root.to_path_buf(),
                version,
                index_size_xor: if version == 0 { 0 } else { 0x5a5a5a5a },
            })
            .collect();
        let roles = REQUIRED_ARCHIVE_ROLES
            .into_iter()
            .map(|role| {
                (
                    role.into(),
                    PazRoleScheme {
                        index_key: FIXTURE_KEY.to_vec(),
                        data_key: if role == "mov" {
                            Vec::new()
                        } else {
                            FIXTURE_KEY.to_vec()
                        },
                        type_passwords: BTreeMap::new(),
                        archive_xor: None,
                        video_key: None,
                    },
                )
            })
            .collect();
        let provider = Arc::new(
            MinoriPazDecryptProvider::new(Hash256::from_sha256(b"fixture-profile"), roles).unwrap(),
        );
        MinoriMountedVfs::mount(
            "fixture",
            "minori:/",
            configs,
            provider,
            Hash256::from_sha256(b"fixture-mount-profile"),
        )
        .unwrap()
    }

    fn blowfish_encrypt(key: &[u8], plaintext: &[u8]) -> Vec<u8> {
        assert!(plaintext.len().is_multiple_of(8));
        let cipher: Blowfish = Blowfish::new_from_slice(key).unwrap();
        let mut bytes = plaintext.to_vec();
        for chunk in bytes.chunks_exact_mut(8) {
            chunk[..4].reverse();
            chunk[4..].reverse();
            let block: &mut [u8; 8] = chunk.try_into().unwrap();
            cipher.encrypt_block(block.into());
            block[..4].reverse();
            block[4..].reverse();
        }
        bytes
    }

    #[test]
    fn traversal_is_rejected() {
        assert_eq!(
            normalize_entry_name("../secret.sc").unwrap_err().code(),
            "ASTRA_EMU_MINORI_ENTRY_PATH"
        );
    }
    #[test]
    fn required_roles_are_strict() {
        let configs = vec![];
        assert_eq!(
            validate_role_set(&configs).unwrap_err().code(),
            "ASTRA_EMU_MINORI_ARCHIVE_MISSING"
        );
    }

    #[test]
    fn v0_fixture_mounts_all_roles_and_reads_across_a_volume_boundary() {
        let temp = tempfile::tempdir().unwrap();
        for role in REQUIRED_ARCHIVE_ROLES {
            let archive = fixture_archive(role, 0);
            let path = temp.path().join(format!("{role}.paz"));
            if role == "scr" {
                let split = archive.len() - 6;
                fs::write(&path, &archive[..split]).unwrap();
                fs::write(temp.path().join("scr.pazA"), &archive[split..]).unwrap();
            } else {
                fs::write(path, archive).unwrap();
            }
        }
        let vfs = mount_fixture(temp.path(), 0);
        assert_eq!(vfs.manifest().entries.len(), REQUIRED_ARCHIVE_ROLES.len());
        let read = vfs.read_range("minori:/scr/scr.bin", 3, 4).unwrap();
        assert_eq!(read.bytes, b"ture");
        assert!(!read.cache_hit);
    }

    #[test]
    fn source_mutation_after_mount_is_blocking() {
        let temp = tempfile::tempdir().unwrap();
        for role in REQUIRED_ARCHIVE_ROLES {
            fs::write(
                temp.path().join(format!("{role}.paz")),
                fixture_archive(role, 0),
            )
            .unwrap();
        }
        let vfs = mount_fixture(temp.path(), 0);
        fs::OpenOptions::new()
            .append(true)
            .open(temp.path().join("scr.paz"))
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        assert_eq!(
            vfs.read_range("minori:/scr/scr.bin", 0, 1)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_SOURCE_CHANGED"
        );
    }

    #[test]
    fn v1_and_v2_fixtures_apply_archive_xor_and_random_reads() {
        for version in [1, 2] {
            let temp = tempfile::tempdir().unwrap();
            for role in REQUIRED_ARCHIVE_ROLES {
                fs::write(
                    temp.path().join(format!("{role}.paz")),
                    fixture_archive(role, version),
                )
                .unwrap();
            }
            let vfs = mount_fixture(temp.path(), version);
            let read = vfs.read_range("minori:/voice/voice.bin", 1, 6).unwrap();
            assert_eq!(read.bytes, b"ixture");
        }
    }
}
