use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use astra_byte_source::{ByteRange, ByteSourceStat, RangeReadResult, SourceRevision};
use astra_core::Hash256;
use astra_emu_family_api::{LegacyProviderError, LegacyVfsListedFile, LegacyVfsReader};
use astra_emu_family_core::{LegacyMountedVfs, LegacyVfsNodeKind};

/// Adapts an in-process family mount to the ABI-safe runtime reader contract.
/// The adapter preserves the mounted URI namespace and never exposes source paths.
pub struct LegacyMountedVfsReaderAdapter {
    mount_set_id: String,
    vfs: Arc<dyn LegacyMountedVfs>,
    access: Mutex<RuntimeVfsAccessState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeVfsAccessMetrics {
    pub resource_count: u64,
    pub unique_range_count: u64,
    pub read_count: u64,
    pub bytes_read: u64,
    pub max_range_bytes: u64,
}

#[derive(Default)]
struct RuntimeVfsAccessState {
    resources: BTreeSet<Hash256>,
    ranges: BTreeSet<(Hash256, u64, u64)>,
    read_count: u64,
    bytes_read: u64,
    max_range_bytes: u64,
}

impl LegacyMountedVfsReaderAdapter {
    pub fn new(
        mount_set_id: impl Into<String>,
        vfs: Arc<dyn LegacyMountedVfs>,
    ) -> Result<Self, LegacyProviderError> {
        let mount_set_id = mount_set_id.into();
        if mount_set_id.is_empty()
            || mount_set_id.len() > 128
            || mount_set_id
                .bytes()
                .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_MOUNT_ID",
                "runtime mount set id is invalid",
            ));
        }
        vfs.manifest().validate(10_000_000).map_err(core_error)?;
        Ok(Self {
            mount_set_id,
            vfs,
            access: Mutex::new(RuntimeVfsAccessState::default()),
        })
    }

    pub fn mounted_vfs(&self) -> &Arc<dyn LegacyMountedVfs> {
        &self.vfs
    }

    pub fn access_metrics(&self) -> Result<RuntimeVfsAccessMetrics, LegacyProviderError> {
        let access = self.access.lock().map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_RUNTIME_METRICS_POISONED",
                "runtime VFS access metrics are poisoned",
            )
        })?;
        Ok(RuntimeVfsAccessMetrics {
            resource_count: access.resources.len() as u64,
            unique_range_count: access.ranges.len() as u64,
            read_count: access.read_count,
            bytes_read: access.bytes_read,
            max_range_bytes: access.max_range_bytes,
        })
    }

    fn validate_mount(&self, mount_set_id: &str) -> Result<(), LegacyProviderError> {
        if mount_set_id != self.mount_set_id {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_MOUNT_MISMATCH",
                "runtime VFS request targets a different mount set",
            ));
        }
        Ok(())
    }

    fn stat_and_revision(
        &self,
        uri: &str,
    ) -> Result<(ByteSourceStat, astra_emu_family_core::LegacyVfsStat), LegacyProviderError> {
        let stat = self.vfs.stat(uri).map_err(core_error)?;
        if stat.kind != LegacyVfsNodeKind::File {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_FILE_REQUIRED",
                "runtime VFS request requires a file URI",
            ));
        }
        let entry = self
            .vfs
            .manifest()
            .entries
            .iter()
            .find(|entry| entry.uri == uri)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_VFS_RUNTIME_MANIFEST_ENTRY",
                    "runtime VFS file is absent from the validated manifest",
                )
            })?;
        let material = format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            self.vfs.manifest().reader_hash,
            self.vfs.manifest().mount_profile_hash,
            entry.source_hash,
            entry.entry_id,
            entry.decoded_size,
            entry.method
        );
        Ok((
            ByteSourceStat {
                len: stat.size,
                revision: SourceRevision(Hash256::from_sha256(material.as_bytes())),
            },
            stat,
        ))
    }
}

impl LegacyVfsReader for LegacyMountedVfsReaderAdapter {
    fn stat_file(
        &self,
        mount_set_id: &str,
        uri: &str,
    ) -> Result<ByteSourceStat, LegacyProviderError> {
        self.validate_mount(mount_set_id)?;
        self.stat_and_revision(uri).map(|value| value.0)
    }

    fn read_file_range(
        &self,
        mount_set_id: &str,
        uri: &str,
        expected_revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, LegacyProviderError> {
        self.validate_mount(mount_set_id)?;
        let (before, _) = self.stat_and_revision(uri)?;
        range.validate(before.len, max_bytes).map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_RUNTIME_RANGE",
                "runtime VFS range is invalid",
            )
        })?;
        if expected_revision != before.revision {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_REVISION",
                "runtime VFS revision does not match the caller binding",
            ));
        }
        let read = self
            .vfs
            .read_range(uri, range.offset, range.len)
            .map_err(core_error)?;
        if read.offset != range.offset || read.bytes.len() as u64 != range.len {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_SHORT_READ",
                "mounted VFS returned an invalid range length",
            ));
        }
        let (after, _) = self.stat_and_revision(uri)?;
        if after != before {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_MUTATED",
                "mounted VFS entry identity changed during the read",
            ));
        }
        let uri_hash = Hash256::from_sha256(uri.as_bytes());
        let mut access = self.access.lock().map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_RUNTIME_METRICS_POISONED",
                "runtime VFS access metrics are poisoned",
            )
        })?;
        access.resources.insert(uri_hash);
        access.ranges.insert((uri_hash, range.offset, range.len));
        access.read_count = access.read_count.checked_add(1).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_RUNTIME_METRICS_OVERFLOW",
                "runtime VFS access count overflowed",
            )
        })?;
        access.bytes_read = access.bytes_read.checked_add(range.len).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_RUNTIME_METRICS_OVERFLOW",
                "runtime VFS byte count overflowed",
            )
        })?;
        access.max_range_bytes = access.max_range_bytes.max(range.len);
        Ok(RangeReadResult {
            range,
            revision: before.revision,
            content_hash: Hash256::from_sha256(&read.bytes),
            bytes: read.bytes,
        })
    }

    fn enumerate_by_extension(
        &self,
        mount_set_id: &str,
        root: &str,
        extension_without_dot: &str,
        max_entries: u32,
    ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
        self.validate_mount(mount_set_id)?;
        if max_entries == 0 || max_entries > 100_000 {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_ENUM_BOUNDS",
                "enumeration limit is outside the supported bounds",
            ));
        }
        if root.contains('\0')
            || extension_without_dot.is_empty()
            || extension_without_dot.contains('/')
            || extension_without_dot.contains('\\')
            || extension_without_dot.contains('\0')
        {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_ENUM_ARGUMENT",
                "enumeration root or extension is invalid",
            ));
        }
        let normalized_root = root.trim_matches('/');
        let suffix = format!(".{}", extension_without_dot.to_ascii_lowercase());
        let mut files = Vec::new();
        for entry in &self.vfs.manifest().entries {
            let uri = &entry.uri;
            let root_matches = normalized_root.is_empty()
                || uri == normalized_root
                || uri.starts_with(&format!("{normalized_root}/"));
            if !root_matches || !uri.to_ascii_lowercase().ends_with(&suffix) {
                continue;
            }
            if files.len() >= max_entries as usize {
                return Err(invalid(
                    "ASTRA_EMU_VFS_RUNTIME_ENUM_BOUNDS",
                    "enumeration exceeded the negotiated entry limit",
                ));
            }
            files.push(LegacyVfsListedFile {
                uri: uri.clone(),
                stat: self.stat_and_revision(uri)?.0,
            });
        }
        Ok(files)
    }
}

fn core_error(error: astra_emu_family_core::LegacyCoreError) -> LegacyProviderError {
    LegacyProviderError::remote(error.code(), error.message())
}

fn invalid(code: &'static str, message: &'static str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use crate::test_support::MemoryVfs;

    use super::*;

    #[test]
    fn mounted_reader_preserves_uri_revision_and_range_bounds() {
        let vfs: Arc<dyn LegacyMountedVfs> = Arc::new(MemoryVfs::new(&[(
            "test:/scr/main.sc",
            b".end\r\n",
            "script",
        )]));
        let reader = LegacyMountedVfsReaderAdapter::new("mount.test", vfs).unwrap();
        let stat = reader.stat_file("mount.test", "test:/scr/main.sc").unwrap();
        let read = reader
            .read_file_range(
                "mount.test",
                "test:/scr/main.sc",
                stat.revision,
                ByteRange { offset: 1, len: 3 },
                4,
            )
            .unwrap();
        assert_eq!(read.bytes, b"end");
        assert_eq!(
            reader.access_metrics().unwrap(),
            RuntimeVfsAccessMetrics {
                resource_count: 1,
                unique_range_count: 1,
                read_count: 1,
                bytes_read: 3,
                max_range_bytes: 3,
            }
        );
        assert_eq!(
            reader
                .stat_file("other.mount", "test:/scr/main.sc")
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_RUNTIME_MOUNT_MISMATCH"
        );
    }
}
