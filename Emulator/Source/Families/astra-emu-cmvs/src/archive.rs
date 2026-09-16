mod access;
mod mount;
mod scripts;
mod source;
use source::*;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Mutex,
    time::SystemTime,
};

use astra_core::Hash256;
use astra_emu_sdk::{
    validate_archive_directory_uri, validate_archive_uri, ArchiveEntry, ArchiveManifest,
    ArchiveNode, ArchiveNodeKind, ArchiveReadResult, ArchiveSource, ArchiveStat, ArchiveStream,
    CoreError, ARCHIVE_MANIFEST_SCHEMA, ARCHIVE_MAX_READ_BYTES,
};
use astra_emu_sdk::{CacheIdentity, PlaintextCache, PlaintextCacheError};
use sha2::{Digest, Sha256};

use crate::{
    decode_pb2_image, decode_pb3_jbp, decode_pb3_type1, decode_pb3_type5, decode_pb3_type6,
    decrypt_cpz5_entry, parse_cpz5_index, parse_cpz_header, parse_pb2_metadata, parse_pb3_metadata,
    CmvsCpzHeader, CmvsCpzVersion, CmvsSchemeProfile, Cpz5IndexEntry, PbDecodedImage,
    PbImageResolver, CMVS_DECRYPT_PROVIDER_ID, CMVS_FAMILY_ID, CMVS_READER_ID,
};

const MAX_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;
const MIN_CACHEABLE_ENTRY_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVES: usize = 256;
const MAX_ENTRIES: usize = 2_000_000;
const MAX_PB3_IMAGE_CHAIN: usize = 32;

#[derive(Clone)]
struct ArchiveFile {
    role: String,
    path: PathBuf,
    byte_size: u64,
    source_stamp: SourceStamp,
    source_hash: Hash256,
    header: CmvsCpzHeader,
}

#[derive(Clone)]
struct LooseSource {
    role: String,
    path: PathBuf,
    media_kind: String,
    byte_size: u64,
    source_stamp: SourceStamp,
    source_hash: Hash256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceStamp {
    byte_size: u64,
    modified: SystemTime,
}

#[derive(Clone)]
struct MountedEntry {
    uri: String,
    entry_id: String,
    archive: usize,
    descriptor: Cpz5IndexEntry,
}

pub struct CmvsArchive {
    mount_id: String,
    prefix: String,
    manifest: ArchiveManifest,
    archives: Vec<ArchiveFile>,
    entries: BTreeMap<String, MountedEntry>,
    loose: BTreeMap<String, LooseSource>,
    scheme: CmvsSchemeProfile,
    private_profile_hash: Hash256,
    cache: Option<PlaintextCache>,
    ephemeral_entry: Mutex<Option<(String, Vec<u8>)>>,
}

struct CmvsPbImageResolver<'a> {
    vfs: &'a CmvsArchive,
    parent_uri: &'a str,
    chain: Vec<String>,
}

impl PbImageResolver for CmvsPbImageResolver<'_> {
    fn resolve_pb3_base(&self, reference: &str) -> Result<PbDecodedImage, CoreError> {
        let uri = resolve_pb3_base_uri(&self.vfs.prefix, self.parent_uri, reference)?;
        self.vfs.decode_pb3_image_inner(&uri, self.chain.clone())
    }
}

impl CmvsArchive {
    fn entry(&self, uri: &str) -> Result<&MountedEntry, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        self.entries
            .get(uri)
            .ok_or_else(|| invalid("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry was not found"))
    }

    fn loose_role_from_uri(&self, uri: &str) -> Option<String> {
        let loose_prefix = format!("{}loose/", self.prefix);
        let role = uri.strip_prefix(&loose_prefix)?;
        if role.is_empty() || role.contains('/') {
            return None;
        }
        self.loose.contains_key(role).then(|| role.to_owned())
    }

    fn read_loose_range(
        &self,
        source: &LooseSource,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, CoreError> {
        verify_loose_stamp(source)?;
        let mut file = File::open(&source.path).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_LOOSE_IO",
                "CMVS loose resource could not be opened",
            )
        })?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| invalid("ASTRA_EMU_CMVS_LOOSE_IO", "CMVS loose resource seek failed"))?;
        let length = usize::try_from(length).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_LOOSE_IO",
                "CMVS loose resource range exceeds platform bounds",
            )
        })?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_LOOSE_IO",
                "CMVS loose resource was truncated",
            )
        })?;
        verify_loose_stamp(source)?;
        Ok(bytes)
    }

    /// Decodes a PB3 image through this mounted session.  Base-image lookup is
    /// kept here rather than in the format parser so archive identity, URI
    /// safety, recursion depth, and cycle detection remain host-owned.
    pub fn decode_pb3_image(&self, uri: &str) -> Result<PbDecodedImage, CoreError> {
        self.decode_pb3_image_inner(uri, Vec::new())
    }

    /// Decodes a PB2 or PB3 image through this mounted session.  PB3 type-6
    /// keeps its base-image resolution inside the host-owned VFS; PB2 has no
    /// documented cross-entry dependency in the supported reference variants.
    pub fn decode_pb_image(&self, uri: &str) -> Result<PbDecodedImage, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        let (source, _) = self.decode_entry(self.entry(uri)?)?;
        if source.starts_with(crate::PB2_MAGIC) {
            return decode_pb2_image(&source);
        }
        self.decode_pb3_image_inner(uri, Vec::new())
    }

    pub fn pb3_image_type(&self, uri: &str) -> Result<u16, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        let (source, _) = self.decode_entry(self.entry(uri)?)?;
        Ok(parse_pb3_metadata(&source)?.image_type)
    }

    pub fn pb_image_type(&self, uri: &str) -> Result<u16, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        let (source, _) = self.decode_entry(self.entry(uri)?)?;
        if source.starts_with(crate::PB2_MAGIC) {
            return Ok(parse_pb2_metadata(&source)?.image_type);
        }
        Ok(parse_pb3_metadata(&source)?.image_type)
    }

    fn decode_pb3_image_inner(
        &self,
        uri: &str,
        mut chain: Vec<String>,
    ) -> Result<PbDecodedImage, CoreError> {
        validate_archive_uri(&self.prefix, uri)?;
        if chain.len() >= MAX_PB3_IMAGE_CHAIN || chain.iter().any(|candidate| candidate == uri) {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PB3_BASE_CYCLE",
                "CMVS PB3 base-image dependency is cyclic or exceeds its depth budget",
            ));
        }
        let (source, _) = self.decode_entry(self.entry(uri)?)?;
        let metadata = parse_pb3_metadata(&source)?;
        chain.push(uri.into());
        match metadata.image_type {
            1 => decode_pb3_type1(&source),
            2 | 3 => decode_pb3_jbp(&source),
            5 => decode_pb3_type5(&source),
            6 => decode_pb3_type6(
                &source,
                &CmvsPbImageResolver {
                    vfs: self,
                    parent_uri: uri,
                    chain,
                },
            ),
            _ => Err(invalid(
                "ASTRA_EMU_CMVS_PB3_VARIANT",
                "CMVS PB3 image variant is not implemented",
            )),
        }
    }

    fn decode_entry(&self, entry: &MountedEntry) -> Result<(Vec<u8>, bool), CoreError> {
        let archive = &self.archives[entry.archive];
        verify_archive_stamp(archive)?;
        let cache_identity = CacheIdentity {
            family_id: CMVS_FAMILY_ID.into(),
            source_hash: archive.source_hash,
            entry_id: entry.entry_id.clone(),
            private_profile_hash: self.private_profile_hash,
            decrypt_provider_id: CMVS_DECRYPT_PROVIDER_ID.into(),
            descriptor_schema_hash: Hash256::from_sha256(
                crate::CMVS_DECRYPT_DESCRIPTOR_SCHEMA.as_bytes(),
            ),
            codec_identity: "cpz5-entry-v1".into(),
        };
        let cache = self
            .cache
            .as_ref()
            .filter(|_| entry.descriptor.stored_size >= MIN_CACHEABLE_ENTRY_BYTES);
        if entry.descriptor.stored_size < MIN_CACHEABLE_ENTRY_BYTES {
            let ephemeral = self.ephemeral_entry.lock().map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_EPHEMERAL_CACHE",
                    "CMVS ephemeral entry cache is poisoned",
                )
            })?;
            if let Some((entry_id, bytes)) = ephemeral.as_ref() {
                if entry_id == &entry.entry_id {
                    return Ok((bytes.clone(), true));
                }
            }
        }
        if let Some(bytes) = cache
            .map(|cache| cache.get(&cache_identity))
            .transpose()
            .map_err(cache_error)?
            .flatten()
        {
            if u64::try_from(bytes.len()).ok() != Some(entry.descriptor.stored_size) {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_CACHE_SIZE",
                    "CMVS cached entry size does not match its descriptor",
                ));
            }
            return Ok((bytes, true));
        }
        let mut source = File::open(&archive.path).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_SOURCE_IO",
                "CMVS archive source could not be opened",
            )
        })?;
        source
            .seek(SeekFrom::Start(entry.descriptor.offset))
            .map_err(|_| invalid("ASTRA_EMU_CMVS_SOURCE_IO", "CMVS entry seek failed"))?;
        let length = usize::try_from(entry.descriptor.stored_size).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_ENTRY_SIZE",
                "CMVS entry exceeds platform bounds",
            )
        })?;
        let mut bytes = vec![0; length];
        source.read_exact(&mut bytes).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_SOURCE_SHORT_READ",
                "CMVS entry source was truncated",
            )
        })?;
        verify_archive_stamp(archive)?;
        decrypt_cpz5_entry(&archive.header, &entry.descriptor, &mut bytes, &self.scheme)?;
        if let Some(cache) = cache {
            cache.put(&cache_identity, &bytes).map_err(cache_error)?;
        }
        if entry.descriptor.stored_size < MIN_CACHEABLE_ENTRY_BYTES {
            let mut ephemeral = self.ephemeral_entry.lock().map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_EPHEMERAL_CACHE",
                    "CMVS ephemeral entry cache is poisoned",
                )
            })?;
            *ephemeral = Some((entry.entry_id.clone(), bytes.clone()));
        }
        Ok((bytes, false))
    }
}

fn media_kind(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "pb2" | "pb3" | "png" | "jpg" | "jpeg" | "bmp" => "image",
        "ogg" | "wav" | "mv" | "mv2" => "audio",
        "mgv" | "mpg" | "mpeg" | "wmv" => "video",
        "ps2" | "ps3" => "script",
        _ => "binary",
    }
}

fn loose_media_kind(path: &std::path::Path) -> Result<&'static str, CoreError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_LOOSE_NAME",
                "CMVS loose resource has no valid file name",
            )
        })?;
    Ok(media_kind(file_name))
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

pub(crate) fn resolve_pb3_base_uri(
    prefix: &str,
    parent_uri: &str,
    reference: &str,
) -> Result<String, CoreError> {
    if reference.is_empty()
        || reference.contains(['/', '\\'])
        || reference.contains("..")
        || !parent_uri.starts_with(prefix)
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_BASE_REFERENCE",
            "CMVS PB3 base image reference is invalid",
        ));
    }
    let (directory, _) = parent_uri.rsplit_once('/').ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_BASE_REFERENCE",
            "CMVS PB3 parent URI has no filename",
        )
    })?;
    let uri = format!("{directory}/{reference}");
    validate_archive_uri(prefix, &uri)?;
    Ok(uri)
}

fn cache_error(error: PlaintextCacheError) -> CoreError {
    match error {
        PlaintextCacheError::EntryLimit => invalid(
            "ASTRA_EMU_CMVS_CACHE_ENTRY_LIMIT",
            "CMVS decoded entry exceeds the configured cache limit",
        ),
        PlaintextCacheError::Corrupt => invalid(
            "ASTRA_EMU_CMVS_CACHE_CORRUPT",
            "CMVS cached entry is corrupt",
        ),
        PlaintextCacheError::Permission(_) => invalid(
            "ASTRA_EMU_CMVS_CACHE_PERMISSION",
            "CMVS cache privacy permissions could not be enforced",
        ),
        PlaintextCacheError::Io(_) => invalid("ASTRA_EMU_CMVS_CACHE_IO", "CMVS cache I/O failed"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn source_stamp_detects_a_size_mutation() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(b"first").unwrap();
        source.flush().unwrap();
        let original = source_stamp(&source.as_file().metadata().unwrap()).unwrap();
        source.write_all(b"-mutated").unwrap();
        source.flush().unwrap();
        let mutated = source_stamp(&source.as_file().metadata().unwrap()).unwrap();
        assert_ne!(original, mutated);
    }

    #[test]
    fn resolves_pb3_base_in_the_current_vfs_directory() {
        assert_eq!(
            resolve_pb3_base_uri("cmvs:/", "cmvs:/scene/current.pb3", "base.pb3").unwrap(),
            "cmvs:/scene/base.pb3"
        );
        assert_eq!(
            resolve_pb3_base_uri("cmvs:/", "cmvs:/scene/current.pb3", "../base.pb3")
                .unwrap_err()
                .code(),
            "ASTRA_EMU_CMVS_PB3_BASE_REFERENCE"
        );
    }

    #[test]
    fn classifies_loose_resources_from_the_source_extension() {
        assert_eq!(
            loose_media_kind(std::path::Path::new("startup.ps3")).unwrap(),
            "script"
        );
        assert_eq!(
            loose_media_kind(std::path::Path::new("opening.mgv")).unwrap(),
            "video"
        );
    }
}
