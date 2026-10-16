#[path = "audio_decode.rs"]
mod audio_decode;
use audio_decode::SymphoniaBackend;
#[path = "audio_worker.rs"]
mod audio_worker;
use audio_worker::run_worker;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use parking_lot::Mutex;

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::embedding::HostedAudioOperation;
use abi_stable::std_types::RVec;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};
use rfvp::host_api::{AudioStreamId, SoftAudioConfig, SoftAudioMixer};

const OUTPUT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: 48_000,
    channels: 2,
    format: PcmFormat::I16,
};
const QUEUE_CAPACITY: usize = 64;
const COMMAND_WAIT: Duration = Duration::from_millis(20);

struct Job {
    command: HostedAudioOperation,
    completion: Sender<FamilyResult<()>>,
}

pub(crate) struct AudioBridge {
    tx: Option<Sender<Job>>,
    error: Arc<Mutex<Option<FamilyError>>>,
    playback_states: Arc<Mutex<BTreeMap<u32, bool>>>,
    cancelled: Arc<AtomicBool>,
    sink: Option<Arc<AudioSinkBox>>,
    worker: Option<JoinHandle<()>>,
}

impl AudioBridge {
    pub(crate) fn new(sink: AudioSinkBox) -> FamilyResult<Self> {
        sink.configure(OUTPUT).into_result()?;

        let sink = Arc::new(sink);
        let (tx, rx) = bounded(QUEUE_CAPACITY);
        let error = Arc::new(Mutex::new(None));
        let worker_error = Arc::clone(&error);
        let playback_states = Arc::new(Mutex::new(BTreeMap::new()));
        let worker_playback_states = Arc::clone(&playback_states);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_sink = Arc::clone(&sink);
        let worker = thread::Builder::new()
            .name("astra-fvp-audio".into())
            .spawn(move || {
                let result = run_worker(
                    rx,
                    worker_sink.clone(),
                    Arc::clone(&worker_cancelled),
                    worker_playback_states,
                );
                if let Err(value) = result {
                    if !worker_cancelled.load(Ordering::Acquire) && !worker_sink.is_cancelled() {
                        *worker_error.lock() = Some(value);
                    }
                }
                let _ = worker_sink.cancel();
            })
            .map_err(|_| {
                FamilyError::invalid("ASTRA_EMU_FVP_AUDIO_WORKER", "audio worker could not start")
            })?;

        Ok(Self {
            tx: Some(tx),
            error,
            playback_states,
            cancelled,
            sink: Some(sink),
            worker: Some(worker),
        })
    }

    pub(crate) fn submit(&mut self, operation: HostedAudioOperation) -> FamilyResult<()> {
        self.dispatch(operation)
    }

    fn dispatch(&self, command: HostedAudioOperation) -> FamilyResult<()> {
        self.check_error()?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_AUDIO_CANCELLED",
                "audio worker is cancelled",
            ));
        }
        let tx = self.tx.as_ref().ok_or_else(|| {
            FamilyError::invalid("ASTRA_EMU_FVP_AUDIO_CLOSED", "audio worker is closed")
        })?;
        let (completion, result) = bounded(1);
        match tx.try_send(Job {
            command,
            completion,
        }) {
            Ok(()) => match result.recv() {
                Ok(value) => value,
                Err(_) => Err(FamilyError::invalid(
                    "ASTRA_EMU_FVP_AUDIO_CLOSED",
                    "audio worker stopped before applying the command",
                )),
            },
            Err(TrySendError::Full(_)) => Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_AUDIO_CAPACITY",
                "audio command queue is full",
            )),
            Err(TrySendError::Disconnected(_)) => Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_AUDIO_CLOSED",
                "audio worker stopped",
            )),
        }
    }

    fn check_error(&self) -> FamilyResult<()> {
        self.error.lock().clone().map_or(Ok(()), Err)
    }

    pub(crate) fn playback_states(
        &self,
    ) -> FamilyResult<Vec<(rfvp::host_api::AudioStreamId, bool)>> {
        self.check_error()?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_AUDIO_CANCELLED",
                "audio worker is cancelled",
            ));
        }
        let states = self
            .playback_states
            .lock()
            .iter()
            .map(|(id, playing)| (rfvp::host_api::AudioStreamId(*id), *playing))
            .collect();
        Ok(states)
    }

    pub(crate) fn close(&mut self) -> FamilyResult<()> {
        self.cancelled.store(true, Ordering::Release);
        self.tx.take();

        // `write` is allowed to wait for bounded sink capacity. Cancel before
        // joining so a full host queue cannot make lifecycle shutdown hang.
        let cancel_error = self
            .sink
            .as_ref()
            .and_then(|sink| sink.cancel().into_result().err());
        let join_error = self.worker.take().and_then(|worker| {
            worker.join().err().map(|_| {
                FamilyError::invalid(
                    "ASTRA_EMU_FVP_AUDIO_JOIN",
                    "audio worker panicked while stopping",
                )
            })
        });
        self.sink.take();

        if let Some(value) = join_error {
            return Err(value);
        }
        if let Some(value) = cancel_error {
            return Err(value);
        }
        self.check_error()
    }
}

impl Drop for AudioBridge {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn mixer_config() -> SoftAudioConfig {
    SoftAudioConfig {
        output_sample_rate: OUTPUT.sample_rate,
        // RFVP 0.6.0's native BgmPlayer owns four slots. A fading or paused
        // voice still occupies its slot when the next BGM starts.
        max_active_bgm: 4,
        ..SoftAudioConfig::default()
    }
}

fn record_operation_state(
    mixer: &SoftAudioMixer<SymphoniaBackend>,
    states: &Mutex<BTreeMap<u32, bool>>,
    operation: &HostedAudioOperation,
) {
    let Some(id) = operation_stream_id(operation) else {
        return;
    };
    states.lock().insert(id.0, mixer.is_playing(id));
}

fn apply_command(
    mixer: &mut SoftAudioMixer<SymphoniaBackend>,
    playback_states: &Mutex<BTreeMap<u32, bool>>,
    operation: HostedAudioOperation,
) -> FamilyResult<()> {
    apply_operation(mixer, &operation)?;
    record_operation_state(mixer, playback_states, &operation);
    Ok(())
}

fn refresh_playback_states(
    mixer: &SoftAudioMixer<SymphoniaBackend>,
    states: &Mutex<BTreeMap<u32, bool>>,
) {
    let mut states = states.lock();
    for (id, playing) in states.iter_mut() {
        *playing = mixer.is_playing(rfvp::host_api::AudioStreamId(*id));
    }
}

fn operation_stream_id(operation: &HostedAudioOperation) -> Option<rfvp::host_api::AudioStreamId> {
    match operation {
        HostedAudioOperation::LoadEncoded { id, .. }
        | HostedAudioOperation::CreateStream { id, .. }
        | HostedAudioOperation::SubmitI16 { id, .. }
        | HostedAudioOperation::SubmitF32 { id, .. }
        | HostedAudioOperation::Play { id, .. }
        | HostedAudioOperation::Stop { id, .. }
        | HostedAudioOperation::Pause(id)
        | HostedAudioOperation::Resume(id)
        | HostedAudioOperation::SetParams { id, .. }
        | HostedAudioOperation::DestroyStream(id) => Some(*id),
        HostedAudioOperation::LoadResource { id, .. } => Some(*id),
        HostedAudioOperation::SetMasterVolume(_) => None,
    }
}

fn apply_operation(
    mixer: &mut SoftAudioMixer<SymphoniaBackend>,
    operation: &HostedAudioOperation,
) -> FamilyResult<()> {
    let (name, result) = match operation {
        HostedAudioOperation::LoadResource { .. } => {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_AUDIO_RESOURCE",
                "resource audio must be resolved by the family session",
            ))
        }
        HostedAudioOperation::LoadEncoded { id, kind, bytes } => {
            ("audio load", mixer.load_encoded(*id, *kind, bytes))
        }
        HostedAudioOperation::CreateStream { id, desc } => {
            ("audio stream creation", mixer.create_stream(*id, *desc))
        }
        HostedAudioOperation::SubmitI16 { id, samples } => {
            ("audio i16 submission", mixer.submit_i16(*id, samples))
        }
        HostedAudioOperation::SubmitF32 { id, samples } => {
            ("audio f32 submission", mixer.submit_f32(*id, samples))
        }
        HostedAudioOperation::Play {
            id,
            params,
            fade_in_ms,
        } => ("audio play", mixer.play(*id, *params, *fade_in_ms)),
        HostedAudioOperation::Stop { id, fade_ms } => ("audio stop", mixer.stop(*id, *fade_ms)),
        HostedAudioOperation::Pause(id) => ("audio pause", mixer.pause(*id)),
        HostedAudioOperation::Resume(id) => ("audio resume", mixer.resume(*id)),
        HostedAudioOperation::SetParams { id, params } => {
            ("audio parameter update", mixer.set_params(*id, *params))
        }
        HostedAudioOperation::SetMasterVolume(volume) => {
            ("audio master volume", mixer.set_master_volume(*volume))
        }
        HostedAudioOperation::DestroyStream(id) => {
            mixer.destroy_stream(*id);
            return Ok(());
        }
    };
    result.map_err(|cause| {
        let error = crate::error::rfvp_operation(cause, name);
        let slot_kind = match operation_stream_id(operation).and_then(AudioStreamId::slot_kind) {
            Some(rfvp::host_api::AudioSlotKind::Bgm) => "bgm",
            Some(rfvp::host_api::AudioSlotKind::Se) => "se",
            Some(rfvp::host_api::AudioSlotKind::Movie) => "movie",
            None => "none",
        };
        tracing::error!(
            event = "astra.emu.fvp.audio.operation_failed",
            operation = name,
            code = error.code(),
            slot_kind,
            bgm_limit = mixer.config().max_active_bgm,
            se_limit = mixer.config().max_active_se,
            total_limit = mixer.config().max_active_total,
            bgm_voices = mixer.voice_count(rfvp::host_api::AudioSlotKind::Bgm),
            se_voices = mixer.voice_count(rfvp::host_api::AudioSlotKind::Se),
            bgm_playing = (0..rfvp::host_api::BGM_LOGICAL_SLOT_COUNT)
                .filter(|slot| mixer.is_playing(AudioStreamId::bgm(*slot)))
                .count(),
            se_playing = (0..rfvp::host_api::SE_LOGICAL_SLOT_COUNT)
                .filter(|slot| mixer.is_playing(AudioStreamId::se(*slot)))
                .count(),
        );
        error
    })
}

fn write_pcm(sink: &AudioSinkBox, samples: Vec<i16>) -> FamilyResult<()> {
    let chunk = PcmChunk::I16(RVec::from(samples));
    chunk.validate(OUTPUT)?;
    match sink.write(chunk).into_result()? {
        AudioWriteStatus::Accepted => Ok(()),
        AudioWriteStatus::Cancelled => Err(FamilyError::invalid(
            "ASTRA_EMU_FVP_AUDIO_CANCELLED",
            "audio sink cancelled output",
        )),
        AudioWriteStatus::Closed => Err(FamilyError::invalid(
            "ASTRA_EMU_FVP_AUDIO_CLOSED",
            "audio sink closed output",
        )),
    }
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
