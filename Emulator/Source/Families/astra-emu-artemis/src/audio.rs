//! Software audio mixer for the Artemis family.
//!
//! The vendored Artemis core only emits media commands; real decoding and
//! mixing are host-side. This module owns a mixer worker thread that decodes
//! sources with symphonia, applies the Artemis gain/pan/fade/loop semantics,
//! and pushes interleaved stereo i16 chunks into the bounded host sink. A
//! full queue blocks the worker, pacing audio to the real output device;
//! cancellation comes from the sink or the session shutdown.
//!
//! Natural end of a non-looping source is reported back to the session so it
//! can call `notify_sound_finished` on the runtime; the worker never touches
//! the runtime itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use art3m1s_core::media::MediaSource;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};

use crate::error;

pub(crate) const SAMPLE_RATE: u32 = 48_000;
pub(crate) const CHANNELS: u16 = 2;
pub(crate) const OUTPUT_FORMAT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: SAMPLE_RATE,
    channels: CHANNELS,
    format: PcmFormat::F32,
};

/// Stereo frames per sink write: 10 ms at 48 kHz.
const CHUNK_FRAMES: usize = 480;
/// Stereo frames per millisecond at the output rate.

// ---------------------------------------------------------------------------
// Mixer protocol
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channel {
    Bgm,
    Se,
    Voice,
}

/// A resolved, readable audio source plus the optional A/B loop file.
#[derive(Clone)]
pub(crate) struct SourcePair {
    pub base: Arc<dyn MediaSource>,
    /// Artemis `loop_file`: the segment that loops after the intro pass.
    pub loop_file: Option<Arc<dyn MediaSource>>,
}

pub(crate) enum MixerCommand {
    Play {
        id: Option<String>,
        channel: Channel,
        sources: SourcePair,
        loop_play: bool,
        gain: f32,
        pan: f32,
        fade_ms: u64,
    },
    /// `id: None` targets the single BGM voice.
    Stop {
        id: Option<String>,
        fade_ms: u64,
    },
    Fade {
        id: Option<String>,
        gain: f32,
        time_ms: u64,
    },
    Pan {
        id: Option<String>,
        pan: f32,
        time_ms: u64,
    },
    StopAll {
        fade_ms: u64,
    },
    SetVolume {
        channel: Channel,
        value: f32,
    },
    Suspend(bool),
    Shutdown,
}

/// Completion reported from the mixer worker to the session thread.
pub(crate) struct SoundFinished {
    /// `None` is the BGM channel, `Some(id)` an SE or voice.
    pub id: Option<String>,
}

pub(crate) struct AudioBridge {
    tx: SyncSender<MixerCommand>,
    finished_rx: Receiver<SoundFinished>,
    error: Arc<Mutex<Option<FamilyError>>>,
    cancelled: Arc<AtomicBool>,
    sink: Arc<AudioSinkBox>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl AudioBridge {
    pub(crate) fn start(sink: AudioSinkBox) -> FamilyResult<Self> {
        sink.configure(OUTPUT_FORMAT).into_result()?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<MixerCommand>(128);
        let (finished_tx, finished_rx) = std::sync::mpsc::sync_channel::<SoundFinished>(128);
        let error = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let sink = Arc::new(sink);
        let worker_state = WorkerState {
            tx: finished_tx,
            error: Arc::clone(&error),
            cancelled: Arc::clone(&cancelled),
        };
        let worker_sink = Arc::clone(&sink);
        let handle = std::thread::Builder::new()
            .name("astra-emu-artemis-audio".into())
            .spawn(move || run_mixer(rx, worker_state, worker_sink))
            .map_err(|_| {
                error::invalid(
                    "ASTRA_EMU_ARTEMIS_AUDIO_WORKER",
                    "failed to spawn the audio mixer worker",
                )
            })?;
        Ok(Self {
            tx,
            finished_rx,
            error,
            cancelled,
            sink,
            handle: Some(handle),
        })
    }

    /// Commands are bounded; queue failure ends the session.
    pub(crate) fn send(&self, command: MixerCommand) -> FamilyResult<()> {
        self.tx.try_send(command).map_err(|_| {
            error::invalid(
                "ASTRA_EMU_ARTEMIS_AUDIO_QUEUE",
                "audio command queue unavailable",
            )
        })
    }

    /// Drains natural-completion notifications produced by the worker.
    pub(crate) fn drain_finished(&self) -> Vec<SoundFinished> {
        self.finished_rx.try_iter().collect()
    }

    pub(crate) fn check_error(&self) -> FamilyResult<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_AUDIO_CANCELLED",
                "host audio queue is cancelled",
            ));
        }
        if let Some(slot) = self.error.lock().unwrap().as_ref() {
            return Err(slot.clone());
        }
        Ok(())
    }

    /// Cancels the sink first so the worker cannot stay blocked in `write`,
    /// then signals shutdown and joins the worker.
    pub(crate) fn close(mut self) -> FamilyResult<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> FamilyResult<()> {
        self.cancelled.store(true, Ordering::Release);
        let cancelled = self.sink.cancel().into_result();
        let _ = self.tx.try_send(MixerCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            handle.join().map_err(|_| {
                error::invalid(
                    "ASTRA_EMU_ARTEMIS_AUDIO_WORKER",
                    "the audio mixer worker panicked",
                )
            })?
        }
        cancelled?;
        if let Some(slot) = self.error.lock().unwrap().as_ref() {
            return Err(slot.clone());
        }
        Ok(())
    }
}

impl Drop for AudioBridge {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

struct WorkerState {
    tx: SyncSender<SoundFinished>,
    error: Arc<Mutex<Option<FamilyError>>>,
    cancelled: Arc<AtomicBool>,
}

fn report_error(state: &WorkerState, message: String) {
    let mut slot = state.error.lock().unwrap();
    if slot.is_none() {
        *slot = Some(error::invalid("ASTRA_EMU_ARTEMIS_AUDIO_DECODE", message));
    }
}

mod mixer;
fn run_mixer(rx: Receiver<MixerCommand>, state: WorkerState, sink: Arc<AudioSinkBox>) {
    let result = mixer::run(rx, &state, &sink);
    if let Err(failure) = result {
        if !state.cancelled.load(Ordering::Acquire) {
            report_error(&state, failure.to_string());
        }
    }
}

#[cfg(test)]
mod tests;
