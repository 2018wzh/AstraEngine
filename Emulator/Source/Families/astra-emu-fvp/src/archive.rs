use std::{collections::BTreeSet, sync::Arc};

use astra_byte_source::{
    BoundedByteSource, ByteRange, ByteSourceStat, SourceRevision, DEFAULT_MAX_RANGE_BYTES,
};
use astra_core::Hash256;
use astra_emu_family_api::LegacyProviderError;
use astra_emu_family_core::{
    LegacyPackManifest, LegacyVfsEntry, LegacyVfsSource, LEGACY_PACK_MANIFEST_SCHEMA,
};
use encoding_rs::{Encoding, GBK, SHIFT_JIS, UTF_8};

use crate::FvpNls;

pub struct FvpArchive {
    storage: ArchiveStorage,
    entries: Vec<FvpArchiveEntry>,
}

impl std::fmt::Debug for FvpArchive {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FvpArchive")
            .field("entries", &self.entries)
            .finish_non_exhaustive()
    }
}

enum ArchiveStorage {
    Memory(Vec<u8>),
    Host {
        reader: Arc<dyn astra_emu_family_api::LegacyVfsReader>,
        mount_set_id: String,
        uri: String,
        stat: ByteSourceStat,
    },
    Source {
        source: Arc<dyn BoundedByteSource>,
        stat: ByteSourceStat,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FvpArchiveEntry {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    pub hash: Option<Hash256>,
}

impl FvpArchive {
    pub fn parse(
        bytes: Vec<u8>,
        nls: FvpNls,
        max_entries: usize,
    ) -> Result<Self, LegacyProviderError> {
        let stat = ByteSourceStat {
            len: bytes.len() as u64,
            revision: SourceRevision(Hash256::from_sha256(&bytes)),
        };
        Self::parse_metadata(
            ArchiveStorage::Memory(bytes.clone()),
            &bytes,
            stat,
            nls,
            max_entries,
            true,
        )
    }

    pub fn open_host(
        reader: Arc<dyn astra_emu_family_api::LegacyVfsReader>,
        mount_set_id: String,
        uri: String,
        nls: FvpNls,
        max_entries: usize,
    ) -> Result<Self, LegacyProviderError> {
        let stat = reader.stat_file(&mount_set_id, &uri)?;
        let header = read_host_range(
            reader.as_ref(),
            &mount_set_id,
            &uri,
            stat,
            ByteRange { offset: 0, len: 8 },
        )?;
        if header.len() < 8 {
            return Err(error(
                "ASTRA_FVP_ARCHIVE_HEADER",
                "archive header is truncated",
            ));
        }
        let count = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
        let names_size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let metadata_len = 8usize
            .checked_add(count.checked_mul(12).ok_or_else(|| {
                error(
                    "ASTRA_FVP_ARCHIVE_TABLE_OVERFLOW",
                    "entry table size overflowed",
                )
            })?)
            .and_then(|value| value.checked_add(names_size))
            .ok_or_else(|| {
                error(
                    "ASTRA_FVP_ARCHIVE_NAMES_OVERFLOW",
                    "archive metadata size overflowed",
                )
            })?;
        if metadata_len as u64 > DEFAULT_MAX_RANGE_BYTES {
            return Err(error(
                "ASTRA_FVP_ARCHIVE_METADATA_BOUNDS",
                "archive metadata exceeds the bounded range limit",
            ));
        }
        let metadata = read_host_range(
            reader.as_ref(),
            &mount_set_id,
            &uri,
            stat,
            ByteRange {
                offset: 0,
                len: metadata_len as u64,
            },
        )?;
        Self::parse_metadata(
            ArchiveStorage::Host {
                reader,
                mount_set_id,
                uri,
                stat,
            },
            &metadata,
            stat,
            nls,
            max_entries,
            false,
        )
    }

    pub fn open_source(
        source: Arc<dyn BoundedByteSource>,
        nls: FvpNls,
        max_entries: usize,
    ) -> Result<Self, LegacyProviderError> {
        let stat = source.stat().map_err(byte_source_error)?;
        let header = read_source_range(source.as_ref(), stat, ByteRange { offset: 0, len: 8 })?;
        if header.len() < 8 {
            return Err(error(
                "ASTRA_FVP_ARCHIVE_HEADER",
                "archive header is truncated",
            ));
        }
        let count = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
        let names_size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let metadata_len = 8usize
            .checked_add(count.checked_mul(12).ok_or_else(|| {
                error(
                    "ASTRA_FVP_ARCHIVE_TABLE_OVERFLOW",
                    "entry table size overflowed",
                )
            })?)
            .and_then(|value| value.checked_add(names_size))
            .ok_or_else(|| {
                error(
                    "ASTRA_FVP_ARCHIVE_NAMES_OVERFLOW",
                    "archive metadata size overflowed",
                )
            })?;
        if metadata_len as u64 > DEFAULT_MAX_RANGE_BYTES {
            return Err(error(
                "ASTRA_FVP_ARCHIVE_METADATA_BOUNDS",
                "archive metadata exceeds the bounded range limit",
            ));
        }
        let metadata = read_source_range(
            source.as_ref(),
            stat,
            ByteRange {
                offset: 0,
                len: metadata_len as u64,
            },
        )?;
        Self::parse_metadata(
            ArchiveStorage::Source { source, stat },
            &metadata,
            stat,
            nls,
            max_entries,
            false,
        )
    }

    fn parse_metadata(
        storage: ArchiveStorage,
        bytes: &[u8],
        stat: ByteSourceStat,
        nls: FvpNls,
        max_entries: usize,
        payloads_available: bool,
    ) -> Result<Self, LegacyProviderError> {
        if bytes.len() < 8 {
            return Err(error(
                "ASTRA_FVP_ARCHIVE_HEADER",
                "archive header is truncated",
            ));
        }
        let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let names_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        if count > max_entries {
            return Err(error(
                "ASTRA_FVP_ARCHIVE_ENTRY_COUNT",
                "archive entry count exceeds the configured bound",
            ));
        }
        let table_size = count.checked_mul(12).ok_or_else(|| {
            error(
                "ASTRA_FVP_ARCHIVE_TABLE_OVERFLOW",
                "entry table size overflowed",
            )
        })?;
        let names_start = 8usize.checked_add(table_size).ok_or_else(|| {
            error(
                "ASTRA_FVP_ARCHIVE_TABLE_OVERFLOW",
                "entry table offset overflowed",
            )
        })?;
        let names_end = names_start.checked_add(names_size).ok_or_else(|| {
            error(
                "ASTRA_FVP_ARCHIVE_NAMES_OVERFLOW",
                "filename table size overflowed",
            )
        })?;
        let names = bytes.get(names_start..names_end).ok_or_else(|| {
            error(
                "ASTRA_FVP_ARCHIVE_NAMES_BOUNDS",
                "filename table extends beyond the archive",
            )
        })?;
        let mut entries = Vec::with_capacity(count);
        let mut unique = BTreeSet::new();
        for index in 0..count {
            let base = 8 + index * 12;
            let name_offset =
                u32::from_le_bytes(bytes[base..base + 4].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(bytes[base + 4..base + 8].try_into().unwrap()) as u64;
            let size = u32::from_le_bytes(bytes[base + 8..base + 12].try_into().unwrap()) as u64;
            let tail = names.get(name_offset..).ok_or_else(|| {
                error(
                    "ASTRA_FVP_ARCHIVE_NAME_OFFSET",
                    "filename offset is outside the table",
                )
            })?;
            let end = tail.iter().position(|byte| *byte == 0).ok_or_else(|| {
                error(
                    "ASTRA_FVP_ARCHIVE_NAME_TERMINATOR",
                    "filename is not NUL-terminated",
                )
            })?;
            let (name, _, malformed) = encoding(nls).decode(&tail[..end]);
            if malformed {
                return Err(error(
                    "ASTRA_FVP_ARCHIVE_NAME_ENCODING",
                    "filename cannot be decoded",
                ));
            }
            let name = normalize_name(&name)?;
            if !unique.insert(name.clone()) {
                return Err(error(
                    "ASTRA_FVP_ARCHIVE_NAME_DUPLICATE",
                    "archive filename is duplicated",
                ));
            }
            let data_end = offset.checked_add(size).ok_or_else(|| {
                error(
                    "ASTRA_FVP_ARCHIVE_ENTRY_OVERFLOW",
                    "entry bounds overflowed",
                )
            })?;
            if data_end > stat.len {
                return Err(error(
                    "ASTRA_FVP_ARCHIVE_ENTRY_BOUNDS",
                    "entry extends beyond the archive",
                ));
            }
            let hash = if payloads_available {
                Some(Hash256::from_sha256(
                    &bytes[offset as usize..data_end as usize],
                ))
            } else {
                None
            };
            entries.push(FvpArchiveEntry {
                name,
                offset,
                size,
                hash,
            });
        }
        Ok(Self { storage, entries })
    }
    pub fn entries(&self) -> &[FvpArchiveEntry] {
        &self.entries
    }
    pub fn read(&self, name: &str) -> Result<Vec<u8>, LegacyProviderError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| error("ASTRA_EMU_VFS_NOT_FOUND", "archive entry is not present"))?;
        match &self.storage {
            ArchiveStorage::Memory(bytes) => {
                Ok(bytes[entry.offset as usize..(entry.offset + entry.size) as usize].to_vec())
            }
            ArchiveStorage::Host {
                reader,
                mount_set_id,
                uri,
                stat,
            } => read_host_range(
                reader.as_ref(),
                mount_set_id,
                uri,
                *stat,
                ByteRange {
                    offset: entry.offset,
                    len: entry.size,
                },
            ),
            ArchiveStorage::Source { source, stat } => read_source_range(
                source.as_ref(),
                *stat,
                ByteRange {
                    offset: entry.offset,
                    len: entry.size,
                },
            ),
        }
    }

    pub(crate) fn stat(&self) -> ByteSourceStat {
        match &self.storage {
            ArchiveStorage::Memory(bytes) => ByteSourceStat {
                len: bytes.len() as u64,
                revision: SourceRevision(Hash256::from_sha256(bytes)),
            },
            ArchiveStorage::Host { stat, .. } | ArchiveStorage::Source { stat, .. } => *stat,
        }
    }

    pub(crate) fn source(&self) -> Option<&Arc<dyn BoundedByteSource>> {
        match &self.storage {
            ArchiveStorage::Source { source, .. } => Some(source),
            ArchiveStorage::Memory(_) | ArchiveStorage::Host { .. } => None,
        }
    }

    pub(crate) fn entry(&self, index: usize) -> Option<&FvpArchiveEntry> {
        self.entries.get(index)
    }

    pub fn manifest(
        &self,
        mount_id: &str,
        folder: &str,
        reader_hash: Hash256,
    ) -> Result<LegacyPackManifest, LegacyProviderError> {
        let (source_size, source_hash) = match &self.storage {
            ArchiveStorage::Memory(bytes) => (bytes.len() as u64, Hash256::from_sha256(bytes)),
            ArchiveStorage::Host { stat, .. } | ArchiveStorage::Source { stat, .. } => {
                (stat.len, stat.revision.0)
            }
        };
        let mut entries = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let bytes = self.read(&entry.name)?;
            entries.push(LegacyVfsEntry {
                uri: format!("fvp:/{folder}/{}", entry.name),
                entry_id: format!("{folder}:{}", entry.name),
                source_id: folder.into(),
                source_offset: entry.offset,
                stored_size: entry.size,
                decoded_size: entry.size,
                source_hash: Hash256::from_sha256(&bytes),
                content_hash: Some(Hash256::from_sha256(&bytes)),
                method: "raw".into(),
                media_kind: classify(&bytes).into(),
            });
        }
        let manifest = LegacyPackManifest {
            schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
            family_id: "fvp".into(),
            mount_id: mount_id.into(),
            prefix: "fvp:/".into(),
            reader_id: "astra.fvp.bin.v1".into(),
            reader_hash,
            decrypt_provider_id: "astra.emu.fvp.raw.v1".into(),
            private_profile_hash: Hash256::from_sha256(b"astra.emu.fvp.no_private_profile"),
            mount_profile_hash: reader_hash,
            sources: vec![LegacyVfsSource {
                source_id: folder.into(),
                archive_role: Some(folder.into()),
                byte_size: source_size,
                part_count: 1,
                source_hash,
            }],
            entries,
        };
        manifest.validate(1_000_000).map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_FVP_MANIFEST_CORE",
                "in-process VFS manifest failed core validation",
            )
        })?;
        Ok(manifest)
    }
}

fn read_source_range(
    source: &dyn BoundedByteSource,
    stat: ByteSourceStat,
    range: ByteRange,
) -> Result<Vec<u8>, LegacyProviderError> {
    let end = range.offset.checked_add(range.len).ok_or_else(|| {
        error(
            "ASTRA_FVP_ARCHIVE_ENTRY_OVERFLOW",
            "archive range overflowed",
        )
    })?;
    if end > stat.len {
        return Err(error(
            "ASTRA_FVP_ARCHIVE_ENTRY_BOUNDS",
            "archive range is out of bounds",
        ));
    }
    let capacity = usize::try_from(range.len).map_err(|_| {
        error(
            "ASTRA_FVP_ARCHIVE_ENTRY_BOUNDS",
            "archive range cannot fit in memory",
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut offset = range.offset;
    while offset < end {
        let len = (end - offset).min(DEFAULT_MAX_RANGE_BYTES);
        let result = source
            .read_range(
                stat.revision,
                ByteRange { offset, len },
                DEFAULT_MAX_RANGE_BYTES,
            )
            .map_err(byte_source_error)?;
        bytes.extend_from_slice(&result.bytes);
        offset = offset.checked_add(len).ok_or_else(|| {
            error(
                "ASTRA_FVP_ARCHIVE_ENTRY_OVERFLOW",
                "archive range overflowed",
            )
        })?;
    }
    Ok(bytes)
}

fn byte_source_error(source: astra_byte_source::ByteSourceError) -> LegacyProviderError {
    let code = match source {
        astra_byte_source::ByteSourceError::RangeLimit => "ASTRA_EMU_VFS_READ_LIMIT",
        astra_byte_source::ByteSourceError::RangeOverflow => "ASTRA_EMU_VFS_READ_OVERFLOW",
        astra_byte_source::ByteSourceError::RangeBounds => "ASTRA_EMU_VFS_READ_BOUNDS",
        astra_byte_source::ByteSourceError::RevisionMismatch => "ASTRA_EMU_VFS_SOURCE_CHANGED",
        astra_byte_source::ByteSourceError::ShortRead => "ASTRA_EMU_VFS_SHORT_READ",
        astra_byte_source::ByteSourceError::Poisoned => "ASTRA_EMU_VFS_SOURCE_POISONED",
        astra_byte_source::ByteSourceError::Io(_) => "ASTRA_EMU_VFS_SOURCE_IO",
    };
    error(code, "bounded archive source read failed")
}

fn read_host_range(
    reader: &dyn astra_emu_family_api::LegacyVfsReader,
    mount_set_id: &str,
    uri: &str,
    stat: ByteSourceStat,
    range: ByteRange,
) -> Result<Vec<u8>, LegacyProviderError> {
    let end = range.offset.checked_add(range.len).ok_or_else(|| {
        error(
            "ASTRA_FVP_ARCHIVE_ENTRY_OVERFLOW",
            "archive range overflowed",
        )
    })?;
    if end > stat.len {
        return Err(error(
            "ASTRA_FVP_ARCHIVE_ENTRY_BOUNDS",
            "archive range is out of bounds",
        ));
    }
    let capacity = usize::try_from(range.len).map_err(|_| {
        error(
            "ASTRA_FVP_ARCHIVE_ENTRY_BOUNDS",
            "archive range cannot fit in memory",
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut offset = range.offset;
    while offset < end {
        let len = (end - offset).min(DEFAULT_MAX_RANGE_BYTES);
        let result = reader.read_file_range(
            mount_set_id,
            uri,
            stat.revision,
            ByteRange { offset, len },
            DEFAULT_MAX_RANGE_BYTES,
        )?;
        bytes.extend_from_slice(&result.bytes);
        offset = offset.checked_add(len).ok_or_else(|| {
            error(
                "ASTRA_FVP_ARCHIVE_ENTRY_OVERFLOW",
                "archive range overflowed",
            )
        })?;
    }
    Ok(bytes)
}

fn encoding(nls: FvpNls) -> &'static Encoding {
    match nls {
        FvpNls::ShiftJis => SHIFT_JIS,
        FvpNls::Gbk => GBK,
        FvpNls::Utf8 => UTF_8,
    }
}
fn normalize_name(value: &str) -> Result<String, LegacyProviderError> {
    let value = value.replace('\\', "/").to_ascii_lowercase();
    if value.is_empty()
        || value.starts_with('/')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || value.contains(':')
    {
        return Err(error("ASTRA_FVP_ARCHIVE_NAME", "archive name is unsafe"));
    }
    Ok(value)
}
pub(crate) fn classify(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"OggS") {
        "audio.ogg"
    } else if bytes.starts_with(b"RIFF") {
        "audio.riff"
    } else if bytes.starts_with(b"hzc1") {
        "image.hzc1"
    } else if bytes.starts_with(&[0x30, 0x26, 0xb2, 0x75]) {
        "video.asf"
    } else {
        "application.octet_stream"
    }
}
fn error(code: &'static str, message: &str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn archive_parses_case_insensitive_nested_entries_and_classifies_payload() {
        let bytes = archive(&[("BG/Scene", b"OggSdata")]);
        let parsed = FvpArchive::parse(bytes, FvpNls::Utf8, 8).unwrap();
        assert_eq!(parsed.entries()[0].name, "bg/scene");
        assert_eq!(parsed.read("bg/scene").unwrap(), b"OggSdata".as_slice());
        let manifest = parsed
            .manifest("mount.test", "bgm", Hash256::from_sha256(b"reader"))
            .unwrap();
        assert_eq!(manifest.entries[0].media_kind, "audio.ogg");
        assert!(manifest.entries[0].uri.contains("bg/scene"));
    }

    #[test]
    fn archive_rejects_duplicate_case_folded_names_and_unsafe_paths() {
        let duplicate = archive(&[("Scene", b"a"), ("scene", b"b")]);
        assert_eq!(
            FvpArchive::parse(duplicate, FvpNls::Utf8, 8)
                .unwrap_err()
                .code(),
            "ASTRA_FVP_ARCHIVE_NAME_DUPLICATE"
        );
        let traversal = archive(&[("../scene", b"a")]);
        assert_eq!(
            FvpArchive::parse(traversal, FvpNls::Utf8, 8)
                .unwrap_err()
                .code(),
            "ASTRA_FVP_ARCHIVE_NAME"
        );
    }

    proptest! {
        #[test]
        fn arbitrary_archive_bytes_are_total_and_deterministic(bytes in proptest::collection::vec(any::<u8>(), 0..4096), max_entries in 0usize..128) {
            let summarize = |result: Result<FvpArchive, LegacyProviderError>| {
                result
                    .map(|archive| {
                        archive.entries().iter().map(|entry| {
                            (entry.name.clone(), entry.offset, entry.size, entry.hash)
                        }).collect::<Vec<_>>()
                    })
                    .map_err(|error| error.code().to_owned())
            };
            let first = summarize(FvpArchive::parse(bytes.clone(), FvpNls::Utf8, max_entries));
            let second = summarize(FvpArchive::parse(bytes, FvpNls::Utf8, max_entries));
            prop_assert_eq!(first, second);
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut name_offsets = Vec::new();
        for (name, _) in entries {
            name_offsets.push(names.len() as u32);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }
        let payload_start = 8 + entries.len() * 12 + names.len();
        let mut payload_offset = payload_start as u32;
        let mut bytes = (entries.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
        for ((_, payload), name_offset) in entries.iter().zip(name_offsets) {
            bytes.extend_from_slice(&name_offset.to_le_bytes());
            bytes.extend_from_slice(&payload_offset.to_le_bytes());
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            payload_offset += payload.len() as u32;
        }
        bytes.extend_from_slice(&names);
        for (_, payload) in entries {
            bytes.extend_from_slice(payload);
        }
        bytes
    }
}
