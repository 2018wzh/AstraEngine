use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use astra_byte_source::{
    BoundedByteSource, ByteRange, ByteSourceError, ByteSourceStat, RangeReadResult, SourceRevision,
};
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
    resources: BTreeSet<u32>,
    ranges: BTreeSet<(u32, u64, u64)>,
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
    ) -> Result<(ByteSourceStat, astra_emu_family_core::LegacyVfsStat, u32), LegacyProviderError>
    {
        let resource_identity = Hash256::from_sha256(uri.as_bytes());
        let stat = self.vfs.stat(uri).map_err(|error| {
            let error = core_error(error);
            tracing::debug!(
                target: "astra_emu_family_support::runtime_vfs",
                event = "astra_emu_vfs_runtime_stat_failed",
                resource_identity = %resource_identity,
                diagnostic = %error.code(),
                "runtime VFS stat failed"
            );
            error
        })?;
        if stat.kind != LegacyVfsNodeKind::File {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_FILE_REQUIRED",
                "runtime VFS request requires a file URI",
            ));
        }
        let resource_id = self
            .vfs
            .manifest()
            .entries
            .iter()
            .position(|entry| entry.uri == uri)
            .ok_or_else(|| {
                let error = invalid(
                    "ASTRA_EMU_VFS_RUNTIME_MANIFEST_ENTRY",
                    "runtime VFS file is absent from the validated manifest",
                );
                tracing::debug!(
                    target: "astra_emu_family_support::runtime_vfs",
                    event = "astra_emu_vfs_runtime_manifest_lookup_failed",
                    resource_identity = %resource_identity,
                    diagnostic = %error.code(),
                    "runtime VFS manifest lookup failed"
                );
                error
            })
            .and_then(|index| {
                u32::try_from(index).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_VFS_RUNTIME_RESOURCE_ID",
                        "runtime VFS manifest resource index overflowed",
                    )
                })
            })?;
        Ok((
            ByteSourceStat {
                len: stat.size,
                revision: SourceRevision(1),
            },
            stat,
            resource_id,
        ))
    }
}

/// Binds one ABI-safe runtime VFS resource as a revision-pinned byte source.
///
/// The source is intentionally range-only: consumers pair it with
/// `BoundedByteSourceReader` when a decoder needs `Read + Seek`, so no host
/// path or whole-resource buffer crosses the family boundary.
pub struct LegacyRuntimeVfsByteSource {
    reader: Arc<dyn LegacyVfsReader>,
    mount_set_id: String,
    uri: String,
}

impl LegacyRuntimeVfsByteSource {
    pub fn new(
        reader: Arc<dyn LegacyVfsReader>,
        mount_set_id: impl Into<String>,
        uri: impl Into<String>,
    ) -> Result<Self, LegacyProviderError> {
        let mount_set_id = mount_set_id.into();
        let uri = uri.into();
        if mount_set_id.is_empty() || uri.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_SOURCE_ID",
                "runtime VFS byte source identity is invalid",
            ));
        }
        reader.stat_file(&mount_set_id, &uri)?;
        Ok(Self {
            reader,
            mount_set_id,
            uri,
        })
    }

    fn source_error(error: LegacyProviderError) -> ByteSourceError {
        ByteSourceError::Io(std::io::Error::other(error.code().to_owned()))
    }
}

impl BoundedByteSource for LegacyRuntimeVfsByteSource {
    fn stat(&self) -> Result<ByteSourceStat, ByteSourceError> {
        self.reader
            .stat_file(&self.mount_set_id, &self.uri)
            .map_err(Self::source_error)
    }

    fn read_range(
        &self,
        expected_revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, ByteSourceError> {
        self.reader
            .read_file_range(
                &self.mount_set_id,
                &self.uri,
                expected_revision,
                range,
                max_bytes,
            )
            .map_err(Self::source_error)
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
        let (before, _, resource_id) = self.stat_and_revision(uri)?;
        range.validate(before.len, max_bytes).map_err(|error| {
            tracing::error!(
                target: "astra_emu_family_support::runtime_vfs",
                event = "astra_emu_vfs_runtime_range_rejected",
                diagnostic_code = "ASTRA_EMU_VFS_RUNTIME_RANGE",
                resource_id = %resource_id,
                source_length = before.len,
                range_offset = range.offset,
                range_length = range.len,
                max_bytes,
                error = %error,
                "runtime VFS rejected an out-of-contract range"
            );
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
        let (after, _, after_resource_id) = self.stat_and_revision(uri)?;
        if after != before || after_resource_id != resource_id {
            return Err(invalid(
                "ASTRA_EMU_VFS_RUNTIME_MUTATED",
                "mounted VFS entry identity changed during the read",
            ));
        }
        let mut access = self.access.lock().map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_RUNTIME_METRICS_POISONED",
                "runtime VFS access metrics are poisoned",
            )
        })?;
        access.resources.insert(resource_id);
        access.ranges.insert((resource_id, range.offset, range.len));
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
    use std::{io::Read, sync::Arc};

    use astra_byte_source::BoundedByteSourceReader;

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
        assert_eq!(read.bytes.as_slice(), b"end");
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

    #[test]
    fn bounded_enumeration_filters_without_opening_payloads() {
        let vfs: Arc<dyn LegacyMountedVfs> = Arc::new(MemoryVfs::new(&[
            ("test:/Sakura.hcb", b"hcb", "script"),
            ("test:/graph.bin", b"pack", "archive"),
            ("test:/movie/intro.bin", b"movie", "archive"),
            ("test:/voice.ogg", b"ogg", "audio"),
        ]));
        let reader = LegacyMountedVfsReaderAdapter::new("mount.test", vfs).unwrap();

        let bins = reader
            .enumerate_by_extension("mount.test", "test:/", "bin", 2)
            .unwrap();
        assert_eq!(bins.len(), 2);
        assert_eq!(bins[0].uri, "test:/graph.bin");
        assert_eq!(bins[1].uri, "test:/movie/intro.bin");
        assert_eq!(reader.access_metrics().unwrap().read_count, 0);

        assert_eq!(
            reader
                .enumerate_by_extension("mount.test", "test:/", "bin", 1)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_RUNTIME_ENUM_BOUNDS"
        );
        assert_eq!(
            reader
                .enumerate_by_extension("mount.test", "test:/", "../bin", 2)
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_RUNTIME_ENUM_ARGUMENT"
        );
    }

    #[test]
    fn runtime_byte_source_reads_through_the_bound_reader() {
        let vfs: Arc<dyn LegacyMountedVfs> = Arc::new(MemoryVfs::new(&[(
            "test:/mov/clip.bin",
            b"abcdef",
            "movie",
        )]));
        let reader: Arc<dyn LegacyVfsReader> =
            Arc::new(LegacyMountedVfsReaderAdapter::new("mount.test", vfs).unwrap());
        let source = Arc::new(
            LegacyRuntimeVfsByteSource::new(reader, "mount.test", "test:/mov/clip.bin").unwrap(),
        );
        let mut stream = BoundedByteSourceReader::new(source, 2).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"abcdef");
    }
}
