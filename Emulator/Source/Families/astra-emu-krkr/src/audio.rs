//! PCM bridge from the engine audio thread to the host audio sink.
//!
//! The engine mixer thread calls [`PcmSink::push`]. The host `AudioSink` is a
//! bounded, cancellable queue; when it reports `Cancelled` or `Closed` the
//! bridge remembers the state so the session can fail fast instead of
//! blocking engine shutdown on a dead queue.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use abi_stable::std_types::RVec;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};

pub(crate) struct PcmBridge {
    sink: Arc<AudioSinkBox>,
    cancelled: Arc<AtomicBool>,
    pushed_frames: Arc<AtomicU64>,
    dropped_frames: Arc<AtomicU64>,
    error: Arc<Mutex<Option<FamilyError>>>,
}

pub(crate) struct PcmTap {
    sink: Arc<AudioSinkBox>,
    cancelled: Arc<AtomicBool>,
    pushed_frames: Arc<AtomicU64>,
    dropped_frames: Arc<AtomicU64>,
    error: Arc<Mutex<Option<FamilyError>>>,
}

impl PcmBridge {
    pub(crate) fn new(sink: AudioSinkBox, format: PcmFormatSpec) -> FamilyResult<Self> {
        sink.configure(format).into_result()?;
        Ok(Self {
            sink: Arc::new(sink),
            cancelled: Arc::new(AtomicBool::new(false)),
            pushed_frames: Arc::new(AtomicU64::new(0)),
            dropped_frames: Arc::new(AtomicU64::new(0)),
            error: Arc::new(Mutex::new(None)),
        })
    }

    pub(crate) fn tap(&self) -> PcmTap {
        PcmTap {
            sink: Arc::clone(&self.sink),
            cancelled: Arc::clone(&self.cancelled),
            pushed_frames: Arc::clone(&self.pushed_frames),
            dropped_frames: Arc::clone(&self.dropped_frames),
            error: Arc::clone(&self.error),
        }
    }

    #[allow(dead_code)] // exposed for session-level diagnostics in later revisions
    pub(crate) fn pushed_frames(&self) -> u64 {
        self.pushed_frames.load(Ordering::Relaxed)
    }

    pub(crate) fn check_error(&self) -> FamilyResult<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_KRKR_AUDIO_CANCELLED",
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

impl PcmTap {
    /// Called from the engine audio thread. Never blocks on family state;
    /// the bounded host queue enforces backpressure.
    pub(crate) fn push(&self, samples: &[i16], channels: u16) {
        if self.cancelled.load(Ordering::Acquire) {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut chunk = RVec::with_capacity(samples.len());
        chunk.extend_from_slice(samples);
        let frame_count = u64::try_from(samples.len() / usize::from(channels)).unwrap_or(0);
        match self.sink.write(PcmChunk::I16(chunk)).into_result() {
            Ok(AudioWriteStatus::Accepted) => {
                self.pushed_frames.fetch_add(frame_count, Ordering::Relaxed);
            }
            Ok(AudioWriteStatus::Cancelled) | Ok(AudioWriteStatus::Closed) => {
                self.cancelled.store(true, Ordering::Release);
                self.dropped_frames
                    .fetch_add(frame_count, Ordering::Relaxed);
            }
            Err(error) => {
                let mut slot = self.error.lock().unwrap();
                if slot.is_none() {
                    *slot = Some(error);
                }
                self.dropped_frames
                    .fetch_add(frame_count, Ordering::Relaxed);
            }
        }
    }
}

pub(crate) const fn engine_output_format(sample_rate: u32) -> PcmFormatSpec {
    PcmFormatSpec {
        sample_rate,
        channels: 2,
        format: PcmFormat::I16,
    }
}
