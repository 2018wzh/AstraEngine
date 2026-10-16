//! PCM bridge from the kira mixer thread to the host audio sink.
//!
//! The vendored engine mixes through `TapBackend`. Preserve its F32 samples;
//! the bounded bridge separates native mixer replacement from cancellation
//! of the whole session's blocking Host sink. No upstream audio change needed.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};

use abi_stable::std_types::RVec;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex as ParkingMutex;

pub(crate) const OUTPUT_FORMAT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: 48_000,
    channels: 2,
    format: PcmFormat::F32,
};

pub(crate) struct PcmBridge {
    sink: Arc<AudioSinkBox>,
    cancelled: Arc<AtomicBool>,
    error: Arc<ParkingMutex<Option<FamilyError>>>,
    queue: Sender<QueuedPcm>,
    stopping: Receiver<()>,
    stop: Mutex<Option<Sender<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct QueuedPcm {
    samples: RVec<f32>,
    mixer_stopping: Receiver<()>,
}

impl PcmBridge {
    pub(crate) fn new(sink: AudioSinkBox) -> FamilyResult<Self> {
        sink.configure(OUTPUT_FORMAT).into_result()?;
        let (queue, receiver) = bounded(2);
        let (stop, stopping) = bounded(0);
        let bridge = Self {
            sink: Arc::new(sink),
            cancelled: Arc::new(AtomicBool::new(false)),
            error: Arc::new(ParkingMutex::new(None)),
            queue,
            stopping,
            stop: Mutex::new(Some(stop)),
            worker: Mutex::new(None),
        };
        let tap = Tap {
            sink: Arc::clone(&bridge.sink),
            cancelled: Arc::clone(&bridge.cancelled),
            error: Arc::clone(&bridge.error),
        };
        let stopping = bridge.stopping.clone();
        let worker = thread::Builder::new()
            .name("astra-siglus-pcm".into())
            .spawn(move || forward_pcm(tap, receiver, stopping))
            .map_err(|_| {
                FamilyError::invalid(
                    "ASTRA_EMU_SIGLUS_AUDIO_WORKER",
                    "PCM worker could not start",
                )
            })?;
        *bridge.worker.lock().map_err(|_| lock_error())? = Some(worker);
        Ok(bridge)
    }

    /// The callback handed to the engine's tap registry. It runs on the kira
    /// mixer worker thread. Each mixer has its own interruptible queue write;
    /// cancellation never closes another mixer's Host sink.
    pub(crate) fn tap_factory(&self) -> siglus_scene_vm::audio::kira_hub::hosted_tap::TapFactory {
        let queue = self.queue.clone();
        let stopping = self.stopping.clone();
        Arc::new(move || {
            let (cancel, cancelled) = bounded(0);
            let cancel = ParkingMutex::new(Some(cancel));
            let queue = queue.clone();
            let stopping = stopping.clone();
            let callback = Arc::new(move |samples: &[f32]| {
                let chunk = QueuedPcm {
                    samples: samples.to_vec().into(),
                    mixer_stopping: cancelled.clone(),
                };
                let _ = crossbeam_channel::select_biased! {
                    recv(cancelled) -> _ => false,
                    recv(stopping) -> _ => false,
                    send(queue, chunk) -> result => result.is_ok(),
                };
            });
            let cancel_callback = Arc::new(move || {
                cancel.lock().take();
            });
            (callback, cancel_callback)
        })
    }

    pub(crate) fn check_error(&self) -> FamilyResult<()> {
        if let Some(error) = self.error.lock().as_ref() {
            return Err(error.clone());
        }
        if self.cancelled.load(Ordering::Acquire) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_SIGLUS_AUDIO_CANCELLED",
                "host audio queue is cancelled",
            ));
        }
        Ok(())
    }

    pub(crate) fn close(&self) -> FamilyResult<()> {
        self.cancelled.store(true, Ordering::Release);
        let stop_error = match self.stop.lock() {
            Ok(mut stop) => {
                stop.take();
                None
            }
            Err(poison) => {
                poison.into_inner().take();
                Some(lock_error())
            }
        };
        let cancel_error = self.sink.cancel().into_result().err();
        let (mut worker, worker_lock_error) = match self.worker.lock() {
            Ok(worker) => (worker, None),
            Err(poison) => (poison.into_inner(), Some(lock_error())),
        };
        let worker_error = worker
            .take()
            .and_then(|worker| worker.join().err())
            .map(|_| FamilyError::invalid("ASTRA_EMU_SIGLUS_AUDIO_WORKER", "PCM worker panicked"));
        let mut failure = None;
        for error in [
            stop_error,
            cancel_error,
            worker_lock_error,
            worker_error,
            self.error.lock().clone(),
        ]
        .into_iter()
        .flatten()
        {
            failure = Some(match failure {
                None => error,
                Some(previous) => FamilyError::invalid(
                    "ASTRA_EMU_SIGLUS_AUDIO_CLOSE",
                    format!("{previous}; {error}"),
                ),
            });
        }
        failure.map_or(Ok(()), Err)
    }
}

fn lock_error() -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_SIGLUS_AUDIO_LOCK",
        "audio error state lock is poisoned",
    )
}

impl Drop for PcmBridge {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(event = "astra.emu.siglus.audio.cancel_failed", diagnostic = %error.code);
        }
    }
}

struct Tap {
    sink: Arc<AudioSinkBox>,
    cancelled: Arc<AtomicBool>,
    error: Arc<ParkingMutex<Option<FamilyError>>>,
}

impl Tap {
    fn push(&self, chunk: RVec<f32>) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        match self.sink.write(PcmChunk::F32(chunk)).into_result() {
            Ok(AudioWriteStatus::Accepted) => {}
            Ok(AudioWriteStatus::Cancelled) | Ok(AudioWriteStatus::Closed) => {
                self.cancelled.store(true, Ordering::Release);
            }
            Err(error) => {
                let mut slot = self.error.lock();
                if slot.is_none() {
                    *slot = Some(error);
                }
            }
        }
    }
}

fn forward_pcm(tap: Tap, receiver: Receiver<QueuedPcm>, stopping: Receiver<()>) {
    loop {
        let chunk = crossbeam_channel::select_biased! {
            recv(stopping) -> _ => return,
            recv(receiver) -> chunk => match chunk { Ok(chunk) => chunk, Err(_) => return },
        };
        if matches!(
            chunk.mixer_stopping.try_recv(),
            Err(crossbeam_channel::TryRecvError::Empty)
        ) {
            tap.push(chunk.samples);
        }
    }
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
