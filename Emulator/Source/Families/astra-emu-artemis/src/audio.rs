//! Native session-owned Kira audio service. The device never belongs to Manager.
use crate::error::{self, NativeError, NativeResult};
use art3m1s_core::media::MediaSource;
use parking_lot::Mutex;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channel {
    Bgm,
    Se,
    Voice,
    Video,
}
#[derive(Clone)]
pub(crate) struct SourcePair {
    pub base: Arc<dyn MediaSource>,
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
    StopVideo {
        id: Option<String>,
    },
    StopVideoAll,
    SetVolume {
        channel: Channel,
        value: f32,
    },
    Suspend(bool),
    Shutdown,
}
pub(crate) struct SoundFinished {
    pub id: Option<String>,
}
pub(crate) struct AudioBridge {
    tx: SyncSender<MixerCommand>,
    finished_rx: Receiver<SoundFinished>,
    error: Arc<Mutex<Option<NativeError>>>,
    cancelled: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl AudioBridge {
    pub(crate) fn start(cancelled: Arc<AtomicBool>) -> NativeResult<Self> {
        let (tx, rx) = std::sync::mpsc::sync_channel(128);
        let (finished_tx, finished_rx) = std::sync::mpsc::sync_channel(128);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let error = Arc::new(Mutex::new(None));
        let state = WorkerState {
            tx: finished_tx,
            error: error.clone(),
            cancelled: cancelled.clone(),
        };
        let handle = std::thread::Builder::new()
            .name("artemis-native-audio".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    mixer::run(rx, &state, ready_tx)
                }));
                let failure = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(_) => Some(error::invalid(
                        "ASTRA_EMU_ARTEMIS_AUDIO_PANIC",
                        "native audio worker panicked",
                    )),
                };
                if let Some(error) = failure {
                    *state.error.lock() = Some(error);
                }
            })
            .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_AUDIO_WORKER", e.to_string()))?;
        let mut bridge = Self {
            tx,
            finished_rx,
            error,
            cancelled,
            handle: Some(handle),
        };
        if ready_rx.recv().ok() != Some(true) {
            let _ = bridge.shutdown();
            return Err(bridge.error.lock().clone().unwrap_or_else(|| {
                error::invalid(
                    "ASTRA_EMU_ARTEMIS_AUDIO_DEVICE",
                    "native audio device initialization failed",
                )
            }));
        }
        Ok(bridge)
    }
    pub(crate) fn send(&self, command: MixerCommand) -> NativeResult<ControlFlow<()>> {
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(ControlFlow::Break(()));
        }
        match self.tx.try_send(command) {
            Ok(()) => Ok(ControlFlow::Continue(())),
            Err(std::sync::mpsc::TrySendError::Disconnected(_))
                if self.cancelled.load(Ordering::Acquire) =>
            {
                Ok(ControlFlow::Break(()))
            }
            Err(_) => Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_AUDIO_QUEUE",
                "audio command queue unavailable",
            )),
        }
    }
    pub(crate) fn drain_finished(&self) -> impl Iterator<Item = SoundFinished> + '_ {
        self.finished_rx.try_iter()
    }
    pub(crate) fn check_error(&self) -> NativeResult<()> {
        if let Some(error) = self.error.lock().clone() {
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn close(mut self) -> NativeResult<()> {
        self.shutdown()
    }
    fn shutdown(&mut self) -> NativeResult<()> {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.tx.try_send(MixerCommand::Shutdown);
        let joined = self.handle.take().map(|handle| handle.join()).transpose();
        if joined.is_err() {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_AUDIO_WORKER",
                "native audio worker panicked",
            ));
        }
        self.check_error()
    }
}
impl Drop for AudioBridge {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
struct WorkerState {
    tx: SyncSender<SoundFinished>,
    error: Arc<Mutex<Option<NativeError>>>,
    cancelled: Arc<AtomicBool>,
}
mod mixer;
#[cfg(test)]
mod tests;
