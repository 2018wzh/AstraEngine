use crate::{error::check_cancelled, PatchError, PatchErrorKind};
use astra_byte_source::{ByteRange, AUDIT_CHUNK_BYTES, DEFAULT_MAX_RANGE_BYTES};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    sync::{atomic::AtomicBool, Arc},
};

pub(crate) const MAX_EXACT_INTEGER: u64 = (1_u64 << 53) - 1;

pub(crate) struct TaskFile {
    file: Option<File>,
    writable: bool,
    cancelled: Arc<AtomicBool>,
    snapshot: Option<(u64, std::time::SystemTime)>,
}

impl TaskFile {
    pub(crate) fn new(
        file: File,
        writable: bool,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, PatchError> {
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(PatchError::new(
                PatchErrorKind::Path,
                "expected an ordinary file",
            ));
        }
        let snapshot = if writable {
            None
        } else {
            Some((metadata.len(), metadata.modified()?))
        };
        Ok(Self {
            file: Some(file),
            writable,
            cancelled,
            snapshot,
        })
    }

    fn opened(&self) -> Result<&File, PatchError> {
        check_cancelled(&self.cancelled)?;
        let file = self.file.as_ref().ok_or_else(closed)?;
        if let Some((length, modified)) = self.snapshot {
            let observed = file.metadata()?;
            if observed.len() != length || observed.modified()? != modified {
                return Err(PatchError::new(
                    PatchErrorKind::Integrity,
                    "input changed during conversion",
                ));
            }
        }
        Ok(file)
    }

    pub(crate) fn size(&self) -> Result<u64, PatchError> {
        let size = self.opened()?.metadata()?.len();
        exact(size)?;
        Ok(size)
    }

    pub(crate) fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), PatchError> {
        exact(offset)?;
        let range = ByteRange {
            offset,
            len: bytes.len() as u64,
        };
        range
            .validate(self.size()?, DEFAULT_MAX_RANGE_BYTES)
            .map_err(|cause| {
                PatchError::with_source(
                    PatchErrorKind::Bounds,
                    "file read exceeds its range",
                    cause,
                )
            })?;
        let cancelled = &self.cancelled;
        let file = self.file.as_mut().ok_or_else(closed)?;
        file.seek(SeekFrom::Start(offset))?;
        for chunk in bytes.chunks_mut(AUDIT_CHUNK_BYTES) {
            check_cancelled(cancelled)?;
            file.read_exact(chunk)?;
        }
        check_cancelled(cancelled)?;
        self.opened()?;
        Ok(())
    }

    pub(crate) fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), PatchError> {
        self.require_writable()?;
        exact(offset)?;
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| PatchError::new(PatchErrorKind::Bounds, "file write offset overflow"))?;
        exact(end)?;
        let cancelled = &self.cancelled;
        let file = self.file.as_mut().ok_or_else(closed)?;
        file.seek(SeekFrom::Start(offset))?;
        for chunk in bytes.chunks(AUDIT_CHUNK_BYTES) {
            check_cancelled(cancelled)?;
            file.write_all(chunk)?;
        }
        check_cancelled(cancelled)
    }

    pub(crate) fn truncate(&mut self, length: u64) -> Result<(), PatchError> {
        self.require_writable()?;
        exact(length)?;
        self.opened()?.set_len(length)?;
        Ok(())
    }

    pub(crate) fn sha256(&mut self) -> Result<String, PatchError> {
        check_cancelled(&self.cancelled)?;
        let before = self.opened()?.metadata()?;
        let cancelled = &self.cancelled;
        let file = self.file.as_mut().ok_or_else(closed)?;
        file.seek(SeekFrom::Start(0))?;
        let mut buffer = scratch(AUDIT_CHUNK_BYTES)?;
        let mut digest = Sha256::new();
        let mut consumed = 0_u64;
        loop {
            check_cancelled(cancelled)?;
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            consumed = consumed
                .checked_add(count as u64)
                .ok_or_else(|| PatchError::new(PatchErrorKind::Bounds, "file length overflow"))?;
            digest.update(&buffer[..count]);
        }
        let after = file.metadata()?;
        if before.len() != consumed
            || before.len() != after.len()
            || before.modified()? != after.modified()?
        {
            return Err(PatchError::new(
                PatchErrorKind::Integrity,
                "input changed while hashing",
            ));
        }
        check_cancelled(cancelled)?;
        Ok(format!("{:x}", digest.finalize()))
    }

    pub(crate) fn close(&mut self) -> Result<(), PatchError> {
        if let Some(file) = self.file.take() {
            if self.writable {
                file.sync_all()?;
            }
        }
        Ok(())
    }

    fn require_writable(&self) -> Result<(), PatchError> {
        self.opened()?;
        if self.writable {
            Ok(())
        } else {
            Err(PatchError::new(
                PatchErrorKind::Path,
                "input files are read-only",
            ))
        }
    }
}

fn closed() -> PatchError {
    PatchError::new(PatchErrorKind::Io, "file handle is closed")
}

pub(crate) fn exact(value: u64) -> Result<(), PatchError> {
    if value > MAX_EXACT_INTEGER {
        Err(PatchError::new(
            PatchErrorKind::Bounds,
            "integer exceeds Luau exact range",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn scratch(length: usize) -> Result<Vec<u8>, PatchError> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|cause| {
        PatchError::with_source(
            PatchErrorKind::Memory,
            "native buffer allocation failed",
            cause,
        )
    })?;
    bytes.resize(length, 0);
    Ok(bytes)
}

pub(crate) fn copy(source: &mut TaskFile, output: &mut TaskFile) -> Result<String, PatchError> {
    let length = source.size()?;
    let before = source.opened()?.metadata()?;
    let mut digest = Sha256::new();
    let mut bytes = scratch(AUDIT_CHUNK_BYTES)?;
    let mut offset = 0_u64;
    while offset < length {
        let count =
            usize::try_from((length - offset).min(bytes.len() as u64)).map_err(|cause| {
                PatchError::with_source(PatchErrorKind::Bounds, "copy range overflow", cause)
            })?;
        source.read(offset, &mut bytes[..count])?;
        digest.update(&bytes[..count]);
        output.write(offset, &bytes[..count])?;
        offset += count as u64;
    }
    let after = source.opened()?.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(PatchError::new(
            PatchErrorKind::Integrity,
            "input changed while copying",
        ));
    }
    output.truncate(length)?;
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn chunked_copy_crosses_range_boundary_and_does_not_modify_input() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("source");
        let output = root.path().join("output");
        let cancelled = Arc::new(AtomicBool::new(false));
        let pattern = vec![0x5a; AUDIT_CHUNK_BYTES];
        let mut seed = File::create(&input).unwrap();
        for _ in 0..17 {
            seed.write_all(&pattern).unwrap();
        }
        seed.write_all(b"tail").unwrap();
        drop(seed);
        let mut source =
            TaskFile::new(File::open(&input).unwrap(), false, cancelled.clone()).unwrap();
        let mut target = TaskFile::new(
            File::options()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&output)
                .unwrap(),
            true,
            cancelled.clone(),
        )
        .unwrap();
        let original = source.sha256().unwrap();
        assert_eq!(copy(&mut source, &mut target).unwrap(), original);
        assert_eq!(target.size().unwrap(), 17 * AUDIT_CHUNK_BYTES as u64 + 4);
        assert_eq!(target.sha256().unwrap(), original);
        assert_eq!(source.sha256().unwrap(), original);
        let mut tail = [0_u8; 4];
        target
            .read(17 * AUDIT_CHUNK_BYTES as u64, &mut tail)
            .unwrap();
        assert_eq!(&tail, b"tail");
        cancelled.store(true, Ordering::Release);
        assert_eq!(
            target.write(0, b"x").unwrap_err().kind(),
            PatchErrorKind::Cancelled
        );
        target.close().unwrap();
    }

    #[test]
    fn read_only_handles_and_exact_ranges_reject_writes_and_overflow() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("empty");
        File::create(&path).unwrap();
        let mut source = TaskFile::new(
            File::open(&path).unwrap(),
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(source.size().unwrap(), 0);
        assert_eq!(
            source.sha256().unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        source.read(0, &mut []).unwrap();
        assert_eq!(
            source.read(1, &mut []).unwrap_err().kind(),
            PatchErrorKind::Bounds
        );
        assert_eq!(
            source.write(0, b"x").unwrap_err().kind(),
            PatchErrorKind::Path
        );
        assert_eq!(
            exact(MAX_EXACT_INTEGER + 1).unwrap_err().kind(),
            PatchErrorKind::Bounds
        );
    }
}
