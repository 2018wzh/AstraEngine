use astra_byte_source::{
    BoundedByteSource, ByteRange, ByteSourceError, ByteSourceStat, MemoryByteSource,
    RangeReadResult, SourceRevision,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(crate) struct CancellableSource {
    pub(crate) source: MemoryByteSource,
    pub(crate) cancelled: Arc<AtomicBool>,
}

impl CancellableSource {
    fn check(&self) -> Result<(), ByteSourceError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(
                std::io::Error::new(std::io::ErrorKind::Interrupted, "package loading cancelled")
                    .into(),
            )
        } else {
            Ok(())
        }
    }
}

impl BoundedByteSource for CancellableSource {
    fn stat(&self) -> Result<ByteSourceStat, ByteSourceError> {
        self.check()?;
        self.source.stat()
    }
    fn read_range(
        &self,
        revision: SourceRevision,
        range: ByteRange,
        max_bytes: u64,
    ) -> Result<RangeReadResult, ByteSourceError> {
        self.check()?;
        self.source.read_range(revision, range, max_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_package::{
        AstraContainerBuilder, AstraContainerReader, ContainerKind, SectionPayload,
    };

    struct CountedSource {
        inner: CancellableSource,
        full_scan_starts: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl BoundedByteSource for CountedSource {
        fn stat(&self) -> Result<ByteSourceStat, ByteSourceError> {
            self.inner.stat()
        }
        fn read_range(
            &self,
            revision: SourceRevision,
            range: ByteRange,
            max_bytes: u64,
        ) -> Result<RangeReadResult, ByteSourceError> {
            if range.offset == 0 && range.len == astra_byte_source::AUDIT_CHUNK_BYTES as u64 {
                self.full_scan_starts.fetch_add(1, Ordering::Relaxed);
            }
            self.inner.read_range(revision, range, max_bytes)
        }
    }

    #[test]
    fn audited_source_retains_shared_bytes_and_checks_sections() {
        let bytes = Arc::new(
            AstraContainerBuilder::new(ContainerKind::Package)
                .add_section(SectionPayload::raw(
                    "payload",
                    "test.payload",
                    vec![7; 2 * 1024 * 1024],
                ))
                .write()
                .unwrap()
                .into_bytes(),
        );
        let expected = astra_core::Hash256::from_sha256(&bytes);
        let source = Arc::new(CancellableSource {
            source: MemoryByteSource::from_shared(Arc::clone(&bytes)),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        let scans = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::new(CountedSource {
            inner: Arc::try_unwrap(source).ok().unwrap(),
            full_scan_starts: Arc::clone(&scans),
        });
        let (reader, hash) = AstraContainerReader::open_storage_audited_source(counted).unwrap();
        assert_eq!(scans.load(Ordering::Relaxed), 1);
        assert_eq!(hash, expected);
        assert_eq!(Arc::strong_count(&bytes), 2);
        assert_eq!(
            reader.read_section("payload").unwrap(),
            vec![7; 2 * 1024 * 1024]
        );
        drop(reader);
        assert_eq!(Arc::strong_count(&bytes), 1);
    }

    #[test]
    fn cancelled_worker_returns_and_releases_owned_bytes() {
        let bytes = Arc::new(vec![0; 2 * 1024 * 1024]);
        let source = Arc::new(CancellableSource {
            source: MemoryByteSource::from_shared(Arc::clone(&bytes)),
            cancelled: Arc::new(AtomicBool::new(true)),
        });
        let worker =
            std::thread::spawn(move || AstraContainerReader::open_storage_audited_source(source));
        assert!(worker.join().unwrap().is_err());
        assert_eq!(Arc::strong_count(&bytes), 1);
    }
}
