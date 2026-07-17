use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{CancellationToken, GrantedSourceEntry, GrantedSourceReader, SourceScanError};
use astra_byte_source::{
    AccessedResourceLedger, ByteRange, ByteSourceStat, RangeReadResult, SourceRevision,
    DEFAULT_MAX_RANGE_BYTES,
};
use astra_core::Hash256;
use astra_emu_family_api::{LegacyProviderError, LegacyVfsReader};
use sha2::{Digest, Sha256};

const MAX_ENUMERATED_ENTRIES: usize = 100_001;
const AUDIT_RANGE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfsAccessMetrics {
    pub resource_count: u64,
    pub unique_range_count: u64,
    pub read_count: u64,
    pub bytes_read: u64,
    pub max_range_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfsAuditSummary {
    pub resource_count: u64,
    pub range_count: u64,
    pub bytes_read: u64,
    pub max_range_bytes: u64,
    pub manifest_hash: Hash256,
}

#[derive(Default)]
pub struct DesktopVfsRegistry {
    mounts: Mutex<BTreeMap<String, DesktopVfsMount>>,
}

struct DesktopVfsMount {
    root: PathBuf,
    files: BTreeMap<String, BoundFile>,
    overlays: BTreeMap<String, Arc<[u8]>>,
    ledger: Mutex<AccessedResourceLedger>,
}

#[derive(Clone)]
struct BoundFile {
    relative: PathBuf,
    byte_size: u64,
    revision: SourceRevision,
}

impl DesktopVfsRegistry {
    pub fn bind(&self, mount_set_id: &str, platform_token: &str) -> Result<(), String> {
        let root = fs::canonicalize(PathBuf::from(platform_token))
            .map_err(|_| "ASTRA_EMU_VFS_ROOT_INVALID")?;
        if !root.is_dir() {
            return Err("ASTRA_EMU_VFS_ROOT_INVALID".into());
        }
        let mut files = BTreeMap::new();
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            let entries = fs::read_dir(&directory).map_err(|_| "ASTRA_EMU_VFS_ENUMERATION")?;
            for entry in entries {
                let entry = entry.map_err(|_| "ASTRA_EMU_VFS_ENUMERATION")?;
                let kind = entry.file_type().map_err(|_| "ASTRA_EMU_VFS_ENUMERATION")?;
                if kind.is_symlink() {
                    return Err("ASTRA_EMU_VFS_SYMLINK_UNSUPPORTED".into());
                }
                if kind.is_dir() {
                    pending.push(entry.path());
                } else if kind.is_file() {
                    let relative = entry
                        .path()
                        .strip_prefix(&root)
                        .map_err(|_| "ASTRA_EMU_VFS_PATH_INVALID")?
                        .to_path_buf();
                    let key = relative_path_string(&relative)
                        .map_err(|_| "ASTRA_EMU_VFS_PATH_INVALID")?
                        .to_ascii_lowercase();
                    let metadata = entry.metadata().map_err(|_| "ASTRA_EMU_VFS_METADATA")?;
                    let revision =
                        revision_from_metadata(&metadata).map_err(|_| "ASTRA_EMU_VFS_METADATA")?;
                    if files
                        .insert(
                            key,
                            BoundFile {
                                relative,
                                byte_size: metadata.len(),
                                revision,
                            },
                        )
                        .is_some()
                    {
                        return Err("ASTRA_EMU_VFS_CASE_COLLISION".into());
                    }
                    if files.len() > MAX_ENUMERATED_ENTRIES {
                        return Err("ASTRA_EMU_VFS_ENTRY_BOUNDS".into());
                    }
                }
            }
        }
        let mut mounts = self
            .mounts
            .lock()
            .map_err(|_| "ASTRA_EMU_VFS_REGISTRY_LOCK")?;
        if mounts
            .insert(
                mount_set_id.to_owned(),
                DesktopVfsMount {
                    root,
                    files,
                    overlays: BTreeMap::new(),
                    ledger: Mutex::new(AccessedResourceLedger::default()),
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
            .map_err(|_| "ASTRA_EMU_VFS_REGISTRY_LOCK")?;
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

    pub fn access_metrics(&self, mount_set_id: &str) -> Result<VfsAccessMetrics, String> {
        let mounts = self
            .mounts
            .lock()
            .map_err(|_| "ASTRA_EMU_VFS_REGISTRY_LOCK".to_owned())?;
        let mount = mounts
            .get(mount_set_id)
            .ok_or_else(|| "ASTRA_EMU_VFS_MOUNT_MISSING".to_owned())?;
        let ledger = mount
            .ledger
            .lock()
            .map_err(|_| "ASTRA_EMU_VFS_LEDGER_LOCK".to_owned())?;
        Ok(VfsAccessMetrics {
            resource_count: ledger.unique_resource_count(),
            unique_range_count: ledger.unique_range_count(),
            read_count: ledger.read_count(),
            bytes_read: ledger.bytes_read(),
            max_range_bytes: ledger.max_range_bytes(),
        })
    }

    pub fn audit_mount(&self, mount_set_id: &str) -> Result<VfsAuditSummary, String> {
        let resource_ids = {
            let mounts = self
                .mounts
                .lock()
                .map_err(|_| "ASTRA_EMU_VFS_REGISTRY_LOCK".to_owned())?;
            let mount = mounts
                .get(mount_set_id)
                .ok_or_else(|| "ASTRA_EMU_VFS_MOUNT_MISSING".to_owned())?;
            mount
                .files
                .keys()
                .chain(mount.overlays.keys())
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        let mut manifest = Sha256::new();
        let mut range_count = 0_u64;
        let mut bytes_read = 0_u64;
        let mut max_range_bytes = 0_u64;
        for resource_id in &resource_ids {
            let stat = self
                .stat_file(mount_set_id, resource_id)
                .map_err(|error| error.code().to_owned())?;
            let mut content = Sha256::new();
            let mut offset = 0_u64;
            while offset < stat.len {
                let len = (stat.len - offset).min(AUDIT_RANGE_BYTES);
                let result = self
                    .read_file_range(
                        mount_set_id,
                        resource_id,
                        stat.revision,
                        ByteRange { offset, len },
                        AUDIT_RANGE_BYTES,
                    )
                    .map_err(|error| error.code().to_owned())?;
                content.update(&result.bytes);
                range_count = range_count
                    .checked_add(1)
                    .ok_or_else(|| "ASTRA_EMU_VFS_AUDIT_RANGE_OVERFLOW".to_owned())?;
                bytes_read = bytes_read
                    .checked_add(len)
                    .ok_or_else(|| "ASTRA_EMU_VFS_AUDIT_BYTE_OVERFLOW".to_owned())?;
                max_range_bytes = max_range_bytes.max(len);
                offset = offset
                    .checked_add(len)
                    .ok_or_else(|| "ASTRA_EMU_VFS_AUDIT_RANGE_OVERFLOW".to_owned())?;
            }
            let after = self
                .stat_file(mount_set_id, resource_id)
                .map_err(|error| error.code().to_owned())?;
            if after != stat {
                return Err("ASTRA_EMU_VFS_AUDIT_REVISION_DRIFT".into());
            }
            let content_hash = Hash256::from_bytes(content.finalize().into());
            let id = resource_id.as_bytes();
            manifest.update(
                u64::try_from(id.len())
                    .map_err(|_| "ASTRA_EMU_VFS_AUDIT_ID_BOUNDS")?
                    .to_le_bytes(),
            );
            manifest.update(id);
            manifest.update(stat.len.to_le_bytes());
            manifest.update(stat.revision.0.as_bytes());
            manifest.update(content_hash.as_bytes());
        }
        Ok(VfsAuditSummary {
            resource_count: u64::try_from(resource_ids.len())
                .map_err(|_| "ASTRA_EMU_VFS_AUDIT_RESOURCE_BOUNDS")?,
            range_count,
            bytes_read,
            max_range_bytes,
            manifest_hash: Hash256::from_bytes(manifest.finalize().into()),
        })
    }

    fn record_access(
        &self,
        mount_set_id: &str,
        resource_id: &str,
        result: &RangeReadResult,
    ) -> Result<(), LegacyProviderError> {
        let mounts = self.mounts.lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REGISTRY_LOCK",
                "desktop VFS registry lock is poisoned",
            )
        })?;
        let mount = mounts.get(mount_set_id).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MOUNT_MISSING",
                "desktop VFS mount is not active",
            )
        })?;
        let mut ledger = mount.ledger.lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_LEDGER_LOCK",
                "desktop VFS access ledger lock is poisoned",
            )
        })?;
        ledger.record(resource_id, result).map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REPEAT_MISMATCH",
                "a previously observed VFS range changed content",
            )
        })?;
        Ok(())
    }
}

impl LegacyVfsReader for DesktopVfsRegistry {
    fn stat_file(
        &self,
        mount_set_id: &str,
        uri: &str,
    ) -> Result<ByteSourceStat, LegacyProviderError> {
        validate_relative_path(uri).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_PATH_INVALID", "VFS URI is unsafe")
        })?;
        let mounts = self.mounts.lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REGISTRY_LOCK",
                "desktop VFS registry lock is poisoned",
            )
        })?;
        let mount = mounts.get(mount_set_id).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MOUNT_MISSING",
                "desktop VFS mount is not active",
            )
        })?;
        if let Some(bytes) = mount.overlays.get(&uri.to_ascii_lowercase()) {
            return Ok(ByteSourceStat {
                len: bytes.len() as u64,
                revision: SourceRevision(Hash256::from_sha256(bytes)),
            });
        }
        let root = mount.root.clone();
        let bound = mount
            .files
            .get(&uri.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                LegacyProviderError::invalid("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry is not present")
            })?;
        drop(mounts);
        let path = root.join(&bound.relative);
        let link_metadata = fs::symlink_metadata(&path).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_READ", "VFS metadata read failed")
        })?;
        if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MUTATED",
                "VFS entry type changed after the mount was bound",
            ));
        }
        let canonical = fs::canonicalize(&path).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_READ", "VFS path resolution failed")
        })?;
        let revision = revision_from_metadata(&link_metadata).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_METADATA", "VFS metadata is invalid")
        })?;
        if !canonical.starts_with(&root)
            || link_metadata.len() != bound.byte_size
            || revision != bound.revision
        {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MUTATED",
                "VFS entry identity changed after the mount was bound",
            ));
        }
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
        if range.len > max_bytes || range.len > DEFAULT_MAX_RANGE_BYTES {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_BOUNDS",
                "VFS range exceeds the requested byte bound",
            ));
        }
        let stat = self.stat_file(mount_set_id, uri)?;
        if stat.revision != expected_revision {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REVISION_MISMATCH",
                "VFS entry revision changed",
            ));
        }
        let end = range.offset.checked_add(range.len).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_RANGE_OVERFLOW", "VFS range overflowed")
        })?;
        if end > stat.len {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_RANGE_BOUNDS",
                "VFS range exceeds the entry length",
            ));
        }
        let mounts = self.mounts.lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REGISTRY_LOCK",
                "desktop VFS registry lock is poisoned",
            )
        })?;
        let mount = mounts.get(mount_set_id).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MOUNT_MISSING",
                "desktop VFS mount is not active",
            )
        })?;
        if let Some(overlay) = mount.overlays.get(&uri.to_ascii_lowercase()) {
            let start = usize::try_from(range.offset).map_err(|_| {
                LegacyProviderError::invalid("ASTRA_EMU_VFS_RANGE_BOUNDS", "VFS range is invalid")
            })?;
            let end = usize::try_from(end).map_err(|_| {
                LegacyProviderError::invalid("ASTRA_EMU_VFS_RANGE_BOUNDS", "VFS range is invalid")
            })?;
            let bytes = overlay[start..end].to_vec();
            let result = RangeReadResult {
                range,
                revision: expected_revision,
                content_hash: Hash256::from_sha256(&bytes),
                bytes,
            };
            drop(mounts);
            self.record_access(mount_set_id, &uri.to_ascii_lowercase(), &result)?;
            return Ok(result);
        }
        let root = mount.root.clone();
        let bound = mount
            .files
            .get(&uri.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                LegacyProviderError::invalid("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry is not present")
            })?;
        drop(mounts);
        if bound.revision != expected_revision {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REVISION_MISMATCH",
                "VFS entry revision changed",
            ));
        }
        let path = root.join(&bound.relative);
        let mut file = fs::File::open(&path).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_READ", "VFS entry open failed")
        })?;
        file.seek(SeekFrom::Start(range.offset)).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_READ", "VFS range seek failed")
        })?;
        let len = usize::try_from(range.len).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_BOUNDS", "VFS range is too large")
        })?;
        let mut bytes = vec![0; len];
        file.read_exact(&mut bytes).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_SHORT_READ", "VFS range read was short")
        })?;
        let after = self.stat_file(mount_set_id, uri)?;
        if after != stat {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_CHANGED_DURING_READ",
                "VFS entry changed while it was being read",
            ));
        }
        let result = RangeReadResult {
            range,
            revision: stat.revision,
            content_hash: Hash256::from_sha256(&bytes),
            bytes,
        };
        self.record_access(mount_set_id, &uri.to_ascii_lowercase(), &result)?;
        Ok(result)
    }
}

fn revision_from_metadata(metadata: &fs::Metadata) -> Result<SourceRevision, std::io::Error> {
    let modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut material = Vec::with_capacity(24);
    material.extend_from_slice(&metadata.len().to_le_bytes());
    material.extend_from_slice(&modified.as_nanos().to_le_bytes());
    Ok(SourceRevision(Hash256::from_sha256(&material)))
}

pub struct DesktopGrantedSource {
    root: PathBuf,
}

impl DesktopGrantedSource {
    pub fn new(platform_token: &str) -> Result<Self, SourceScanError> {
        let root = fs::canonicalize(PathBuf::from(platform_token))
            .map_err(|_| SourceScanError::Enumeration)?;
        if !root.is_dir() {
            return Err(SourceScanError::Enumeration);
        }
        Ok(Self { root })
    }

    fn resolved_file(&self, relative_path: &str) -> Result<PathBuf, SourceScanError> {
        validate_relative_path(relative_path)?;
        let candidate = self
            .root
            .join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let resolved = fs::canonicalize(candidate).map_err(|_| SourceScanError::Read)?;
        if !resolved.starts_with(&self.root) || !resolved.is_file() {
            return Err(SourceScanError::InvalidPath);
        }
        Ok(resolved)
    }
}

impl GrantedSourceReader for DesktopGrantedSource {
    fn enumerate(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<GrantedSourceEntry>, SourceScanError> {
        let mut pending = vec![self.root.clone()];
        let mut entries = Vec::new();
        while let Some(directory) = pending.pop() {
            if cancellation.is_cancelled() {
                return Err(SourceScanError::Cancelled);
            }
            let children = fs::read_dir(directory).map_err(|_| SourceScanError::Enumeration)?;
            for child in children {
                let child = child.map_err(|_| SourceScanError::Enumeration)?;
                let file_type = child
                    .file_type()
                    .map_err(|_| SourceScanError::Enumeration)?;
                if file_type.is_symlink() {
                    return Err(SourceScanError::InvalidPath);
                }
                let path = child.path();
                if file_type.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let metadata = child.metadata().map_err(|_| SourceScanError::Enumeration)?;
                let relative = path
                    .strip_prefix(&self.root)
                    .map_err(|_| SourceScanError::InvalidPath)?;
                let relative = relative_path_string(relative)?;
                let modified_ns = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
                    .unwrap_or(0);
                entries.push(GrantedSourceEntry {
                    relative_path: relative,
                    modified_ns,
                    byte_size: metadata.len(),
                    is_file: true,
                });
                if entries.len() >= MAX_ENUMERATED_ENTRIES {
                    return Err(SourceScanError::EntryBounds);
                }
            }
        }
        Ok(entries)
    }

    fn read_file(&self, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, SourceScanError> {
        let path = self.resolved_file(relative_path)?;
        let metadata = fs::metadata(&path).map_err(|_| SourceScanError::Read)?;
        if metadata.len() > max_bytes {
            return Err(SourceScanError::ScriptBounds);
        }
        let bytes = fs::read(path).map_err(|_| SourceScanError::Read)?;
        if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > max_bytes {
            return Err(SourceScanError::Read);
        }
        Ok(bytes)
    }
}

fn relative_path_string(path: &Path) -> Result<String, SourceScanError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(SourceScanError::InvalidPath);
        };
        let part = part.to_str().ok_or(SourceScanError::InvalidPath)?;
        if part.is_empty() || part == "." || part == ".." {
            return Err(SourceScanError::InvalidPath);
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(SourceScanError::InvalidPath);
    }
    Ok(parts.join("/"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_read_accepts_a_large_operation_budget_without_exceeding_the_chunk_limit() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("large.bin"), b"metadata").unwrap();
        let registry = DesktopVfsRegistry::default();
        registry
            .bind("mount.range-budget", directory.path().to_str().unwrap())
            .unwrap();
        let stat = registry
            .stat_file("mount.range-budget", "large.bin")
            .unwrap();
        let result = registry
            .read_file_range(
                "mount.range-budget",
                "large.bin",
                stat.revision,
                ByteRange { offset: 0, len: 8 },
                512 * 1024 * 1024,
            )
            .unwrap();
        assert_eq!(result.bytes, b"metadata");
    }

    #[test]
    fn bound_mount_rejects_same_size_content_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("SCRIPT.BIN");
        fs::write(&path, b"before").unwrap();
        let registry = DesktopVfsRegistry::default();
        registry
            .bind("mount.test", directory.path().to_str().unwrap())
            .unwrap();
        assert_eq!(
            registry
                .read_file("mount.test", "script.bin", 1024)
                .unwrap(),
            b"before"
        );
        fs::write(path, b"mutate").unwrap();
        let error = registry
            .read_file("mount.test", "script.bin", 1024)
            .unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_VFS_MUTATED");
    }

    #[test]
    fn trusted_overlay_is_mount_scoped_and_case_insensitive() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("script.bin"), b"base").unwrap();
        let registry = DesktopVfsRegistry::default();
        registry
            .bind("mount.test", directory.path().to_str().unwrap())
            .unwrap();
        registry
            .install_overlays(
                "mount.test",
                [("SCRIPT.BIN".into(), b"patched".to_vec())]
                    .into_iter()
                    .collect(),
            )
            .unwrap();
        assert_eq!(
            registry
                .read_file("mount.test", "script.bin", 1024)
                .unwrap(),
            b"patched"
        );
        registry.unbind("mount.test");
        assert!(registry
            .read_file("mount.test", "script.bin", 1024)
            .is_err());
    }

    #[test]
    fn access_ledger_and_full_audit_are_bounded_and_path_redacted() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("script.bin"), b"script").unwrap();
        fs::write(directory.path().join("voice.ogg"), b"voice-bytes").unwrap();
        let registry = DesktopVfsRegistry::default();
        registry
            .bind("mount.audit", directory.path().to_str().unwrap())
            .unwrap();
        let stat = registry.stat_file("mount.audit", "script.bin").unwrap();
        registry
            .read_file_range(
                "mount.audit",
                "script.bin",
                stat.revision,
                ByteRange { offset: 0, len: 3 },
                DEFAULT_MAX_RANGE_BYTES,
            )
            .unwrap();
        let startup = registry.access_metrics("mount.audit").unwrap();
        assert_eq!(startup.resource_count, 1);
        assert_eq!(startup.unique_range_count, 1);
        assert_eq!(startup.read_count, 1);
        assert_eq!(startup.bytes_read, 3);
        assert_eq!(startup.max_range_bytes, 3);

        let first = registry.audit_mount("mount.audit").unwrap();
        let second = registry.audit_mount("mount.audit").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.resource_count, 2);
        assert_eq!(first.range_count, 2);
        assert_eq!(first.bytes_read, 17);
        assert!(first.max_range_bytes <= AUDIT_RANGE_BYTES);
        assert!(!first.manifest_hash.to_string().contains("script.bin"));
    }
}
