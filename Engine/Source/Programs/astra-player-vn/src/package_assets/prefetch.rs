use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use astra_runtime::TaskScope;

use super::{NativeVnHostError, PackageAssetStore};

type ImageLoader = dyn Fn(&str) -> Result<(), String> + Send + Sync;

struct Request {
    scope: TaskScope,
    asset_id: String,
}

pub(crate) struct ImagePrefetchResult {
    pub scope: TaskScope,
    pub asset_id: String,
    pub result: Result<(), String>,
}

pub(crate) struct PackageImagePrefetcher {
    commands: Option<SyncSender<Request>>,
    completions: Receiver<ImagePrefetchResult>,
    workers: Vec<JoinHandle<()>>,
    stopped: Arc<AtomicBool>,
    scope: TaskScope,
}

impl PackageImagePrefetcher {
    const QUEUE_CAPACITY: usize = 32;
    const WORKER_COUNT: usize = 2;

    pub fn start(store: Arc<PackageAssetStore>) -> Result<Self, NativeVnHostError> {
        Self::start_loader(Arc::new(move |asset_id| {
            store
                .load_image(asset_id)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }))
    }

    fn start_loader(load: Arc<ImageLoader>) -> Result<Self, NativeVnHostError> {
        let (commands, worker_commands) = mpsc::sync_channel::<Request>(Self::QUEUE_CAPACITY);
        let (worker_completions, completions) = mpsc::channel();
        let worker_commands = Arc::new(Mutex::new(worker_commands));
        let stopped = Arc::new(AtomicBool::new(false));
        let mut owner = Self {
            commands: Some(commands),
            completions,
            workers: Vec::new(),
            stopped: stopped.clone(),
            scope: TaskScope::new(),
        };
        for worker_index in 0..Self::WORKER_COUNT {
            let load = load.clone();
            let commands = worker_commands.clone();
            let completions = worker_completions.clone();
            let stopped = stopped.clone();
            let worker_budget = astra_plugin::WorkerBudgetBroker::global().clone();
            let worker = thread::Builder::new()
                .name(format!("astra-image-prefetch-{worker_index}"))
                .spawn(move || loop {
                    let request = commands.lock().expect("image queue poisoned").recv();
                    let Ok(request) = request else { break };
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    if request.scope.is_cancelled() {
                        continue;
                    }
                    let result = worker_budget
                        .run_scoped(|| {
                            if stopped.load(Ordering::Acquire) || request.scope.is_cancelled() {
                                return Ok(());
                            }
                            load(&request.asset_id)
                        })
                        .map_err(|error| error.to_string())
                        .and_then(|result| result);
                    if completions
                        .send(ImagePrefetchResult {
                            scope: request.scope,
                            asset_id: request.asset_id,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                })
                .map_err(|error| {
                    NativeVnHostError::Asset(format!("ASTRA_PLAYER_IMAGE_PREFETCH_THREAD: {error}"))
                })?;
            owner.workers.push(worker);
        }
        Ok(owner)
    }

    pub fn reset_generation(&mut self) {
        self.scope.cancel();
        self.scope = TaskScope::new();
    }

    pub fn is_current(&self, scope: &TaskScope) -> bool {
        &self.scope == scope
    }

    pub fn try_schedule(&self, asset_id: String) -> Result<bool, NativeVnHostError> {
        let commands = self.commands.as_ref().ok_or_else(|| failure("STOPPED"))?;
        match commands.try_send(Request {
            scope: self.scope.clone(),
            asset_id,
        }) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err(failure("DISCONNECTED")),
        }
    }

    pub fn drain_completions(&self) -> Result<Vec<ImagePrefetchResult>, NativeVnHostError> {
        let mut completed = Vec::new();
        loop {
            match self.completions.try_recv() {
                Ok(result) => completed.push(result),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) if self.commands.is_none() => break,
                Err(TryRecvError::Disconnected) => return Err(failure("COMPLETION_DISCONNECTED")),
            }
        }
        Ok(completed)
    }

    pub fn shutdown(&mut self) -> Result<Vec<ImagePrefetchResult>, NativeVnHostError> {
        // Close independently of queued loads. Workers finish their current decode only.
        self.stopped.store(true, Ordering::Release);
        self.scope.cancel();
        self.commands.take();
        let mut panicked = false;
        for worker in self.workers.drain(..) {
            panicked |= worker.join().is_err();
        }
        if panicked {
            return Err(failure("PANICKED"));
        }
        self.drain_completions()
    }
}

impl Drop for PackageImagePrefetcher {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn failure(code: &str) -> NativeVnHostError {
    NativeVnHostError::Asset(format!("ASTRA_PLAYER_IMAGE_PREFETCH_{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Condvar, time::Duration};

    #[test]
    fn replaced_generation_rejects_late_failure_and_accepts_new_result() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_gate = gate.clone();
        let (started, starts) = mpsc::channel();
        let mut owner = PackageImagePrefetcher::start_loader(Arc::new(move |asset| {
            if asset == "old" {
                started.send(()).unwrap();
                let (lock, wake) = &*worker_gate;
                drop(
                    wake.wait_while(lock.lock().unwrap(), |open| !*open)
                        .unwrap(),
                );
                return Err("old decode failed".into());
            }
            Ok(())
        }))
        .unwrap();
        owner.try_schedule("old".into()).unwrap();
        starts.recv_timeout(Duration::from_secs(2)).unwrap();
        owner.reset_generation();
        owner.try_schedule("new".into()).unwrap();
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let mut seen = BTreeSet::new();
        for _ in 0..2 {
            let result = owner
                .completions
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            seen.insert(result.asset_id.clone());
            assert_eq!(owner.is_current(&result.scope), result.asset_id == "new");
            assert_eq!(result.result.is_ok(), result.asset_id == "new");
        }
        assert_eq!(seen.len(), 2);
        owner.shutdown().unwrap();
    }

    #[test]
    fn shutdown_skips_queued_loads_and_joins_inflight_workers() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_gate = gate.clone();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker_calls = calls.clone();
        let (started, starts) = mpsc::channel();
        let mut owner = PackageImagePrefetcher::start_loader(Arc::new(move |_| {
            worker_calls.fetch_add(1, Ordering::Relaxed);
            started.send(()).unwrap();
            let (lock, wake) = &*worker_gate;
            drop(
                wake.wait_while(lock.lock().unwrap(), |open| !*open)
                    .unwrap(),
            );
            Ok(())
        }))
        .unwrap();
        owner.try_schedule("inflight".into()).unwrap();
        starts.recv_timeout(Duration::from_secs(2)).unwrap();
        for index in 0..32 {
            let _ = owner.try_schedule(format!("queued.{index}"));
        }
        let stopped = owner.stopped.clone();
        let shutdown = thread::spawn(move || {
            owner.shutdown().unwrap();
            assert!(owner.workers.is_empty());
            assert!(owner.try_schedule("after.close".into()).is_err());
        });
        while !stopped.load(Ordering::Acquire) {
            thread::yield_now();
        }
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        shutdown.join().unwrap();
        assert!(calls.load(Ordering::Relaxed) <= PackageImagePrefetcher::WORKER_COUNT);
    }

    use std::collections::BTreeSet;
}
