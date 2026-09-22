use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

type Worker = (tokio::sync::oneshot::Sender<()>, JoinHandle<()>);

#[derive(Debug, Default)]
pub(super) struct ThreadRegistry(Mutex<Vec<Worker>>);

impl ThreadRegistry {
    pub(super) fn register(
        &self,
        cancel: tokio::sync::oneshot::Sender<()>,
        worker: JoinHandle<()>,
    ) {
        let mut workers = self.0.lock().unwrap_or_else(|error| error.into_inner());
        // Long-lived JSONL servers must not retain completed session threads.
        let mut index = 0;
        while index < workers.len() {
            if workers[index].1.is_finished() {
                join(workers.swap_remove(index).1);
            } else {
                index += 1;
            }
        }
        workers.push((cancel, worker));
    }
}

/// Owns dedicated Headless executor threads for one CLI run or JSONL server.
///
/// Drop cancels all remaining host loops and joins their threads, including
/// executor and native resource destruction. Keep this owner outside the scope
/// containing sessions and products, so their normal shutdown runs first.
#[derive(Debug, Default)]
pub struct HeadlessThreadOwner {
    pub(super) registry: Arc<ThreadRegistry>,
}

impl Drop for HeadlessThreadOwner {
    fn drop(&mut self) {
        let workers = std::mem::take(
            &mut *self
                .registry
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        let mut handles = Vec::with_capacity(workers.len());
        for (cancel, worker) in workers {
            let _ = cancel.send(());
            handles.push(worker);
        }
        for worker in handles {
            join(worker);
        }
    }
}

fn join(worker: JoinHandle<()>) {
    if worker.join().is_err() {
        tracing::error!(
            event = "platform.headless.thread.panicked",
            "Headless executor panicked while closing its session"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn drop_cancels_every_worker_and_waits_for_resource_destruction() {
        struct Resource(Arc<AtomicUsize>);
        impl Drop for Resource {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let destroyed = Arc::new(AtomicUsize::new(0));
        let owner = HeadlessThreadOwner::default();
        for _ in 0..3 {
            let (cancel, cancelled) = tokio::sync::oneshot::channel();
            let resource = Resource(destroyed.clone());
            let worker = std::thread::spawn(move || {
                let _resource = resource;
                let _ = cancelled.blocking_recv();
            });
            owner.registry.register(cancel, worker);
        }
        drop(owner);
        assert_eq!(destroyed.load(Ordering::SeqCst), 3);
    }
}
