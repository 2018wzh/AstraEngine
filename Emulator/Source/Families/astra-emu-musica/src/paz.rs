use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use crate::{MusicaLocaleHook, MUSICA_READER_ID};
use astra_core::Hash256;
use astra_emu_family_core::{
    validate_legacy_vfs_directory_uri, validate_legacy_vfs_uri, LegacyCoreError, LegacyMountedVfs,
    LegacyPackManifest, LegacyVfsEntry, LegacyVfsNode, LegacyVfsNodeKind, LegacyVfsReadResult,
    LegacyVfsSource, LegacyVfsStat, LegacyVfsStream, LEGACY_PACK_MANIFEST_SCHEMA,
    LEGACY_VFS_MAX_READ_BYTES,
};
use blowfish::cipher::{BlockCipherDecrypt, KeyInit as BlowfishKeyInit};
use blowfish::Blowfish;
use flate2::read::ZlibDecoder;
use rc4::{Rc4, StreamCipher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type PazError = LegacyCoreError;

pub const REQUIRED_ARCHIVE_ROLES: [&str; 8] =
    ["bg", "bgm", "scr", "st", "sys", "se", "voice", "mov"];
pub const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;
const STREAM_CHUNK_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct PazArchiveConfig {
    pub rc4_skip_crc: bool,
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
    /// Original CP932 index bytes retained only in the mount session. The
    /// entry RC4 key follows GARbro's decoded-name-to-CP932 derivation.
    pub crypto_name: Vec<u8>,
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
}

/// Family-owned PAZ transform state. Keys are loaded once by the factory and
/// remain private to the VFS mount; no callback or generic decrypt trait is
/// involved in entry reads.
#[derive(Debug)]
pub struct MusicaPazDecryptor {
    roles: BTreeMap<String, PazRoleScheme>,
    locale_hook: MusicaLocaleHook,
}

impl MusicaPazDecryptor {
    pub fn new_with_locale(
        roles: BTreeMap<String, PazRoleScheme>,
        locale_hook: MusicaLocaleHook,
    ) -> Result<Self, PazError> {
        if roles
            .keys()
            .any(|role| !REQUIRED_ARCHIVE_ROLES.contains(&role.as_str()))
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_KEY_ROLES",
                "Musica key set contains an unknown archive role",
            ));
        }
        for role in REQUIRED_ARCHIVE_ROLES {
            let scheme = roles.get(role).ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_KEY_ROLES",
                    "Musica key set is missing a required archive role",
                )
            })?;
            validate_blowfish_key(&scheme.index_key)?;
            if role == "mov" {
                if !scheme.data_key.is_empty() {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_MOVIE_KEY",
                        "movie archive must not provide a data key",
                    ));
                }
            } else {
                validate_blowfish_key(&scheme.data_key)?;
            }
        }
        Ok(Self { roles, locale_hook })
    }

    pub const fn locale_hook(&self) -> MusicaLocaleHook {
        self.locale_hook
    }

    fn scheme(&self, role: &str) -> Result<&PazRoleScheme, PazError> {
        self.roles.get(role).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_KEY_ROLES",
                "Musica key set has no scheme for the archive role",
            )
        })
    }

    fn decrypt_index(&self, role: &str, encrypted: &[u8]) -> Result<Vec<u8>, PazError> {
        let mut bytes = encrypted.to_vec();
        blowfish_decrypt_in_place(&self.scheme(role)?.index_key, &mut bytes)?;
        Ok(bytes)
    }

    /// Stateless RC4 layer for random-access reads: keys per call and drops
    /// keystream bytes up to the requested offset. The sequential stream
    /// path keeps a single keyed cipher instead (see `MusicaEntryStream`).
    fn apply_entry_rc4_stateless(
        &self,
        version: u8,
        rc4_skip_crc: bool,
        entry: &PazEntryDescriptor,
        absolute_offset: u64,
        bytes: &mut [u8],
    ) -> Result<(), PazError> {
        if version == 0 {
            return Ok(());
        }
        let scheme = self.scheme(&entry.archive_role)?;
        if let Some(password) = password_for_entry(entry, scheme) {
            let key = entry_key_material_with_locale(entry, Some(password), self.locale_hook)?;
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RC4_KEY",
                    "entry RC4 key length is invalid",
                )
            })?;
            let version_skip = if version >= 2 && rc4_skip_crc {
                (crc32(&key) >> 12 & 0xff) as u64
            } else {
                0
            };
            let skip = version_skip
                .checked_add(absolute_offset)
                .ok_or_else(|| error("ASTRA_EMU_MUSICA_RC4_SKIP", "entry RC4 offset overflowed"))?;
            let mut remaining = skip;
            let mut discarded = [0u8; 64 * 1024];
            while remaining != 0 {
                let count =
                    usize::try_from(remaining.min(discarded.len() as u64)).map_err(|_| {
                        error("ASTRA_EMU_MUSICA_RC4_SKIP", "entry RC4 skip is too large")
                    })?;
                cipher.apply_keystream(&mut discarded[..count]);
                discarded[..count].fill(0);
                remaining -= count as u64;
            }
            cipher.apply_keystream(bytes);
        }
        Ok(())
    }

    /// Stateless Blowfish layer (and the mov byte-table transform). RC4 is
    /// applied by the caller: the sequential stream keeps one keyed cipher,
    /// while random access re-keys per call and drops keystream bytes.
    fn decrypt_entry_chunk_blowfish(
        &self,
        version: u8,
        entry: &PazEntryDescriptor,
        absolute_offset: u64,
        encrypted: Vec<u8>,
    ) -> Result<Vec<u8>, PazError> {
        let scheme = self.scheme(&entry.archive_role)?;
        let mut bytes = encrypted;
        if entry.archive_role == "mov" {
            let video_key = entry.video_key.as_ref().ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_VIDEO_KEY",
                    "movie entry is missing its decrypted index key",
                )
            })?;
            if video_key.len() != 256 {
                return Err(error(
                    "ASTRA_EMU_MUSICA_VIDEO_KEY",
                    "movie index key must contain exactly 256 bytes",
                ));
            }
            if version == 0 {
                let mut inverse = [0u8; 256];
                for (plain, encrypted_byte) in video_key.iter().enumerate() {
                    inverse[*encrypted_byte as usize] = plain as u8;
                }
                for byte in &mut bytes {
                    *byte = inverse[*byte as usize];
                }
                return Ok(bytes);
            }
            let entry_key = entry_key_material_with_locale(
                entry,
                password_for_entry(entry, scheme),
                self.locale_hook,
            )?;
            let key = (0..256)
                .map(|index| video_key[index] ^ entry_key[index % entry_key.len()])
                .collect::<Vec<_>>();
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RC4_KEY",
                    "movie RC4 key length is invalid",
                )
            })?;
            let block_len = usize::try_from(entry.aligned_size.min(0x10000)).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_VIDEO_SIZE",
                    "movie transform size exceeds the platform bound",
                )
            })?;
            if block_len == 0 {
                return Ok(bytes);
            }
            let mut keystream = vec![0; block_len];
            cipher.apply_keystream(&mut keystream);
            let offset = usize::try_from(absolute_offset).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RC4_OFFSET",
                    "movie transform offset exceeds the platform bound",
                )
            })?;
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte ^= keystream[(offset + index) % keystream.len()];
            }
            return Ok(bytes);
        }

        blowfish_decrypt_in_place(&scheme.data_key, &mut bytes)?;
        Ok(bytes)
    }
}

#[derive(Clone)]
struct ArchiveSource {
    rc4_skip_crc: bool,
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
}

pub struct MusicaMountedVfs {
    mount_id: String,
    prefix: String,
    manifest: LegacyPackManifest,
    archives: Vec<ArchiveSource>,
    entries: BTreeMap<String, MountedEntry>,
    /// Musica runs on a case-insensitive Windows filesystem, while the PAZ
    /// index preserves the original spelling. Keep canonical URIs in
    /// `entries` and use this unique ASCII-folded index only for lookup.
    folded_entries: BTreeMap<String, String>,
    decryptor: Arc<MusicaPazDecryptor>,
}

impl MusicaMountedVfs {
    pub fn mount(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decryptor: Arc<MusicaPazDecryptor>,
        launch_profile_hash: Hash256,
    ) -> Result<Self, PazError> {
        let mount_id = mount_id.into();
        let prefix = prefix.into();
        if prefix != "musica:/" {
            return Err(error(
                "ASTRA_EMU_MUSICA_PREFIX",
                "Musica mounts require the stable musica:/ prefix",
            ));
        }
        validate_role_set(&configs)?;
        let mut archives = Vec::with_capacity(configs.len());
        let mut entries = BTreeMap::new();
        let mut folded_entries = BTreeMap::new();
        let mut entry_ids = BTreeSet::new();
        let mut prepared = Vec::with_capacity(configs.len());
        for config in configs {
            let parts = discover_parts(&config.path, &config.game_root)?;
            let total_length = parts
                .iter()
                .try_fold(0u64, |total, part| total.checked_add(part.length))
                .ok_or_else(|| error("ASTRA_EMU_MUSICA_ARCHIVE_SIZE", "PAZ size overflowed"))?;
            if parts.first().is_none_or(|part| part.length == 0) {
                return Err(error(
                    "ASTRA_EMU_MUSICA_ARCHIVE_EMPTY",
                    "required PAZ archive is empty",
                ));
            }
            if config.version > 2 {
                return Err(error(
                    "ASTRA_EMU_MUSICA_VERSION",
                    "only PAZ versions 0 through 2 are supported",
                ));
            }
            let mut source = ArchiveSource {
                rc4_skip_crc: config.rc4_skip_crc,
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
                decryptor.as_ref(),
                decryptor.locale_hook(),
            )?;
            prepared.push((config, source, parsed));
        }
        for (config, mut source, parsed) in prepared {
            let source_hash = hash_parts(&source.parts)?;
            source.hash = source_hash;
            let archive_index = archives.len();
            for entry in parsed {
                let uri = format!(
                    "{}{}/{}",
                    prefix,
                    config.role,
                    normalize_entry_name(&entry.name)?
                );
                validate_legacy_vfs_uri(&prefix, &uri)?;
                insert_mounted_entry(
                    &mut entries,
                    &mut folded_entries,
                    &mut entry_ids,
                    MountedEntry {
                        descriptor: entry,
                        uri,
                        archive: archive_index,
                    },
                )?;
            }
            archives.push(source);
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
                source_hash: archives[entry.archive].hash,
                content_hash: None,
                method: entry_method(&entry.descriptor).into(),
                media_kind: media_kind(&entry.descriptor.name).into(),
            })
            .collect();
        let manifest = LegacyPackManifest {
            schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
            family_id: "musica".into(),
            mount_id: mount_id.clone(),
            prefix: prefix.clone(),
            reader_id: MUSICA_READER_ID.into(),
            reader_hash,
            launch_profile_hash,
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
            folded_entries,
            decryptor,
        })
    }

    fn entry(&self, uri: &str) -> Result<&MountedEntry, PazError> {
        validate_legacy_vfs_uri(&self.prefix, uri)?;
        self.entries
            .get(uri)
            .or_else(|| {
                self.folded_entries
                    .get(&fold_entry_uri(uri))
                    .and_then(|canonical| self.entries.get(canonical))
            })
            .ok_or_else(|| error("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry was not found"))
    }

    fn stream_for(&self, entry: &MountedEntry) -> Result<MusicaDecodedStream, PazError> {
        let archive = self.archives[entry.archive].clone();
        verify_source_unchanged(&archive)?;
        let raw = MusicaEntryStream {
            archive,
            entry: entry.descriptor.clone(),
            decryptor: Arc::clone(&self.decryptor),
            encrypted_position: 0,
            pending: Vec::new(),
            pending_position: 0,
            rc4: None,
        };
        let inner = if entry.descriptor.packed {
            MusicaDecodedInner::Zlib(ZlibDecoder::new(raw))
        } else {
            MusicaDecodedInner::Raw(raw)
        };
        Ok(MusicaDecodedStream {
            inner,
            remaining: entry.descriptor.unpacked_size,
            eof_checked: false,
        })
    }

    fn read_raw_range(
        &self,
        entry: &MountedEntry,
        offset: u64,
        length: u64,
    ) -> Result<Option<Vec<u8>>, PazError> {
        if entry.descriptor.packed {
            return Ok(None);
        }
        let archive = &self.archives[entry.archive];
        verify_source_unchanged(archive)?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| error("ASTRA_EMU_VFS_READ_OVERFLOW", "range read overflowed"))?;
        let encrypted_start = if entry.descriptor.archive_role == "mov" {
            offset
        } else {
            offset & !7
        };
        let requested_end = end.max(encrypted_start);
        let encrypted_end = if entry.descriptor.archive_role == "mov" {
            requested_end
        } else {
            requested_end
                .checked_add(7)
                .ok_or_else(|| error("ASTRA_EMU_MUSICA_SOURCE_BOUNDS", "range end overflowed"))?
                & !7
        }
        .min(entry.descriptor.aligned_size);
        if encrypted_end < encrypted_start {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_BOUNDS",
                "entry encrypted range is invalid",
            ));
        }
        let encrypted_offset = entry
            .descriptor
            .offset
            .checked_add(encrypted_start)
            .ok_or_else(|| error("ASTRA_EMU_MUSICA_SOURCE_BOUNDS", "entry offset overflowed"))?;
        let mut encrypted =
            read_source_range(archive, encrypted_offset, encrypted_end - encrypted_start)?;
        xor_byte(&mut encrypted, archive.xor_key);
        let mut decoded = self.decryptor.decrypt_entry_chunk_blowfish(
            archive.version,
            &entry.descriptor,
            encrypted_start,
            encrypted,
        )?;
        if entry.descriptor.archive_role != "mov" {
            self.decryptor.apply_entry_rc4_stateless(
                archive.version,
                archive.rc4_skip_crc,
                &entry.descriptor,
                encrypted_start,
                &mut decoded,
            )?;
        }
        let stored_end = entry.descriptor.stored_size.min(decoded.len() as u64);
        let local_start = offset.saturating_sub(encrypted_start);
        let local_end = end.saturating_sub(encrypted_start).min(stored_end);
        if local_end < local_start || local_end > decoded.len() as u64 {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                "raw entry range exceeds the stored payload",
            ));
        }
        let output = decoded[local_start as usize..local_end as usize].to_vec();
        if output.len() as u64 != length {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                "raw entry range transform returned a short payload",
            ));
        }
        Ok(Some(output))
    }
}

fn insert_mounted_entry(
    entries: &mut BTreeMap<String, MountedEntry>,
    folded_entries: &mut BTreeMap<String, String>,
    entry_ids: &mut BTreeSet<String>,
    entry: MountedEntry,
) -> Result<(), PazError> {
    if !entry_ids.insert(entry.descriptor.entry_id.clone()) || entries.contains_key(&entry.uri) {
        return Err(error(
            "ASTRA_EMU_MUSICA_ENTRY_DUPLICATE",
            "PAZ set contains a duplicate URI or entry id",
        ));
    }
    let folded = fold_entry_uri(&entry.uri);
    if folded_entries.contains_key(&folded) {
        return Err(error(
            "ASTRA_EMU_MUSICA_ENTRY_CASE_CONFLICT",
            "PAZ set contains URIs that collide under Windows case folding",
        ));
    }
    folded_entries.insert(folded, entry.uri.clone());
    entries.insert(entry.uri.clone(), entry);
    Ok(())
}

fn fold_entry_uri(uri: &str) -> String {
    uri.to_ascii_lowercase()
}

impl LegacyMountedVfs for MusicaMountedVfs {
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
                    "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                    "archive content changed after mount",
                ));
            }
        }
        Ok(())
    }

    fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, PazError> {
        validate_legacy_vfs_directory_uri(&self.prefix, uri)?;
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
                archive_role: None,
                method: None,
            });
        }
        let entry = self.entry(uri)?;
        Ok(LegacyVfsStat {
            uri: entry.uri.clone(),
            entry_id: Some(entry.descriptor.entry_id.clone()),
            kind: LegacyVfsNodeKind::File,
            size: entry.descriptor.unpacked_size,
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
        if end > entry.descriptor.unpacked_size {
            return Err(error(
                "ASTRA_EMU_VFS_READ_BOUNDS",
                "range read is outside the entry",
            ));
        }
        if length == 0 {
            return Ok(LegacyVfsReadResult {
                uri: entry.uri.clone(),
                offset,
                bytes: Vec::<u8>::new().into(),
                eof: end == entry.descriptor.unpacked_size,
            });
        }
        let bytes = if let Some(bytes) = self.read_raw_range(entry, offset, length)? {
            bytes
        } else {
            let mut stream = self.stream_for(entry)?;
            discard_stream(&mut stream, offset)?;
            let mut bytes = vec![0u8; length as usize];
            stream.read_exact(&mut bytes).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_ENTRY_SHORT",
                    "decoded entry stream returned a short read",
                )
            })?;
            if end == entry.descriptor.unpacked_size {
                let mut probe = [0u8; 1];
                if stream.read(&mut probe).map_err(|_| {
                    error(
                        "ASTRA_EMU_MUSICA_ENTRY_READ",
                        "decoded entry stream failed at EOF",
                    )
                })? != 0
                {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                        "decoded entry exceeds its index size",
                    ));
                }
            }
            bytes
        };
        Ok(LegacyVfsReadResult {
            uri: entry.uri.clone(),
            offset,
            bytes: bytes.into(),
            eof: end == entry.descriptor.unpacked_size,
        })
    }

    fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, PazError> {
        Ok(Box::new(self.stream_for(self.entry(uri)?)?))
    }
}

enum MusicaDecodedInner {
    Raw(MusicaEntryStream),
    Zlib(ZlibDecoder<MusicaEntryStream>),
}

struct MusicaDecodedStream {
    inner: MusicaDecodedInner,
    remaining: u64,
    eof_checked: bool,
}

impl Read for MusicaDecodedStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            if self.eof_checked {
                return Ok(0);
            }
            let mut padding = [0u8; 17];
            let mut padding_len = 0usize;
            loop {
                let count = match &mut self.inner {
                    MusicaDecodedInner::Raw(stream) => stream.read(&mut padding[padding_len..]),
                    MusicaDecodedInner::Zlib(stream) => stream.read(&mut padding[padding_len..]),
                }?;
                if count == 0 {
                    self.eof_checked = true;
                    if padding[..padding_len].iter().all(|byte| *byte == 0) {
                        return Ok(0);
                    }
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                    ));
                }
                padding_len += count;
                if padding_len == padding.len() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                    ));
                }
            }
        }
        let limit = usize::try_from(self.remaining.min(buffer.len() as u64))
            .map_err(|_| std::io::Error::other("ASTRA_EMU_MUSICA_ENTRY_SIZE"))?;
        let count = match &mut self.inner {
            MusicaDecodedInner::Raw(stream) => stream.read(&mut buffer[..limit]),
            MusicaDecodedInner::Zlib(stream) => stream.read(&mut buffer[..limit]),
        }?;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "ASTRA_EMU_MUSICA_ENTRY_SHORT",
            ));
        }
        self.remaining -= count as u64;
        Ok(count)
    }
}

struct MusicaEntryStream {
    archive: ArchiveSource,
    entry: PazEntryDescriptor,
    decryptor: Arc<MusicaPazDecryptor>,
    encrypted_position: u64,
    pending: Vec<u8>,
    pending_position: usize,
    /// RC4 keyed once per entry and advanced continuously across chunks.
    /// Random access still goes through the stateless
    /// `decrypt_entry_chunk` (which re-keys and drops keystream bytes);
    /// the sequential path must not pay that quadratic cost per chunk.
    rc4: Option<(Rc4, u64)>,
}

impl Read for MusicaEntryStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut written = 0usize;
        while written < buffer.len() {
            if self.pending_position < self.pending.len() {
                let count =
                    (self.pending.len() - self.pending_position).min(buffer.len() - written);
                buffer[written..written + count].copy_from_slice(
                    &self.pending[self.pending_position..self.pending_position + count],
                );
                self.pending_position += count;
                written += count;
                continue;
            }
            if self.encrypted_position >= self.entry.aligned_size {
                break;
            }
            self.fill_pending().map_err(to_io_error)?;
            if self.pending.is_empty() {
                break;
            }
        }
        Ok(written)
    }
}

impl MusicaEntryStream {
    /// RC4 keyed once at entry position zero, then advanced continuously.
    /// The seed material and the v2 CRC skip are identical to the stateless
    /// `decrypt_entry_chunk` path.
    fn apply_stream_rc4(&mut self, rc4_skip_crc: bool, bytes: &mut [u8]) -> Result<(), PazError> {
        if let Some((cipher, consumed)) = self.rc4.as_mut() {
            if *consumed == self.encrypted_position {
                cipher.apply_keystream(bytes);
                *consumed += bytes.len() as u64;
                return Ok(());
            }
        }
        let scheme = self.decryptor.scheme(&self.entry.archive_role)?;
        if let Some(password) = password_for_entry(&self.entry, scheme) {
            let key = entry_key_material_with_locale(
                &self.entry,
                Some(password),
                self.decryptor.locale_hook(),
            )?;
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RC4_KEY",
                    "entry RC4 key length is invalid",
                )
            })?;
            let version_skip = if self.archive.version >= 2 && rc4_skip_crc {
                (crc32(&key) >> 12 & 0xff) as u64
            } else {
                0
            };
            let mut remaining = version_skip + self.encrypted_position;
            let mut discarded = [0u8; 64 * 1024];
            while remaining != 0 {
                let count =
                    usize::try_from(remaining.min(discarded.len() as u64)).map_err(|_| {
                        error("ASTRA_EMU_MUSICA_RC4_SKIP", "entry RC4 skip is too large")
                    })?;
                cipher.apply_keystream(&mut discarded[..count]);
                discarded[..count].fill(0);
                remaining -= count as u64;
            }
            cipher.apply_keystream(bytes);
            self.rc4 = Some((cipher, self.encrypted_position + bytes.len() as u64));
            return Ok(());
        }
        Ok(())
    }

    fn fill_pending(&mut self) -> Result<(), PazError> {
        let remaining = self.entry.aligned_size - self.encrypted_position;
        let mut count = remaining.min(STREAM_CHUNK_BYTES);
        if self.entry.archive_role != "mov" {
            count &= !7;
        }
        if count == 0 {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_ALIGNMENT",
                "entry stream lost block alignment",
            ));
        }
        let offset = self
            .entry
            .offset
            .checked_add(self.encrypted_position)
            .ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_SOURCE_BOUNDS",
                    "entry stream offset overflowed",
                )
            })?;
        let mut encrypted = read_source_range(&self.archive, offset, count)?;
        xor_byte(&mut encrypted, self.archive.xor_key);
        let mut decoded = self.decryptor.decrypt_entry_chunk_blowfish(
            self.archive.version,
            &self.entry,
            self.encrypted_position,
            encrypted,
        )?;
        if !self.entry.archive_role.eq_ignore_ascii_case("mov") {
            self.apply_stream_rc4(self.archive.rc4_skip_crc, &mut decoded)?;
        }
        let output_end = self
            .entry
            .stored_size
            .min(self.encrypted_position + decoded.len() as u64);
        let output_len = output_end.saturating_sub(self.encrypted_position) as usize;
        decoded.truncate(output_len);
        self.pending = decoded;
        self.pending_position = 0;
        self.encrypted_position += count;
        Ok(())
    }
}

fn discard_stream(stream: &mut MusicaDecodedStream, mut count: u64) -> Result<(), PazError> {
    let mut scratch = [0u8; 64 * 1024];
    while count != 0 {
        let wanted = (count as usize).min(scratch.len());
        let read = stream.read(&mut scratch[..wanted]).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_ENTRY_READ",
                "decoded entry stream failed while seeking",
            )
        })?;
        if read == 0 {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_SHORT",
                "decoded entry stream ended before the requested range",
            ));
        }
        count -= read as u64;
    }
    Ok(())
}

fn to_io_error(error: PazError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

fn validate_role_set(configs: &[PazArchiveConfig]) -> Result<(), PazError> {
    let mut roles = BTreeSet::new();
    for config in configs {
        if !REQUIRED_ARCHIVE_ROLES.contains(&config.role.as_str())
            || !roles.insert(config.role.as_str())
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_ARCHIVE_ROLE",
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
                "ASTRA_EMU_MUSICA_ARCHIVE_NAME",
                "archive file name does not match its role",
            ));
        }
    }
    if REQUIRED_ARCHIVE_ROLES
        .iter()
        .any(|role| !roles.contains(role))
    {
        return Err(error(
            "ASTRA_EMU_MUSICA_ARCHIVE_MISSING",
            "all required PAZ roles must be supplied",
        ));
    }
    Ok(())
}

fn parse_archive_index(
    source: &mut ArchiveSource,
    expected_index_xor: u32,
    decryptor: &MusicaPazDecryptor,
    locale_hook: MusicaLocaleHook,
) -> Result<Vec<PazEntryDescriptor>, PazError> {
    let (index_offset, encrypted_size): (u64, u64) = if source.version == 0 {
        let raw = read_source_range(source, 0, 4)?;
        (4, u32::from_le_bytes(raw.try_into().unwrap()) as u64)
    } else {
        let raw = read_source_range(source, 0x20, 4)?;
        let raw_size = u32::from_le_bytes(raw.try_into().unwrap());
        source.xor_key = (raw_size >> 24) as u8;
        let derived = u32::from_le_bytes([source.xor_key; 4]);
        if expected_index_xor != 0 && expected_index_xor != derived {
            return Err(error(
                "ASTRA_EMU_MUSICA_INDEX_XOR",
                "configured index XOR does not match the archive header",
            ));
        }
        (0x24, (raw_size ^ derived) as u64)
    };
    if encrypted_size == 0
        || encrypted_size > MAX_INDEX_BYTES
        || !encrypted_size.is_multiple_of(8)
        || index_offset
            .checked_add(encrypted_size)
            .is_none_or(|end| end > source.length)
    {
        return Err(error(
            "ASTRA_EMU_MUSICA_INDEX_SIZE",
            "PAZ index size is invalid or out of bounds",
        ));
    }
    let mut encrypted = read_source_range(source, index_offset, encrypted_size)?;
    xor_byte(&mut encrypted, source.xor_key);
    let decoded = decryptor.decrypt_index(&source.role, &encrypted)?;
    let mut cursor = Cursor::new(decoded.as_slice());
    let count = read_u32(&mut cursor)? as usize;
    if count > 1_000_000 {
        return Err(error(
            "ASTRA_EMU_MUSICA_ENTRY_COUNT",
            "PAZ entry count exceeds the configured limit",
        ));
    }
    let video_key = if source.role == "mov" {
        let mut key = vec![0u8; 256];
        cursor
            .read_exact(&mut key)
            .map_err(|_| error("ASTRA_EMU_MUSICA_VIDEO_KEY", "PAZ movie key is truncated"))?;
        Some(key)
    } else {
        None
    };
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let (name, crypto_name) = read_c_string(&mut cursor, locale_hook)?;
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
            tracing::error!(event = "astra_emu_musica_entry_bounds_invalid", archive_role = %source.role, entry_index = index);
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_BOUNDS",
                "PAZ entry descriptor is invalid or out of bounds",
            ));
        }
        entries.push(PazEntryDescriptor {
            archive_role: source.role.clone(),
            entry_id: format!("{}:{index}", source.role),
            name,
            crypto_name,
            offset,
            unpacked_size,
            stored_size,
            aligned_size,
            packed,
            video_key: video_key.clone(),
        });
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
            "ASTRA_EMU_MUSICA_ENTRY_PATH",
            "PAZ entry name is absolute or traverses",
        ));
    }
    Ok(normalized)
}

fn read_c_string(
    cursor: &mut Cursor<&[u8]>,
    locale_hook: MusicaLocaleHook,
) -> Result<(String, Vec<u8>), PazError> {
    let start = cursor.position() as usize;
    let bytes = cursor.get_ref();
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| start + relative)
        .ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_INDEX_STRING",
                "PAZ entry name is not terminated",
            )
        })?;
    if end - start > 4096 {
        return Err(error(
            "ASTRA_EMU_MUSICA_INDEX_STRING",
            "PAZ entry name exceeds the configured limit",
        ));
    }
    let crypto_name = bytes[start..end].to_vec();
    let text = locale_hook.decode(&crypto_name).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_INDEX_ENCODING",
            "PAZ entry name is not valid CP932",
        )
    })?;
    cursor.set_position((end + 1) as u64);
    Ok((text, crypto_name))
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, PazError> {
    let mut bytes = [0; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| error("ASTRA_EMU_MUSICA_INDEX_SHORT", "PAZ index is truncated"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32(cursor: &mut Cursor<&[u8]>) -> Result<i32, PazError> {
    Ok(read_u32(cursor)? as i32)
}

fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, PazError> {
    let mut bytes = [0; 8];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| error("ASTRA_EMU_MUSICA_INDEX_SHORT", "PAZ index is truncated"))?;
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
            "ASTRA_EMU_MUSICA_SOURCE_BOUNDS",
            "archive range is out of bounds",
        ));
    }
    let mut file = File::open(path).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_ARCHIVE_OPEN",
            "PAZ archive cannot be opened",
        )
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| error("ASTRA_EMU_MUSICA_ARCHIVE_SEEK", "PAZ archive seek failed"))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_ARCHIVE_SHORT_READ",
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
            "ASTRA_EMU_MUSICA_SOURCE_BOUNDS",
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
        output.extend_from_slice(&read_exact_range(&part.path, local, count, part.length)?);
        remaining -= count;
        start += count;
        logical = part_end;
        if remaining == 0 {
            break;
        }
    }
    if remaining != 0 {
        return Err(error(
            "ASTRA_EMU_MUSICA_ARCHIVE_SHORT_READ",
            "multipart PAZ returned a short read",
        ));
    }
    Ok(output)
}

fn hash_parts(parts: &[ArchivePart]) -> Result<Hash256, PazError> {
    let mut source_hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    for part in parts {
        let file = File::open(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_ARCHIVE_OPEN",
                "PAZ archive cannot be opened",
            )
        })?;
        let mut file = file.take(part.length.checked_add(1).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_ARCHIVE_SIZE",
                "archive part bound overflowed",
            )
        })?);
        let mut part_bytes = 0u64;
        loop {
            let count = file.read(&mut buffer).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_ARCHIVE_READ",
                    "PAZ archive hash read failed",
                )
            })?;
            if count == 0 {
                break;
            }
            part_bytes += count as u64;
            source_hasher.update(&buffer[..count]);
        }
        if part_bytes != part.length {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part size changed while hashing",
            ));
        }
    }
    for part in parts {
        let metadata = fs::metadata(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part disappeared while hashing",
            )
        })?;
        if metadata.len() != part.length || metadata.modified().ok() != part.modified {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive metadata changed while hashing",
            ));
        }
    }
    Ok(Hash256::from_bytes(source_hasher.finalize().into()))
}

fn verify_source_unchanged(source: &ArchiveSource) -> Result<(), PazError> {
    for part in &source.parts {
        let metadata = fs::metadata(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part disappeared after mount",
            )
        })?;
        if metadata.len() != part.length || metadata.modified().ok() != part.modified {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive metadata changed after mount",
            ));
        }
    }
    Ok(())
}

fn discover_parts(base: &Path, game_root: &Path) -> Result<Vec<ArchivePart>, PazError> {
    let game_root = game_root
        .canonicalize()
        .map_err(|_| error("ASTRA_EMU_MUSICA_GAME_ROOT", "game root cannot be resolved"))?;
    let base = base.canonicalize().map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_ARCHIVE_OPEN",
            "required PAZ archive cannot be resolved",
        )
    })?;
    if !base.starts_with(&game_root) {
        return Err(error(
            "ASTRA_EMU_MUSICA_ARCHIVE_PATH",
            "PAZ archive resolves outside game root",
        ));
    }
    let metadata = fs::metadata(&base).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_ARCHIVE_OPEN",
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
        match fs::metadata(&path) {
            Ok(metadata) => {
                if missing_seen {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_MULTIPART_GAP",
                        "PAZ multipart suffixes must be contiguous",
                    ));
                }
                if metadata.len() == 0 {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_MULTIPART_EMPTY",
                        "PAZ multipart volume is empty",
                    ));
                }
                let path = path.canonicalize().map_err(|_| {
                    error(
                        "ASTRA_EMU_MUSICA_MULTIPART_OPEN",
                        "PAZ multipart volume cannot be resolved",
                    )
                })?;
                if !path.starts_with(&game_root) {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_ARCHIVE_PATH",
                        "PAZ multipart volume resolves outside game root",
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
                    "ASTRA_EMU_MUSICA_MULTIPART_OPEN",
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
        "png" | "jpg" | "bmp" | "ani" | "sqz" => "image",
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

fn entry_key_material_with_locale(
    entry: &PazEntryDescriptor,
    password: Option<&str>,
    locale_hook: MusicaLocaleHook,
) -> Result<Vec<u8>, PazError> {
    let lowered_name = entry.name.to_lowercase();
    let mut key = locale_hook.encode(&lowered_name).map_err(|_| {
        error(
            "ASTRA_EMU_MUSICA_RC4_KEY",
            "entry name cannot be encoded as CP932",
        )
    })?;
    key.extend_from_slice(format!(" {:08X} ", entry.unpacked_size).as_bytes());
    if let Some(password) = password {
        key.extend_from_slice(&locale_hook.encode(password).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_RC4_KEY",
                "entry password cannot be encoded as CP932",
            )
        })?);
    }
    if key.is_empty() {
        return Err(error("ASTRA_EMU_MUSICA_RC4_KEY", "entry RC4 key is empty"));
    }
    Ok(key)
}

fn validate_blowfish_key(key: &[u8]) -> Result<(), PazError> {
    if !(4..=56).contains(&key.len()) {
        return Err(error(
            "ASTRA_EMU_MUSICA_BLOWFISH_KEY",
            "Blowfish key length must be between 4 and 56 bytes",
        ));
    }
    Ok(())
}

fn blowfish_decrypt_in_place(key: &[u8], bytes: &mut [u8]) -> Result<(), PazError> {
    validate_blowfish_key(key)?;
    if !bytes.len().is_multiple_of(8) {
        return Err(error(
            "ASTRA_EMU_MUSICA_BLOWFISH_ALIGNMENT",
            "Blowfish input is not block aligned",
        ));
    }
    let cipher: Blowfish = Blowfish::new_from_slice(key)
        .map_err(|_| error("ASTRA_EMU_MUSICA_BLOWFISH_KEY", "Blowfish key is invalid"))?;
    for chunk in bytes.as_chunks_mut::<8>().0.iter_mut() {
        chunk[..4].reverse();
        chunk[4..].reverse();
        cipher.decrypt_block((&mut *chunk).into());
        chunk[..4].reverse();
        chunk[4..].reverse();
    }
    Ok(())
}

fn xor_byte(bytes: &mut [u8], key: u8) {
    if key != 0 {
        for byte in bytes {
            *byte ^= key;
        }
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

fn error(code: &'static str, message: impl Into<String>) -> PazError {
    PazError::invalid(code, message)
}

#[cfg(test)]
#[path = "paz_stream_tests.rs"]
mod stream_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_key_uses_lowercase_cp932_name_and_size() {
        let entry = PazEntryDescriptor {
            archive_role: "scr".into(),
            entry_id: "scr:0".into(),
            name: "SCRIPT.SC".into(),
            crypto_name: b"SCRIPT.SC".to_vec(),
            offset: 0,
            unpacked_size: 0x12,
            stored_size: 8,
            aligned_size: 8,
            packed: false,
            video_key: None,
        };
        assert_eq!(
            entry_key_material_with_locale(&entry, Some("pw"), MusicaLocaleHook::japanese_cp932())
                .unwrap(),
            b"script.sc 00000012 pw"
        );
    }

    #[test]
    fn movie_data_key_is_rejected() {
        let mut roles = BTreeMap::new();
        for role in REQUIRED_ARCHIVE_ROLES {
            roles.insert(
                role.to_owned(),
                PazRoleScheme {
                    index_key: b"index-key".to_vec(),
                    data_key: if role == "mov" {
                        b"bad".to_vec()
                    } else {
                        b"data-key".to_vec()
                    },
                    type_passwords: BTreeMap::new(),
                },
            );
        }
        assert_eq!(
            MusicaPazDecryptor::new_with_locale(roles, MusicaLocaleHook::japanese_cp932())
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MUSICA_MOVIE_KEY"
        );
    }

    #[test]
    fn traversal_is_blocking() {
        assert_eq!(
            normalize_entry_name("../secret").unwrap_err().code(),
            "ASTRA_EMU_MUSICA_ENTRY_PATH"
        );
    }
}
