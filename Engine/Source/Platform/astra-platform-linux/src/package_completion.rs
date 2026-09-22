use std::sync::mpsc;

use astra_platform::{PackageSourceHandle, PlatformError};
use astra_platform_common::{CachedPackageSource, FilePackageSource, ResourceTable};
use tokio::sync::oneshot;

pub(super) struct PackageCompletion {
    pub reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    pub result: Result<CachedPackageSource, PlatformError>,
}

impl PackageCompletion {
    pub fn publish(self, sender: &mpsc::Sender<Self>, wake: impl FnOnce()) {
        // Publish before waking: a Wait event loop otherwise sees no completion
        // and may sleep indefinitely until unrelated user input arrives.
        if sender.send(self).is_ok() {
            wake();
        }
    }

    pub fn deliver(self, sources: &mut ResourceTable<PackageSourceResource, PackageSourceHandle>) {
        let result = self
            .result
            .and_then(|source| sources.insert(PackageSourceResource::Cached(source)));
        deliver_package_result(self.reply, result, sources);
    }
}

pub(super) fn deliver_package_result(
    reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    result: Result<PackageSourceHandle, PlatformError>,
    sources: &mut ResourceTable<PackageSourceResource, PackageSourceHandle>,
) {
    // Cancellation can race successful open. The caller never received this
    // handle, so the event-loop owner must reclaim it before shutdown.
    if let Err(Ok(handle)) = reply.send(result) {
        let _ = sources.remove(handle);
    }
}

pub(super) enum PackageSourceResource {
    Bundled(FilePackageSource),
    Cached(CachedPackageSource),
}

impl PackageSourceResource {
    pub fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        match self {
            Self::Bundled(source) => source.read_range(offset, length),
            Self::Cached(source) => source.read_range(offset, length),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_core::Hash256;
    use astra_platform::{PackageCachePolicy, PlatformErrorCode};
    use astra_platform_common::VerifiedPackageCache;
    use std::{thread, time::Duration};

    fn source(root: &std::path::Path) -> CachedPackageSource {
        let mut cache = VerifiedPackageCache::open(
            root,
            PackageCachePolicy {
                max_entry_bytes: 24,
                max_total_bytes: 24,
            },
        )
        .unwrap();
        let bytes = b"linux package completion";
        let hash = Hash256::from_sha256(bytes).to_string();
        cache.store_verified(&hash, bytes).unwrap();
        cache.open_source(&hash).unwrap()
    }

    #[test]
    fn worker_completion_wakes_idle_owner_after_publication() {
        let root = tempfile::tempdir().unwrap();
        let cached = source(root.path());
        let (tx, rx) = mpsc::channel();
        let (wake_tx, wake_rx) = mpsc::channel();
        let (reply, mut answer) = oneshot::channel();
        let worker = thread::spawn(move || {
            PackageCompletion {
                reply,
                result: Ok(cached),
            }
            .publish(&tx, || wake_tx.send(()).unwrap());
        });
        wake_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let completion = rx
            .try_recv()
            .expect("completion must be queued before wake");
        let mut sources = ResourceTable::new("package_source");
        completion.deliver(&mut sources);
        let handle = answer.try_recv().unwrap().unwrap();
        assert_eq!(
            sources.get_mut(handle).unwrap().read_range(0, 5).unwrap(),
            b"linux"
        );
        sources.remove(handle).unwrap();
        sources.ensure_empty().unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn cancelled_open_reclaims_handle_and_cache_lease() {
        let root = tempfile::tempdir().unwrap();
        let (reply, answer) = oneshot::channel();
        drop(answer);
        let mut sources = ResourceTable::new("package_source");
        PackageCompletion {
            reply,
            result: Ok(source(root.path())),
        }
        .deliver(&mut sources);
        sources.ensure_empty().unwrap();
        // The original 24-byte entry fills the budget. A replacement must
        // evict it, which fails if the cancelled request retained its lease.
        let mut cache = VerifiedPackageCache::open(
            root.path(),
            PackageCachePolicy {
                max_entry_bytes: 24,
                max_total_bytes: 24,
            },
        )
        .unwrap();
        let bytes = b"replacement";
        let replacement_hash = Hash256::from_sha256(bytes).to_string();
        cache.store_verified(&replacement_hash, bytes).unwrap();
        assert!(!cache
            .contains(&Hash256::from_sha256(b"linux package completion").to_string())
            .unwrap());
        assert!(cache.contains(&replacement_hash).unwrap());
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn failed_open_is_delivered_without_allocating_a_handle() {
        let (reply, mut answer) = oneshot::channel();
        let mut sources = ResourceTable::new("package_source");
        PackageCompletion {
            reply,
            result: Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open",
                "test invalid hash",
            )),
        }
        .deliver(&mut sources);
        assert_eq!(
            answer.try_recv().unwrap().unwrap_err().code,
            PlatformErrorCode::IntegrityMismatch
        );
        sources.ensure_empty().unwrap();
    }
}
