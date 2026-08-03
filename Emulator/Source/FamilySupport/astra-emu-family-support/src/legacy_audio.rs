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

use astra_core::Hash256;
use astra_emu_family_api::{LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioSampleFormat};
use astra_media::{open_symphonia_audio_stream, MediaError, SymphoniaAudioStreamDecoder};
use astra_platform::{
    AudioOutputHandle, AudioOutputRequest, AudioPacket, HostLaunchProfile, PlatformHostClient,
    PlatformHostFactory,
};
use serde::Serialize;

pub const LEGACY_AUDIO_MAX_RESOURCE_BYTES: u64 = 512 * 1024 * 1024;
const COMMAND_CAPACITY: usize = 4096;
const TARGET_LATENCY_MS: u32 = 150;
const LOW_WATER_MS: u32 = 90;
const WORKER_POLL_MS: u64 = 4;
const MAX_STREAMS: usize = 512;
const MAX_SEGMENT_SAMPLES: usize = 4_194_304;

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
    Execute {
        command: LegacyAudioCommandV1,
        resource: Option<Vec<u8>>,
    },
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
    StopMovie {
        stream_id: u32,
    },
    Suspend(bool),
    Reset(SyncSender<Result<(), String>>),
    Shutdown(SyncSender<Result<Vec<u8>, String>>),
}

pub struct LegacyAudioPlaybackService {
    commands: SyncSender<WorkerCommand>,
    telemetry: Arc<TelemetryAtomics>,
    failure: Arc<Mutex<Option<String>>>,
    audible: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LegacyAudioPlaybackService {
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
        let (commands, receiver) = sync_channel(COMMAND_CAPACITY);
        let telemetry = Arc::new(TelemetryAtomics::default());
        let failure = Arc::new(Mutex::new(None));
        let audible = Arc::new(AtomicBool::new(false));
        let worker_telemetry = Arc::clone(&telemetry);
        let worker_failure = Arc::clone(&failure);
        let worker_audible = Arc::clone(&audible);
        let worker = thread::Builder::new()
            .name("astra-emu-legacy-audio".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_worker(
                        client,
                        shutdown_host,
                        receiver,
                        worker_telemetry,
                        worker_audible,
                    )
                }));
                let error = match result {
                    Ok(Ok(())) => return,
                    Ok(Err(error)) => error,
                    Err(_) => "ASTRA_EMU_AUDIO_WORKER_PANIC".to_owned(),
                };
                if let Ok(mut failure) = worker_failure.lock() {
                    *failure = Some(error);
                }
            })
            .map_err(|_| "ASTRA_EMU_AUDIO_WORKER_START".to_owned())?;
        Ok(Self {
            commands,
            telemetry,
            failure,
            audible,
            worker: Some(worker),
        })
    }

    pub fn execute(
        &self,
        command: LegacyAudioCommandV1,
        resource: Option<Vec<u8>>,
    ) -> Result<(), String> {
        command.validate().map_err(|error| error.to_string())?;
        self.check_failure()?;
        self.try_send(WorkerCommand::Execute { command, resource })
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
        self.try_send(WorkerCommand::StopMovie { stream_id })
    }

    pub fn set_suspended(&self, suspended: bool) -> Result<(), String> {
        self.try_send(WorkerCommand::Suspend(suspended))
    }

    pub fn pump(&self) -> Result<(), String> {
        self.check_failure()
    }

    pub fn telemetry(&self) -> LegacyAudioTelemetry {
        self.telemetry.snapshot()
    }

    pub fn meter_hash(&self) -> Hash256 {
        let telemetry = self.telemetry();
        Hash256::from_sha256(
            format!(
                "{}:{}:{}:{}:{}",
                telemetry.submitted_frames,
                telemetry.consumed_frames,
                telemetry.queued_frames,
                telemetry.underflow_count,
                telemetry.packet_count
            )
            .as_bytes(),
        )
    }

    pub fn has_audible_output(&self) -> bool {
        self.audible.load(Ordering::Relaxed)
    }

    pub fn reset(&self) -> Result<(), String> {
        self.request(WorkerCommand::Reset).and_then(|result| result)
    }

    pub fn shutdown(mut self) -> Result<Vec<u8>, String> {
        let result = self
            .request(WorkerCommand::Shutdown)
            .and_then(|result| result);
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                return Err("ASTRA_EMU_AUDIO_WORKER_JOIN".into());
            }
        }
        result
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
                TrySendError::Full(_) => "ASTRA_EMU_AUDIO_COMMAND_QUEUE_OVERFLOW".to_owned(),
                TrySendError::Disconnected(_) => {
                    "ASTRA_EMU_AUDIO_COMMAND_CHANNEL_CLOSED".to_owned()
                }
            })
    }

    fn check_failure(&self) -> Result<(), String> {
        match self.failure.lock() {
            Ok(failure) => failure.clone().map_or(Ok(()), Err),
            Err(_) => Err("ASTRA_EMU_AUDIO_FAILURE_LOCK_POISONED".into()),
        }
    }
}

impl Drop for LegacyAudioPlaybackService {
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
            if worker.join().is_err() {
                tracing::error!(event = "astra_emu_audio_worker_join_failed");
            }
        } else {
            tracing::error!(event = "astra_emu_audio_service_forced_detach");
        }
    }
}

struct AudioStream {
    source_rate: u32,
    source_channels: u16,
    output_rate: u32,
    output_channels: u16,
    sample_format: LegacyAudioSampleFormat,
    segments: VecDeque<Vec<f32>>,
    segment_cursor: usize,
    source_buffer: Vec<f32>,
    mix_buffer: Vec<f32>,
    decoder: Option<SymphoniaAudioStreamDecoder>,
    decoder_source: Option<(String, Arc<[u8]>)>,
    decoder_eof: bool,
    playing: bool,
    paused: bool,
    repeat: bool,
    volume: f32,
    pan: f32,
    output: Option<AudioOutputHandle>,
    packet_sequence: u64,
    /// Absolute device sample cursor at which the final non-silent sample ends.
    /// The final packet is padded to the latency target, allowing the worker to
    /// close the output without either truncating content or racing a callback
    /// into an empty queue.
    end_content_sample: Option<u64>,
    fade_in_total_frames: usize,
    fade_in_remaining_frames: usize,
    fade_out_total_frames: usize,
    fade_out_remaining_frames: usize,
}

impl AudioStream {
    fn submitted(sample_rate: u32, channels: u16, sample_format: LegacyAudioSampleFormat) -> Self {
        Self {
            source_rate: sample_rate,
            source_channels: channels,
            output_rate: 0,
            output_channels: 0,
            sample_format,
            segments: VecDeque::new(),
            segment_cursor: 0,
            source_buffer: Vec::new(),
            mix_buffer: Vec::new(),
            decoder: None,
            decoder_source: None,
            decoder_eof: false,
            playing: false,
            paused: false,
            repeat: false,
            volume: 1.0,
            pan: 0.0,
            output: None,
            packet_sequence: 0,
            end_content_sample: None,
            fade_in_total_frames: 0,
            fade_in_remaining_frames: 0,
            fade_out_total_frames: 0,
            fade_out_remaining_frames: 0,
        }
    }
}

struct WorkerState {
    client: PlatformHostClient,
    streams: BTreeMap<u32, AudioStream>,
    master_volume: f32,
    suspended: bool,
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
    let mut state = WorkerState {
        client,
        streams: BTreeMap::new(),
        master_volume: 1.0,
        suspended: false,
        telemetry,
        audible,
    };
    loop {
        match receiver.recv_timeout(Duration::from_millis(WORKER_POLL_MS)) {
            Ok(WorkerCommand::Shutdown(reply)) => {
                let result = runtime.block_on(state.shutdown(shutdown_host));
                let terminal = result.as_ref().map(|_| ()).map_err(Clone::clone);
                let _ = reply.send(result);
                terminal?;
                return Ok(());
            }
            Ok(WorkerCommand::Reset(reply)) => {
                let _ = reply.send(runtime.block_on(state.reset()));
            }
            Ok(command) => runtime.block_on(state.execute(command))?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("ASTRA_EMU_AUDIO_COMMAND_CHANNEL_CLOSED".into());
            }
        }
        if !state.suspended {
            runtime.block_on(state.refill())?;
        }
    }
}

impl WorkerState {
    async fn execute(&mut self, command: WorkerCommand) -> Result<(), String> {
        self.telemetry.command_count.fetch_add(1, Ordering::Relaxed);
        match command {
            WorkerCommand::Execute { command, resource } => {
                self.execute_legacy(command, resource).await
            }
            WorkerCommand::BeginMovie {
                stream_id,
                sample_rate,
                channels,
                samples,
            } => {
                self.replace_stream(
                    stream_id,
                    AudioStream::submitted(sample_rate, channels, LegacyAudioSampleFormat::F32),
                )
                .await?;
                self.append_f32(stream_id, sample_rate, channels, samples)?;
                if self.streams.get(&stream_id).is_some_and(|stream| {
                    stream
                        .segments
                        .front()
                        .is_some_and(|samples| !samples.is_empty())
                }) {
                    self.play(stream_id, 1.0, 0.0, false, 0).await
                } else {
                    Ok(())
                }
            }
            WorkerCommand::AppendMovie {
                stream_id,
                sample_rate,
                channels,
                samples,
            } => self.append_f32(stream_id, sample_rate, channels, samples),
            WorkerCommand::StopMovie { stream_id } => self.remove_stream(stream_id).await,
            WorkerCommand::Suspend(value) => {
                self.suspended = value;
                for stream in self.streams.values_mut() {
                    if let Some(output) = stream.output {
                        if value {
                            self.client.pause_audio(output).await
                        } else if stream.playing && !stream.paused {
                            self.client.resume_audio(output).await
                        } else {
                            Ok(())
                        }
                        .map_err(|error| error.to_string())?;
                    }
                }
                Ok(())
            }
            WorkerCommand::Reset(_) | WorkerCommand::Shutdown(_) => {
                unreachable!("handled by worker loop")
            }
        }
    }

    async fn execute_legacy(
        &mut self,
        command: LegacyAudioCommandV1,
        resource: Option<Vec<u8>>,
    ) -> Result<(), String> {
        let (operation, stream_id) = legacy_audio_command_identity(&command);
        tracing::debug!(
            event = "astra_emu_legacy_audio_command",
            operation,
            stream_id
        );
        match command {
            LegacyAudioCommandV1::LoadResource {
                stream_id,
                encoding,
                resource_uri,
            } => {
                let encoded =
                    resource.ok_or_else(|| "ASTRA_EMU_AUDIO_RESOURCE_MISSING".to_owned())?;
                let codec = resolve_codec(encoding, &resource_uri, &encoded)?;
                let source: Arc<[u8]> = encoded.into();
                let decoder = open_symphonia_audio_stream(
                    &codec,
                    Arc::clone(&source),
                    LEGACY_AUDIO_MAX_RESOURCE_BYTES,
                )
                .map_err(redacted_media_error)?;
                let mut stream = AudioStream::submitted(
                    decoder.sample_rate(),
                    decoder.channels(),
                    LegacyAudioSampleFormat::I16,
                );
                stream.decoder = Some(decoder);
                stream.decoder_source = Some((codec, source));
                self.replace_stream(stream_id, stream).await
            }
            LegacyAudioCommandV1::CreateStream {
                stream_id,
                sample_rate,
                channels,
                sample_format,
            } => {
                self.replace_stream(
                    stream_id,
                    AudioStream::submitted(sample_rate, channels, sample_format),
                )
                .await
            }
            LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => {
                let stream = self
                    .streams
                    .get_mut(&stream_id)
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
                if stream.sample_format != LegacyAudioSampleFormat::I16 {
                    return Err("ASTRA_EMU_AUDIO_SAMPLE_FORMAT_MISMATCH".into());
                }
                stream.segments.push_back(
                    samples
                        .into_iter()
                        .map(|sample| f32::from(sample) / 32768.0)
                        .collect(),
                );
                Ok(())
            }
            LegacyAudioCommandV1::SubmitF32 { stream_id, samples } => {
                let stream = self
                    .streams
                    .get(&stream_id)
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
                self.append_f32(
                    stream_id,
                    stream.source_rate,
                    stream.source_channels,
                    samples,
                )
            }
            LegacyAudioCommandV1::Play {
                stream_id,
                volume,
                pan,
                repeat,
                fade_in_ms,
            } => self.play(stream_id, volume, pan, repeat, fade_in_ms).await,
            LegacyAudioCommandV1::Stop { stream_id, fade_ms } => {
                if self
                    .streams
                    .get(&stream_id)
                    .is_some_and(|stream| stream.output.is_some())
                {
                    self.request_stop(stream_id, fade_ms).await
                } else {
                    if let Some(stream) = self.streams.get_mut(&stream_id) {
                        stream.playing = false;
                    }
                    Ok(())
                }
            }
            LegacyAudioCommandV1::Pause { stream_id } => {
                if self
                    .streams
                    .get(&stream_id)
                    .is_some_and(|stream| stream.output.is_some())
                {
                    self.pause(stream_id).await
                } else {
                    Ok(())
                }
            }
            LegacyAudioCommandV1::Resume { stream_id } => {
                if self
                    .streams
                    .get(&stream_id)
                    .is_some_and(|stream| stream.output.is_some())
                {
                    self.resume(stream_id).await
                } else {
                    Ok(())
                }
            }
            LegacyAudioCommandV1::SetParams {
                stream_id,
                volume,
                pan,
                repeat,
            } => {
                if let Some(stream) = self
                    .streams
                    .get_mut(&stream_id)
                    .filter(|stream| stream.output.is_some())
                {
                    stream.volume = volume;
                    stream.pan = pan;
                    stream.repeat = repeat;
                }
                Ok(())
            }
            LegacyAudioCommandV1::DestroyStream { stream_id } => {
                if self.streams.contains_key(&stream_id) {
                    self.remove_stream(stream_id).await
                } else {
                    Ok(())
                }
            }
            LegacyAudioCommandV1::MasterVolume { volume } => {
                self.master_volume = volume;
                Ok(())
            }
        }
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
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if stream.sample_format != LegacyAudioSampleFormat::F32
            || stream.source_rate != sample_rate
            || stream.source_channels != channels
            || samples.iter().any(|sample| !sample.is_finite())
        {
            return Err("ASTRA_EMU_AUDIO_SAMPLE_FORMAT_MISMATCH".into());
        }
        stream.segments.push_back(samples);
        Ok(())
    }

    async fn play(
        &mut self,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    ) -> Result<(), String> {
        let format = self
            .client
            .query_audio_device_format()
            .await
            .map_err(|error| error.to_string())?;
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if stream.output.is_some() {
            return Err("ASTRA_EMU_AUDIO_PLAY_STATE".into());
        }
        stream.output_rate = format.sample_rate;
        stream.output_channels = format.channels;
        stream.output = Some(
            self.client
                .open_audio_output(AudioOutputRequest {
                    sample_rate: format.sample_rate,
                    channels: format.channels,
                    max_buffered_frames: (format.sample_rate as usize * 2).max(1),
                    start_paused: true,
                })
                .await
                .map_err(|error| error.to_string())?,
        );
        stream.volume = volume;
        stream.pan = pan;
        stream.repeat = repeat;
        stream.playing = true;
        stream.paused = false;
        stream.packet_sequence = 0;
        stream.end_content_sample = None;
        stream.fade_in_total_frames = frames_for_ms(format.sample_rate, fade_in_ms)?;
        stream.fade_in_remaining_frames = stream.fade_in_total_frames;
        stream.fade_out_total_frames = 0;
        stream.fade_out_remaining_frames = 0;
        Ok(())
    }

    async fn request_stop(&mut self, stream_id: u32, fade_ms: u32) -> Result<(), String> {
        if fade_ms == 0 {
            return self.stop(stream_id).await;
        }
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if !stream.playing || stream.output.is_none() {
            return Err("ASTRA_EMU_AUDIO_STOP_STATE".into());
        }
        stream.fade_out_total_frames = frames_for_ms(stream.output_rate, fade_ms)?.max(1);
        stream.fade_out_remaining_frames = stream.fade_out_total_frames;
        Ok(())
    }

    async fn pause(&mut self, stream_id: u32) -> Result<(), String> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if let Some(output) = stream.output {
            self.client
                .pause_audio(output)
                .await
                .map_err(|error| error.to_string())?;
        }
        stream.paused = true;
        Ok(())
    }

    async fn resume(&mut self, stream_id: u32) -> Result<(), String> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if let Some(output) = stream.output {
            self.client
                .resume_audio(output)
                .await
                .map_err(|error| error.to_string())?;
        }
        stream.paused = false;
        Ok(())
    }

    async fn stop(&mut self, stream_id: u32) -> Result<(), String> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        if let Some(output) = stream.output.take() {
            self.client
                .abort_audio(output)
                .await
                .map_err(|error| error.to_string())?;
        }
        stream.playing = false;
        Ok(())
    }

    async fn remove_stream(&mut self, stream_id: u32) -> Result<(), String> {
        if self.streams.contains_key(&stream_id) {
            self.stop(stream_id).await?;
        }
        self.streams
            .remove(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        Ok(())
    }

    async fn replace_stream(&mut self, stream_id: u32, stream: AudioStream) -> Result<(), String> {
        if !self.streams.contains_key(&stream_id) && self.streams.len() >= MAX_STREAMS {
            return Err("ASTRA_EMU_AUDIO_STREAM_LIMIT".into());
        }
        if self.streams.contains_key(&stream_id) {
            self.stop(stream_id).await?;
        }
        self.streams.insert(stream_id, stream);
        Ok(())
    }

    async fn refill(&mut self) -> Result<(), String> {
        let ids = self
            .streams
            .iter()
            .filter(|(_, stream)| stream.playing && !stream.paused)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let mut active = 0_u64;
        for id in ids {
            active += 1;
            self.refill_stream(id).await?;
        }
        self.telemetry
            .active_streams
            .store(active, Ordering::Relaxed);
        self.refresh_output_telemetry().await?;
        Ok(())
    }

    async fn refill_stream(&mut self, stream_id: u32) -> Result<(), String> {
        let (output, rate, channels, state) = {
            let stream = self
                .streams
                .get(&stream_id)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
            let output = stream
                .output
                .ok_or_else(|| "ASTRA_EMU_AUDIO_OUTPUT_MISSING".to_owned())?;
            let state = self
                .client
                .query_audio(output)
                .await
                .map_err(|error| error.to_string())?;
            (output, stream.output_rate, stream.output_channels, state)
        };
        if let Some(end_content_sample) = self
            .streams
            .get(&stream_id)
            .and_then(|stream| stream.end_content_sample)
        {
            if state.consumed_samples >= end_content_sample {
                self.client
                    .pause_audio(output)
                    .await
                    .map_err(|error| error.to_string())?;
                self.client
                    .abort_audio(output)
                    .await
                    .map_err(|error| error.to_string())?;
                let stream = self
                    .streams
                    .get_mut(&stream_id)
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
                stream.output = None;
                stream.playing = false;
                stream.end_content_sample = None;
                tracing::debug!(
                    event = "astra_emu_audio_stream_eof_closed",
                    stream_id,
                    consumed_samples = state.consumed_samples
                );
            }
            return Ok(());
        }
        let queued = state.queued_frames;
        let low = frames_for_ms(rate, LOW_WATER_MS)?;
        let target = frames_for_ms(rate, TARGET_LATENCY_MS)?;
        if queued > low {
            return Ok(());
        }
        let frames = target.saturating_sub(queued);
        let source_frames = {
            let stream = self
                .streams
                .get(&stream_id)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
            frames
                .checked_mul(stream.source_rate as usize)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_REFILL_BOUNDS".to_owned())?
                .div_ceil(rate as usize)
        };
        self.ensure_decoded(stream_id, source_frames).await?;
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_STREAM_MISSING".to_owned())?;
        let source_needed = source_frames
            .checked_mul(usize::from(stream.source_channels))
            .ok_or_else(|| "ASTRA_EMU_AUDIO_REFILL_BOUNDS".to_owned())?;
        take_segmented_into(
            &mut stream.segments,
            &mut stream.segment_cursor,
            source_needed,
            &mut stream.source_buffer,
        );
        let source_exhausted = stream.decoder_eof
            && stream.decoder.is_none()
            && queued_samples(&stream.segments, stream.segment_cursor) == 0;
        if stream.source_buffer.is_empty() && !source_exhausted {
            return Ok(());
        }
        if stream.source_buffer.is_empty() {
            stream.mix_buffer.clear();
        } else {
            resample_chunk_into(
                &stream.source_buffer,
                stream.source_rate,
                stream.source_channels,
                rate,
                channels,
                &mut stream.mix_buffer,
            )?;
        }
        let content_sample_count = stream.mix_buffer.len();
        if source_exhausted {
            let target_samples = frames
                .checked_mul(usize::from(channels))
                .ok_or_else(|| "ASTRA_EMU_AUDIO_REFILL_BOUNDS".to_owned())?;
            if stream.mix_buffer.len() < target_samples {
                stream.mix_buffer.resize(target_samples, 0.0);
            }
        }
        apply_gain_pan(
            &mut stream.mix_buffer,
            channels,
            stream.volume * self.master_volume,
            stream.pan,
        )?;
        apply_fade_envelopes(
            &mut stream.mix_buffer,
            channels,
            stream.fade_in_total_frames,
            &mut stream.fade_in_remaining_frames,
            stream.fade_out_total_frames,
            &mut stream.fade_out_remaining_frames,
        )?;
        let stop_after_packet =
            stream.fade_out_total_frames != 0 && stream.fade_out_remaining_frames == 0;
        stream.source_buffer.clear();
        stream.packet_sequence = stream
            .packet_sequence
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SEQUENCE_OVERFLOW".to_owned())?;
        let sequence = stream.packet_sequence;
        let first_packet = sequence == 1;
        let end_content_sample = source_exhausted.then(|| {
            state
                .submitted_samples
                .saturating_add(content_sample_count as u64)
        });
        stream.mix_buffer = self
            .client
            .submit_audio_owned(
                output,
                AudioPacket {
                    sequence,
                    channels,
                    samples: std::mem::take(&mut stream.mix_buffer),
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        if first_packet {
            self.client
                .resume_audio(output)
                .await
                .map_err(|error| error.to_string())?;
        }
        stream.end_content_sample = end_content_sample;
        self.telemetry.packet_count.fetch_add(1, Ordering::Relaxed);
        if stop_after_packet {
            self.stop(stream_id).await?;
        }
        Ok(())
    }

    async fn refresh_output_telemetry(&mut self) -> Result<(), String> {
        let mut submitted = 0_u64;
        let mut consumed = 0_u64;
        let mut queued = 0_u64;
        let mut underflows = 0_u64;
        let mut audible = false;
        for stream in self.streams.values_mut() {
            let Some(output) = stream.output else {
                continue;
            };
            let state = self
                .client
                .query_audio_output(output)
                .await
                .map_err(|error| error.to_string())?;
            submitted = submitted
                .checked_add(state.submitted_frames)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            consumed = consumed
                .checked_add(state.played_frames)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            queued = queued
                .checked_add(state.buffered_frames as u64)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            underflows = underflows
                .checked_add(state.underflow_count)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            audible |= state.meter.peak_dbfs.is_finite() && state.meter.peak_dbfs > -90.0;
        }
        self.telemetry
            .submitted_frames
            .store(submitted, Ordering::Relaxed);
        self.telemetry
            .consumed_frames
            .store(consumed, Ordering::Relaxed);
        self.telemetry
            .queued_frames
            .store(queued, Ordering::Relaxed);
        self.telemetry
            .underflow_count
            .store(underflows, Ordering::Relaxed);
        self.audible.store(audible, Ordering::Relaxed);
        Ok(())
    }

    async fn ensure_decoded(&mut self, stream_id: u32, frames: usize) -> Result<(), String> {
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
            let stream = self.streams.get_mut(&stream_id).unwrap();
            match stream
                .decoder
                .as_mut()
                .unwrap()
                .next_chunk()
                .map_err(redacted_media_error)?
            {
                Some(chunk) => {
                    if chunk.sample_rate != stream.source_rate
                        || chunk.channels != stream.source_channels
                    {
                        return Err("ASTRA_EMU_AUDIO_STREAM_FORMAT_CHANGE".into());
                    }
                    stream.segments.push_back(
                        chunk
                            .pcm_s16le
                            .chunks_exact(2)
                            .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
                            .collect(),
                    );
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
                            Arc::clone(source),
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

    async fn reset(&mut self) -> Result<(), String> {
        let ids = self.streams.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.remove_stream(id).await?;
        }
        Ok(())
    }

    async fn shutdown(&mut self, shutdown_host: bool) -> Result<Vec<u8>, String> {
        self.refresh_output_telemetry().await?;
        let mut trace = serde_json::to_vec(&self.telemetry.snapshot())
            .map_err(|_| "ASTRA_EMU_AUDIO_TELEMETRY_ENCODE".to_owned())?;
        trace.push(b'\n');
        self.reset().await?;
        if shutdown_host {
            self.client
                .shutdown()
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(trace)
    }
}

fn legacy_audio_command_identity(command: &LegacyAudioCommandV1) -> (&'static str, u32) {
    match command {
        LegacyAudioCommandV1::LoadResource { stream_id, .. } => ("load_resource", *stream_id),
        LegacyAudioCommandV1::CreateStream { stream_id, .. } => ("create_stream", *stream_id),
        LegacyAudioCommandV1::SubmitI16 { stream_id, .. } => ("submit_i16", *stream_id),
        LegacyAudioCommandV1::SubmitF32 { stream_id, .. } => ("submit_f32", *stream_id),
        LegacyAudioCommandV1::Play { stream_id, .. } => ("play", *stream_id),
        LegacyAudioCommandV1::Stop { stream_id, .. } => ("stop", *stream_id),
        LegacyAudioCommandV1::Pause { stream_id } => ("pause", *stream_id),
        LegacyAudioCommandV1::Resume { stream_id } => ("resume", *stream_id),
        LegacyAudioCommandV1::SetParams { stream_id, .. } => ("set_params", *stream_id),
        LegacyAudioCommandV1::DestroyStream { stream_id } => ("destroy_stream", *stream_id),
        LegacyAudioCommandV1::MasterVolume { .. } => ("master_volume", 0),
    }
}

fn validate_segment(sample_rate: u32, channels: u16, samples: &[f32]) -> Result<(), String> {
    if sample_rate == 0
        || !(1..=2).contains(&channels)
        || samples.len() > MAX_SEGMENT_SAMPLES
        || !samples.len().is_multiple_of(usize::from(channels))
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err("ASTRA_EMU_AUDIO_MOVIE_SEGMENT_INVALID".into());
    }
    Ok(())
}

fn queued_samples(segments: &VecDeque<Vec<f32>>, cursor: usize) -> usize {
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if index == 0 {
                segment.len().saturating_sub(cursor)
            } else {
                segment.len()
            }
        })
        .sum()
}

fn take_segmented_into(
    segments: &mut VecDeque<Vec<f32>>,
    cursor: &mut usize,
    limit: usize,
    output: &mut Vec<f32>,
) {
    output.clear();
    output.reserve(limit);
    while output.len() < limit {
        let Some(front) = segments.front() else {
            break;
        };
        let available = front
            .len()
            .saturating_sub(*cursor)
            .min(limit - output.len());
        output.extend_from_slice(&front[*cursor..*cursor + available]);
        *cursor += available;
        if *cursor == front.len() {
            segments.pop_front();
            *cursor = 0;
        }
    }
}

fn frames_for_ms(rate: u32, milliseconds: u32) -> Result<usize, String> {
    usize::try_from(u64::from(rate) * u64::from(milliseconds) / 1_000)
        .map_err(|_| "ASTRA_EMU_AUDIO_LATENCY_BOUNDS".into())
}

fn resample_chunk_into(
    samples: &[f32],
    source_rate: u32,
    source_channels: u16,
    output_rate: u32,
    output_channels: u16,
    output: &mut Vec<f32>,
) -> Result<(), String> {
    if source_rate == 0
        || output_rate == 0
        || !(1..=2).contains(&source_channels)
        || !(1..=2).contains(&output_channels)
        || samples.is_empty()
        || !samples.len().is_multiple_of(usize::from(source_channels))
    {
        return Err("ASTRA_EMU_AUDIO_RESAMPLE_FORMAT".into());
    }
    let source_frames = samples.len() / usize::from(source_channels);
    let output_frames = source_frames
        .saturating_mul(output_rate as usize)
        .div_ceil(source_rate as usize);
    output.clear();
    let output_samples = output_frames.saturating_mul(usize::from(output_channels));
    output.reserve(output_samples);
    for frame in 0..output_frames {
        let position = frame as f64 * source_rate as f64 / output_rate as f64;
        let left = position.floor() as usize;
        let right = (left + 1).min(source_frames - 1);
        let fraction = (position - left as f64) as f32;
        let read = |channel: usize| {
            let channel = channel.min(usize::from(source_channels) - 1);
            let a = samples[left.min(source_frames - 1) * usize::from(source_channels) + channel];
            let b = samples[right * usize::from(source_channels) + channel];
            a + (b - a) * fraction
        };
        match (source_channels, output_channels) {
            (1, 1) => output.push(read(0)),
            (1, 2) => {
                let value = read(0);
                output.extend_from_slice(&[value, value]);
            }
            (2, 1) => output.push((read(0) + read(1)) * 0.5),
            (2, 2) => output.extend_from_slice(&[read(0), read(1)]),
            _ => unreachable!(),
        }
    }
    Ok(())
}

fn apply_gain_pan(samples: &mut [f32], channels: u16, gain: f32, pan: f32) -> Result<(), String> {
    if !gain.is_finite() || !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err("ASTRA_EMU_AUDIO_GAIN_INVALID".into());
    }
    if channels == 1 {
        for sample in samples {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    } else if channels == 2 {
        let left = gain * (1.0 - pan.max(0.0));
        let right = gain * (1.0 + pan.min(0.0));
        for frame in samples.chunks_exact_mut(2) {
            frame[0] = (frame[0] * left).clamp(-1.0, 1.0);
            frame[1] = (frame[1] * right).clamp(-1.0, 1.0);
        }
    } else {
        return Err("ASTRA_EMU_AUDIO_CHANNELS_INVALID".into());
    }
    Ok(())
}

fn apply_fade_envelopes(
    samples: &mut [f32],
    channels: u16,
    fade_in_total: usize,
    fade_in_remaining: &mut usize,
    fade_out_total: usize,
    fade_out_remaining: &mut usize,
) -> Result<(), String> {
    if channels == 0 || !samples.len().is_multiple_of(usize::from(channels)) {
        return Err("ASTRA_EMU_AUDIO_FADE_FORMAT".into());
    }
    for frame in samples.chunks_exact_mut(usize::from(channels)) {
        let fade_in = if *fade_in_remaining == 0 || fade_in_total == 0 {
            1.0
        } else {
            let completed = fade_in_total.saturating_sub(*fade_in_remaining);
            *fade_in_remaining -= 1;
            completed as f32 / fade_in_total as f32
        };
        let fade_out = if *fade_out_remaining == 0 || fade_out_total == 0 {
            if fade_out_total == 0 {
                1.0
            } else {
                0.0
            }
        } else {
            let gain = *fade_out_remaining as f32 / fade_out_total as f32;
            *fade_out_remaining -= 1;
            gain
        };
        for sample in frame {
            *sample *= fade_in * fade_out;
        }
    }
    Ok(())
}

fn resolve_codec(declared: LegacyAudioEncoding, uri: &str, bytes: &[u8]) -> Result<String, String> {
    let declared = match declared {
        LegacyAudioEncoding::Unknown => None,
        LegacyAudioEncoding::Wav => Some("wav"),
        LegacyAudioEncoding::Ogg => Some("ogg"),
        LegacyAudioEncoding::Mp3 => Some("mp3"),
        LegacyAudioEncoding::Flac => Some("flac"),
    };
    let extension = uri
        .rsplit_once('.')
        .map(|(_, value)| value.to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "wav" | "ogg" | "mp3" | "flac"));
    let detected = if bytes.starts_with(b"OggS") {
        Some("ogg")
    } else if bytes.starts_with(b"fLaC") {
        Some("flac")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        Some("wav")
    } else if bytes.starts_with(b"ID3") {
        Some("mp3")
    } else {
        None
    };
    let selected = declared
        .map(str::to_owned)
        .or(extension)
        .or_else(|| detected.map(str::to_owned))
        .ok_or_else(|| "ASTRA_EMU_AUDIO_CODEC_UNIDENTIFIED".to_owned())?;
    if detected.is_some_and(|value| value != selected) {
        return Err("ASTRA_EMU_AUDIO_CODEC_IDENTITY_MISMATCH".into());
    }
    Ok(selected)
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
    astra_platform_windows::factory()
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
    use astra_platform::{
        host_channel, AudioDeviceFormat, AudioMeter, AudioOutputState, AudioOutputStatus,
        HostCommand, PlatformHostProfile,
    };

    #[test]
    fn segmented_pcm_consumption_never_rebuilds_history() {
        let first = vec![0.1, 0.2, 0.3, 0.4];
        let first_pointer = first.as_ptr();
        let second = vec![0.5, 0.6, 0.7, 0.8];
        let second_pointer = second.as_ptr();
        let mut segments = VecDeque::from([first, second]);
        let mut cursor = 0;
        let mut output = Vec::new();

        take_segmented_into(&mut segments, &mut cursor, 2, &mut output);
        assert_eq!(output, vec![0.1, 0.2]);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].as_ptr(), first_pointer);
        assert_eq!(segments[1].as_ptr(), second_pointer);

        take_segmented_into(&mut segments, &mut cursor, 4, &mut output);
        assert_eq!(output, vec![0.3, 0.4, 0.5, 0.6]);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].as_ptr(), second_pointer);
        assert_eq!(cursor, 2);
    }

    #[test]
    fn movie_segment_validation_rejects_partial_frames_and_non_finite_samples() {
        assert_eq!(
            validate_segment(48_000, 2, &[0.0]).unwrap_err(),
            "ASTRA_EMU_AUDIO_MOVIE_SEGMENT_INVALID"
        );
        assert_eq!(
            validate_segment(48_000, 1, &[f32::NAN]).unwrap_err(),
            "ASTRA_EMU_AUDIO_MOVIE_SEGMENT_INVALID"
        );
    }

    #[test]
    fn resample_chunk_converts_channels_without_intermediate_history() {
        let mut output = Vec::new();
        resample_chunk_into(&[0.25, -0.25, 0.5, -0.5], 48_000, 2, 48_000, 1, &mut output)
            .expect("stereo to mono");
        assert_eq!(output, vec![0.0, 0.0]);
    }

    #[test]
    fn fade_envelopes_advance_once_per_frame() {
        let mut samples = vec![1.0; 8];
        let mut fade_in_remaining = 2;
        let mut fade_out_remaining = 2;
        apply_fade_envelopes(
            &mut samples,
            2,
            2,
            &mut fade_in_remaining,
            2,
            &mut fade_out_remaining,
        )
        .expect("valid stereo fade");
        assert_eq!(&samples[..2], &[0.0, 0.0]);
        assert_eq!(&samples[2..4], &[0.25, 0.25]);
        assert_eq!(&samples[4..], &[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(fade_in_remaining, 0);
        assert_eq!(fade_out_remaining, 0);
    }

    #[test]
    fn control_commands_preserve_legacy_idempotence_before_stream_creation() {
        let (client, _backend, _events) = host_channel(
            PlatformHostProfile::windows_release(
                "legacy-audio-controls",
                "dev.astraengine.audio-controls-test",
            ),
            16,
            4,
        )
        .expect("host channel");
        let mut state = WorkerState {
            client,
            streams: BTreeMap::new(),
            master_volume: 1.0,
            suspended: false,
            telemetry: Arc::new(TelemetryAtomics::default()),
            audible: Arc::new(AtomicBool::new(false)),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            for command in [
                LegacyAudioCommandV1::Stop {
                    stream_id: 9,
                    fade_ms: 250,
                },
                LegacyAudioCommandV1::Pause { stream_id: 9 },
                LegacyAudioCommandV1::Resume { stream_id: 9 },
                LegacyAudioCommandV1::SetParams {
                    stream_id: 9,
                    volume: 0.5,
                    pan: 0.0,
                    repeat: false,
                },
                LegacyAudioCommandV1::DestroyStream { stream_id: 9 },
            ] {
                state
                    .execute_legacy(command, None)
                    .await
                    .expect("missing control target is an idempotent no-op");
            }
            state
                .execute_legacy(
                    LegacyAudioCommandV1::CreateStream {
                        stream_id: 9,
                        sample_rate: 48_000,
                        channels: 2,
                        sample_format: LegacyAudioSampleFormat::F32,
                    },
                    None,
                )
                .await
                .expect("create stream");
            state
                .execute_legacy(
                    LegacyAudioCommandV1::Stop {
                        stream_id: 9,
                        fade_ms: 250,
                    },
                    None,
                )
                .await
                .expect("unplayed stream stop is idempotent");
            state
                .execute_legacy(LegacyAudioCommandV1::DestroyStream { stream_id: 9 }, None)
                .await
                .expect("destroy stream");
            state
                .execute_legacy(LegacyAudioCommandV1::DestroyStream { stream_id: 9 }, None)
                .await
                .expect("repeated destroy is idempotent");
        });
    }

    #[test]
    fn worker_refills_while_the_runtime_thread_is_blocked() {
        let (client, mut backend, _events) = host_channel(
            PlatformHostProfile::windows_release(
                "legacy-audio-worker",
                "dev.astraengine.audio-worker-test",
            ),
            64,
            8,
        )
        .expect("host channel");
        let backend_thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend runtime");
            runtime.block_on(async move {
                let output = AudioOutputHandle::from_parts(1, 1).expect("audio handle");
                let mut submitted_samples = 0_u64;
                let mut submitted_frames = 0_u64;
                while let Some(command) = backend.next_command().await {
                    match command {
                        HostCommand::QueryAudioDeviceFormat { reply }
                        | HostCommand::QueryAudioOutputFormat { reply } => {
                            let _ = reply.send(Ok(AudioDeviceFormat {
                                sample_rate: 48_000,
                                channels: 2,
                            }));
                        }
                        HostCommand::OpenAudioOutput { reply, .. } => {
                            let _ = reply.send(Ok(output));
                        }
                        HostCommand::QueryAudio { reply, .. } => {
                            let _ = reply.send(Ok(AudioOutputState {
                                queued_frames: 0,
                                callback_count: submitted_frames,
                                submitted_samples,
                                consumed_samples: submitted_samples,
                                underflow_count: 0,
                                meter: AudioMeter {
                                    sample_count: submitted_samples,
                                    peak_dbfs: -12.0,
                                    rms_dbfs: -18.0,
                                },
                            }));
                        }
                        HostCommand::SubmitAudio { packet, reply, .. } => {
                            submitted_samples += packet.samples.len() as u64;
                            submitted_frames += packet.frame_count() as u64;
                            let _ = reply.send(Ok(packet.samples));
                        }
                        HostCommand::QueryAudioOutput { reply, .. } => {
                            let _ = reply.send(Ok(AudioOutputStatus {
                                submitted_frames,
                                played_frames: submitted_frames,
                                buffered_frames: 0,
                                underflow_count: 0,
                                meter: AudioMeter {
                                    sample_count: submitted_samples,
                                    peak_dbfs: -12.0,
                                    rms_dbfs: -18.0,
                                },
                            }));
                        }
                        HostCommand::ResumeAudio { reply, .. }
                        | HostCommand::PauseAudio { reply, .. }
                        | HostCommand::AbortAudio { reply, .. }
                        | HostCommand::CloseAudio { reply, .. } => {
                            let _ = reply.send(Ok(()));
                        }
                        HostCommand::DrainAudio { reply, .. } => {
                            let _ = reply.send(Ok(AudioMeter {
                                sample_count: submitted_samples,
                                peak_dbfs: -12.0,
                                rms_dbfs: -18.0,
                            }));
                        }
                        HostCommand::Shutdown { reply } => {
                            let _ = reply.send(Ok(()));
                            break;
                        }
                        other => panic!("unexpected host command: {}", other.operation()),
                    }
                }
            });
        });
        let service = LegacyAudioPlaybackService::start_with_client(client.clone(), false)
            .expect("audio worker");
        service
            .execute(
                LegacyAudioCommandV1::CreateStream {
                    stream_id: 7,
                    sample_rate: 48_000,
                    channels: 2,
                    sample_format: LegacyAudioSampleFormat::F32,
                },
                None,
            )
            .expect("create stream");
        service
            .execute(
                LegacyAudioCommandV1::SubmitF32 {
                    stream_id: 7,
                    samples: vec![0.25; 96_000],
                },
                None,
            )
            .expect("submit PCM");
        service
            .execute(
                LegacyAudioCommandV1::Play {
                    stream_id: 7,
                    volume: 1.0,
                    pan: 0.0,
                    repeat: false,
                    fade_in_ms: 0,
                },
                None,
            )
            .expect("play stream");

        std::thread::sleep(Duration::from_millis(250));
        service.pump().expect("worker remains healthy");
        let telemetry = service.telemetry();
        assert!(telemetry.packet_count > 0);
        assert!(telemetry.submitted_frames > 0);

        service.shutdown().expect("worker shutdown");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("client runtime");
        runtime.block_on(client.shutdown()).expect("host shutdown");
        backend_thread.join().expect("backend join");
    }
}
