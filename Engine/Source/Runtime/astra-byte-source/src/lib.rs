use std::{
    collections::BTreeSet,
    fmt,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

mod owned_buffer;
mod owned_pcm;
mod owned_writable_buffer;

#[cfg(feature = "ffi")]
pub use owned_buffer::FfiOwnedByteBuffer;
pub use owned_buffer::OwnedByteBuffer;
#[cfg(feature = "ffi")]
pub use owned_pcm::{FfiOwnedF32Buffer, FfiOwnedI16Buffer};
pub use owned_pcm::{OwnedF32Buffer, OwnedI16Buffer};
#[cfg(feature = "ffi")]
pub use owned_writable_buffer::FfiOwnedWritableByteBuffer;
pub use owned_writable_buffer::OwnedWritableByteBuffer;

pub const DEFAULT_MAX_RANGE_BYTES: u64 = 16 * 1024 * 1024;
pub const AUDIT_CHUNK_BYTES: usize = 1024 * 1024;
pub const DEFAULT_READER_BUFFER_BYTES: usize = 4 * 1024 * 1024;

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
    #[error("ASTRA_BYTE_SOURCE_POISONED: source synchronization state is poisoned")]
    Poisoned,
    #[error("ASTRA_BYTE_SOURCE_IO: source I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct SourceRevision(pub u64);

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

#[derive(Debug, PartialEq, Eq)]
pub struct RangeReadResult {
    pub range: ByteRange,
    pub revision: SourceRevision,
    pub bytes: OwnedByteBuffer,
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

/// A bounded, revision-pinned `Read + Seek` view over a range source.
///
/// It retains at most one caller-selected range buffer and never materializes
/// the full source. This is intended for demuxers that consume seekable media
/// while preserving the source's revision and range limits.
pub struct BoundedByteSourceReader {
    source: Arc<dyn BoundedByteSource>,
    stat: ByteSourceStat,
    cursor: u64,
    buffer_offset: u64,
    buffer: Vec<u8>,
    buffer_bytes: usize,
}

impl BoundedByteSourceReader {
    pub fn new(
        source: Arc<dyn BoundedByteSource>,
        buffer_bytes: usize,
    ) -> Result<Self, ByteSourceError> {
        if buffer_bytes == 0 || buffer_bytes as u64 > DEFAULT_MAX_RANGE_BYTES {
            return Err(ByteSourceError::RangeLimit);
        }
        let stat = source.stat()?;
        Ok(Self {
            source,
            stat,
            cursor: 0,
            buffer_offset: 0,
            buffer: Vec::new(),
            buffer_bytes,
        })
    }

    pub fn stat(&self) -> ByteSourceStat {
        self.stat
    }

    fn refill(&mut self) -> Result<(), ByteSourceError> {
        if self.cursor >= self.stat.len {
            self.buffer.clear();
            self.buffer_offset = self.cursor;
            return Ok(());
        }
        let length = (self.stat.len - self.cursor).min(self.buffer_bytes as u64);
        let range = ByteRange {
            offset: self.cursor,
            len: length,
        };
        let read = self
            .source
            .read_range(self.stat.revision, range, self.buffer_bytes as u64)?;
        if read.revision != self.stat.revision {
            return Err(ByteSourceError::RevisionMismatch);
        }
        if read.range != range || read.bytes.len() as u64 != length {
            return Err(ByteSourceError::ShortRead);
        }
        self.buffer_offset = range.offset;
        self.buffer = read.bytes;
        Ok(())
    }

    fn io_error(error: ByteSourceError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, error)
    }
}

impl Read for BoundedByteSourceReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.cursor == self.stat.len {
            return Ok(0);
        }
        let mut written = 0usize;
        while written < output.len() && self.cursor < self.stat.len {
            let buffer_end = self.buffer_offset.saturating_add(self.buffer.len() as u64);
            if self.buffer.is_empty()
                || self.cursor < self.buffer_offset
                || self.cursor >= buffer_end
            {
                self.refill().map_err(Self::io_error)?;
                continue;
            }
            let source_start = usize::try_from(self.cursor - self.buffer_offset)
                .map_err(|_| Self::io_error(ByteSourceError::RangeOverflow))?;
            let available = self.buffer.len() - source_start;
            let copy = available.min(output.len() - written);
            output[written..written + copy]
                .copy_from_slice(&self.buffer[source_start..source_start + copy]);
            written += copy;
            self.cursor = self
                .cursor
                .checked_add(copy as u64)
                .ok_or_else(|| Self::io_error(ByteSourceError::RangeOverflow))?;
        }
        Ok(written)
    }
}

impl Seek for BoundedByteSourceReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.cursor) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.stat.len) + i128::from(offset),
        };
        if target < 0 || target > i128::from(self.stat.len) {
            return Err(Self::io_error(ByteSourceError::RangeBounds));
        }
        self.cursor =
            u64::try_from(target).map_err(|_| Self::io_error(ByteSourceError::RangeOverflow))?;
        Ok(self.cursor)
    }
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
        let modified_ns = u64::try_from(modified_ns).map_err(|_| {
            ByteSourceError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "source modification timestamp exceeds u64 nanoseconds",
            ))
        })?;
        Ok(ByteSourceStat {
            len: metadata.len(),
            revision: SourceRevision(modified_ns),
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
            bytes: bytes.into(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct MemoryByteSource {
    bytes: Arc<Vec<u8>>,
    revision: SourceRevision,
}

impl MemoryByteSource {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::new(bytes),
            revision: SourceRevision(1),
        }
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
        let bytes = OwnedByteBuffer::from_owner(
            MemoryRange {
                bytes: Arc::clone(&self.bytes),
                start,
                end,
            },
            MemoryRange::as_slice,
        );
        Ok(RangeReadResult {
            range,
            revision: stat.revision,
            bytes,
        })
    }
}

struct MemoryRange {
    bytes: Arc<Vec<u8>>,
    start: usize,
    end: usize,
}

impl MemoryRange {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[self.start..self.end]
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
    observed: BTreeSet<AccessKey>,
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
        self.observed.insert(key);
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
        for key in &self.observed {
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
        digest.update(result.bytes.as_slice());
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
        assert_eq!(result.bytes.as_slice(), b"tail");
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
            source.read_range(SourceRevision(2), ByteRange { offset: 0, len: 1 }, 16,),
            Err(ByteSourceError::RevisionMismatch)
        ));
    }

    #[astra_headless_test::test]
    fn range_backed_reader_is_bounded_and_seekable() {
        let source: Arc<dyn BoundedByteSource> =
            Arc::new(MemoryByteSource::new(b"abcdefghij".to_vec()));
        let mut reader = BoundedByteSourceReader::new(source, 3).unwrap();
        let mut first = [0u8; 5];
        reader.read_exact(&mut first).unwrap();
        assert_eq!(&first, b"abcde");
        reader.seek(SeekFrom::Current(-2)).unwrap();
        let mut second = [0u8; 4];
        reader.read_exact(&mut second).unwrap();
        assert_eq!(&second, b"defg");
        assert_eq!(reader.seek(SeekFrom::End(-2)).unwrap(), 8);
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, b"ij");
    }
}
