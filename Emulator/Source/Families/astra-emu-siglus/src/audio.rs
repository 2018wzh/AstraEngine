//! PCM bridge from the kira mixer thread to the host audio sink.
//!
//! The vendored engine mixes through `TapBackend` (see
//! `siglus_scene_vm::audio::kira_hub::hosted_tap`); its worker converts the
//! mixer output to stereo i16 here. A bounded bridge queue separates native
//! mixer replacement from session cancellation of the blocking Host sink.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};

use abi_stable::std_types::RVec;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};
use crossbeam_channel::{bounded, Receiver, Sender};

pub(crate) const OUTPUT_FORMAT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: 48_000,
    channels: 2,
    format: PcmFormat::I16,
};

pub(crate) struct PcmBridge {
    sink: Arc<AudioSinkBox>,
    cancelled: Arc<AtomicBool>,
    pushed_frames: Arc<AtomicU64>,
    dropped_frames: Arc<AtomicU64>,
    error: Arc<Mutex<Option<FamilyError>>>,
    queue: Sender<QueuedPcm>,
    stopping: Receiver<()>,
    stop: Mutex<Option<Sender<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct QueuedPcm {
    samples: RVec<i16>,
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
            pushed_frames: Arc::new(AtomicU64::new(0)),
            dropped_frames: Arc::new(AtomicU64::new(0)),
            error: Arc::new(Mutex::new(None)),
            queue,
            stopping,
            stop: Mutex::new(Some(stop)),
            worker: Mutex::new(None),
        };
        let tap = Tap {
            sink: Arc::clone(&bridge.sink),
            cancelled: Arc::clone(&bridge.cancelled),
            pushed_frames: Arc::clone(&bridge.pushed_frames),
            dropped_frames: Arc::clone(&bridge.dropped_frames),
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
        let dropped_frames = Arc::clone(&self.dropped_frames);
        Arc::new(move || {
            let (cancel, cancelled) = bounded(0);
            let cancel = Mutex::new(Some(cancel));
            let queue = queue.clone();
            let stopping = stopping.clone();
            let dropped_frames = Arc::clone(&dropped_frames);
            let callback = Arc::new(move |samples: &[f32]| {
                let chunk = QueuedPcm {
                    samples: samples
                        .iter()
                        .map(|sample| (sample.clamp(-1.0, 1.0) * 32_767.0) as i16)
                        .collect(),
                    mixer_stopping: cancelled.clone(),
                };
                let accepted = crossbeam_channel::select_biased! {
                    recv(cancelled) -> _ => false,
                    recv(stopping) -> _ => false,
                    send(queue, chunk) -> result => result.is_ok(),
                };
                if !accepted {
                    dropped_frames.fetch_add(samples.len() as u64 / 2, Ordering::Relaxed);
                }
            });
            let cancel_callback = Arc::new(move || {
                cancel
                    .lock()
                    .expect("mixer cancellation lock is poisoned")
                    .take();
            });
            (callback, cancel_callback)
        })
    }

    #[allow(dead_code)] // exposed for session-level diagnostics in later revisions
    pub(crate) fn pushed_frames(&self) -> u64 {
        self.pushed_frames.load(Ordering::Relaxed)
    }

    pub(crate) fn check_error(&self) -> FamilyResult<()> {
        if let Some(error) = self.error.lock().map_err(|_| lock_error())?.as_ref() {
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
        self.stop.lock().map_err(|_| lock_error())?.take();
        self.sink.cancel().into_result()?;
        if let Some(worker) = self.worker.lock().map_err(|_| lock_error())?.take() {
            worker.join().map_err(|_| {
                FamilyError::invalid("ASTRA_EMU_SIGLUS_AUDIO_WORKER", "PCM worker panicked")
            })?;
        }
        match self.error.lock().map_err(|_| lock_error())?.as_ref() {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
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
    pushed_frames: Arc<AtomicU64>,
    dropped_frames: Arc<AtomicU64>,
    error: Arc<Mutex<Option<FamilyError>>>,
}

impl Tap {
    fn push(&self, chunk: RVec<i16>) {
        let frames = chunk.len() as u64 / 2;
        if self.cancelled.load(Ordering::Acquire) {
            self.dropped_frames.fetch_add(frames, Ordering::Relaxed);
            return;
        }
        match self.sink.write(PcmChunk::I16(chunk)).into_result() {
            Ok(AudioWriteStatus::Accepted) => {
                self.pushed_frames.fetch_add(frames, Ordering::Relaxed);
            }
            Ok(AudioWriteStatus::Cancelled) | Ok(AudioWriteStatus::Closed) => {
                self.cancelled.store(true, Ordering::Release);
                self.dropped_frames.fetch_add(frames, Ordering::Relaxed);
            }
            Err(error) => {
                let mut slot = self.error.lock().unwrap();
                if slot.is_none() {
                    *slot = Some(error);
                }
                self.dropped_frames.fetch_add(frames, Ordering::Relaxed);
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
        } else {
            tap.dropped_frames
                .fetch_add(chunk.samples.len() as u64 / 2, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
