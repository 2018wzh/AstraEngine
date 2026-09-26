use super::{cancelled_error, host_error, invalid, DecodeResource};
use astra_platform::{DecodeKind, DecodeOutput, PlatformDecodeRequest, PlatformError};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use tokio::sync::oneshot;

type Reply = oneshot::Sender<Result<DecodeOutput, PlatformError>>;
struct Job {
    request: PlatformDecodeRequest,
    reply: Reply,
}

pub(crate) struct DecodeSession {
    requests: mpsc::SyncSender<Job>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<Result<(), PlatformError>>>,
}

impl DecodeSession {
    pub fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
        let resource = DecodeResource::new(kind)?;
        let (requests, incoming) = mpsc::sync_channel::<Job>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("astra-linux-decode".into())
            .spawn(move || {
                let mut resource = resource;
                while !worker_stop.load(Ordering::Acquire) {
                    let job = match incoming.recv_timeout(Duration::from_millis(20)) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let cancelled = || worker_stop.load(Ordering::Acquire) || job.reply.is_closed();
                    let result = resource.decode_cancellable(job.request, &cancelled);
                    if job.reply.send(result).is_err() {
                        // A caller can disappear after decode's final cancellation check.
                        // Drop the native stream before accepting any later work.
                        resource.video.take();
                        resource.failed = true;
                    }
                }
                let closed = resource.close();
                drop(resource); // NULL/join media tasks and remove the private input first.
                while let Ok(job) = incoming.try_recv() {
                    let _ = job.reply.send(Err(cancelled_error()));
                }
                closed
            })
            .map_err(|_| host_error("decode.open", "Linux decode worker could not be started"))?;
        Ok(Self {
            requests,
            stop,
            thread: Some(thread),
        })
    }

    pub fn close(mut self) -> Result<(), PlatformError> {
        self.finish()
    }

    fn finish(&mut self) -> Result<(), PlatformError> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| host_error("decode.close", "Linux decode worker panicked"))??;
        }
        Ok(())
    }

    pub fn submit(&self, request: PlatformDecodeRequest, reply: Reply) {
        if self.stop.load(Ordering::Acquire) {
            let _ = reply.send(Err(cancelled_error()));
            return;
        }
        if let Err(error) = self.requests.try_send(Job { request, reply }) {
            let (job, error) = match error {
                mpsc::TrySendError::Full(job) => (job, invalid("decode request queue is full")),
                mpsc::TrySendError::Disconnected(job) => (job, invalid("decode worker is closed")),
            };
            let _ = job.reply.send(Err(error));
        }
    }
}

impl Drop for DecodeSession {
    fn drop(&mut self) {
        if self.finish().is_err() {
            tracing::error!(
                event = "platform.linux.decode.shutdown_failed",
                diagnostic_code = "ASTRA_LINUX_DECODE_SHUTDOWN_FAILED",
                "Linux decoder shutdown failed"
            );
        }
    }
}
