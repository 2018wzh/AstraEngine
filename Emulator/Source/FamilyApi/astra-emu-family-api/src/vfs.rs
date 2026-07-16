use std::collections::BTreeSet;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{validate_symbol, LegacyProviderError};

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
            if !entry.uri.starts_with(&self.prefix)
                || entry.uri.contains("..")
                || entry.uri.contains('\\')
            {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_VFS_URI",
                    "legacy pack entry URI is outside the declared prefix",
                ));
            }
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
