use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const DEFAULT_MAX_RANGE_BYTES: u64 = 16 * 1024 * 1024;
pub const AUDIT_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum ByteSourceError {
    #[error("ASTRA_BYTE_SOURCE_RANGE_LIMIT: requested range exceeds the configured bound")]
    RangeLimit,
    #[error("ASTRA_BYTE_SOURCE_RANGE_OVERFLOW: requested range overflowed")]
    RangeOverflow,
    #[error("ASTRA_BYTE_SOURCE_RANGE_BOUNDS: requested range exceeds the source length")]
    RangeBounds,
    #[error("ASTRA_BYTE_SOURCE_REVISION_MISMATCH: source revision changed")]
    RevisionMismatch,
    #[error("ASTRA_BYTE_SOURCE_SHORT_READ: source returned fewer bytes than requested")]
    ShortRead,
    #[error("ASTRA_BYTE_SOURCE_REPEAT_MISMATCH: a previously observed range changed")]
    RepeatMismatch,
    #[error("ASTRA_BYTE_SOURCE_POISONED: source synchronization state is poisoned")]
    Poisoned,
    #[error("ASTRA_BYTE_SOURCE_IO: source I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct SourceRevision(pub Hash256);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ByteSourceStat {
    pub len: u64,
    pub revision: SourceRevision,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct ByteRange {
    pub offset: u64,
    pub len: u64,
}

impl ByteRange {
    pub fn validate(self, source_len: u64, max_bytes: u64) -> Result<(), ByteSourceError> {
        if self.len > max_bytes || self.len > DEFAULT_MAX_RANGE_BYTES {
            return Err(ByteSourceError::RangeLimit);
        }
        let end = self
            .offset
            .checked_add(self.len)
            .ok_or(ByteSourceError::RangeOverflow)?;
        if end > source_len {
            return Err(ByteSourceError::RangeBounds);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RangeReadResult {
    pub range: ByteRange,
    pub revision: SourceRevision,
    pub content_hash: Hash256,
    pub bytes: Vec<u8>,
}

pub trait BoundedByteSource: Send + Sync {
    fn stat(&self) -> Result<ByteSourceStat, ByteSourceError>;

    fn read_range(
        &self,
        expected_revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, ByteSourceError>;
}

pub struct FileByteSource {
    path: PathBuf,
    file: Mutex<File>,
}

impl fmt::Debug for FileByteSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileByteSource")
            .field("path", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl FileByteSource {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ByteSourceError> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    fn observed_stat(&self) -> Result<ByteSourceStat, ByteSourceError> {
        let metadata = std::fs::metadata(&self.path)?;
        let modified_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                ByteSourceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
            })?
            .as_nanos();
        let mut material = Vec::with_capacity(24);
        material.extend_from_slice(&metadata.len().to_le_bytes());
        material.extend_from_slice(&modified_ns.to_le_bytes());
        Ok(ByteSourceStat {
            len: metadata.len(),
            revision: SourceRevision(Hash256::from_sha256(&material)),
        })
    }
}

impl BoundedByteSource for FileByteSource {
    fn stat(&self) -> Result<ByteSourceStat, ByteSourceError> {
        self.observed_stat()
    }

    fn read_range(
        &self,
        expected_revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, ByteSourceError> {
        let before = self.observed_stat()?;
        if before.revision != expected_revision {
            return Err(ByteSourceError::RevisionMismatch);
        }
        range.validate(before.len, max_bytes)?;
        let len = usize::try_from(range.len).map_err(|_| ByteSourceError::RangeLimit)?;
        let mut bytes = vec![0; len];
        let mut file = self.file.lock().map_err(|_| ByteSourceError::Poisoned)?;
        file.seek(SeekFrom::Start(range.offset))?;
        file.read_exact(&mut bytes)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::UnexpectedEof => ByteSourceError::ShortRead,
                _ => ByteSourceError::Io(error),
            })?;
        drop(file);
        let after = self.observed_stat()?;
        if after != before {
            return Err(ByteSourceError::RevisionMismatch);
        }
        Ok(RangeReadResult {
            range,
            revision: before.revision,
            content_hash: Hash256::from_sha256(&bytes),
            bytes,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MemoryByteSource {
    bytes: Vec<u8>,
    revision: SourceRevision,
}

impl MemoryByteSource {
    pub fn new(bytes: Vec<u8>) -> Self {
        let revision = SourceRevision(Hash256::from_sha256(&bytes));
        Self { bytes, revision }
    }
}

impl BoundedByteSource for MemoryByteSource {
    fn stat(&self) -> Result<ByteSourceStat, ByteSourceError> {
        Ok(ByteSourceStat {
            len: self.bytes.len() as u64,
            revision: self.revision,
        })
    }

    fn read_range(
        &self,
        expected_revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, ByteSourceError> {
        let stat = self.stat()?;
        if stat.revision != expected_revision {
            return Err(ByteSourceError::RevisionMismatch);
        }
        range.validate(stat.len, max_bytes)?;
        let start = usize::try_from(range.offset).map_err(|_| ByteSourceError::RangeBounds)?;
        let end =
            usize::try_from(range.offset + range.len).map_err(|_| ByteSourceError::RangeBounds)?;
        let bytes = self.bytes[start..end].to_vec();
        Ok(RangeReadResult {
            range,
            revision: stat.revision,
            content_hash: Hash256::from_sha256(&bytes),
            bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AccessKey {
    resource_id: String,
    revision: SourceRevision,
    range: ByteRange,
}

#[derive(Debug, Default)]
pub struct AccessedResourceLedger {
    observed: BTreeMap<AccessKey, Hash256>,
    read_count: u64,
    bytes_read: u64,
    max_range_bytes: u64,
}

impl AccessedResourceLedger {
    pub fn record(
        &mut self,
        resource_id: &str,
        result: &RangeReadResult,
    ) -> Result<(), ByteSourceError> {
        let key = AccessKey {
            resource_id: resource_id.to_owned(),
            revision: result.revision,
            range: result.range,
        };
        if self
            .observed
            .insert(key, result.content_hash)
            .is_some_and(|previous| previous != result.content_hash)
        {
            return Err(ByteSourceError::RepeatMismatch);
        }
        self.read_count = self.read_count.saturating_add(1);
        self.bytes_read = self.bytes_read.saturating_add(result.range.len);
        self.max_range_bytes = self.max_range_bytes.max(result.range.len);
        Ok(())
    }

    pub fn read_count(&self) -> u64 {
        self.read_count
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    pub fn max_range_bytes(&self) -> u64 {
        self.max_range_bytes
    }

    pub fn unique_resource_count(&self) -> u64 {
        let mut resources = BTreeSet::new();
        for key in self.observed.keys() {
            resources.insert(key.resource_id.as_str());
        }
        u64::try_from(resources.len()).unwrap_or(u64::MAX)
    }

    pub fn unique_range_count(&self) -> u64 {
        u64::try_from(self.observed.len()).unwrap_or(u64::MAX)
    }
}

pub fn audit_source(source: &dyn BoundedByteSource) -> Result<Hash256, ByteSourceError> {
    let stat = source.stat()?;
    let mut digest = Sha256::new();
    let mut offset = 0_u64;
    while offset < stat.len {
        let len = (stat.len - offset).min(AUDIT_CHUNK_BYTES as u64);
        let result = source.read_range(
            stat.revision,
            ByteRange { offset, len },
            DEFAULT_MAX_RANGE_BYTES,
        )?;
        digest.update(&result.bytes);
        offset = offset
            .checked_add(len)
            .ok_or(ByteSourceError::RangeOverflow)?;
    }
    Ok(Hash256::from_bytes(digest.finalize().into()))
}

#[cfg(test)]
mod tests {
    use std::io::{Seek, Write};

    use super::*;

    #[astra_headless_test::test]
    fn sparse_source_larger_than_512_mib_uses_bounded_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.bin");
        let mut file = File::create(&path).unwrap();
        file.set_len(768 * 1024 * 1024).unwrap();
        file.seek(SeekFrom::Start(768 * 1024 * 1024 - 4)).unwrap();
        file.write_all(b"tail").unwrap();
        drop(file);

        let source = FileByteSource::open(path).unwrap();
        let stat = source.stat().unwrap();
        let result = source
            .read_range(
                stat.revision,
                ByteRange {
                    offset: stat.len - 4,
                    len: 4,
                },
                DEFAULT_MAX_RANGE_BYTES,
            )
            .unwrap();
        assert_eq!(result.bytes, b"tail");
    }

    #[astra_headless_test::test]
    fn range_limit_and_revision_are_fail_closed() {
        let source = MemoryByteSource::new(vec![7; 32]);
        let stat = source.stat().unwrap();
        assert!(matches!(
            source.read_range(stat.revision, ByteRange { offset: 0, len: 17 }, 16,),
            Err(ByteSourceError::RangeLimit)
        ));
        assert!(matches!(
            source.read_range(
                SourceRevision(Hash256::from_sha256(b"wrong")),
                ByteRange { offset: 0, len: 1 },
                16,
            ),
            Err(ByteSourceError::RevisionMismatch)
        ));
    }
}
