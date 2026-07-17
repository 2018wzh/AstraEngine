use std::{collections::BTreeSet, io::Read};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{validate_symbol, LegacyProviderError};

pub const LEGACY_VFS_MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyPackMountRequest {
    pub mount_id: String,
    pub prefix: String,
    pub trusted_patch_hash: Hash256,
    pub decoder_id: String,
}

pub trait LegacyVfsStream: Read + Send {}
impl<T: Read + Send> LegacyVfsStream for T {}

pub trait LegacyMountedVfs: Send + Sync {
    fn mount_id(&self) -> &str;
    fn manifest(&self) -> &LegacyPackManifest;
    fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyProviderError>;
    fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyProviderError>;
    fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<LegacyVfsReadResult, LegacyProviderError>;
    fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, LegacyProviderError>;
}

pub trait LegacyPackMountProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn mount(
        &self,
        request: &LegacyPackMountRequest,
    ) -> Result<Box<dyn LegacyMountedVfs>, LegacyProviderError>;
    fn unmount(&self, mount_id: &str) -> Result<(), LegacyProviderError>;
}

pub fn validate_legacy_vfs_uri(prefix: &str, uri: &str) -> Result<(), LegacyProviderError> {
    if !prefix.ends_with(":/")
        || !uri.starts_with(prefix)
        || uri.contains('\\')
        || uri.split('/').any(|part| part == ".." || part == ".")
        || uri.bytes().any(|byte| byte == 0)
    {
        return Err(LegacyProviderError::invalid(
            "ASTRA_EMU_VFS_URI",
            "legacy VFS URI is absolute, traverses, or is outside the mount prefix",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyVfsEntry {
    pub uri: String,
    pub entry_id: String,
    pub offset: u64,
    pub size: u64,
    pub content_hash: Hash256,
    pub media_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyPackManifest {
    pub mount_id: String,
    pub prefix: String,
    pub reader_id: String,
    pub reader_hash: Hash256,
    pub entries: Vec<LegacyVfsEntry>,
}

impl LegacyPackManifest {
    pub fn validate(
        &self,
        source_size: u64,
        max_entries: usize,
    ) -> Result<(), LegacyProviderError> {
        validate_symbol("mount_id", &self.mount_id)?;
        validate_symbol("reader_id", &self.reader_id)?;
        if self.prefix.is_empty() || !self.prefix.ends_with(":/") {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_PREFIX",
                "legacy VFS prefix must end with :/",
            ));
        }
        if self.entries.len() > max_entries {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_ENTRY_COUNT",
                "legacy pack entry count exceeds the configured bound",
            ));
        }
        let mut uris = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            validate_legacy_vfs_uri(&self.prefix, &entry.uri)?;
            if !uris.insert(entry.uri.as_str()) || !ids.insert(entry.entry_id.as_str()) {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_VFS_ENTRY_DUPLICATE",
                    "legacy pack contains a duplicate URI or entry id",
                ));
            }
            let end = entry.offset.checked_add(entry.size).ok_or_else(|| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_VFS_ENTRY_OVERFLOW",
                    "entry bounds overflowed",
                )
            })?;
            if end > source_size {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_VFS_ENTRY_BOUNDS",
                    "entry extends beyond the pack source",
                ));
            }
        }
        Ok(())
    }
}
