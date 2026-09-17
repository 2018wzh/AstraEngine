use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use crate::archive::{
    validate_archive_directory_uri, validate_archive_uri, validate_decrypt_output,
    validate_decrypt_request, ArchiveEntry, ArchiveManifest, ArchiveNode, ArchiveNodeKind,
    ArchiveReadResult, ArchiveSource, ArchiveStat, ArchiveStream, PazDecryptDescriptor,
    PazDecryptPhase, PazDecryptRequest, PazDecryptTransport, ARCHIVE_MANIFEST_SCHEMA,
    ARCHIVE_MAX_READ_BYTES, PAZ_DECRYPT_CHUNK_BYTES, PAZ_DECRYPT_MAX_BATCH_BYTES,
};
use astra_byte_source::OwnedByteBuffer;
use astra_core::Hash256;
use blowfish::cipher::{BlockCipherDecrypt, KeyInit};
use blowfish::Blowfish;
use encoding_rs::SHIFT_JIS;
use flate2::read::ZlibDecoder;
use rc4::{Rc4, StreamCipher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::archive::{CacheIdentity, PlaintextCache, PlaintextCacheError};

use crate::{MUSICA_DECRYPT_DESCRIPTOR_SCHEMA, MUSICA_DECRYPT_PROVIDER_ID, MUSICA_READER_ID};

type PazError = crate::CoreError;

pub const REQUIRED_ARCHIVE_ROLES: [&str; 8] =
    ["bg", "bgm", "scr", "st", "sys", "se", "voice", "mov"];
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
pub struct ArchiveEntryDescriptor {
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

/// The only production decrypt provider for Musica PAZ archives.
/// Key material remains process-local and is never serializable.
pub struct MusicaPazDecryptProvider {
    private_profile_hash: Hash256,
    roles: BTreeMap<String, PazRoleScheme>,
}

impl MusicaPazDecryptProvider {
    pub fn new(
        private_profile_hash: Hash256,
        roles: BTreeMap<String, PazRoleScheme>,
    ) -> Result<Self, PazError> {
        if roles
            .keys()
            .any(|role| !REQUIRED_ARCHIVE_ROLES.contains(&role.as_str()))
        {
            return Err(error(
                "ASTRA_EMU_MUSICA_DECODER_CONFIG",
                "decoder id or archive role is invalid",
            ));
        }
        for role in REQUIRED_ARCHIVE_ROLES {
            let scheme = roles.get(role).ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_DECODER_ROLE",
                    "decoder is missing a required archive role",
                )
            })?;
            validate_blowfish_key(&scheme.index_key)?;
            if role != "mov" {
                validate_blowfish_key(&scheme.data_key)?;
            } else if scheme.data_key.len() > 56 {
                return Err(error(
                    "ASTRA_EMU_MUSICA_BLOWFISH_KEY",
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
                "ASTRA_EMU_MUSICA_DECODER_ROLE",
                "decoder has no scheme for the archive role",
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum MusicaDecryptDescriptor {
    Index {
        role: String,
        version: u8,
        stream_offset: u64,
    },
    Entry {
        version: u8,
        entry: ArchiveEntryDescriptor,
        stream_offset: u64,
    },
}

impl MusicaPazDecryptProvider {
    pub fn provider_id(&self) -> &str {
        MUSICA_DECRYPT_PROVIDER_ID
    }
    pub fn private_profile_hash(&self) -> Hash256 {
        self.private_profile_hash
    }
    pub fn descriptor_schema_id(&self) -> &str {
        MUSICA_DECRYPT_DESCRIPTOR_SCHEMA
    }
    pub fn descriptor_schema_hash(&self) -> Hash256 {
        Hash256::from_sha256(MUSICA_DECRYPT_DESCRIPTOR_SCHEMA.as_bytes())
    }

    pub fn decrypt(&self, request: PazDecryptRequest<'_>) -> Result<Vec<u8>, PazError> {
        validate_decrypt_request(self, &request)?;
        if request.descriptors.len() != 1 {
            return Err(error(
                "ASTRA_EMU_MUSICA_DESCRIPTOR_BATCH",
                "Musica decrypt batches require exactly one descriptor",
            ));
        }
        let descriptor: MusicaDecryptDescriptor =
            serde_json::from_slice(&request.descriptors[0].payload).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_DESCRIPTOR",
                    "Musica decrypt descriptor is invalid",
                )
            })?;
        if !matches!(
            (&request.phase, &descriptor),
            (
                PazDecryptPhase::Index,
                MusicaDecryptDescriptor::Index { .. }
            ) | (
                PazDecryptPhase::Entry,
                MusicaDecryptDescriptor::Entry { .. }
            )
        ) {
            return Err(error(
                "ASTRA_EMU_MUSICA_DESCRIPTOR_PHASE",
                "Musica decrypt descriptor phase does not match the request",
            ));
        }
        let absolute_offset = match &descriptor {
            MusicaDecryptDescriptor::Index { stream_offset, .. }
            | MusicaDecryptDescriptor::Entry { stream_offset, .. } => stream_offset
                .checked_add(request.transport.chunk_offset)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_DECRYPT_OFFSET",
                        "decrypt stream offset overflowed",
                    )
                })?,
        };
        let output = match descriptor {
            MusicaDecryptDescriptor::Index { role, .. } => {
                blowfish_decrypt(self.scheme(&role)?.index_key.as_slice(), request.bytes)?
            }
            MusicaDecryptDescriptor::Entry { version, entry, .. } => {
                self.decrypt_entry_chunk(version, &entry, absolute_offset, request.bytes)?
            }
        };
        validate_decrypt_output(&request, &output)?;
        if output.len() != request.bytes.len() {
            return Err(error(
                "ASTRA_EMU_MUSICA_DECRYPT_SIZE",
                "Musica transform changed the chunk size",
            ));
        }
        Ok(output)
    }
}

impl MusicaPazDecryptProvider {
    fn decrypt_entry_chunk(
        &self,
        version: u8,
        entry: &ArchiveEntryDescriptor,
        absolute_offset: u64,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, PazError> {
        let scheme = self.scheme(&entry.archive_role)?;
        let mut bytes = encrypted.to_vec();
        if entry.archive_role == "mov" {
            let video_key = entry.video_key.as_ref().ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_VIDEO_KEY",
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
                    "ASTRA_EMU_MUSICA_RC4_KEY",
                    "video RC4 key cannot be encoded as CP932",
                ));
            }
            let key = (0..256)
                .map(|index| video_key[index] ^ entry_key[index % entry_key.len()])
                .collect::<Vec<_>>();
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RC4_KEY",
                    "video RC4 key length is invalid",
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
                    "ASTRA_EMU_MUSICA_RC4_KEY",
                    "entry RC4 key cannot be encoded as CP932",
                ));
            }
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "ASTRA_EMU_MUSICA_RC4_KEY",
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
                .ok_or_else(|| error("ASTRA_EMU_MUSICA_RC4_SKIP", "entry RC4 offset overflowed"))?;
            if skip > 0 {
                let mut remaining = skip;
                let mut discarded = vec![0; PAZ_DECRYPT_CHUNK_BYTES];
                while remaining > 0 {
                    let length =
                        usize::try_from(remaining.min(discarded.len() as u64)).map_err(|_| {
                            error(
                                "ASTRA_EMU_MUSICA_RC4_SKIP",
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
struct ArchiveFile {
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
    descriptor: ArchiveEntryDescriptor,
    uri: String,
    archive: usize,
    encrypted_hash: Hash256,
}

pub struct MusicaMountedVfs {
    mount_id: String,
    prefix: String,
    manifest: ArchiveManifest,
    archives: Vec<ArchiveFile>,
    entries: BTreeMap<String, MountedEntry>,
    decrypt_provider: Arc<MusicaPazDecryptProvider>,
    cache: Option<PlaintextCache>,
}

impl MusicaMountedVfs {
    pub fn mount(
        mount_id: impl Into<String>,
        prefix: impl Into<String>,
        configs: Vec<PazArchiveConfig>,
        decrypt_provider: Arc<MusicaPazDecryptProvider>,
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
        decrypt_provider: Arc<MusicaPazDecryptProvider>,
        mount_profile_hash: Hash256,
        cache: Option<PlaintextCache>,
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
        let mut entry_ids = BTreeSet::new();
        let mut prepared = Vec::with_capacity(configs.len());
        for config in configs {
            tracing::info!(
                event = "astra_emu_musica_archive_mount_started",
                archive_role = %config.role,
                version = config.version
            );
            let parts = discover_parts(&config.path, &config.game_root)?;
            let total_length = parts
                .iter()
                .try_fold(0u64, |total, part| total.checked_add(part.length))
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_MUSICA_ARCHIVE_SIZE",
                        "multipart PAZ size overflowed",
                    )
                })?;
            if parts[0].length == 0 {
                return Err(error(
                    "ASTRA_EMU_MUSICA_ARCHIVE_EMPTY",
                    format!("required archive role {} is empty", config.role),
                ));
            }
            if config.version > 2 {
                return Err(error(
                    "ASTRA_EMU_MUSICA_VERSION",
                    "only PAZ versions 0 through 2 are supported",
                ));
            }
            let mut source = ArchiveFile {
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
                event = "astra_emu_musica_archive_index_decoded",
                archive_role = %config.role,
                entry_count = parsed.len()
            );
            prepared.push((config, source, parsed));
        }
        for (config, mut source, parsed) in prepared {
            let (source_hash, encrypted_hashes) =
                hash_parts_and_entries(&source.parts, source.length, &parsed)?;
            source.hash = source_hash;
            tracing::info!(
                event = "astra_emu_musica_archive_hashed",
                archive_role = %config.role,
                archive_hash = %source.hash
            );
            let archive_index = archives.len();
            for (entry, encrypted_hash) in parsed.into_iter().zip(encrypted_hashes) {
                let uri = format!(
                    "{}{}/{}",
                    prefix,
                    config.role,
                    normalize_entry_name(&entry.name)?
                );
                validate_archive_uri(&prefix, &uri)?;
                if !entry_ids.insert(entry.entry_id.clone()) || entries.contains_key(&uri) {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_ENTRY_DUPLICATE",
                        "PAZ set contains a duplicate URI or entry id",
                    ));
                }
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
                event = "astra_emu_musica_archive_mount_completed",
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
            .map(|entry| ArchiveEntry {
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
        let manifest = ArchiveManifest {
            schema: ARCHIVE_MANIFEST_SCHEMA.into(),
            family_id: "musica".into(),
            mount_id: mount_id.clone(),
            prefix: prefix.clone(),
            reader_id: MUSICA_READER_ID.into(),
            reader_hash,
            decrypt_provider_id: decrypt_provider.provider_id().into(),
            private_profile_hash: decrypt_provider.private_profile_hash(),
            mount_profile_hash,
            sources: archives
                .iter()
                .map(|archive| ArchiveSource {
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
            family_id: "musica".into(),
            source_hash: archive.hash,
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
            .map_err(cache_error)?
            .flatten()
        {
            if bytes.len() as u64 != entry.descriptor.unpacked_size {
                return Err(error(
                    "ASTRA_EMU_MUSICA_CACHE_SIZE",
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
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive bytes changed after mount",
            ));
        }
        xor_byte(&mut encrypted, archive.xor_key);
        let mut decoded = decrypt_bytes(
            self.decrypt_provider.as_ref(),
            MusicaDecryptDescriptor::Entry {
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
                .map_err(|_| error("ASTRA_EMU_MUSICA_ZLIB", "entry zlib stream is invalid"))?;
            decoded = unpacked;
        }
        let expected_size = entry.descriptor.unpacked_size as usize;
        if decoded.len() > expected_size
            && decoded.len() - expected_size <= 16
            && decoded[expected_size..].iter().all(|byte| *byte == 0)
        {
            tracing::debug!(
                event = "astra_emu_musica_entry_zero_padding_removed",
                archive_role = %entry.descriptor.archive_role,
                entry_id = %entry.descriptor.entry_id,
                padding_size = decoded.len() - expected_size
            );
            decoded.truncate(expected_size);
        }
        if decoded.len() != expected_size {
            tracing::error!(
                event = "astra_emu_musica_entry_size_mismatch",
                archive_role = %entry.descriptor.archive_role,
                entry_id = %entry.descriptor.entry_id,
                packed = entry.descriptor.packed,
                stored_size = entry.descriptor.stored_size,
                unpacked_size = entry.descriptor.unpacked_size,
                decoded_size = decoded.len()
            );
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_SIZE",
                "decoded entry size does not match its index descriptor",
            ));
        }
        if let Some(cache) = &self.cache {
            cache.put(&identity, &decoded).map_err(cache_error)?;
        }
        Ok((decoded, false))
    }

    fn entry(&self, uri: &str) -> Result<&MountedEntry, PazError> {
        validate_archive_uri(&self.prefix, uri)?;
        self.entries
            .get(uri)
            .ok_or_else(|| error("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry was not found"))
    }
}

impl MusicaMountedVfs {
    pub fn mount_id(&self) -> &str {
        &self.mount_id
    }
    pub fn manifest(&self) -> &ArchiveManifest {
        &self.manifest
    }

    pub fn validate_sources(&self) -> Result<(), PazError> {
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

    pub fn read_dir(&self, uri: &str) -> Result<Vec<ArchiveNode>, PazError> {
        validate_archive_directory_uri(&self.prefix, uri)?;
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
                .or_insert_with(|| ArchiveNode {
                    uri: format!("{base}{name}"),
                    name: name.to_owned(),
                    kind: if directory {
                        ArchiveNodeKind::Directory
                    } else {
                        ArchiveNodeKind::File
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

    pub fn stat(&self, uri: &str) -> Result<ArchiveStat, PazError> {
        if uri == self.prefix
            || self
                .entries
                .keys()
                .any(|candidate| candidate.starts_with(&format!("{}/", uri.trim_end_matches('/'))))
        {
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: None,
                kind: ArchiveNodeKind::Directory,
                size: 0,
                archive_role: None,
                method: None,
            });
        }
        let entry = self.entry(uri)?;
        Ok(ArchiveStat {
            uri: uri.into(),
            entry_id: Some(entry.descriptor.entry_id.clone()),
            kind: ArchiveNodeKind::File,
            size: entry.descriptor.unpacked_size,
            archive_role: Some(entry.descriptor.archive_role.clone()),
            method: Some(entry_method(&entry.descriptor).into()),
        })
    }

    pub fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<ArchiveReadResult, PazError> {
        if length > ARCHIVE_MAX_READ_BYTES {
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
        let bytes = OwnedByteBuffer::from_owner(
            DecodedRange {
                bytes: decoded,
                start: offset as usize,
                end: end as usize,
            },
            DecodedRange::as_slice,
        );
        Ok(ArchiveReadResult {
            uri: uri.into(),
            offset,
            bytes,
            eof: end == entry.descriptor.unpacked_size,
            cache_hit,
        })
    }

    pub fn open_stream(&self, uri: &str) -> Result<Box<dyn ArchiveStream>, PazError> {
        Ok(Box::new(Cursor::new(
            self.decoded_entry(self.entry(uri)?)?.0,
        )))
    }
}

struct DecodedRange {
    bytes: Vec<u8>,
    start: usize,
    end: usize,
}

impl DecodedRange {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[self.start..self.end]
    }
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
                "archive file name does not match its declared role",
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
    source: &mut ArchiveFile,
    expected_index_xor: u32,
    decrypt_provider: &MusicaPazDecryptProvider,
) -> Result<Vec<ArchiveEntryDescriptor>, PazError> {
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
                "ASTRA_EMU_MUSICA_INDEX_XOR",
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
            "ASTRA_EMU_MUSICA_INDEX_SIZE",
            "PAZ index size is empty, unaligned, or out of bounds",
        ));
    }
    let mut encrypted = read_source_range(source, index_offset, encrypted_size)?;
    xor_byte(&mut encrypted, source.xor_key);
    let decoded = decrypt_bytes(
        decrypt_provider,
        MusicaDecryptDescriptor::Index {
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
            "ASTRA_EMU_MUSICA_ENTRY_COUNT",
            "PAZ entry count exceeds the configured limit",
        ));
    }
    let video_key = if source.role == "mov" {
        let mut key = vec![0u8; 256];
        cursor
            .read_exact(&mut key)
            .map_err(|_| error("ASTRA_EMU_MUSICA_VIDEO_KEY", "PAZ video key is truncated"))?;
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
                event = "astra_emu_musica_entry_bounds_invalid",
                archive_role = %source.role,
                entry_index = index,
                offset,
                unpacked_size,
                stored_size,
                aligned_size,
                archive_size = source.length
            );
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_BOUNDS",
                "PAZ entry descriptor is oversized, unaligned, or out of bounds",
            ));
        }
        entries.push(ArchiveEntryDescriptor {
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
            "ASTRA_EMU_MUSICA_INDEX_SHORT",
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
            "ASTRA_EMU_MUSICA_ENTRY_PATH",
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
    let (text, _, malformed) = SHIFT_JIS.decode(&bytes[start..end]);
    if malformed {
        return Err(error(
            "ASTRA_EMU_MUSICA_INDEX_ENCODING",
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

fn read_source_range(source: &ArchiveFile, offset: u64, length: u64) -> Result<Vec<u8>, PazError> {
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
            "ASTRA_EMU_MUSICA_ARCHIVE_SHORT_READ",
            "multipart PAZ returned a short read",
        ));
    }
    Ok(output)
}

fn hash_parts_and_entries(
    parts: &[ArchivePart],
    source_length: u64,
    entries: &[ArchiveEntryDescriptor],
) -> Result<(Hash256, Vec<Hash256>), PazError> {
    let mut ranges = Vec::with_capacity(entries.len());
    let mut entry_hashes = vec![None; entries.len()];
    for (index, entry) in entries.iter().enumerate() {
        let end = entry
            .offset
            .checked_add(entry.aligned_size)
            .ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_ENTRY_BOUNDS",
                    "entry encrypted range overflowed",
                )
            })?;
        if end > source_length {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_BOUNDS",
                "entry encrypted range exceeds the archive",
            ));
        }
        if entry.aligned_size == 0 {
            entry_hashes[index] = Some(Hash256::from_sha256(&[]));
        } else {
            ranges.push((entry.offset, end, index));
        }
    }
    ranges.sort_unstable_by_key(|(start, end, index)| (*start, *end, *index));
    for pair in ranges.windows(2) {
        if pair[1].0 < pair[0].1 {
            return Err(error(
                "ASTRA_EMU_MUSICA_ENTRY_OVERLAP",
                "PAZ encrypted entry ranges overlap",
            ));
        }
    }

    let mut source_hasher = Sha256::new();
    let mut active_entry = None::<Sha256>;
    let mut range_index = 0usize;
    let mut logical_offset = 0u64;
    // Windows reserves a relatively small main-thread stack. Archive hashing is a
    // normal host operation, so its MiB-sized scratch area belongs on the heap.
    let mut buffer = vec![0u8; 1024 * 1024];
    for part in parts {
        let file = File::open(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_ARCHIVE_OPEN",
                "PAZ archive cannot be opened",
            )
        })?;
        let read_bound = part.length.checked_add(1).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_ARCHIVE_SIZE",
                "archive part read bound overflowed",
            )
        })?;
        let mut file = file.take(read_bound);
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
            part_bytes = part_bytes.checked_add(count as u64).ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_ARCHIVE_SIZE",
                    "archive part read size overflowed",
                )
            })?;
            source_hasher.update(&buffer[..count]);
            let chunk_end = logical_offset.checked_add(count as u64).ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_ARCHIVE_SIZE",
                    "archive logical offset overflowed",
                )
            })?;
            while let Some(&(range_start, range_end, original_index)) = ranges.get(range_index) {
                if range_start >= chunk_end {
                    break;
                }
                if range_end <= logical_offset {
                    return Err(error(
                        "ASTRA_EMU_MUSICA_ENTRY_HASH",
                        "entry encrypted range was not covered by the archive stream",
                    ));
                }
                let overlap_start = range_start.max(logical_offset);
                let overlap_end = range_end.min(chunk_end);
                let start = usize::try_from(overlap_start - logical_offset).map_err(|_| {
                    error(
                        "ASTRA_EMU_MUSICA_ENTRY_HASH",
                        "entry chunk start exceeds host bounds",
                    )
                })?;
                let end = usize::try_from(overlap_end - logical_offset).map_err(|_| {
                    error(
                        "ASTRA_EMU_MUSICA_ENTRY_HASH",
                        "entry chunk end exceeds host bounds",
                    )
                })?;
                active_entry
                    .get_or_insert_with(Sha256::new)
                    .update(&buffer[start..end]);
                if overlap_end != range_end {
                    break;
                }
                let digest = active_entry.take().ok_or_else(|| {
                    error("ASTRA_EMU_MUSICA_ENTRY_HASH", "entry hash state is missing")
                })?;
                entry_hashes[original_index] = Some(Hash256::from_bytes(digest.finalize().into()));
                range_index += 1;
            }
            logical_offset = chunk_end;
        }
        if part_bytes != part.length {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part size changed while hashing",
            ));
        }
    }
    if logical_offset != source_length
        || range_index != ranges.len()
        || active_entry.is_some()
        || entry_hashes.iter().any(Option::is_none)
    {
        return Err(error(
            "ASTRA_EMU_MUSICA_ARCHIVE_SHORT_READ",
            "archive stream did not cover every declared byte and entry",
        ));
    }
    for part in parts {
        let metadata = std::fs::metadata(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part disappeared while hashing",
            )
        })?;
        if metadata.len() != part.length || metadata.modified().ok() != part.modified {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part metadata changed while hashing",
            ));
        }
    }
    let entry_hashes = entry_hashes
        .into_iter()
        .map(|hash| {
            hash.ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_ENTRY_HASH",
                    "entry hash was not finalized",
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        Hash256::from_bytes(source_hasher.finalize().into()),
        entry_hashes,
    ))
}

fn hash_parts(parts: &[ArchivePart]) -> Result<Hash256, PazError> {
    let source_length = parts.iter().try_fold(0u64, |total, part| {
        total.checked_add(part.length).ok_or_else(|| {
            error(
                "ASTRA_EMU_MUSICA_ARCHIVE_SIZE",
                "multipart PAZ size overflowed",
            )
        })
    })?;
    hash_parts_and_entries(parts, source_length, &[]).map(|(hash, _)| hash)
}

fn verify_source_unchanged(source: &ArchiveFile) -> Result<(), PazError> {
    for part in &source.parts {
        let metadata = std::fs::metadata(&part.path).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part disappeared after mount",
            )
        })?;
        if metadata.len() != part.length || metadata.modified().ok() != part.modified {
            return Err(error(
                "ASTRA_EMU_MUSICA_SOURCE_CHANGED",
                "archive part metadata changed after mount",
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
            "PAZ archive resolves outside the game root",
        ));
    }
    let metadata = std::fs::metadata(&base).map_err(|_| {
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
        match std::fs::metadata(&path) {
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
        "png" | "jpg" | "bmp" => "image",
        "ogg" | "wav" => "audio",
        "mpg" | "avi" | "wmv" => "video",
        _ => "binary",
    }
}

fn entry_method(entry: &ArchiveEntryDescriptor) -> &'static str {
    match (entry.archive_role.as_str(), entry.packed) {
        ("mov", false) => "movie-transform",
        ("mov", true) => "movie-transform+zlib",
        (_, false) => "blowfish+rc4",
        (_, true) => "blowfish+rc4+zlib",
    }
}

fn password_for_entry<'a>(
    entry: &ArchiveEntryDescriptor,
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
    provider: &MusicaPazDecryptProvider,
    descriptor: MusicaDecryptDescriptor,
    bytes: &[u8],
) -> Result<Vec<u8>, PazError> {
    if bytes.is_empty() {
        return Err(error(
            "ASTRA_EMU_MUSICA_DECRYPT_EMPTY",
            "Musica decrypt input is empty",
        ));
    }
    let mut output = Vec::with_capacity(bytes.len());
    for (batch_index, batch) in bytes.chunks(PAZ_DECRYPT_MAX_BATCH_BYTES).enumerate() {
        let batch_offset = u64::try_from(batch_index)
            .ok()
            .and_then(|index| index.checked_mul(PAZ_DECRYPT_MAX_BATCH_BYTES as u64))
            .ok_or_else(|| {
                error(
                    "ASTRA_EMU_MUSICA_DECRYPT_OFFSET",
                    "decrypt batch offset overflowed",
                )
            })?;
        let batch_descriptor = descriptor.with_stream_offset(batch_offset);
        let payload = serde_json::to_vec(&batch_descriptor).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_DESCRIPTOR",
                "Musica decrypt descriptor could not be encoded",
            )
        })?;
        let opaque = PazDecryptDescriptor {
            schema_id: MUSICA_DECRYPT_DESCRIPTOR_SCHEMA.into(),
            schema_hash: provider.descriptor_schema_hash(),
            payload,
        };
        let phase = match batch_descriptor {
            MusicaDecryptDescriptor::Index { .. } => PazDecryptPhase::Index,
            MusicaDecryptDescriptor::Entry { .. } => PazDecryptPhase::Entry,
        };
        for (chunk_index, chunk) in batch.chunks(PAZ_DECRYPT_CHUNK_BYTES).enumerate() {
            let chunk_offset = (chunk_index * PAZ_DECRYPT_CHUNK_BYTES) as u64;
            output.extend_from_slice(&provider.decrypt(PazDecryptRequest {
                phase,
                descriptors: std::slice::from_ref(&opaque),
                transport: PazDecryptTransport {
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
            "ASTRA_EMU_MUSICA_DECRYPT_SIZE",
            "Musica decrypt output size is inconsistent",
        ));
    }
    Ok(output)
}

impl MusicaDecryptDescriptor {
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

fn cache_error(error_value: PlaintextCacheError) -> PazError {
    match error_value {
        PlaintextCacheError::EntryLimit => error(
            "ASTRA_EMU_MUSICA_CACHE_ENTRY_LIMIT",
            "plaintext cache entry exceeds its configured budget",
        ),
        PlaintextCacheError::Corrupt => error(
            "ASTRA_EMU_MUSICA_CACHE_CORRUPT",
            "plaintext cache metadata or content is corrupt",
        ),
        PlaintextCacheError::Permission(_) => error(
            "ASTRA_EMU_MUSICA_CACHE_PERMISSION",
            "plaintext cache privacy permissions could not be enforced",
        ),
        PlaintextCacheError::Io(_) => {
            error("ASTRA_EMU_MUSICA_CACHE_IO", "plaintext cache I/O failed")
        }
    }
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

fn blowfish_decrypt(key: &[u8], encrypted: &[u8]) -> Result<Vec<u8>, PazError> {
    validate_blowfish_key(key)?;
    if !encrypted.len().is_multiple_of(8) {
        return Err(error(
            "ASTRA_EMU_MUSICA_BLOWFISH_ALIGNMENT",
            "Blowfish input is not block aligned",
        ));
    }
    let cipher: Blowfish = Blowfish::new_from_slice(key)
        .map_err(|_| error("ASTRA_EMU_MUSICA_BLOWFISH_KEY", "Blowfish key is invalid"))?;
    let mut bytes = encrypted.to_vec();
    for chunk in bytes.as_chunks_mut::<8>().0.iter_mut() {
        chunk[..4].reverse();
        chunk[4..].reverse();
        cipher.decrypt_block((&mut *chunk).into());
        chunk[..4].reverse();
        chunk[4..].reverse();
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
#[path = "paz/tests.rs"]
mod tests;
