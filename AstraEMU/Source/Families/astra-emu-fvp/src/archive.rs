use std::collections::BTreeSet;

use astra_core::Hash256;
use astra_emu_family_api::{LegacyPackManifest, LegacyProviderError, LegacyVfsEntry};
use encoding_rs::{Encoding, GBK, SHIFT_JIS, UTF_8};

use crate::FvpNls;

#[derive(Debug, Clone)]
pub struct FvpArchive {
    bytes: Vec<u8>,
    entries: Vec<FvpArchiveEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FvpArchiveEntry {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    pub hash: Hash256,
}

impl FvpArchive {
    pub fn parse(
        bytes: Vec<u8>,
        nls: FvpNls,
        max_entries: usize,
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
            if data_end > bytes.len() as u64 {
                return Err(error(
                    "ASTRA_FVP_ARCHIVE_ENTRY_BOUNDS",
                    "entry extends beyond the archive",
                ));
            }
            let payload = &bytes[offset as usize..data_end as usize];
            entries.push(FvpArchiveEntry {
                name,
                offset,
                size,
                hash: Hash256::from_sha256(payload),
            });
        }
        Ok(Self { bytes, entries })
    }
    pub fn entries(&self) -> &[FvpArchiveEntry] {
        &self.entries
    }
    pub fn read(&self, name: &str) -> Option<&[u8]> {
        let e = self.entries.iter().find(|e| e.name == name)?;
        Some(&self.bytes[e.offset as usize..(e.offset + e.size) as usize])
    }
    pub fn manifest(
        &self,
        mount_id: &str,
        folder: &str,
        reader_hash: Hash256,
    ) -> LegacyPackManifest {
        LegacyPackManifest {
            mount_id: mount_id.into(),
            prefix: "fvp:/".into(),
            reader_id: "astra.fvp.bin.v1".into(),
            reader_hash,
            entries: self
                .entries
                .iter()
                .map(|entry| LegacyVfsEntry {
                    uri: format!("fvp:/{folder}/{}", entry.name),
                    entry_id: format!("{folder}:{}", entry.name),
                    offset: entry.offset,
                    size: entry.size,
                    content_hash: entry.hash,
                    media_kind: classify(self.read(&entry.name).unwrap()).into(),
                })
                .collect(),
        }
    }
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
fn classify(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"OggS") {
        "audio.ogg"
    } else if bytes.starts_with(b"RIFF") {
        "audio.riff"
    } else if bytes.starts_with(b"hzc1") {
        "image.hzc1"
    } else if bytes.starts_with(&[0x30, 0x26, 0xb2, 0x75]) {
        "video.asf"
    } else {
        "application/octet-stream"
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
        assert_eq!(parsed.read("bg/scene"), Some(b"OggSdata".as_slice()));
        let manifest = parsed.manifest("mount.test", "bgm", Hash256::from_sha256(b"reader"));
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
