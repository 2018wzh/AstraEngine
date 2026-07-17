use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use astra_byte_source::{ByteRange, ByteSourceStat, RangeReadResult, SourceRevision};
use astra_core::Hash256;
use astra_emu_family_api::{LegacyProviderError, LegacyVfsReader};
use astra_emu_manager_core::{
    CancellationToken, GrantedSourceEntry, GrantedSourceReader, SourceScanError,
};

use crate::android_platform::{self, AndroidDocumentEntry};

const MAX_ENTRIES: usize = 100_000;
const MAX_INDEX_BYTES: usize = 32 * 1024 * 1024;
const MAX_MOUNT_HASH_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Default)]
pub struct AndroidVfsRegistry {
    mounts: Mutex<BTreeMap<String, AndroidVfsMount>>,
}

struct AndroidVfsMount {
    files: BTreeMap<String, BoundDocument>,
    overlays: BTreeMap<String, Arc<[u8]>>,
}

#[derive(Clone)]
struct BoundDocument {
    document_uri: String,
    byte_size: u64,
    modified_ms: i64,
    revision: SourceRevision,
}

impl AndroidVfsRegistry {
    pub fn bind(&self, mount_set_id: &str, platform_token: &str) -> Result<(), String> {
        let entries =
            android_platform::enumerate_tree(platform_token, MAX_ENTRIES, MAX_INDEX_BYTES)?;
        let total_bytes = entries.iter().try_fold(0_u64, |total, entry| {
            total
                .checked_add(entry.byte_size)
                .ok_or_else(|| "ASTRA_EMU_ANDROID_VFS_BOUNDS".to_owned())
        })?;
        if total_bytes > MAX_MOUNT_HASH_BYTES {
            return Err("ASTRA_EMU_ANDROID_VFS_BOUNDS".into());
        }
        let mut files = BTreeMap::new();
        for entry in entries {
            let mut revision_material = entry.document_uri.as_bytes().to_vec();
            revision_material.extend_from_slice(&entry.modified_ms.to_le_bytes());
            revision_material.extend_from_slice(&entry.byte_size.to_le_bytes());
            let key = entry.relative_path.to_ascii_lowercase();
            if files
                .insert(
                    key,
                    BoundDocument {
                        document_uri: entry.document_uri,
                        byte_size: entry.byte_size,
                        modified_ms: entry.modified_ms,
                        revision: SourceRevision(Hash256::from_sha256(&revision_material)),
                    },
                )
                .is_some()
            {
                return Err("ASTRA_EMU_ANDROID_VFS_CASE_COLLISION".into());
            }
        }
        let mut mounts = self
            .mounts
            .lock()
            .map_err(|_| "ASTRA_EMU_ANDROID_VFS_LOCK")?;
        if mounts
            .insert(
                mount_set_id.into(),
                AndroidVfsMount {
                    files,
                    overlays: BTreeMap::new(),
                },
            )
            .is_some()
        {
            return Err("ASTRA_EMU_VFS_MOUNT_DUPLICATE".into());
        }
        Ok(())
    }

    pub fn install_overlays(
        &self,
        mount_set_id: &str,
        overlays: BTreeMap<String, Vec<u8>>,
    ) -> Result<(), String> {
        if overlays.len() > 4096 {
            return Err("ASTRA_EMU_VFS_OVERLAY_COUNT".into());
        }
        let mut normalized = BTreeMap::new();
        let mut total_bytes = 0_usize;
        for (path, bytes) in overlays {
            validate_relative_path(&path).map_err(|_| "ASTRA_EMU_VFS_OVERLAY_PATH")?;
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| "ASTRA_EMU_VFS_OVERLAY_BOUNDS".to_owned())?;
            if total_bytes > 64 * 1024 * 1024 {
                return Err("ASTRA_EMU_VFS_OVERLAY_BOUNDS".into());
            }
            if normalized
                .insert(path.to_ascii_lowercase(), Arc::from(bytes))
                .is_some()
            {
                return Err("ASTRA_EMU_VFS_OVERLAY_COLLISION".into());
            }
        }
        let mut mounts = self
            .mounts
            .lock()
            .map_err(|_| "ASTRA_EMU_ANDROID_VFS_LOCK")?;
        let mount = mounts
            .get_mut(mount_set_id)
            .ok_or("ASTRA_EMU_VFS_MOUNT_MISSING")?;
        if !mount.overlays.is_empty() {
            return Err("ASTRA_EMU_VFS_OVERLAY_ALREADY_INSTALLED".into());
        }
        mount.overlays = normalized;
        Ok(())
    }

    pub fn unbind(&self, mount_set_id: &str) {
        if let Ok(mut mounts) = self.mounts.lock() {
            mounts.remove(mount_set_id);
        }
    }
}

impl LegacyVfsReader for AndroidVfsRegistry {
    fn stat_file(
        &self,
        mount_set_id: &str,
        uri: &str,
    ) -> Result<ByteSourceStat, LegacyProviderError> {
        validate_relative_path(uri).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_PATH_INVALID", "VFS URI is unsafe")
        })?;
        let mounts = self.mounts.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_ANDROID_VFS_LOCK", "VFS lock is poisoned")
        })?;
        let mount = mounts.get(mount_set_id).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_MOUNT_MISSING", "VFS mount is not active")
        })?;
        if let Some(bytes) = mount.overlays.get(&uri.to_ascii_lowercase()) {
            return Ok(ByteSourceStat {
                len: bytes.len() as u64,
                revision: SourceRevision(Hash256::from_sha256(bytes)),
            });
        }
        let bound = mount
            .files
            .get(&uri.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                LegacyProviderError::invalid("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry is not present")
            })?;
        Ok(ByteSourceStat {
            len: bound.byte_size,
            revision: bound.revision,
        })
    }

    fn read_file_range(
        &self,
        mount_set_id: &str,
        uri: &str,
        expected_revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, LegacyProviderError> {
        let stat = self.stat_file(mount_set_id, uri)?;
        range.validate(stat.len, max_bytes).map_err(|error| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_BOUNDS", error.to_string())
        })?;
        if stat.revision != expected_revision {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REVISION",
                "VFS revision changed",
            ));
        }
        let mounts = self.mounts.lock().map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_ANDROID_VFS_LOCK", "VFS lock is poisoned")
        })?;
        let mount = mounts.get(mount_set_id).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_MOUNT_MISSING", "VFS mount is not active")
        })?;
        let bytes = if let Some(bytes) = mount.overlays.get(&uri.to_ascii_lowercase()) {
            bytes[range.offset as usize..(range.offset + range.len) as usize].to_vec()
        } else {
            let bound = mount
                .files
                .get(&uri.to_ascii_lowercase())
                .cloned()
                .ok_or_else(|| {
                    LegacyProviderError::invalid(
                        "ASTRA_EMU_VFS_NOT_FOUND",
                        "VFS entry is not present",
                    )
                })?;
            drop(mounts);
            android_platform::read_document_range(
                &bound.document_uri,
                bound.byte_size,
                bound.modified_ms,
                range.offset,
                range.len as u32,
            )
            .map_err(|_| {
                LegacyProviderError::invalid(
                    "ASTRA_EMU_ANDROID_VFS_RANGE_READ",
                    "SAF document range read failed",
                )
            })?
        };
        Ok(RangeReadResult {
            range,
            revision: stat.revision,
            content_hash: Hash256::from_sha256(&bytes),
            bytes,
        })
    }
}

pub struct AndroidGrantedSource {
    tree_uri: String,
    index: Mutex<BTreeMap<String, AndroidDocumentEntry>>,
}

impl AndroidGrantedSource {
    pub fn new(platform_token: &str) -> Result<Self, SourceScanError> {
        if !platform_token.starts_with("content://") || platform_token.len() > 8192 {
            return Err(SourceScanError::InvalidPath);
        }
        Ok(Self {
            tree_uri: platform_token.into(),
            index: Mutex::new(BTreeMap::new()),
        })
    }
}

impl GrantedSourceReader for AndroidGrantedSource {
    fn enumerate(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<GrantedSourceEntry>, SourceScanError> {
        if cancellation.is_cancelled() {
            return Err(SourceScanError::Cancelled);
        }
        let documents =
            android_platform::enumerate_tree(&self.tree_uri, MAX_ENTRIES, MAX_INDEX_BYTES)
                .map_err(|_| SourceScanError::Enumeration)?;
        let mut index = BTreeMap::new();
        let mut entries = Vec::with_capacity(documents.len());
        for document in documents {
            if cancellation.is_cancelled() {
                return Err(SourceScanError::Cancelled);
            }
            let key = document.relative_path.to_ascii_lowercase();
            entries.push(GrantedSourceEntry {
                relative_path: document.relative_path.clone(),
                modified_ns: document.modified_ms.saturating_mul(1_000_000),
                byte_size: document.byte_size,
                is_file: true,
            });
            if index.insert(key, document).is_some() {
                return Err(SourceScanError::InvalidPath);
            }
        }
        *self
            .index
            .lock()
            .map_err(|_| SourceScanError::Enumeration)? = index;
        Ok(entries)
    }

    fn read_file(&self, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, SourceScanError> {
        validate_relative_path(relative_path)?;
        let document = self
            .index
            .lock()
            .map_err(|_| SourceScanError::Read)?
            .get(&relative_path.to_ascii_lowercase())
            .cloned()
            .ok_or(SourceScanError::Read)?;
        if document.byte_size > max_bytes {
            return Err(SourceScanError::ScriptBounds);
        }
        let bytes = android_platform::read_document(&document.document_uri, max_bytes)
            .map_err(|_| SourceScanError::Read)?;
        if bytes.len() as u64 != document.byte_size {
            return Err(SourceScanError::Read);
        }
        Ok(bytes)
    }
}

fn validate_relative_path(path: &str) -> Result<(), SourceScanError> {
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || path
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(SourceScanError::InvalidPath);
    }
    Ok(())
}
