use std::{future::Future, thread};

use astra_platform::{PackageSourceHandle, PlatformError, PlatformErrorCode};
use tokio::sync::oneshot;

pub(super) struct PackageWorker {
    cancel: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PackageWorker {
    pub fn spawn(work: impl FnOnce(oneshot::Receiver<()>) + Send + 'static) -> Self {
        let (cancel, cancelled) = oneshot::channel();
        Self {
            cancel: Some(cancel),
            thread: Some(thread::spawn(move || work(cancelled))),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(|worker| worker.is_finished())
    }

    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

impl Drop for PackageWorker {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.thread.take() {
            if worker.join().is_err() {
                tracing::error!(
                    event = "platform.linux.package.worker_panic",
                    diagnostic_code = "ASTRA_PLATFORM_PACKAGE_WORKER_PANIC",
                    "Linux package worker panicked during shutdown"
                );
            }
        }
    }
}

pub(super) async fn cancellable_fetch<T>(
    reply: &mut oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    cancelled: oneshot::Receiver<()>,
    fetch: impl Future<Output = Result<T, PlatformError>>,
) -> Result<T, PlatformError> {
    tokio::select! {
        biased;
        _ = cancelled => Err(cancelled_error()),
        _ = reply.closed() => Err(cancelled_error()),
        result = fetch => result,
    }
}

fn cancelled_error() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Cancelled,
        "package.https.open",
        "package request was cancelled",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::time::Duration;

    struct DownloadGuard(Arc<AtomicBool>);
    impl Drop for DownloadGuard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    async fn pending_download(
        started: mpsc::Sender<()>,
        dropped: Arc<AtomicBool>,
        root: std::path::PathBuf,
    ) -> Result<(), PlatformError> {
        let _guard = DownloadGuard(dropped);
        let mut cache = astra_platform_common::VerifiedPackageCache::open(
            root,
            astra_platform::PackageCachePolicy {
                max_entry_bytes: 64,
                max_total_bytes: 64,
            },
        )
        .unwrap();
        let hash = astra_core::Hash256::from_sha256(b"complete package").to_string();
        let mut staging = cache.begin_staging(&hash).unwrap();
        staging.write(b"complete").unwrap();
        started.send(()).unwrap();
        std::future::pending().await
    }

    #[test]
    fn dropping_owner_cancels_pending_download_and_joins_worker() {
        let (mut reply, _answer) = oneshot::channel();
        let (started, ready) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let worker_dropped = dropped.clone();
        let root = tempfile::tempdir().unwrap();
        let worker_root = root.path().to_path_buf();
        let worker = PackageWorker::spawn(move |cancelled| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            let result = runtime.block_on(cancellable_fetch(
                &mut reply,
                cancelled,
                pending_download(started, worker_dropped, worker_root),
            ));
            assert_eq!(result.unwrap_err().code, PlatformErrorCode::Cancelled);
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(worker);
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn receiver_cancellation_drops_download_before_completion() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (mut reply, answer) = oneshot::channel();
        let (_cancel, cancelled) = oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let (started, ready) = mpsc::channel();
        let root = tempfile::tempdir().unwrap();
        let caller = thread::spawn(move || {
            ready.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(answer);
        });
        let result = runtime.block_on(cancellable_fetch(
            &mut reply,
            cancelled,
            pending_download(started, dropped.clone(), root.path().to_path_buf()),
        ));
        caller.join().unwrap();
        assert_eq!(result.unwrap_err().code, PlatformErrorCode::Cancelled);
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
