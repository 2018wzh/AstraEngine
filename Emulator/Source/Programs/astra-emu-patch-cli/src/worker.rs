use astra_emu_patch::{PatchError, PatchErrorKind, PatchProgress, PatchRequest, PatchResult};
use crossbeam_channel::{bounded, never, select, Receiver};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

const THREAD_NAME: &str = "astra-emu-patch-worker";
type Completion = Result<PatchResult, PatchError>;

pub(super) fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A conversion panic can carry a buffer, local path, or private text.
        // Join reports a fixed error instead of displaying that panic payload.
        if thread::current().name() != Some(THREAD_NAME) {
            previous(info);
        }
    }));
}

struct Worker {
    cancelled: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<PatchProgress>>>,
    completion: Receiver<Completion>,
    wake: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn start(request: PatchRequest, cancelled: Arc<AtomicBool>) -> Result<Self, PatchError> {
        let latest = Arc::new(Mutex::new(None));
        let (completion_tx, completion) = bounded(1);
        let (wake_tx, wake) = bounded(1);
        let progress = latest.clone();
        let signal = cancelled.clone();
        let thread = thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || {
                let result = astra_emu_patch::apply(request, signal, move |value| {
                    *progress
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(value);
                    // Only the latest value is retained; a full wake channel means
                    // the caller already knows to inspect that slot.
                    let _ = wake_tx.try_send(());
                });
                let _ = completion_tx.send(result);
            })
            .map_err(|cause| {
                PatchError::with_source(PatchErrorKind::Io, "cannot start conversion worker", cause)
            })?;
        Ok(Self {
            cancelled,
            latest,
            completion,
            wake,
            thread: Some(thread),
        })
    }

    fn take_progress(&self) -> Option<PatchProgress> {
        self.latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    fn join(&mut self) -> Result<(), PatchError> {
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            return Err(PatchError::new(
                PatchErrorKind::Script,
                "conversion worker panicked",
            ));
        }
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancelled.store(true, Ordering::Release);
            let _ = self.join();
        }
    }
}

pub(super) fn apply(
    request: PatchRequest,
    cancelled: Arc<AtomicBool>,
    mut progress: impl FnMut(PatchProgress),
) -> Completion {
    let mut worker = Worker::start(request, cancelled)?;
    let mut wake = worker.wake.clone();
    let result = loop {
        select! {
            recv(worker.completion) -> result => break result.unwrap_or_else(|_| {
                Err(PatchError::new(PatchErrorKind::Script,
                    "conversion worker disconnected before returning a result"))
            }),
            recv(wake) -> notification => {
                if notification.is_err() {
                    // The progress callback can be dropped before completion
                    // is sent. Disable this arm instead of spinning on EOF.
                    wake = never();
                }
                if let Some(value) = worker.take_progress() {
                    progress(value);
                }
            }
        }
    };
    worker.join()?;
    if let Some(value) = worker.take_progress() {
        progress(value);
    }
    result
}
