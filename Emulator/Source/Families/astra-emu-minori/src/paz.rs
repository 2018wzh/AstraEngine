use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use astra_core::Hash256;
use astra_emu_family_api::{
    validate_legacy_vfs_uri, LegacyMountedVfs, LegacyPackManifest, LegacyProviderError,
    LegacyVfsEntry, LegacyVfsNode, LegacyVfsNodeKind, LegacyVfsReadResult, LegacyVfsStat,
    LegacyVfsStream, LEGACY_VFS_MAX_READ_BYTES,
};
use blowfish::cipher::{BlockCipherDecrypt, KeyInit};
use blowfish::Blowfish;
use encoding_rs::SHIFT_JIS;
use flate2::read::ZlibDecoder;
use rc4::{Rc4, StreamCipher};
use sha2::{Digest, Sha256};

use crate::{CacheIdentity, PlaintextCache, MINORI_READER_ID};

pub const REQUIRED_ARCHIVE_ROLES: [&str; 6] = ["scr", "st", "sys", "se", "voice", "mov"];
pub const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PazArchiveConfig {
    pub role: String,
    pub path: PathBuf,
    pub version: u8,
    pub index_size_xor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub trait MinoriDecodeService: Send + Sync {
    fn decoder_id(&self) -> &str;
    fn patch_hash(&self) -> Hash256;
    fn decode_index(
        &self,
        role: &str,
        version: u8,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, LegacyProviderError>;
    fn decode_entry(
        &self,
        version: u8,
        entry: &PazEntryDescriptor,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, LegacyProviderError>;
}

#[derive(Debug, Clone)]
pub struct PazRoleScheme {
    pub index_key: Vec<u8>,
    pub data_key: Vec<u8>,
    pub type_password: Option<String>,
    pub archive_xor: Option<u32>,
    pub video_key: Option<[u8; 256]>,
}

/// Native implementation of the GARbro PAZ algorithm. Key material is deliberately
/// non-serializable and must only be constructed inside a trusted mount session.
pub struct NativePazDecoder {
    decoder_id: String,
    patch_hash: Hash256,
    roles: BTreeMap<String, PazRoleScheme>,
}

impl NativePazDecoder {
    pub fn new(
        decoder_id: impl Into<String>,
        patch_hash: Hash256,
        roles: BTreeMap<String, PazRoleScheme>,
    ) -> Result<Self, LegacyProviderError> {
        let decoder_id = decoder_id.into();
        if decoder_id.is_empty()
            || roles
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
            validate_blowfish_key(&scheme.data_key)?;
        }
        Ok(Self {
            decoder_id,
            patch_hash,
            roles,
        })
    }

    fn scheme(&self, role: &str) -> Result<&PazRoleScheme, LegacyProviderError> {
        self.roles.get(role).ok_or_else(|| {
            error(
                "ASTRA_EMU_MINORI_DECODER_ROLE",
                "decoder has no scheme for the archive role",
            )
        })
    }
}

impl MinoriDecodeService for NativePazDecoder {
    fn decoder_id(&self) -> &str {
        &self.decoder_id
    }
    fn patch_hash(&self) -> Hash256 {
        self.patch_hash
    }

    fn decode_index(
        &self,
        role: &str,
        _version: u8,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, LegacyProviderError> {
        blowfish_decrypt(self.scheme(role)?.index_key.as_slice(), encrypted)
    }

    fn decode_entry(
        &self,
        version: u8,
        entry: &PazEntryDescriptor,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, LegacyProviderError> {
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
            let block_len = bytes.len().min(0x10000);
            if block_len == 0 {
                return Ok(bytes);
            }
            let mut block = vec![0; block_len];
            cipher.apply_keystream(&mut block);
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte ^= block[index % block.len()];
            }
            return Ok(bytes);
        }
        bytes = blowfish_decrypt(&scheme.data_key, &bytes)?;
        if version > 0 && scheme.type_password.is_some() {
            let password = scheme.type_password.as_deref().unwrap_or_default();
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
            if version >= 2 {
                let skip = ((crc32fast::hash(&key) >> 12) & 0xff) as usize;
                let mut discarded = vec![0; skip];
                cipher.apply_keystream(&mut discarded);
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
    decoder: Arc<dyn MinoriDecodeService>,
    cache: Option<PlaintextCache>,
}

impl MinoriMountedVfs {
    pub fn mount(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decoder: Arc<dyn MinoriDecodeService>,
    ) -> Result<Self, LegacyProviderError> {
        Self::mount_with_cache(mount_id, prefix, configs, decoder, None)
    }

    pub fn mount_with_cache(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decoder: Arc<dyn MinoriDecodeService>,
        cache: Option<PlaintextCache>,
    ) -> Result<Self, LegacyProviderError> {
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
            let parts = discover_parts(&config.path)?;
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
            let parsed = parse_archive_index(&mut source, config.index_size_xor, decoder.as_ref())?;
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
                offset: entry.descriptor.offset,
                size: entry.descriptor.unpacked_size,
                content_hash: entry.encrypted_hash,
                media_kind: media_kind(&entry.descriptor.name).into(),
            })
            .collect();
        let manifest = LegacyPackManifest {
            mount_id: mount_id.clone(),
            prefix: prefix.clone(),
            reader_id: MINORI_READER_ID.into(),
            reader_hash,
            entries: manifest_entries,
        };
        Ok(Self {
            mount_id,
            prefix,
            manifest,
            archives,
            entries,
            decoder,
            cache,
        })
    }

    fn decoded_entry(&self, entry: &MountedEntry) -> Result<(Vec<u8>, bool), LegacyProviderError> {
        let archive = &self.archives[entry.archive];
        verify_source_unchanged(archive)?;
        let identity = CacheIdentity {
            archive_hash: archive.hash,
            entry_id: entry.descriptor.entry_id.clone(),
            patch_hash: self.decoder.patch_hash(),
            decoder_id: self.decoder.decoder_id().into(),
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
        let mut decoded =
            self.decoder
                .decode_entry(archive.version, &entry.descriptor, &encrypted)?;
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

    fn entry(&self, uri: &str) -> Result<&MountedEntry, LegacyProviderError> {
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

    fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyProviderError> {
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

    fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyProviderError> {
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
            content_hash: Some(entry.encrypted_hash),
            archive_role: Some(entry.descriptor.archive_role.clone()),
            method: Some(
                if entry.descriptor.packed {
                    "blowfish+rc4+zlib"
                } else {
                    "blowfish+rc4"
                }
                .into(),
            ),
        })
    }

    fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<LegacyVfsReadResult, LegacyProviderError> {
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

    fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, LegacyProviderError> {
        Ok(Box::new(Cursor::new(
            self.decoded_entry(self.entry(uri)?)?.0,
        )))
    }
}

fn validate_role_set(configs: &[PazArchiveConfig]) -> Result<(), LegacyProviderError> {
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
    decoder: &dyn MinoriDecodeService,
) -> Result<Vec<PazEntryDescriptor>, LegacyProviderError> {
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
    let decoded = decoder.decode_index(&source.role, source.version, &encrypted)?;
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

fn normalize_entry_name(name: &str) -> Result<String, LegacyProviderError> {
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

fn read_c_string(cursor: &mut Cursor<&[u8]>) -> Result<String, LegacyProviderError> {
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

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, LegacyProviderError> {
    let mut bytes = [0; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| error("ASTRA_EMU_MINORI_INDEX_SHORT", "PAZ index is truncated"))?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_i32(cursor: &mut Cursor<&[u8]>) -> Result<i32, LegacyProviderError> {
    Ok(read_u32(cursor)? as i32)
}
fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, LegacyProviderError> {
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
) -> Result<Vec<u8>, LegacyProviderError> {
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
) -> Result<Vec<u8>, LegacyProviderError> {
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

fn hash_parts(parts: &[ArchivePart]) -> Result<Hash256, LegacyProviderError> {
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

fn verify_source_unchanged(source: &ArchiveSource) -> Result<(), LegacyProviderError> {
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

fn discover_parts(base: &Path) -> Result<Vec<ArchivePart>, LegacyProviderError> {
    let metadata = std::fs::metadata(base).map_err(|_| {
        error(
            "ASTRA_EMU_MINORI_ARCHIVE_OPEN",
            "required PAZ archive cannot be opened",
        )
    })?;
    let mut parts = vec![ArchivePart {
        path: base.to_owned(),
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
fn error(code: &'static str, message: impl Into<String>) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

fn validate_blowfish_key(key: &[u8]) -> Result<(), LegacyProviderError> {
    if !(4..=56).contains(&key.len()) {
        return Err(error(
            "ASTRA_EMU_MINORI_BLOWFISH_KEY",
            "Blowfish key length must be between 4 and 56 bytes",
        ));
    }
    Ok(())
}

fn blowfish_decrypt(key: &[u8], encrypted: &[u8]) -> Result<Vec<u8>, LegacyProviderError> {
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
    use std::{fs, io::Write};

    struct PassthroughDecoder;

    impl MinoriDecodeService for PassthroughDecoder {
        fn decoder_id(&self) -> &str {
            "fixture"
        }

        fn patch_hash(&self) -> Hash256 {
            Hash256::from_sha256(b"fixture-patch")
        }

        fn decode_index(
            &self,
            _role: &str,
            _version: u8,
            encrypted: &[u8],
        ) -> Result<Vec<u8>, LegacyProviderError> {
            Ok(encrypted.to_vec())
        }

        fn decode_entry(
            &self,
            _version: u8,
            _entry: &PazEntryDescriptor,
            encrypted: &[u8],
        ) -> Result<Vec<u8>, LegacyProviderError> {
            Ok(encrypted.to_vec())
        }
    }

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

        let mut archive = Vec::new();
        if version == 0 {
            archive.extend_from_slice(&(index.len() as u32).to_le_bytes());
            archive.extend_from_slice(&index);
            archive.extend_from_slice(payload);
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
                version,
                index_size_xor: if version == 0 { 0 } else { 0x5a5a5a5a },
            })
            .collect();
        MinoriMountedVfs::mount("fixture", "minori:/", configs, Arc::new(PassthroughDecoder))
            .unwrap()
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
