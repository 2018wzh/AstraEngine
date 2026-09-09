use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use abi_stable::std_types::RVec;
use astra_emu_family_api::{
    AudioSinkBox, AudioWriteStatus, FamilyError, FamilyResult, PcmChunk, PcmFormat, PcmFormatSpec,
};
use rfvp::host_api::{
    AudioSampleFormat, AudioStreamDesc, AudioStreamId, EncodedAudioKind, RfvpError,
    SoftAudioConfig, SoftAudioMixer, SoftAudioVorbis,
};
use rfvp::hosted::HostedAudioOperation;

use crate::video::VideoAudioPlayback;

const OUTPUT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: 48_000,
    channels: 2,
    format: PcmFormat::I16,
};
const QUEUE_CAPACITY: usize = 64;
const COMMAND_WAIT: Duration = Duration::from_millis(20);

struct SymphoniaBackend;

struct DecoderState {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: u16,
    had_samples: bool,
    pending: Vec<i16>,
}

impl SoftAudioVorbis for SymphoniaBackend {
    type Decoder = DecoderState;

    fn open(
        &mut self,
        bytes: &[u8],
    ) -> rfvp::host_api::RfvpResult<(Self::Decoder, AudioStreamDesc)> {
        self.open_with_kind(bytes, EncodedAudioKind::Unknown)
    }

    fn decode_interleaved_i16(
        &mut self,
        state: &mut Self::Decoder,
        out: &mut [i16],
    ) -> rfvp::host_api::RfvpResult<usize> {
        use symphonia::core::audio::SampleBuffer;

        while state.pending.len() < out.len() {
            let packet = match state.format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break
                }
                Err(error) => return Err(map_symphonia_error(&error)),
            };
            if packet.track_id() != state.track_id {
                continue;
            }

            let decoded = state
                .decoder
                .decode(&packet)
                .map_err(|error| map_symphonia_error(&error))?;
            if decoded.spec().rate != state.sample_rate
                || decoded.spec().channels.count() as u16 != state.channels
            {
                return Err(RfvpError::InvalidData);
            }
            let mut samples = SampleBuffer::<i16>::new(decoded.capacity() as u64, *decoded.spec());
            samples.copy_interleaved_ref(decoded);
            state.had_samples |= !samples.samples().is_empty();
            state.pending.extend_from_slice(samples.samples());
        }

        let count = out.len().min(state.pending.len());
        out[..count].copy_from_slice(&state.pending[..count]);
        state.pending.drain(..count);
        Ok(count)
    }

    fn seek_start(&mut self, state: &mut Self::Decoder) -> rfvp::host_api::RfvpResult<()> {
        use symphonia::core::formats::{SeekMode, SeekTo};

        if !state.had_samples {
            return Err(RfvpError::InvalidData);
        }
        state
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::TimeStamp {
                    ts: 0,
                    track_id: state.track_id,
                },
            )
            .map_err(|error| map_symphonia_error(&error))?;
        state.decoder.reset();
        state.pending.clear();
        Ok(())
    }

    fn close(&mut self, _state: Self::Decoder) {}
}

impl SymphoniaBackend {
    fn open_with_kind(
        &mut self,
        bytes: &[u8],
        kind: EncodedAudioKind,
    ) -> rfvp::host_api::RfvpResult<(DecoderState, AudioStreamDesc)> {
        use symphonia::core::{
            codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
            meta::MetadataOptions, probe::Hint,
        };

        let source = Box::new(Cursor::new(bytes.to_vec()));
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        match kind {
            EncodedAudioKind::Wav => {
                hint.with_extension("wav");
            }
            EncodedAudioKind::Ogg => {
                hint.with_extension("ogg");
            }
            EncodedAudioKind::Mp3 => {
                hint.with_extension("mp3");
            }
            EncodedAudioKind::Flac => {
                hint.with_extension("flac");
            }
            EncodedAudioKind::Unknown => {
                if bytes.starts_with(b"RIFF") {
                    hint.with_extension("wav");
                } else if bytes.starts_with(b"OggS") {
                    hint.with_extension("ogg");
                } else if bytes.starts_with(b"fLaC") {
                    hint.with_extension("flac");
                }
            }
        };
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|error| map_symphonia_error(&error))?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or(RfvpError::Unsupported)?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        let sample_rate = params.sample_rate.ok_or(RfvpError::Unsupported)?;
        let channels = params.channels.ok_or(RfvpError::Unsupported)?.count() as u16;
        if channels == 0 || channels > 2 {
            return Err(RfvpError::Unsupported);
        }
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|error| map_symphonia_error(&error))?;
        let desc = AudioStreamDesc {
            sample_rate,
            channels,
            sample_format: AudioSampleFormat::I16,
        };
        Ok((
            DecoderState {
                format,
                decoder,
                track_id,
                sample_rate,
                channels,
                had_samples: false,
                pending: Vec::new(),
            },
            desc,
        ))
    }
}

fn map_symphonia_error(error: &symphonia::core::errors::Error) -> RfvpError {
    use symphonia::core::errors::Error;

    match error {
        Error::IoError(_) => RfvpError::Io,
        Error::DecodeError(_) => RfvpError::InvalidData,
        Error::SeekError(_) => RfvpError::Backend,
        Error::Unsupported(_) => RfvpError::Unsupported,
        Error::LimitError(_) => RfvpError::CapacityExceeded,
        Error::ResetRequired => RfvpError::Backend,
    }
}

enum WorkerCommand {
    Hosted(HostedAudioOperation),
    StartVideo { id: AudioStreamId, bytes: Arc<[u8]> },
    StopVideo { id: AudioStreamId },
}

#[derive(Clone, Copy)]
enum WorkerReply {
    Applied,
    VideoStarted(bool),
}

struct Job {
    command: WorkerCommand,
    completion: SyncSender<FamilyResult<WorkerReply>>,
}

pub(crate) struct AudioBridge {
    tx: Option<SyncSender<Job>>,
    error: Arc<Mutex<Option<FamilyError>>>,
    playback_states: Arc<Mutex<BTreeMap<u32, bool>>>,
    used_stream_ids: BTreeSet<u32>,
    cancelled: Arc<AtomicBool>,
    sink: Option<Arc<AudioSinkBox>>,
    worker: Option<JoinHandle<()>>,
}

impl AudioBridge {
    pub(crate) fn new(sink: AudioSinkBox) -> FamilyResult<Self> {
        sink.configure(OUTPUT).into_result().map_err(|_| {
            FamilyError::invalid(
                "ASTRA_EMU_FVP_AUDIO_CONFIG",
                "audio sink rejected the fixed output format",
            )
        })?;

        let sink = Arc::new(sink);
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
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
                        *worker_error
                            .lock()
                            .expect("audio worker error mutex poisoned") = Some(value);
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
            used_stream_ids: BTreeSet::new(),
            cancelled,
            sink: Some(sink),
            worker: Some(worker),
        })
    }

    pub(crate) fn submit(&mut self, operation: HostedAudioOperation) -> FamilyResult<()> {
        // RFVP emits Tick as a host pacing hint. The worker has its own clock:
        // audio advances when the bounded sink accepts mixed PCM, never when
        // the gameplay thread calls advance.
        if matches!(operation, HostedAudioOperation::Tick { .. }) {
            return Ok(());
        }

        let stream_mutation = stream_mutation(&operation);
        let result = self.dispatch(WorkerCommand::Hosted(operation));
        if matches!(result, Ok(WorkerReply::Applied)) {
            self.record_stream_id(stream_mutation);
        }
        result.map(|_| ())
    }

    pub(crate) fn start_video_audio(
        &mut self,
        id: AudioStreamId,
        bytes: Arc<[u8]>,
    ) -> FamilyResult<bool> {
        match self.dispatch(WorkerCommand::StartVideo { id, bytes })? {
            WorkerReply::VideoStarted(present) => Ok(present),
            WorkerReply::Applied => Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_VIDEO_AUDIO_WORKER",
                "audio worker returned an invalid video start response",
            )),
        }
    }

    pub(crate) fn stop_video_audio(&mut self, id: AudioStreamId) -> FamilyResult<()> {
        match self.dispatch(WorkerCommand::StopVideo { id })? {
            WorkerReply::Applied => Ok(()),
            WorkerReply::VideoStarted(_) => Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_VIDEO_AUDIO_WORKER",
                "audio worker returned an invalid video stop response",
            )),
        }
    }

    pub(crate) fn video_audio_playing(&self, id: AudioStreamId) -> FamilyResult<bool> {
        self.check_error()?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FVP_AUDIO_CANCELLED",
                "audio worker is cancelled",
            ));
        }
        Ok(self
            .playback_states
            .lock()
            .expect("audio playback state mutex poisoned")
            .get(&id.0)
            .copied()
            .unwrap_or(false))
    }

    fn dispatch(&self, command: WorkerCommand) -> FamilyResult<WorkerReply> {
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
        let (completion, result) = mpsc::sync_channel(1);
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

    pub(crate) fn allocate_video_stream_id(&mut self) -> FamilyResult<AudioStreamId> {
        for slot in (0..rfvp::host_api::SE_LOGICAL_SLOT_COUNT).rev() {
            let id = AudioStreamId::se(slot);
            if self.used_stream_ids.insert(id.0) {
                return Ok(id);
            }
        }
        Err(FamilyError::invalid(
            "ASTRA_EMU_FVP_VIDEO_AUDIO_CAPACITY",
            "no free audio stream is available for WMV playback",
        ))
    }

    pub(crate) fn release_stream_id(&mut self, id: AudioStreamId) {
        self.used_stream_ids.remove(&id.0);
        self.playback_states
            .lock()
            .expect("audio playback state mutex poisoned")
            .remove(&id.0);
    }

    fn record_stream_id(&mut self, mutation: Option<StreamMutation>) {
        match mutation {
            Some(StreamMutation::Add(id)) => {
                self.used_stream_ids.insert(id.0);
            }
            Some(StreamMutation::Remove(id)) => {
                self.used_stream_ids.remove(&id.0);
            }
            None => {}
        }
    }

    fn check_error(&self) -> FamilyResult<()> {
        self.error
            .lock()
            .expect("audio worker error mutex poisoned")
            .clone()
            .map_or(Ok(()), Err)
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
            .expect("audio playback state mutex poisoned")
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
        let cancel_error = self.sink.as_ref().and_then(|sink| {
            sink.cancel().into_result().err().map(|_| {
                FamilyError::invalid(
                    "ASTRA_EMU_FVP_AUDIO_CANCEL",
                    "audio sink rejected cancellation",
                )
            })
        });
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

enum StreamMutation {
    Add(AudioStreamId),
    Remove(AudioStreamId),
}

fn stream_mutation(operation: &HostedAudioOperation) -> Option<StreamMutation> {
    match operation {
        HostedAudioOperation::DestroyStream(id) => Some(StreamMutation::Remove(*id)),
        HostedAudioOperation::LoadResource { id, .. }
        | HostedAudioOperation::LoadEncoded { id, .. }
        | HostedAudioOperation::CreateStream { id, .. } => Some(StreamMutation::Add(*id)),
        _ => None,
    }
}

impl Drop for AudioBridge {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn run_worker(
    rx: Receiver<Job>,
    sink: Arc<AudioSinkBox>,
    cancelled: Arc<AtomicBool>,
    playback_states: Arc<Mutex<BTreeMap<u32, bool>>>,
) -> FamilyResult<()> {
    let mut mixer = SoftAudioMixer::new(
        SymphoniaBackend,
        SoftAudioConfig {
            output_sample_rate: OUTPUT.sample_rate,
            ..SoftAudioConfig::default()
        },
    );
    let mix_samples = mixer
        .config()
        .mix_frames
        .checked_mul(usize::from(OUTPUT.channels))
        .ok_or_else(|| {
            FamilyError::invalid("ASTRA_EMU_FVP_AUDIO_BUFFER", "audio buffer size overflow")
        })?;
    let mut video_audio = BTreeMap::new();

    loop {
        if cancelled.load(Ordering::Acquire) || sink.is_cancelled() {
            break;
        }

        let mut received = false;
        loop {
            match rx.try_recv() {
                Ok(job) => {
                    received = true;
                    let result =
                        apply_command(&mut mixer, &mut video_audio, &playback_states, job.command);
                    let _ = job.completion.send(result.clone());
                    result.map(|_| ())?;
                    if cancelled.load(Ordering::Acquire) || sink.is_cancelled() {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    mixer.shutdown();
                    return Ok(());
                }
            }
        }
        if cancelled.load(Ordering::Acquire) || sink.is_cancelled() {
            break;
        }

        let mut samples = vec![0_i16; mix_samples];
        let mixer_active = mixer.mix_next(&mut samples).map_err(crate::error::rfvp)?;
        let video_active = mix_video_audio(&mut video_audio, &playback_states, &mut samples)?;
        if mixer_active || video_active {
            refresh_playback_states(&mixer, &video_audio, &playback_states);
            write_pcm(&sink, samples)?;
            continue;
        }
        refresh_playback_states(&mixer, &video_audio, &playback_states);

        if !received {
            match rx.recv_timeout(COMMAND_WAIT) {
                Ok(job) => {
                    let result =
                        apply_command(&mut mixer, &mut video_audio, &playback_states, job.command);
                    let _ = job.completion.send(result.clone());
                    result.map(|_| ())?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    mixer.shutdown();
                    return Ok(());
                }
            }
        }
    }

    mixer.shutdown();
    Ok(())
}

fn record_operation_state(
    mixer: &SoftAudioMixer<SymphoniaBackend>,
    states: &Mutex<BTreeMap<u32, bool>>,
    operation: &HostedAudioOperation,
) {
    let Some(id) = operation_stream_id(operation) else {
        return;
    };
    states
        .lock()
        .expect("audio playback state mutex poisoned")
        .insert(id.0, mixer.is_playing(id));
}

fn apply_command(
    mixer: &mut SoftAudioMixer<SymphoniaBackend>,
    video_audio: &mut BTreeMap<u32, VideoAudioPlayback>,
    playback_states: &Mutex<BTreeMap<u32, bool>>,
    command: WorkerCommand,
) -> FamilyResult<WorkerReply> {
    match command {
        WorkerCommand::Hosted(operation) => {
            apply_operation(mixer, &operation)?;
            record_operation_state(mixer, playback_states, &operation);
            Ok(WorkerReply::Applied)
        }
        WorkerCommand::StartVideo { id, bytes } => {
            if video_audio.contains_key(&id.0) {
                return Err(FamilyError::invalid(
                    "ASTRA_EMU_FVP_VIDEO_AUDIO_CONCURRENT",
                    "video audio stream is already active",
                ));
            }
            let Some(stream) = VideoAudioPlayback::open(bytes)? else {
                playback_states
                    .lock()
                    .expect("audio playback state mutex poisoned")
                    .insert(id.0, false);
                return Ok(WorkerReply::VideoStarted(false));
            };
            playback_states
                .lock()
                .expect("audio playback state mutex poisoned")
                .insert(id.0, true);
            video_audio.insert(id.0, stream);
            Ok(WorkerReply::VideoStarted(true))
        }
        WorkerCommand::StopVideo { id } => {
            video_audio.remove(&id.0);
            playback_states
                .lock()
                .expect("audio playback state mutex poisoned")
                .remove(&id.0);
            Ok(WorkerReply::Applied)
        }
    }
}

fn mix_video_audio(
    video_audio: &mut BTreeMap<u32, VideoAudioPlayback>,
    playback_states: &Mutex<BTreeMap<u32, bool>>,
    output: &mut [i16],
) -> FamilyResult<bool> {
    let mut active = false;
    let mut finished = Vec::new();
    for (id, stream) in video_audio.iter_mut() {
        active |= stream.mix_into(OUTPUT.sample_rate, output)?;
        if stream.is_finished() {
            finished.push(*id);
        }
    }
    if !finished.is_empty() {
        let mut states = playback_states
            .lock()
            .expect("audio playback state mutex poisoned");
        for id in finished {
            video_audio.remove(&id);
            states.insert(id, false);
        }
    }
    Ok(active)
}

fn refresh_playback_states(
    mixer: &SoftAudioMixer<SymphoniaBackend>,
    video_audio: &BTreeMap<u32, VideoAudioPlayback>,
    states: &Mutex<BTreeMap<u32, bool>>,
) {
    let mut states = states.lock().expect("audio playback state mutex poisoned");
    for (id, playing) in states.iter_mut() {
        if !video_audio.contains_key(id) {
            *playing = mixer.is_playing(rfvp::host_api::AudioStreamId(*id));
        }
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
        HostedAudioOperation::SetMasterVolume(_) | HostedAudioOperation::Tick { .. } => None,
    }
}

fn apply_operation(
    mixer: &mut SoftAudioMixer<SymphoniaBackend>,
    operation: &HostedAudioOperation,
) -> FamilyResult<()> {
    match operation {
        HostedAudioOperation::LoadResource { .. } => Err(FamilyError::invalid(
            "ASTRA_EMU_FVP_AUDIO_RESOURCE",
            "resource audio must be resolved by the family session",
        )),
        HostedAudioOperation::LoadEncoded { id, kind, bytes } => mixer
            .load_encoded(*id, *kind, bytes)
            .map_err(crate::error::rfvp),
        HostedAudioOperation::CreateStream { id, desc } => {
            mixer.create_stream(*id, *desc).map_err(crate::error::rfvp)
        }
        HostedAudioOperation::SubmitI16 { id, samples } => {
            mixer.submit_i16(*id, samples).map_err(crate::error::rfvp)
        }
        HostedAudioOperation::SubmitF32 { id, samples } => {
            mixer.submit_f32(*id, samples).map_err(crate::error::rfvp)
        }
        HostedAudioOperation::Play {
            id,
            params,
            fade_in_ms,
        } => mixer
            .play(*id, *params, *fade_in_ms)
            .map_err(crate::error::rfvp),
        HostedAudioOperation::Stop { id, fade_ms } => {
            mixer.stop(*id, *fade_ms).map_err(crate::error::rfvp)
        }
        HostedAudioOperation::Pause(id) => mixer.pause(*id).map_err(crate::error::rfvp),
        HostedAudioOperation::Resume(id) => mixer.resume(*id).map_err(crate::error::rfvp),
        HostedAudioOperation::SetParams { id, params } => {
            mixer.set_params(*id, *params).map_err(crate::error::rfvp)
        }
        HostedAudioOperation::SetMasterVolume(volume) => {
            mixer.set_master_volume(*volume).map_err(crate::error::rfvp)
        }
        HostedAudioOperation::DestroyStream(id) => {
            mixer.destroy_stream(*id);
            Ok(())
        }
        HostedAudioOperation::Tick { .. } => Ok(()),
    }
}

fn write_pcm(sink: &AudioSinkBox, samples: Vec<i16>) -> FamilyResult<()> {
    let chunk = PcmChunk::I16(RVec::from(samples));
    chunk.validate(OUTPUT)?;
    match sink.write(chunk).into_result().map_err(|_| {
        FamilyError::invalid("ASTRA_EMU_FVP_AUDIO_WRITE", "audio sink rejected mixed PCM")
    })? {
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
mod tests {
    use super::*;

    fn wav_pcm16(samples: &[i16], channels: u16, sample_rate: u32) -> Vec<u8> {
        let data_size =
            u32::try_from(std::mem::size_of_val(samples)).expect("test WAV fits in u32");
        let block_align = channels * 2;
        let byte_rate = sample_rate * u32::from(block_align);
        let mut bytes = Vec::with_capacity(44 + data_size as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn mixer_reports_natural_eof_to_hosted_state() {
        let mut mixer = SoftAudioMixer::new(
            SymphoniaBackend,
            SoftAudioConfig {
                mix_frames: 4,
                ..SoftAudioConfig::default()
            },
        );
        let id = AudioStreamId::se(0);
        mixer
            .load_encoded(
                id,
                EncodedAudioKind::Wav,
                &wav_pcm16(&[1_000, -1_000], 2, OUTPUT.sample_rate),
            )
            .expect("test WAV loads");
        mixer
            .play(id, Default::default(), 0)
            .expect("test voice starts");
        assert!(mixer.is_playing(id));

        let mut output = vec![0_i16; 8];
        assert!(mixer.mix_next(&mut output).expect("test voice mixes"));
        assert!(!mixer.is_playing(id));
        assert!(!mixer.mix_next(&mut output).expect("idle mixer advances"));
    }

    #[test]
    fn mixer_keeps_looping_voice_playing_after_buffer_wrap() {
        let mut mixer = SoftAudioMixer::new(
            SymphoniaBackend,
            SoftAudioConfig {
                mix_frames: 4,
                ..SoftAudioConfig::default()
            },
        );
        let id = AudioStreamId::bgm(0);
        mixer
            .load_encoded(
                id,
                EncodedAudioKind::Wav,
                &wav_pcm16(&[1_000, -1_000], 2, OUTPUT.sample_rate),
            )
            .expect("test WAV loads");
        mixer
            .play(
                id,
                rfvp::host_api::AudioParams {
                    repeat: true,
                    ..Default::default()
                },
                0,
            )
            .expect("looping test voice starts");

        let mut output = vec![0_i16; 8];
        assert!(mixer.mix_next(&mut output).expect("looping voice mixes"));
        assert!(mixer.is_playing(id));
    }
}
