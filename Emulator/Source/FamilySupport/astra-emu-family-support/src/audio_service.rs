use std::{
    collections::{BTreeMap, VecDeque},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use astra_audio_kira::{AstraChunkBackendSettings, AudioServiceConfig, AudioServiceSession};
use astra_byte_source::{OwnedByteBuffer, OwnedF32Buffer, OwnedI16Buffer};
use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV8, LegacyAudioSampleFormat,
    LegacyPcmBufferV8,
};
use astra_media::{open_symphonia_audio_stream, MediaError, SymphoniaAudioStreamDecoder};
use astra_platform::{
    AudioOutputHandle, AudioOutputRequest, AudioWakeRegistration, HostKind, HostLaunchProfile,
    PlatformHostClient, PlatformHostFactory,
};
use serde::Serialize;

pub const LEGACY_AUDIO_MAX_RESOURCE_BYTES: u64 = 512 * 1024 * 1024;
const COMMAND_CAPACITY: usize = 4096;
const MAX_STREAMS: usize = 512;
const STREAM_CHUNK_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LegacyAudioTelemetry {
    pub command_count: u64,
    pub packet_count: u64,
    pub submitted_frames: u64,
    pub consumed_frames: u64,
    pub queued_frames: u64,
    pub decoder_refills: u64,
    pub underflow_count: u64,
    pub active_streams: u64,
}

#[derive(Default)]
struct TelemetryAtomics {
    command_count: AtomicU64,
    packet_count: AtomicU64,
    submitted_frames: AtomicU64,
    consumed_frames: AtomicU64,
    queued_frames: AtomicU64,
    decoder_refills: AtomicU64,
    underflow_count: AtomicU64,
    active_streams: AtomicU64,
}

impl TelemetryAtomics {
    fn snapshot(&self) -> LegacyAudioTelemetry {
        LegacyAudioTelemetry {
            command_count: self.command_count.load(Ordering::Relaxed),
            packet_count: self.packet_count.load(Ordering::Relaxed),
            submitted_frames: self.submitted_frames.load(Ordering::Relaxed),
            consumed_frames: self.consumed_frames.load(Ordering::Relaxed),
            queued_frames: self.queued_frames.load(Ordering::Relaxed),
            decoder_refills: self.decoder_refills.load(Ordering::Relaxed),
            underflow_count: self.underflow_count.load(Ordering::Relaxed),
            active_streams: self.active_streams.load(Ordering::Relaxed),
        }
    }
}

enum WorkerCommand {
    Wake,
    FixedTick(SyncSender<Result<(), String>>),
    Execute {
        command: LegacyAudioCommandV1,
        resource: Option<OwnedByteBuffer>,
    },
    ExecuteLive(LegacyAudioPacketV8),
    BeginMovie {
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    },
    AppendMovie {
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    },
    StopMovie(u32),
    Suspend(bool),
    Reset(SyncSender<Result<(), String>>),
    Shutdown(SyncSender<Result<Vec<u8>, String>>),
}

impl WorkerCommand {
    fn diagnostic_context(&self) -> (&'static str, Option<u32>) {
        match self {
            Self::Wake => ("wake", None),
            Self::FixedTick(_) => ("fixed_tick", None),
            Self::Execute { command, .. } => match command {
                LegacyAudioCommandV1::LoadResource { stream_id, .. } => {
                    ("load_resource", Some(*stream_id))
                }
                LegacyAudioCommandV1::CreateStream { stream_id, .. } => {
                    ("create_stream", Some(*stream_id))
                }
                LegacyAudioCommandV1::SubmitI16 { stream_id, .. } => {
                    ("submit_i16", Some(*stream_id))
                }
                LegacyAudioCommandV1::SubmitF32 { stream_id, .. } => {
                    ("submit_f32", Some(*stream_id))
                }
                LegacyAudioCommandV1::Play { stream_id, .. } => ("play", Some(*stream_id)),
                LegacyAudioCommandV1::Stop { stream_id, .. } => ("stop", Some(*stream_id)),
                LegacyAudioCommandV1::Pause { stream_id } => ("pause", Some(*stream_id)),
                LegacyAudioCommandV1::Resume { stream_id } => ("resume", Some(*stream_id)),
                LegacyAudioCommandV1::SetParams { stream_id, .. } => {
                    ("set_params", Some(*stream_id))
                }
                LegacyAudioCommandV1::DestroyStream { stream_id } => {
                    ("destroy_stream", Some(*stream_id))
                }
                LegacyAudioCommandV1::MasterVolume { .. } => ("master_volume", None),
            },
            Self::ExecuteLive(packet) => ("submit_live_pcm", Some(packet.stream_id)),
            Self::BeginMovie { stream_id, .. } => ("begin_movie", Some(*stream_id)),
            Self::AppendMovie { stream_id, .. } => ("append_movie", Some(*stream_id)),
            Self::StopMovie(stream_id) => ("stop_movie", Some(*stream_id)),
            Self::Suspend(_) => ("suspend", None),
            Self::Reset(_) => ("reset", None),
            Self::Shutdown(_) => ("shutdown", None),
        }
    }
}

pub struct FamilyAudioService {
    commands: SyncSender<WorkerCommand>,
    client: PlatformHostClient,
    telemetry: Arc<TelemetryAtomics>,
    failure: Arc<Mutex<Option<String>>>,
    audible: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    wake_stop: Arc<AtomicBool>,
    wake: AudioWakeRegistration,
    wake_forwarder: Option<JoinHandle<()>>,
    deterministic: bool,
}

impl FamilyAudioService {
    pub fn open() -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "ASTRA_EMU_AUDIO_RUNTIME_CREATE".to_owned())?;
        let mut profile = native_audio_profile()?;
        profile.id = "astra-emu-manager-audio".into();
        let host = runtime
            .block_on(native_audio_factory().start(HostLaunchProfile::platform(profile)))
            .map_err(|error| error.to_string())?;
        Self::start_with_client(host.client, true)
    }

    pub fn start_with_client(
        client: PlatformHostClient,
        shutdown_host: bool,
    ) -> Result<Self, String> {
        let deterministic = client.launch_profile().kind() == HostKind::Headless;
        let (commands, receiver) = sync_channel(COMMAND_CAPACITY);
        let telemetry = Arc::new(TelemetryAtomics::default());
        let failure = Arc::new(Mutex::new(None));
        let audible = Arc::new(AtomicBool::new(false));
        let worker_telemetry = Arc::clone(&telemetry);
        let worker_failure = Arc::clone(&failure);
        let worker_audible = Arc::clone(&audible);
        let worker_client = client.clone();
        let worker = thread::Builder::new()
            .name("astra-emu-kira-audio".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_worker(
                        worker_client,
                        shutdown_host,
                        receiver,
                        worker_telemetry,
                        worker_audible,
                    )
                }));
                let error = match result {
                    Ok(Ok(())) => return,
                    Ok(Err(error)) => error,
                    Err(_) => "ASTRA_EMU_AUDIO_WORKER_PANIC".into(),
                };
                tracing::error!(
                    event = "astra_emu_audio_worker_failed",
                    diagnostic_code = error,
                    "AstraEMU audio worker terminated"
                );
                if let Ok(mut failure) = worker_failure.lock() {
                    *failure = Some(error);
                }
            })
            .map_err(|_| "ASTRA_EMU_AUDIO_WORKER_START".to_owned())?;
        let wake_stop = Arc::new(AtomicBool::new(false));
        let wake = client.audio_wake();
        let forwarder_stop = Arc::clone(&wake_stop);
        let forwarder_commands = commands.clone();
        let forwarder_wake = wake.clone();
        let wake_forwarder = thread::Builder::new()
            .name("astra-emu-audio-wake".into())
            .spawn(move || forward_audio_wakes(forwarder_wake, forwarder_stop, forwarder_commands))
            .map_err(|_| "ASTRA_EMU_AUDIO_WAKE_START".to_owned())?;
        Ok(Self {
            commands,
            client,
            telemetry,
            failure,
            audible,
            worker: Some(worker),
            wake_stop,
            wake,
            wake_forwarder: Some(wake_forwarder),
            deterministic,
        })
    }

    pub fn platform_client(&self) -> PlatformHostClient {
        self.client.clone()
    }

    pub fn execute(
        &self,
        command: LegacyAudioCommandV1,
        resource: Option<OwnedByteBuffer>,
    ) -> Result<(), String> {
        command.validate().map_err(|error| error.to_string())?;
        self.try_send(WorkerCommand::Execute { command, resource })
    }

    pub fn execute_live_pcm(&self, packet: LegacyAudioPacketV8) -> Result<(), String> {
        packet.validate().map_err(|error| error.to_string())?;
        self.try_send(WorkerCommand::ExecuteLive(packet))
    }

    pub fn begin_movie_stream(
        &self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        validate_segment(sample_rate, channels, &samples)?;
        self.try_send(WorkerCommand::BeginMovie {
            stream_id,
            sample_rate,
            channels,
            samples,
        })
    }

    pub fn append_movie_stream(
        &self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        validate_segment(sample_rate, channels, &samples)?;
        self.try_send(WorkerCommand::AppendMovie {
            stream_id,
            sample_rate,
            channels,
            samples,
        })
    }

    pub fn stop_movie_pcm(&self, stream_id: u32) -> Result<(), String> {
        self.try_send(WorkerCommand::StopMovie(stream_id))
    }

    pub fn set_suspended(&self, suspended: bool) -> Result<(), String> {
        self.try_send(WorkerCommand::Suspend(suspended))
    }

    pub fn pump(&self) -> Result<(), String> {
        self.check_failure()?;
        if self.deterministic {
            self.request(WorkerCommand::FixedTick)
                .and_then(|result| result)?;
        }
        Ok(())
    }

    pub fn telemetry(&self) -> LegacyAudioTelemetry {
        self.telemetry.snapshot()
    }

    pub fn has_audible_output(&self) -> bool {
        self.audible.load(Ordering::Relaxed)
    }

    pub fn reset(&self) -> Result<(), String> {
        self.request(WorkerCommand::Reset).and_then(|result| result)
    }

    pub fn shutdown(mut self) -> Result<Vec<u8>, String> {
        self.wake_stop.store(true, Ordering::Release);
        self.wake.notify();
        let result = self
            .request(WorkerCommand::Shutdown)
            .and_then(|result| result);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "ASTRA_EMU_AUDIO_WORKER_JOIN".to_owned())?;
        }
        if let Some(wake) = self.wake_forwarder.take() {
            wake.join()
                .map_err(|_| "ASTRA_EMU_AUDIO_WAKE_JOIN".to_owned())?;
        }
        match result {
            Err(error) if error == "ASTRA_EMU_AUDIO_WORKER_REPLY_CLOSED" => {
                self.check_failure().and(Err(error))
            }
            result => result,
        }
    }

    fn request<T>(
        &self,
        make: impl FnOnce(SyncSender<Result<T, String>>) -> WorkerCommand,
    ) -> Result<Result<T, String>, String> {
        self.check_failure()?;
        let (reply, response) = sync_channel(1);
        self.commands
            .send(make(reply))
            .map_err(|_| "ASTRA_EMU_AUDIO_COMMAND_CHANNEL_CLOSED".to_owned())?;
        response
            .recv()
            .map_err(|_| "ASTRA_EMU_AUDIO_WORKER_REPLY_CLOSED".to_owned())
    }

    fn try_send(&self, command: WorkerCommand) -> Result<(), String> {
        self.check_failure()?;
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => "ASTRA_EMU_AUDIO_COMMAND_QUEUE_OVERFLOW".into(),
                TrySendError::Disconnected(_) => "ASTRA_EMU_AUDIO_COMMAND_CHANNEL_CLOSED".into(),
            })
    }

    fn check_failure(&self) -> Result<(), String> {
        self.failure
            .lock()
            .map_err(|_| "ASTRA_EMU_AUDIO_FAILURE_LOCK_POISONED".to_owned())?
            .clone()
            .map_or(Ok(()), Err)
    }
}

impl Drop for FamilyAudioService {
    fn drop(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let (reply, response) = sync_channel(1);
        if self
            .commands
            .try_send(WorkerCommand::Shutdown(reply))
            .is_ok()
            && response.recv_timeout(Duration::from_secs(2)).is_ok()
        {
            let _ = worker.join();
        } else {
            tracing::error!(event = "astra_emu_audio_service_forced_detach");
        }
        self.wake_stop.store(true, Ordering::Release);
        self.wake.notify();
        if let Some(wake) = self.wake_forwarder.take() {
            let _ = wake.join();
        }
    }
}

fn forward_audio_wakes(
    wake: AudioWakeRegistration,
    stop: Arc<AtomicBool>,
    commands: SyncSender<WorkerCommand>,
) {
    let mut observed = 0;
    while !stop.load(Ordering::Acquire) {
        observed = wake.wait(observed);
        if !stop.load(Ordering::Acquire) {
            let _ = commands.try_send(WorkerCommand::Wake);
        }
    }
}

enum AudioSegment {
    I16(OwnedI16Buffer),
    F32(OwnedF32Buffer),
}

struct AudioStream {
    source_rate: u32,
    source_channels: u16,
    sample_format: LegacyAudioSampleFormat,
    segments: VecDeque<AudioSegment>,
    segment_cursor: usize,
    decoder: Option<SymphoniaAudioStreamDecoder>,
    decoder_source: Option<(String, OwnedByteBuffer)>,
    decoder_eof: bool,
    playing: bool,
    paused: bool,
    repeat: bool,
    volume: f32,
    pan: f32,
    stop_after_frames: usize,
    source_buffer: Vec<f32>,
    mix_buffer: Vec<f32>,
    kira_created: bool,
    finish_submitted: bool,
}

impl AudioStream {
    fn new(sample_rate: u32, channels: u16, sample_format: LegacyAudioSampleFormat) -> Self {
        Self {
            source_rate: sample_rate,
            source_channels: channels,
            sample_format,
            segments: VecDeque::new(),
            segment_cursor: 0,
            decoder: None,
            decoder_source: None,
            decoder_eof: false,
            playing: false,
            paused: false,
            repeat: false,
            volume: 1.0,
            pan: 0.0,
            stop_after_frames: 0,
            source_buffer: Vec::new(),
            mix_buffer: Vec::new(),
            kira_created: false,
            finish_submitted: false,
        }
    }
}

fn update_stream_params(
    streams: &mut BTreeMap<u32, AudioStream>,
    stream_id: u32,
    volume: f32,
    pan: f32,
    repeat: bool,
) -> Result<bool, String> {
    let stream = streams
        .get_mut(&stream_id)
        .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
    if repeat && stream.decoder_source.is_none() {
        return Err("ASTRA_EMU_AUDIO_UNREWINDABLE_REPEAT".into());
    }
    stream.volume = volume;
    stream.pan = pan;
    stream.repeat = repeat;
    Ok(stream.kira_created)
}

fn prepare_fade_stop(
    streams: &mut BTreeMap<u32, AudioStream>,
    stream_id: u32,
    frames: usize,
) -> Result<Option<f32>, String> {
    let stream = streams
        .get_mut(&stream_id)
        .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
    if !stream.kira_created {
        stream.playing = false;
        stream.stop_after_frames = 0;
        return Ok(None);
    }
    stream.stop_after_frames = frames;
    Ok(Some(stream.pan))
}

struct WorkerState {
    client: PlatformHostClient,
    output: AudioOutputHandle,
    service: Option<AudioServiceSession>,
    streams: BTreeMap<u32, AudioStream>,
    master_volume: f32,
    suspended: bool,
    chunk_frames: usize,
    output_rate: u32,
    output_channels: u16,
    telemetry: Arc<TelemetryAtomics>,
    audible: Arc<AtomicBool>,
}

fn run_worker(
    client: PlatformHostClient,
    shutdown_host: bool,
    receiver: Receiver<WorkerCommand>,
    telemetry: Arc<TelemetryAtomics>,
    audible: Arc<AtomicBool>,
) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "ASTRA_EMU_AUDIO_RUNTIME_CREATE".to_owned())?;
    let mut state = runtime.block_on(WorkerState::open(client, telemetry, audible))?;
    loop {
        match receiver.recv() {
            Ok(WorkerCommand::Shutdown(reply)) => {
                let result = runtime.block_on(state.shutdown(shutdown_host));
                let terminal = result.as_ref().map(|_| ()).map_err(Clone::clone);
                let _ = reply.send(result);
                terminal?;
                return Ok(());
            }
            Ok(WorkerCommand::Reset(reply)) => {
                let _ = reply.send(state.reset());
            }
            Ok(WorkerCommand::FixedTick(reply)) => {
                let result = state.poll_fixed_tick();
                let terminal = result.as_ref().map(|_| ()).map_err(Clone::clone);
                let _ = reply.send(result);
                terminal?;
            }
            Ok(WorkerCommand::Wake) => {}
            Ok(command) => {
                let (operation, stream_id) = command.diagnostic_context();
                if let Err(error) = state.execute(command) {
                    tracing::error!(
                        event = "astra_emu_audio_command_failed",
                        diagnostic_code = error,
                        operation,
                        stream_id,
                        "AstraEMU audio command failed"
                    );
                    return Err(error);
                }
            }
            Err(_) => return Err("ASTRA_EMU_AUDIO_COMMAND_CHANNEL_CLOSED".into()),
        }
        if !state.suspended {
            if let Err(error) = state.refill() {
                tracing::error!(
                    event = "astra_emu_audio_refill_failed",
                    diagnostic_code = error,
                    "AstraEMU audio refill failed"
                );
                return Err(error);
            }
        }
        state.refresh_telemetry()?;
    }
}

impl WorkerState {
    async fn open(
        client: PlatformHostClient,
        telemetry: Arc<TelemetryAtomics>,
        audible: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let deterministic = client.launch_profile().kind() == HostKind::Headless;
        let limits = client.launch_profile().limits();
        let chunk_frames = limits.audio_chunk_frames;
        let opened = client
            .open_audio_output(AudioOutputRequest {
                sample_rate: 48_000,
                channels: 2,
                chunk_frames,
                max_buffered_frames: chunk_frames.saturating_mul(STREAM_CHUNK_CAPACITY),
                start_paused: false,
                capture_samples: false,
            })
            .await
            .map_err(|error| error.to_string())?;
        if opened.format.sample_rate == 0 || !matches!(opened.format.channels, 1 | 2) {
            return Err("ASTRA_EMU_AUDIO_DEVICE_FORMAT".into());
        }
        let format = opened.format;
        let output = opened.handle;
        let service = AudioServiceSession::new(
            AudioServiceConfig {
                max_voices: 8,
                max_buses: MAX_STREAMS,
                max_events: 1024,
                pcm_cache_bytes: limits.audio_pcm_cache_bytes,
            },
            AstraChunkBackendSettings {
                sample_rate: opened.format.sample_rate,
                channels: opened.format.channels,
                chunk_frames,
                endpoint: opened.lane,
                deterministic_fixed_tick_hz: deterministic.then_some(60),
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            output,
            service: Some(service),
            streams: BTreeMap::new(),
            master_volume: 1.0,
            suspended: false,
            chunk_frames,
            output_rate: format.sample_rate,
            output_channels: format.channels,
            telemetry,
            audible,
        })
    }

    fn execute(&mut self, command: WorkerCommand) -> Result<(), String> {
        self.telemetry.command_count.fetch_add(1, Ordering::Relaxed);
        match command {
            WorkerCommand::Execute { command, resource } => self.execute_legacy(command, resource),
            WorkerCommand::ExecuteLive(packet) => self.execute_live(packet),
            WorkerCommand::BeginMovie {
                stream_id,
                sample_rate,
                channels,
                samples,
            } => {
                self.replace_stream(
                    stream_id,
                    AudioStream::new(sample_rate, channels, LegacyAudioSampleFormat::F32),
                )?;
                self.append_f32(stream_id, sample_rate, channels, samples)?;
                self.play(stream_id, 1.0, 0.0, false, 0)
            }
            WorkerCommand::AppendMovie {
                stream_id,
                sample_rate,
                channels,
                samples,
            } => self.append_f32(stream_id, sample_rate, channels, samples),
            WorkerCommand::StopMovie(stream_id) => self.remove_stream(stream_id),
            WorkerCommand::Suspend(value) => self.set_suspended(value),
            WorkerCommand::Wake
            | WorkerCommand::FixedTick(_)
            | WorkerCommand::Reset(_)
            | WorkerCommand::Shutdown(_) => Ok(()),
        }
    }

    fn execute_legacy(
        &mut self,
        command: LegacyAudioCommandV1,
        resource: Option<OwnedByteBuffer>,
    ) -> Result<(), String> {
        match command {
            LegacyAudioCommandV1::LoadResource {
                stream_id,
                encoding,
                resource_uri,
            } => {
                let encoded =
                    resource.ok_or_else(|| "ASTRA_EMU_AUDIO_RESOURCE_MISSING".to_owned())?;
                let codec = resolve_codec(encoding, &resource_uri, &encoded)?;
                let decoder = open_symphonia_audio_stream(
                    &codec,
                    encoded.clone(),
                    LEGACY_AUDIO_MAX_RESOURCE_BYTES,
                )
                .map_err(redacted_media_error)?;
                let mut stream = AudioStream::new(
                    decoder.sample_rate(),
                    decoder.channels(),
                    LegacyAudioSampleFormat::I16,
                );
                stream.decoder = Some(decoder);
                stream.decoder_source = Some((codec, encoded));
                self.replace_stream(stream_id, stream)
            }
            LegacyAudioCommandV1::CreateStream {
                stream_id,
                sample_rate,
                channels,
                sample_format,
            } => self.replace_stream(
                stream_id,
                AudioStream::new(sample_rate, channels, sample_format),
            ),
            LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => self.push_segment(
                stream_id,
                LegacyAudioSampleFormat::I16,
                AudioSegment::I16(samples),
            ),
            LegacyAudioCommandV1::SubmitF32 { stream_id, samples } => self.push_segment(
                stream_id,
                LegacyAudioSampleFormat::F32,
                AudioSegment::F32(samples),
            ),
            LegacyAudioCommandV1::Play {
                stream_id,
                volume,
                pan,
                repeat,
                fade_in_ms,
            } => self.play(stream_id, volume, pan, repeat, fade_in_ms),
            LegacyAudioCommandV1::Stop { stream_id, fade_ms } => self.stop(stream_id, fade_ms),
            LegacyAudioCommandV1::Pause { stream_id } => self.pause(stream_id),
            LegacyAudioCommandV1::Resume { stream_id } => self.resume(stream_id),
            LegacyAudioCommandV1::SetParams {
                stream_id,
                volume,
                pan,
                repeat,
            } => {
                let kira_created =
                    update_stream_params(&mut self.streams, stream_id, volume, pan, repeat)?;
                if kira_created {
                    self.set_kira_mix(stream_id, 0)?;
                }
                Ok(())
            }
            LegacyAudioCommandV1::DestroyStream { stream_id } => self.remove_stream(stream_id),
            LegacyAudioCommandV1::MasterVolume { volume } => {
                self.master_volume = volume;
                let ids = self
                    .streams
                    .iter()
                    .filter(|(_, stream)| stream.kira_created)
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>();
                for id in ids {
                    self.set_kira_mix(id, 0)?;
                }
                Ok(())
            }
        }
    }

    fn execute_live(&mut self, packet: LegacyAudioPacketV8) -> Result<(), String> {
        let stream = self
            .streams
            .get(&packet.stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if (packet.sample_rate != 0 && packet.sample_rate != stream.source_rate)
            || (packet.channels != 0 && packet.channels != stream.source_channels)
        {
            return Err("ASTRA_EMU_AUDIO_SAMPLE_FORMAT_MISMATCH".into());
        }
        match packet.pcm {
            LegacyPcmBufferV8::I16(samples) => self.push_segment(
                packet.stream_id,
                LegacyAudioSampleFormat::I16,
                AudioSegment::I16(samples),
            ),
            LegacyPcmBufferV8::F32(samples) => self.push_segment(
                packet.stream_id,
                LegacyAudioSampleFormat::F32,
                AudioSegment::F32(samples),
            ),
        }
    }

    fn push_segment(
        &mut self,
        stream_id: u32,
        format: LegacyAudioSampleFormat,
        segment: AudioSegment,
    ) -> Result<(), String> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if stream.sample_format != format {
            return Err("ASTRA_EMU_AUDIO_SAMPLE_FORMAT_MISMATCH".into());
        }
        stream.segments.push_back(segment);
        Ok(())
    }

    fn append_f32(
        &mut self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if stream.source_rate != sample_rate || stream.source_channels != channels {
            return Err("ASTRA_EMU_AUDIO_SAMPLE_FORMAT_MISMATCH".into());
        }
        self.push_segment(
            stream_id,
            LegacyAudioSampleFormat::F32,
            AudioSegment::F32(samples.into()),
        )
    }

    fn play(
        &mut self,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    ) -> Result<(), String> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if stream.playing || (repeat && stream.decoder_source.is_none()) {
            return Err("ASTRA_EMU_AUDIO_PLAY_STATE".into());
        }
        self.service
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
            .create_stream(
                kira_stream_id(stream_id),
                &format!("legacy.{stream_id}"),
                self.chunk_frames,
                STREAM_CHUNK_CAPACITY,
            )
            .map_err(|error| error.to_string())?;
        let fade_frames = frames_for_ms(self.output_rate, fade_in_ms)?;
        stream.playing = true;
        stream.paused = false;
        stream.repeat = repeat;
        stream.volume = volume;
        stream.pan = pan;
        stream.kira_created = true;
        stream.finish_submitted = false;
        stream.stop_after_frames = 0;
        if fade_frames != 0 {
            self.service
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                .set_stream_mix(kira_stream_id(stream_id), 0.0, pan, 0)
                .map_err(|error| error.to_string())?;
        }
        self.set_kira_mix(stream_id, fade_frames as u64)
    }

    fn stop(&mut self, stream_id: u32, fade_ms: u32) -> Result<(), String> {
        if fade_ms > 0 {
            let frames = frames_for_ms(self.output_rate, fade_ms)?.max(1);
            let Some(pan) = prepare_fade_stop(&mut self.streams, stream_id, frames)? else {
                return Ok(());
            };
            self.service
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                .set_stream_mix(kira_stream_id(stream_id), 0.0, pan, frames as u64)
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        self.destroy_kira_stream(stream_id)?;
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.playing = false;
        }
        Ok(())
    }

    fn pause(&mut self, stream_id: u32) -> Result<(), String> {
        self.service
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
            .pause_stream(kira_stream_id(stream_id))
            .map_err(|error| error.to_string())?;
        self.streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?
            .paused = true;
        Ok(())
    }

    fn resume(&mut self, stream_id: u32) -> Result<(), String> {
        self.service
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
            .resume_stream(kira_stream_id(stream_id))
            .map_err(|error| error.to_string())?;
        self.streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?
            .paused = false;
        Ok(())
    }

    fn set_suspended(&mut self, suspended: bool) -> Result<(), String> {
        self.suspended = suspended;
        let ids = self
            .streams
            .iter()
            .filter(|(_, stream)| stream.playing)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in ids {
            if suspended {
                self.pause(id)?;
            } else {
                self.resume(id)?;
            }
        }
        Ok(())
    }

    fn set_kira_mix(&mut self, stream_id: u32, duration_frames: u64) -> Result<(), String> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        self.service
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
            .set_stream_mix(
                kira_stream_id(stream_id),
                stream.volume * self.master_volume,
                stream.pan,
                duration_frames,
            )
            .map_err(|error| error.to_string())
    }

    fn replace_stream(&mut self, stream_id: u32, stream: AudioStream) -> Result<(), String> {
        if !self.streams.contains_key(&stream_id) && self.streams.len() >= MAX_STREAMS {
            return Err("ASTRA_EMU_AUDIO_STREAM_LIMIT".into());
        }
        if self.streams.contains_key(&stream_id) {
            self.remove_stream(stream_id)?;
        }
        self.streams.insert(stream_id, stream);
        Ok(())
    }

    fn remove_stream(&mut self, stream_id: u32) -> Result<(), String> {
        self.destroy_kira_stream(stream_id)?;
        self.streams
            .remove(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        Ok(())
    }

    fn destroy_kira_stream(&mut self, stream_id: u32) -> Result<(), String> {
        if self
            .streams
            .get(&stream_id)
            .is_some_and(|stream| stream.kira_created)
        {
            self.service
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                .destroy_stream(kira_stream_id(stream_id))
                .map_err(|error| error.to_string())?;
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                stream.kira_created = false;
            }
        }
        Ok(())
    }

    fn refill(&mut self) -> Result<(), String> {
        let ids = self
            .streams
            .iter()
            .filter(|(_, stream)| stream.playing && !stream.paused)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in ids {
            while self
                .service
                .as_ref()
                .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                .stream_has_capacity(kira_stream_id(id))
                .map_err(|error| error.to_string())?
            {
                if !self.refill_one(id)? {
                    break;
                }
            }
            if self
                .streams
                .get(&id)
                .is_some_and(|stream| stream.finish_submitted)
                && self
                    .service
                    .as_ref()
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                    .stream_is_completed(kira_stream_id(id))
                    .map_err(|error| error.to_string())?
            {
                self.destroy_kira_stream(id)?;
                if let Some(stream) = self.streams.get_mut(&id) {
                    stream.playing = false;
                }
            }
        }
        Ok(())
    }

    fn poll_fixed_tick(&mut self) -> Result<(), String> {
        self.service
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
            .poll_fixed_tick()
            .map_err(|error| error.to_string())
    }

    fn refill_one(&mut self, stream_id: u32) -> Result<bool, String> {
        if self
            .streams
            .get(&stream_id)
            .is_some_and(|stream| stream.finish_submitted)
        {
            return Ok(false);
        }
        let source_frames = {
            let stream = self
                .streams
                .get(&stream_id)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
            self.chunk_frames
                .checked_mul(stream.source_rate as usize)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_REFILL_BOUNDS".to_owned())?
                .div_ceil(self.output_rate as usize)
        };
        self.ensure_decoded(stream_id, source_frames)?;
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        let source_needed = source_frames
            .checked_mul(usize::from(stream.source_channels))
            .ok_or_else(|| "ASTRA_EMU_AUDIO_REFILL_BOUNDS".to_owned())?;
        let can_move_source_allocation = stream.source_rate == self.output_rate
            && stream.source_channels == self.output_channels;
        if can_move_source_allocation {
            if let Some(buffer) =
                take_owned_f32_segment(&mut stream.segments, &mut stream.segment_cursor)
            {
                let submitted_frames = buffer.len() / usize::from(self.output_channels);
                let exhausted = stream.decoder_eof
                    && stream.decoder.is_none()
                    && queued_samples(&stream.segments, stream.segment_cursor) == 0;
                let audible = buffer.iter().any(|sample| sample.abs() > 0.000_03);
                self.service
                    .as_mut()
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                    .submit_stream_owned(kira_stream_id(stream_id), buffer)
                    .map_err(|error| error.to_string())?;
                self.telemetry.packet_count.fetch_add(1, Ordering::Relaxed);
                if audible {
                    self.audible.store(true, Ordering::Relaxed);
                }
                let was_stopping = stream.stop_after_frames != 0;
                stream.stop_after_frames =
                    stream.stop_after_frames.saturating_sub(submitted_frames);
                let fade_stop_completed = was_stopping && stream.stop_after_frames == 0;
                if exhausted || fade_stop_completed {
                    self.service
                        .as_mut()
                        .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                        .finish_stream(kira_stream_id(stream_id))
                        .map_err(|error| error.to_string())?;
                    stream.finish_submitted = true;
                }
                return Ok(true);
            }
        }
        if !self
            .service
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
            .stream_has_recyclable_capacity(kira_stream_id(stream_id))
            .map_err(|error| error.to_string())?
        {
            return Ok(false);
        }
        let exhausted = stream.decoder_eof
            && stream.decoder.is_none()
            && queued_samples(&stream.segments, stream.segment_cursor) <= source_needed;
        if can_move_source_allocation {
            stream.mix_buffer.clear();
            if stream.mix_buffer.capacity() < source_needed {
                stream
                    .mix_buffer
                    .reserve(source_needed - stream.mix_buffer.capacity());
            }
            take_segmented(
                &mut stream.segments,
                &mut stream.segment_cursor,
                source_needed,
                &mut stream.mix_buffer,
            );
        } else {
            stream.source_buffer.clear();
            if stream.source_buffer.capacity() < source_needed {
                stream
                    .source_buffer
                    .reserve(source_needed - stream.source_buffer.capacity());
            }
            take_segmented(
                &mut stream.segments,
                &mut stream.segment_cursor,
                source_needed,
                &mut stream.source_buffer,
            );
            resample_chunk_into(
                &stream.source_buffer,
                stream.source_rate,
                stream.source_channels,
                self.output_rate,
                self.output_channels,
                &mut stream.mix_buffer,
            )?;
        }
        if stream.mix_buffer.is_empty() {
            if exhausted && !stream.finish_submitted {
                self.service
                    .as_mut()
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                    .finish_stream(kira_stream_id(stream_id))
                    .map_err(|error| error.to_string())?;
                stream.finish_submitted = true;
            }
            return Ok(false);
        }
        let target_samples = self
            .chunk_frames
            .checked_mul(usize::from(self.output_channels))
            .ok_or_else(|| "ASTRA_EMU_AUDIO_REFILL_BOUNDS".to_owned())?;
        stream.mix_buffer.resize(target_samples, 0.0);
        let was_stopping = stream.stop_after_frames != 0;
        stream.stop_after_frames = stream.stop_after_frames.saturating_sub(self.chunk_frames);
        let fade_stop_completed = was_stopping && stream.stop_after_frames == 0;
        let audible = stream
            .mix_buffer
            .iter()
            .any(|sample| sample.abs() > 0.000_03);
        let buffer = std::mem::take(&mut stream.mix_buffer);
        stream.mix_buffer = self
            .service
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
            .submit_stream_recyclable(kira_stream_id(stream_id), buffer)
            .map_err(|error| error.to_string())?;
        self.telemetry.packet_count.fetch_add(1, Ordering::Relaxed);
        if audible {
            self.audible.store(true, Ordering::Relaxed);
        }
        if exhausted || fade_stop_completed {
            self.service
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?
                .finish_stream(kira_stream_id(stream_id))
                .map_err(|error| error.to_string())?;
            stream.finish_submitted = true;
        }
        Ok(true)
    }

    fn ensure_decoded(&mut self, stream_id: u32, frames: usize) -> Result<(), String> {
        loop {
            let need = {
                let stream = self
                    .streams
                    .get(&stream_id)
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
                queued_samples(&stream.segments, stream.segment_cursor)
                    < frames.saturating_mul(usize::from(stream.source_channels))
                    && stream.decoder.is_some()
            };
            if !need {
                return Ok(());
            }
            let stream = self.streams.get_mut(&stream_id).expect("stream exists");
            match stream
                .decoder
                .as_mut()
                .expect("decoder exists")
                .next_chunk()
                .map_err(redacted_media_error)?
            {
                Some(chunk) => {
                    if chunk.sample_rate != stream.source_rate
                        || chunk.channels != stream.source_channels
                    {
                        return Err("ASTRA_EMU_AUDIO_STREAM_FORMAT_CHANGE".into());
                    }
                    stream
                        .segments
                        .push_back(AudioSegment::I16(chunk.samples.into()));
                    self.telemetry
                        .decoder_refills
                        .fetch_add(1, Ordering::Relaxed);
                }
                None if stream.repeat => {
                    let (codec, source) = stream
                        .decoder_source
                        .as_ref()
                        .ok_or_else(|| "ASTRA_EMU_AUDIO_REPEAT_SOURCE_MISSING".to_owned())?;
                    stream.decoder = Some(
                        open_symphonia_audio_stream(
                            codec,
                            source.clone(),
                            LEGACY_AUDIO_MAX_RESOURCE_BYTES,
                        )
                        .map_err(redacted_media_error)?,
                    );
                }
                None => {
                    stream.decoder = None;
                    stream.decoder_eof = true;
                    return Ok(());
                }
            }
        }
    }

    fn refresh_telemetry(&mut self) -> Result<(), String> {
        let service = self
            .service
            .as_mut()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SERVICE_CLOSED".to_owned())?;
        service.poll_backend().map_err(|error| error.to_string())?;
        let values = service.telemetry();
        let channels = u64::from(self.output_channels);
        let submitted = values.submitted_samples / channels;
        let consumed = values.consumed_samples / channels;
        self.telemetry
            .submitted_frames
            .store(submitted, Ordering::Relaxed);
        self.telemetry
            .consumed_frames
            .store(consumed, Ordering::Relaxed);
        self.telemetry
            .queued_frames
            .store(submitted.saturating_sub(consumed), Ordering::Relaxed);
        self.telemetry.underflow_count.store(
            values
                .underflow_count
                .saturating_add(service.stream_underflow_count()),
            Ordering::Relaxed,
        );
        self.telemetry.active_streams.store(
            self.streams
                .values()
                .filter(|stream| stream.playing)
                .count() as u64,
            Ordering::Relaxed,
        );
        Ok(())
    }

    fn reset(&mut self) -> Result<(), String> {
        let ids = self.streams.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.remove_stream(id)?;
        }
        Ok(())
    }

    async fn shutdown(&mut self, shutdown_host: bool) -> Result<Vec<u8>, String> {
        self.refresh_telemetry()?;
        let mut trace = serde_json::to_vec(&self.telemetry.snapshot())
            .map_err(|_| "ASTRA_EMU_AUDIO_TELEMETRY_ENCODE".to_owned())?;
        trace.push(b'\n');
        self.reset()?;
        drop(self.service.take());
        self.client
            .close_audio(self.output)
            .await
            .map_err(|error| error.to_string())?;
        if shutdown_host {
            self.client
                .shutdown()
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(trace)
    }
}

fn kira_stream_id(stream_id: u32) -> u64 {
    u64::from(stream_id) + 1
}

fn validate_segment(sample_rate: u32, channels: u16, samples: &[f32]) -> Result<(), String> {
    if sample_rate == 0
        || channels == 0
        || samples.is_empty()
        || !samples.len().is_multiple_of(usize::from(channels))
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err("ASTRA_EMU_AUDIO_SEGMENT_INVALID".into());
    }
    Ok(())
}

fn queued_samples(segments: &VecDeque<AudioSegment>, cursor: usize) -> usize {
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            let len = match segment {
                AudioSegment::I16(samples) => samples.len(),
                AudioSegment::F32(samples) => samples.len(),
            };
            if index == 0 {
                len.saturating_sub(cursor)
            } else {
                len
            }
        })
        .sum()
}

fn take_owned_f32_segment(
    segments: &mut VecDeque<AudioSegment>,
    cursor: &mut usize,
) -> Option<OwnedF32Buffer> {
    if *cursor != 0 {
        return None;
    }
    match segments.front() {
        Some(AudioSegment::F32(_)) => {}
        _ => return None,
    }
    match segments.pop_front() {
        Some(AudioSegment::F32(samples)) => Some(samples),
        _ => unreachable!("front segment was validated as f32"),
    }
}

fn take_segmented(
    segments: &mut VecDeque<AudioSegment>,
    cursor: &mut usize,
    count: usize,
    output: &mut Vec<f32>,
) {
    while output.len() < count {
        let Some(front) = segments.front() else {
            break;
        };
        let len = match front {
            AudioSegment::I16(samples) => samples.len(),
            AudioSegment::F32(samples) => samples.len(),
        };
        let take = (count - output.len()).min(len - *cursor);
        match front {
            AudioSegment::I16(samples) => output.extend(
                samples[*cursor..*cursor + take]
                    .iter()
                    .map(|sample| f32::from(*sample) / 32768.0),
            ),
            AudioSegment::F32(samples) => {
                output.extend_from_slice(&samples[*cursor..*cursor + take])
            }
        }
        *cursor += take;
        if *cursor == len {
            segments.pop_front();
            *cursor = 0;
        }
    }
}

fn frames_for_ms(rate: u32, milliseconds: u32) -> Result<usize, String> {
    usize::try_from(
        u64::from(rate)
            .checked_mul(u64::from(milliseconds))
            .ok_or_else(|| "ASTRA_EMU_AUDIO_FRAME_OVERFLOW".to_owned())?
            .div_ceil(1000),
    )
    .map_err(|_| "ASTRA_EMU_AUDIO_FRAME_OVERFLOW".into())
}

fn resample_chunk_into(
    source: &[f32],
    source_rate: u32,
    source_channels: u16,
    output_rate: u32,
    output_channels: u16,
    output: &mut Vec<f32>,
) -> Result<(), String> {
    if source_rate == 0
        || output_rate == 0
        || source_channels == 0
        || output_channels == 0
        || !source.len().is_multiple_of(usize::from(source_channels))
    {
        return Err("ASTRA_EMU_AUDIO_RESAMPLE_FORMAT".into());
    }
    let source_frames = source.len() / usize::from(source_channels);
    let output_frames = source_frames
        .checked_mul(output_rate as usize)
        .ok_or_else(|| "ASTRA_EMU_AUDIO_RESAMPLE_BOUNDS".to_owned())?
        .div_ceil(source_rate as usize);
    output.clear();
    output.reserve(output_frames.saturating_mul(usize::from(output_channels)));
    for frame in 0..output_frames {
        let source_frame = frame
            .saturating_mul(source_rate as usize)
            .checked_div(output_rate as usize)
            .unwrap_or(0)
            .min(source_frames.saturating_sub(1));
        for channel in 0..usize::from(output_channels) {
            let source_channel = channel.min(usize::from(source_channels) - 1);
            output.push(source[source_frame * usize::from(source_channels) + source_channel]);
        }
    }
    Ok(())
}

fn resolve_codec(declared: LegacyAudioEncoding, uri: &str, bytes: &[u8]) -> Result<String, String> {
    let codec = match declared {
        LegacyAudioEncoding::Wav => "wav",
        LegacyAudioEncoding::Ogg => "ogg",
        LegacyAudioEncoding::Mp3 => "mp3",
        LegacyAudioEncoding::Flac => "flac",
        LegacyAudioEncoding::Unknown if bytes.starts_with(b"RIFF") => "wav",
        LegacyAudioEncoding::Unknown if bytes.starts_with(b"OggS") => "ogg",
        LegacyAudioEncoding::Unknown if bytes.starts_with(b"fLaC") => "flac",
        LegacyAudioEncoding::Unknown
            if bytes.starts_with(b"ID3") || bytes.first().is_some_and(|byte| *byte == 0xff) =>
        {
            "mp3"
        }
        LegacyAudioEncoding::Unknown => return Err(format!("ASTRA_EMU_AUDIO_CODEC_UNKNOWN:{uri}")),
    };
    Ok(codec.into())
}

fn redacted_media_error(error: MediaError) -> String {
    match error {
        MediaError::Diagnostics(values) => format!(
            "ASTRA_EMU_AUDIO_DECODE:{}",
            values
                .iter()
                .map(|value| value.code.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
        MediaError::Message(_) => "ASTRA_EMU_AUDIO_DECODE:ASTRA_MEDIA_PROVIDER_MESSAGE".into(),
    }
}

#[cfg(target_os = "windows")]
fn native_audio_profile() -> Result<astra_platform::PlatformHostProfile, String> {
    Ok(astra_platform::PlatformHostProfile::windows_release(
        "astra-emu-manager",
        "dev.astraengine.astraemu-manager",
    ))
}
#[cfg(target_os = "macos")]
fn native_audio_profile() -> Result<astra_platform::PlatformHostProfile, String> {
    Ok(astra_platform::PlatformHostProfile::macos_release(
        "astra-emu-manager",
        "dev.astraengine.astraemu-manager",
    ))
}
#[cfg(target_os = "linux")]
fn native_audio_profile() -> Result<astra_platform::PlatformHostProfile, String> {
    Ok(
        astra_platform::PlatformHostProfile::linux_steam_sniper_release(
            "astra-emu-manager",
            "dev.astraengine.astraemu-manager",
        ),
    )
}
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn native_audio_profile() -> Result<astra_platform::PlatformHostProfile, String> {
    Err("PLATFORM_NOT_IMPLEMENTED".into())
}
#[cfg(target_os = "windows")]
fn native_audio_factory() -> impl PlatformHostFactory {
    astra_platform_windows::service_factory()
}
#[cfg(target_os = "linux")]
fn native_audio_factory() -> impl PlatformHostFactory {
    astra_platform_linux::factory()
}
#[cfg(target_os = "macos")]
fn native_audio_factory() -> impl PlatformHostFactory {
    astra_platform_macos::factory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segmented_i16_conversion_is_bounded_and_ordered() {
        let mut segments = VecDeque::from([AudioSegment::I16(vec![i16::MIN, 0, i16::MAX].into())]);
        let mut cursor = 0;
        let mut output = Vec::new();
        take_segmented(&mut segments, &mut cursor, 3, &mut output);
        assert_eq!(output.len(), 3);
        assert_eq!(output[0], -1.0);
        assert_eq!(output[1], 0.0);
        assert!(output[2] > 0.99);
    }

    #[test]
    fn same_format_f32_segment_moves_the_original_allocation() {
        let samples = vec![0.25_f32; 128];
        let pointer = samples.as_ptr();
        let mut segments = VecDeque::from([AudioSegment::F32(samples.into())]);
        let mut cursor = 0;
        let moved = take_owned_f32_segment(&mut segments, &mut cursor)
            .expect("same-format segment should move");
        assert_eq!(moved.as_ptr(), pointer);
        assert!(segments.is_empty());
        assert_eq!(cursor, 0);
    }

    #[test]
    fn invalid_movie_segment_fails_fast() {
        assert!(validate_segment(48_000, 2, &[0.0]).is_err());
        assert!(validate_segment(48_000, 2, &[f32::NAN, 0.0]).is_err());
    }

    #[test]
    fn completed_stream_params_update_without_recreating_kira_voice() {
        let mut streams =
            BTreeMap::from([(1, AudioStream::new(48_000, 2, LegacyAudioSampleFormat::F32))]);

        let needs_kira_update = update_stream_params(&mut streams, 1, 0.4, -0.25, false)
            .expect("logical stream remains valid after voice completion");

        assert!(!needs_kira_update);
        let stream = streams
            .get(&1)
            .expect("stream remains owned by logical state");
        assert_eq!(stream.volume, 0.4);
        assert_eq!(stream.pan, -0.25);
    }

    #[test]
    fn completed_stream_fade_stop_only_updates_logical_state() {
        let mut stream = AudioStream::new(48_000, 2, LegacyAudioSampleFormat::F32);
        stream.playing = true;
        let mut streams = BTreeMap::from([(3, stream)]);

        let pan = prepare_fade_stop(&mut streams, 3, 2_400)
            .expect("completed logical stream remains addressable");

        assert_eq!(pan, None);
        let stream = streams.get(&3).expect("logical stream remains owned");
        assert!(!stream.playing);
        assert_eq!(stream.stop_after_frames, 0);
    }
}
