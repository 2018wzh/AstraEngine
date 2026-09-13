//! PCM bridge from the kira mixer thread to the host audio sink.
//!
//! The vendored engine mixes through `TapBackend` (see
//! `siglus_scene_vm::audio::kira_hub::hosted_tap`); its worker converts the
//! mixer output to stereo i16 here and pushes it into the bounded, cancellable
//! host queue. A full queue blocks the mixer thread, which paces audio to real
//! time; cancellation comes only from the host sink.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use abi_stable::std_types::RVec;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};

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
}

impl PcmBridge {
    pub(crate) fn new(sink: AudioSinkBox) -> FamilyResult<Self> {
        sink.configure(OUTPUT_FORMAT).into_result()?;
        Ok(Self {
            sink: Arc::new(sink),
            cancelled: Arc::new(AtomicBool::new(false)),
            pushed_frames: Arc::new(AtomicU64::new(0)),
            dropped_frames: Arc::new(AtomicU64::new(0)),
            error: Arc::new(Mutex::new(None)),
        })
    }

    /// The callback handed to the engine's tap registry. It runs on the kira
    /// mixer worker thread and never blocks on family state other than the
    /// bounded host queue.
    pub(crate) fn tap_callback(&self) -> siglus_scene_vm::audio::kira_hub::hosted_tap::TapCallback {
        let tap = Tap {
            sink: Arc::clone(&self.sink),
            cancelled: Arc::clone(&self.cancelled),
            pushed_frames: Arc::clone(&self.pushed_frames),
            dropped_frames: Arc::clone(&self.dropped_frames),
            error: Arc::clone(&self.error),
        };
        Arc::new(move |samples: &[f32]| tap.push(samples))
    }

    #[allow(dead_code)] // exposed for session-level diagnostics in later revisions
    pub(crate) fn pushed_frames(&self) -> u64 {
        self.pushed_frames.load(Ordering::Relaxed)
    }

    pub(crate) fn check_error(&self) -> FamilyResult<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_SIGLUS_AUDIO_CANCELLED",
                "host audio queue is cancelled",
            ));
        }
        if let Some(error) = self.error.lock().unwrap().as_ref() {
            return Err(error.clone());
        }
        Ok(())
    }

    pub(crate) fn close(&self) -> FamilyResult<()> {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.sink.cancel();
        Ok(())
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
    fn push(&self, samples: &[f32]) {
        if self.cancelled.load(Ordering::Acquire) {
            self.dropped_frames
                .fetch_add(samples.len() as u64 / 2, Ordering::Relaxed);
            return;
        }
        let mut chunk = RVec::with_capacity(samples.len());
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            chunk.push((clamped * 32_767.0) as i16);
        }
        match self.sink.write(PcmChunk::I16(chunk)).into_result() {
            Ok(AudioWriteStatus::Accepted) => {
                self.pushed_frames
                    .fetch_add(samples.len() as u64 / 2, Ordering::Relaxed);
            }
            Ok(AudioWriteStatus::Cancelled) | Ok(AudioWriteStatus::Closed) => {
                self.cancelled.store(true, Ordering::Release);
                self.dropped_frames
                    .fetch_add(samples.len() as u64 / 2, Ordering::Relaxed);
            }
            Err(error) => {
                let mut slot = self.error.lock().unwrap();
                if slot.is_none() {
                    *slot = Some(error);
                }
                self.dropped_frames
                    .fetch_add(samples.len() as u64 / 2, Ordering::Relaxed);
            }
        }
    }
}
