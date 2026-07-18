use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LegacyCoreError;

pub const LEGACY_VFS_MAX_READ_BYTES: u64 = 64 * 1024 * 1024;
pub const LEGACY_PACK_MANIFEST_SCHEMA: &str = "astra.emu.legacy_pack_manifest.v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyVfsNodeKind {
    Directory,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsNode {
    pub uri: String,
    pub name: String,
    pub kind: LegacyVfsNodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsStat {
    pub uri: String,
    pub entry_id: Option<String>,
    pub kind: LegacyVfsNodeKind,
    pub size: u64,
    pub content_hash: Option<Hash256>,
    pub archive_role: Option<String>,
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsReadResult {
    pub uri: String,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub eof: bool,
    pub cache_hit: bool,
}

pub trait LegacyVfsStream: Read + Send {}
impl<T: Read + Send> LegacyVfsStream for T {}

pub trait LegacyMountedVfs: Send + Sync {
    fn mount_id(&self) -> &str;
    fn manifest(&self) -> &LegacyPackManifest;
    fn validate_sources(&self) -> Result<(), LegacyCoreError>;
    fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyCoreError>;
    fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyCoreError>;
    fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<LegacyVfsReadResult, LegacyCoreError>;
    fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, LegacyCoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsSource {
    pub source_id: String,
    pub archive_role: Option<String>,
    pub byte_size: u64,
    pub part_count: u32,
    pub source_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsEntry {
    pub uri: String,
    pub entry_id: String,
    pub source_id: String,
    pub source_offset: u64,
    pub stored_size: u64,
    pub decoded_size: u64,
    pub source_hash: Hash256,
    pub content_hash: Option<Hash256>,
    pub method: String,
    pub media_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyPackManifest {
    pub schema: String,
    pub family_id: String,
    pub mount_id: String,
    pub prefix: String,
    pub reader_id: String,
    pub reader_hash: Hash256,
    pub decrypt_provider_id: String,
    pub private_profile_hash: Hash256,
    pub mount_profile_hash: Hash256,
    pub sources: Vec<LegacyVfsSource>,
    pub entries: Vec<LegacyVfsEntry>,
}

impl LegacyPackManifest {
    pub fn validate(&self, max_entries: usize) -> Result<(), LegacyCoreError> {
        if self.schema != LEGACY_PACK_MANIFEST_SCHEMA
            || !safe_symbol(&self.family_id)
            || !safe_symbol(&self.mount_id)
            || !safe_symbol(&self.reader_id)
            || !safe_symbol(&self.decrypt_provider_id)
            || self.sources.is_empty()
            || self.prefix.is_empty()
            || !self.prefix.ends_with(":/")
            || self.entries.len() > max_entries
        {
            return Err(error(
                "ASTRA_EMU_VFS_MANIFEST",
                "legacy VFS manifest identity is invalid",
            ));
        }
        let mut sources = BTreeMap::new();
        for source in &self.sources {
            if !safe_symbol(&source.source_id)
                || source
                    .archive_role
                    .as_deref()
                    .is_some_and(|role| !safe_symbol(role))
                || source.byte_size == 0
                || source.part_count == 0
                || sources.insert(source.source_id.as_str(), source).is_some()
            {
                return Err(error(
                    "ASTRA_EMU_VFS_SOURCE",
                    "legacy VFS source is invalid or duplicated",
                ));
            }
        }
        let mut uris = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            validate_legacy_vfs_uri(&self.prefix, &entry.uri)?;
            let source = sources.get(entry.source_id.as_str()).ok_or_else(|| {
                error(
                    "ASTRA_EMU_VFS_SOURCE_UNKNOWN",
                    "entry references an unknown source",
                )
            })?;
            let end = entry
                .source_offset
                .checked_add(entry.stored_size)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_VFS_ENTRY_OVERFLOW",
                        "entry source range overflowed",
                    )
                })?;
            if !uris.insert(entry.uri.as_str())
                || !ids.insert(entry.entry_id.as_str())
                || entry.entry_id.is_empty()
                || entry.entry_id.len() > 512
                || entry
                    .entry_id
                    .bytes()
                    .any(|byte| byte == 0 || byte.is_ascii_control())
                || !safe_method(&entry.method)
                || !safe_symbol(&entry.media_kind)
                || entry.stored_size == 0
                || entry.decoded_size == 0
                || end > source.byte_size
            {
                return Err(error(
                    "ASTRA_EMU_VFS_ENTRY",
                    "legacy VFS entry is invalid or duplicated",
                ));
            }
        }
        Ok(())
    }
}

pub fn validate_legacy_vfs_uri(prefix: &str, uri: &str) -> Result<(), LegacyCoreError> {
    let relative = uri.strip_prefix(prefix).unwrap_or_default();
    if !prefix.ends_with(":/")
        || !uri.starts_with(prefix)
        || relative.is_empty()
        || relative.starts_with('/')
        || relative.ends_with('/')
        || relative.contains(':')
        || uri.contains('\\')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
        || uri.bytes().any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(error(
            "ASTRA_EMU_VFS_URI",
            "legacy VFS URI is absolute, traverses, or is outside the mount prefix",
        ));
    }
    Ok(())
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_method(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

fn error(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> LegacyPackManifest {
        let source_hash = Hash256::from_sha256(b"source");
        LegacyPackManifest {
            schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
            family_id: "fixture".into(),
            mount_id: "fixture-main".into(),
            prefix: "fixture:/".into(),
            reader_id: "fixture.reader.v1".into(),
            reader_hash: Hash256::from_sha256(b"reader"),
            decrypt_provider_id: "fixture.decrypt.v1".into(),
            private_profile_hash: Hash256::from_sha256(b"private"),
            mount_profile_hash: Hash256::from_sha256(b"mount"),
            sources: vec![LegacyVfsSource {
                source_id: "scr".into(),
                archive_role: Some("scr".into()),
                byte_size: 16,
                part_count: 1,
                source_hash,
            }],
            entries: vec![LegacyVfsEntry {
                uri: "fixture:/scr/main.sc".into(),
                entry_id: "scr-0".into(),
                source_id: "scr".into(),
                source_offset: 8,
                stored_size: 8,
                decoded_size: 4,
                source_hash,
                content_hash: None,
                method: "raw".into(),
                media_kind: "script".into(),
            }],
        }
    }

    #[test]
    fn manifest_v2_accepts_distinct_source_and_content_hashes() {
        let mut manifest = manifest();
        manifest.entries[0].content_hash = Some(Hash256::from_sha256(b"plain"));
        manifest.validate(8).unwrap();
        assert_ne!(
            manifest.entries[0].source_hash,
            manifest.entries[0].content_hash.unwrap()
        );
    }

    #[test]
    fn manifest_rejects_unknown_source_duplicate_uri_and_bounds_overflow() {
        let mut unknown = manifest();
        unknown.entries[0].source_id = "missing".into();
        assert_eq!(
            unknown.validate(8).unwrap_err().code(),
            "ASTRA_EMU_VFS_SOURCE_UNKNOWN"
        );
        let mut duplicate = manifest();
        duplicate.entries.push(duplicate.entries[0].clone());
        assert_eq!(
            duplicate.validate(8).unwrap_err().code(),
            "ASTRA_EMU_VFS_ENTRY"
        );
        let mut overflow = manifest();
        overflow.entries[0].source_offset = u64::MAX;
        assert_eq!(
            overflow.validate(8).unwrap_err().code(),
            "ASTRA_EMU_VFS_ENTRY_OVERFLOW"
        );
    }

    #[test]
    fn traversal_and_absolute_style_uris_are_blocking() {
        assert_eq!(
            validate_legacy_vfs_uri("fixture:/", "fixture:/../secret")
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_URI"
        );
        assert_eq!(
            validate_legacy_vfs_uri("fixture:/", "other:/secret")
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_URI"
        );
    }
}
