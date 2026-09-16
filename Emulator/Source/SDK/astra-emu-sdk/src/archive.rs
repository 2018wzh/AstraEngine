use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

use astra_byte_source::OwnedByteBuffer;
use astra_core::{is_safe_symbol as safe_symbol, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CoreError;

pub const ARCHIVE_MAX_READ_BYTES: u64 = 64 * 1024 * 1024;
pub const ARCHIVE_MANIFEST_SCHEMA: &str = "astra.emu.archive_manifest.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveNodeKind {
    Directory,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArchiveNode {
    pub uri: String,
    pub name: String,
    pub kind: ArchiveNodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArchiveStat {
    pub uri: String,
    pub entry_id: Option<String>,
    pub kind: ArchiveNodeKind,
    pub size: u64,
    pub archive_role: Option<String>,
    pub method: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ArchiveReadResult {
    pub uri: String,
    pub offset: u64,
    pub bytes: OwnedByteBuffer,
    pub eof: bool,
    pub cache_hit: bool,
}

pub trait ArchiveStream: Read + Send {}
impl<T: Read + Send> ArchiveStream for T {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArchiveSource {
    pub source_id: String,
    pub archive_role: Option<String>,
    pub byte_size: u64,
    pub part_count: u32,
    pub source_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArchiveEntry {
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
pub struct ArchiveManifest {
    pub schema: String,
    pub family_id: String,
    pub mount_id: String,
    pub prefix: String,
    pub reader_id: String,
    pub reader_hash: Hash256,
    pub decrypt_provider_id: String,
    pub private_profile_hash: Hash256,
    pub mount_profile_hash: Hash256,
    pub sources: Vec<ArchiveSource>,
    pub entries: Vec<ArchiveEntry>,
}

impl ArchiveManifest {
    pub fn validate(&self, max_entries: usize) -> Result<(), CoreError> {
        if self.schema != ARCHIVE_MANIFEST_SCHEMA
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
                "archive manifest identity is invalid",
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
                    "archive source is invalid or duplicated",
                ));
            }
        }
        let mut uris = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            validate_archive_uri(&self.prefix, &entry.uri)?;
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
                    "archive entry is invalid or duplicated",
                ));
            }
        }
        Ok(())
    }
}

pub fn validate_archive_uri(prefix: &str, uri: &str) -> Result<(), CoreError> {
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
            "archive URI is absolute, traverses, or is outside the mount prefix",
        ));
    }
    Ok(())
}

/// Validates a directory URI without weakening the file URI contract.
///
/// Directory callers may address the mount root and may include one trailing
/// slash. Interior empty components, traversal and alternate separators remain
/// invalid.
pub fn validate_archive_directory_uri(prefix: &str, uri: &str) -> Result<(), CoreError> {
    if uri == prefix {
        return Ok(());
    }
    let canonical = uri.strip_suffix('/').unwrap_or(uri);
    validate_archive_uri(prefix, canonical)
}

fn safe_method(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

fn error(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}
