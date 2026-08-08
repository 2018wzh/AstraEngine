use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use astra_audio_kira::{
    AstraChunkBackendSettings, AudioAssetRevision, AudioChunkTelemetry, AudioServiceCommand,
    AudioServiceConfig, AudioServiceEvent, AudioServiceSession, AudioTimelineStateV1,
};
use astra_media::{PcmAsset, CANONICAL_CHANNELS, CANONICAL_SAMPLE_RATE};
use astra_platform::{
    AudioCaptureReader, AudioOutputHandle, AudioOutputRequest, HostKind, PlatformError,
};
use astra_player_core::{PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor};

pub struct NativeVnProductAudioHost {
    service: Option<AudioServiceSession>,
    output: Option<AudioOutputHandle>,
    prepared_assets: BTreeMap<String, AudioAssetRevision>,
    voice_kinds: BTreeMap<String, String>,
    known_bgm_targets: BTreeSet<String>,
    pending_fade_stops: BTreeMap<String, NativeVnPendingFadeStop>,
    last_meter: Option<NativeVnAudioMeterSnapshot>,
    evidence_capture: Option<AudioCaptureReader>,
    retain_evidence_audio: bool,
    pending_restore: Option<AudioTimelineStateV1>,
    pending_recovery_assets: Vec<(AudioAssetRevision, u32, u16, Arc<Vec<f32>>)>,
    previous_telemetry: AudioChunkTelemetry,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeVnAudioPerformanceSample {
    pub query_ns: u64,
    pub render_ns: u64,
    pub submit_ns: u64,
    pub completion_ns: u64,
}

impl Default for NativeVnProductAudioHost {
    fn default() -> Self {
        Self::new(true)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeVnAudioMeterSnapshot {
    pub rendered_samples: u64,
    pub submitted_samples: u64,
    pub consumed_samples: u64,
    pub underflow_count: u64,
    pub render_ns: u64,
    pub submit_wait_ns: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NativeVnProductAudioSnapshot {
    pub schema: String,
    pub timeline: AudioTimelineStateV1,
    pub voice_kinds: BTreeMap<String, String>,
    pub known_bgm_targets: BTreeSet<String>,
    pub pending_fade_stops: BTreeMap<String, NativeVnPendingFadeStop>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeVnPendingFadeStop {
    pub voice_id: String,
    pub fence: String,
}

impl NativeVnProductAudioHost {
    const BUFFERED_FRAMES: usize = 4_096;
    const MAX_VOICES: usize = 64;
    const MAX_BUSES: usize = 16;
    const MAX_EVENTS: usize = 256;
    pub(crate) const MAX_CONVERTED_SAMPLES: usize = 20_000_000;

    pub fn new(retain_evidence_audio: bool) -> Self {
        Self {
            service: None,
            output: None,
            prepared_assets: BTreeMap::new(),
            voice_kinds: BTreeMap::new(),
            known_bgm_targets: BTreeSet::new(),
            pending_fade_stops: BTreeMap::new(),
            last_meter: None,
            evidence_capture: None,
            retain_evidence_audio,
            pending_restore: None,
            pending_recovery_assets: Vec::new(),
            previous_telemetry: AudioChunkTelemetry::default(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.service
            .as_ref()
            .is_some_and(|service| service.active_voice_count() > 0)
    }

    pub fn has_active_voice(&self) -> bool {
        self.voice_kinds.values().any(|kind| kind == "voice")
    }

    pub fn last_meter(&self) -> Option<NativeVnAudioMeterSnapshot> {
        self.last_meter
    }

    pub fn submitted_timeline(&self) -> Result<Vec<f32>, PlatformError> {
        self.evidence_capture
            .as_ref()
            .map(AudioCaptureReader::take_samples)
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    pub fn snapshot(&self) -> NativeVnProductAudioSnapshot {
        let timeline = self
            .service
            .as_ref()
            .map(|service| service.timeline().clone())
            .or_else(|| self.pending_restore.clone())
            .unwrap_or_else(|| {
                AudioTimelineStateV1::new(CANONICAL_SAMPLE_RATE, CANONICAL_CHANNELS)
            });
        NativeVnProductAudioSnapshot {
            schema: "astra.audio_timeline.v1".into(),
            timeline,
            voice_kinds: self.voice_kinds.clone(),
            known_bgm_targets: self.known_bgm_targets.clone(),
            pending_fade_stops: self.pending_fade_stops.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: NativeVnProductAudioSnapshot) -> Result<(), PlatformError> {
        if snapshot.schema != "astra.audio_timeline.v1"
            || snapshot.timeline.schema != "astra.audio_timeline.v1"
            || snapshot.timeline.device_sample_rate != CANONICAL_SAMPLE_RATE
            || snapshot.timeline.device_channels != CANONICAL_CHANNELS
            || snapshot.voice_kinds.len() != snapshot.timeline.voices.len()
            || snapshot
                .voice_kinds
                .keys()
                .any(|voice_id| !snapshot.timeline.voices.contains_key(voice_id))
            || snapshot
                .pending_fade_stops
                .iter()
                .any(|(fade_id, pending)| {
                    pending.fence.is_empty()
                        || !snapshot.voice_kinds.contains_key(&pending.voice_id)
                        || !snapshot
                            .timeline
                            .buses
                            .values()
                            .any(|bus| bus.fade_id.as_deref() == Some(fade_id.as_str()))
                })
        {
            return Err(player_platform_error(
                "player.audio.restore",
                "ASTRA_PLAYER_AUDIO_TIMELINE_INVALID",
            ));
        }
        if let Some(service) = self.service.as_mut() {
            service
                .restore_timeline(snapshot.timeline.clone())
                .map_err(|error| player_platform_error("player.audio.restore", error))?;
        } else if !snapshot.timeline.voices.is_empty()
            || snapshot
                .timeline
                .buses
                .values()
                .any(|bus| bus.fade_id.is_some())
        {
            return Err(player_platform_error(
                "player.audio.restore",
                "ASTRA_PLAYER_AUDIO_RESTORE_REQUIRES_OPEN_SESSION",
            ));
        } else {
            self.pending_restore = Some(snapshot.timeline.clone());
        }
        self.voice_kinds = snapshot.voice_kinds;
        self.known_bgm_targets = snapshot.known_bgm_targets;
        self.pending_fade_stops = snapshot.pending_fade_stops;
        Ok(())
    }

    pub async fn ensure_open(
        &mut self,
        _source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        if self.service.is_some() {
            return Ok(());
        }
        let client = executor.sink().client().clone();
        let limits = client.launch_profile().limits();
        let capture_samples = self.retain_evidence_audio
            && matches!(client.launch_profile().kind(), HostKind::Headless);
        let opened = client
            .open_audio_output(AudioOutputRequest {
                sample_rate: CANONICAL_SAMPLE_RATE,
                channels: CANONICAL_CHANNELS,
                chunk_frames: limits.audio_chunk_frames,
                max_buffered_frames: Self::BUFFERED_FRAMES,
                start_paused: false,
                capture_samples,
            })
            .await?;
        if opened.format.sample_rate != CANONICAL_SAMPLE_RATE
            || opened.format.channels != CANONICAL_CHANNELS
        {
            return Err(player_platform_error(
                "player.audio.open",
                "ASTRA_PLAYER_AUDIO_OUTPUT_FORMAT_DRIFT",
            ));
        }
        let evidence_capture = opened.capture;
        if capture_samples && evidence_capture.is_none() {
            return Err(player_platform_error(
                "player.audio.capture",
                "ASTRA_PLAYER_AUDIO_CAPTURE_MISSING",
            ));
        }
        let output = opened.handle;
        let mut service = AudioServiceSession::new(
            AudioServiceConfig {
                max_voices: Self::MAX_VOICES,
                max_buses: Self::MAX_BUSES,
                max_events: Self::MAX_EVENTS,
                pcm_cache_bytes: limits.audio_pcm_cache_bytes,
            },
            AstraChunkBackendSettings {
                sample_rate: opened.format.sample_rate,
                channels: opened.format.channels,
                chunk_frames: limits.audio_chunk_frames,
                endpoint: opened.lane,
                deterministic_fixed_tick_hz: matches!(
                    client.launch_profile().kind(),
                    HostKind::Headless
                )
                .then_some(60),
            },
        )
        .map_err(|error| player_platform_error("player.audio.kira.create", error))?;
        for (asset, sample_rate, channels, samples) in self.pending_recovery_assets.drain(..) {
            service
                .prepare_pcm_shared(asset, sample_rate, channels, samples)
                .map_err(|error| player_platform_error("player.audio.recover.prepare", error))?;
        }
        if let Some(timeline) = self.pending_restore.take() {
            service
                .restore_timeline(timeline)
                .map_err(|error| player_platform_error("player.audio.restore", error))?;
        }
        self.output = Some(output);
        self.service = Some(service);
        self.evidence_capture = evidence_capture;
        self.previous_telemetry = AudioChunkTelemetry::default();
        Ok(())
    }

    /// Recreates the selected output endpoint and Kira manager without copying cached PCM.
    /// Failure is terminal for this audio host; no alternate mixer or output provider is selected.
    pub async fn recover_device_loss(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        let service = self.service.take().ok_or_else(|| {
            player_platform_error(
                "player.audio.recover",
                "ASTRA_PLAYER_AUDIO_RECOVERY_REQUIRES_OPEN_SESSION",
            )
        })?;
        self.pending_restore = Some(service.timeline().clone());
        self.pending_recovery_assets = service.prepared_pcm_assets();
        drop(service);
        self.evidence_capture = None;
        self.previous_telemetry = AudioChunkTelemetry::default();
        let output = self.output.take().ok_or_else(|| {
            player_platform_error(
                "player.audio.recover",
                "ASTRA_PLAYER_AUDIO_RECOVERY_OUTPUT_MISSING",
            )
        })?;
        executor.sink().client().close_audio(output).await?;
        self.ensure_open(source, executor).await
    }

    pub async fn start(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        request: &crate::NativeVnAudioRequest,
        audio: PlayerDecodedAudio,
        completed_signals: &mut BTreeSet<String>,
    ) -> Result<(), PlatformError> {
        let audio = audio
            .into_converted(
                CANONICAL_SAMPLE_RATE,
                CANONICAL_CHANNELS,
                Self::MAX_CONVERTED_SAMPLES,
            )
            .map_err(|error| player_platform_error("player.audio.convert", error))?;
        let asset = PcmAsset::from_canonical_samples(request.asset_id.clone(), audio.samples)
            .map_err(|error| player_platform_error("player.audio.asset", error))?;
        self.start_canonical(source, executor, request, asset, completed_signals)
            .await
    }

    pub async fn start_canonical(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        request: &crate::NativeVnAudioRequest,
        asset: PcmAsset,
        completed_signals: &mut BTreeSet<String>,
    ) -> Result<(), PlatformError> {
        if request.target_id.trim().is_empty() || asset.identity != request.asset_id {
            return Err(player_platform_error(
                "player.audio.start",
                "ASTRA_PLAYER_AUDIO_IDENTITY_INVALID",
            ));
        }
        self.ensure_open(source, executor).await?;
        let package_id = executor
            .sink()
            .client()
            .launch_profile()
            .package_id()
            .to_owned();
        let byte_len = u64::try_from(asset.samples.len())
            .ok()
            .and_then(|samples| samples.checked_mul(size_of::<f32>() as u64))
            .ok_or_else(|| player_platform_error("player.audio.asset", "PCM size overflowed"))?;
        let revision = AudioAssetRevision {
            package_id: package_id.clone(),
            uri: request.asset_id.clone(),
            revision: package_id,
            byte_len,
        };
        if let Some(existing) = self.prepared_assets.get(&request.asset_id) {
            if existing != &revision {
                return Err(player_platform_error(
                    "player.audio.asset",
                    "ASTRA_PLAYER_AUDIO_ASSET_REVISION_CONFLICT",
                ));
            }
        } else {
            self.service_mut()?
                .prepare_pcm_shared(
                    revision.clone(),
                    CANONICAL_SAMPLE_RATE,
                    CANONICAL_CHANNELS,
                    asset.samples,
                )
                .map_err(|error| player_platform_error("player.audio.prepare", error))?;
            self.prepared_assets
                .insert(request.asset_id.clone(), revision.clone());
        }

        let looping = parse_audio_bool(request, "loop", request.command == "bgm")?;
        let gain = parse_audio_f32(request, "gain", 1.0)?;
        let bus = request
            .attributes
            .get("bus")
            .cloned()
            .unwrap_or_else(|| request.command.clone());
        let voice_id = request.target_id.clone();
        completed_signals.remove(&voice_id);
        completed_signals.remove(&format!("{voice_id}.end"));
        if request.command == "voice" {
            completed_signals.remove("voice_end");
        }
        if let Some(fence) = request.attributes.get("fence") {
            completed_signals.remove(fence);
        }

        if let Some(fade_id) = self
            .service_ref()?
            .active_bus_fade_id(&bus)
            .map(str::to_owned)
        {
            if self.pending_fade_stops.contains_key(&fade_id) {
                return Err(player_platform_error(
                    "player.audio.start",
                    "ASTRA_PLAYER_AUDIO_START_DURING_FADE_STOP",
                ));
            }
            self.apply(AudioServiceCommand::CancelFade { fade_id })?;
        }
        if self.service_ref()?.voice_bus(&voice_id).is_some() {
            self.apply(AudioServiceCommand::Stop {
                voice_id: voice_id.clone(),
            })?;
            self.voice_kinds.remove(&voice_id).ok_or_else(|| {
                player_platform_error(
                    "player.audio.replace",
                    "ASTRA_PLAYER_AUDIO_REPLACEMENT_OWNER_MISSING",
                )
            })?;
        }

        let fade_frames = request
            .attributes
            .get("fade")
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| player_platform_error("player.audio.fade", "invalid fade"))
                    .and_then(duration_ms_to_frames)
            })
            .transpose()?;
        self.apply(AudioServiceCommand::SetBusGain {
            bus: bus.clone(),
            gain: if fade_frames.is_some_and(|frames| frames > 0) {
                0.0
            } else {
                gain
            },
        })?;
        self.apply(AudioServiceCommand::Play {
            voice_id: voice_id.clone(),
            bus: bus.clone(),
            asset: revision,
            start_frame: 0,
            looping,
        })?;
        if let Some(duration_frames) = fade_frames.filter(|frames| *frames > 0) {
            self.apply(AudioServiceCommand::FadeBus {
                fade_id: format!("fade.{}", request.command_id),
                bus,
                target_gain: gain,
                duration_frames,
            })?;
        }
        self.voice_kinds
            .insert(voice_id.clone(), request.command.clone());
        if request.command == "bgm" {
            self.known_bgm_targets.insert(voice_id);
        }
        Ok(())
    }

    pub async fn pump(
        &mut self,
        _source: &mut crate::NativeVnHostCommandSource,
        _executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
        completed_signals: &mut BTreeSet<String>,
        profile: bool,
    ) -> Result<NativeVnAudioPerformanceSample, PlatformError> {
        let Some(service) = self.service.as_mut() else {
            return Ok(NativeVnAudioPerformanceSample::default());
        };
        let completion_started = profile.then(Instant::now);
        service
            .poll_fixed_tick()
            .map_err(|error| player_platform_error("player.audio.poll", error))?;
        let telemetry = service.telemetry();
        self.last_meter = Some(NativeVnAudioMeterSnapshot::from(telemetry));
        let events = service.take_events();
        let mut fade_stops = Vec::new();
        for event in events {
            match event {
                AudioServiceEvent::VoiceCompleted { voice_id, .. } => {
                    let kind = self.voice_kinds.remove(&voice_id).ok_or_else(|| {
                        player_platform_error(
                            "player.audio.complete",
                            "completion owner is missing",
                        )
                    })?;
                    complete_voice(completed_signals, &voice_id, &kind);
                }
                AudioServiceEvent::FadeCompleted { fade_id, .. } => {
                    if let Some(pending) = self.pending_fade_stops.remove(&fade_id) {
                        fade_stops.push(pending);
                    } else if !fade_id.starts_with("fade.") {
                        return Err(player_platform_error(
                            "player.audio.fade.complete",
                            "completed fade has no owner",
                        ));
                    }
                }
                AudioServiceEvent::DeviceLost { .. } => {
                    return Err(player_platform_error(
                        "player.audio.device_lost",
                        "audio device was lost",
                    ));
                }
            }
        }
        for pending in fade_stops {
            self.apply(AudioServiceCommand::Stop {
                voice_id: pending.voice_id.clone(),
            })?;
            let kind = self.voice_kinds.remove(&pending.voice_id).ok_or_else(|| {
                player_platform_error("player.audio.fade_stop", "completion owner is missing")
            })?;
            complete_voice(completed_signals, &pending.voice_id, &kind);
            completed_signals.insert(pending.fence);
        }
        let performance = NativeVnAudioPerformanceSample {
            query_ns: 0,
            render_ns: telemetry
                .render_ns
                .saturating_sub(self.previous_telemetry.render_ns),
            submit_ns: telemetry
                .submit_wait_ns
                .saturating_sub(self.previous_telemetry.submit_wait_ns),
            completion_ns: elapsed_ns(completion_started, "player.audio.performance.completion")?,
        };
        self.previous_telemetry = telemetry;
        Ok(performance)
    }

    pub fn control(
        &mut self,
        request: &crate::NativeVnAudioControlRequest,
        completed_signals: &mut BTreeSet<String>,
    ) -> Result<(), PlatformError> {
        if matches!(request.action.as_str(), "enable_bus" | "disable_bus") {
            if !matches!(request.target.as_str(), "bgm" | "se")
                || request.duration_ms.is_some()
                || request.fence.is_some()
            {
                return Err(player_platform_error(
                    "player.audio.bus",
                    "ASTRA_PLAYER_AUDIO_BUS_CONTRACT",
                ));
            }
            if let Some(fade_id) = self
                .service_ref()?
                .active_bus_fade_id(&request.target)
                .map(str::to_owned)
            {
                self.apply(AudioServiceCommand::CancelFade { fade_id })?;
            }
            return self.apply(AudioServiceCommand::SetBusGain {
                bus: request.target.clone(),
                gain: if request.action == "enable_bus" {
                    1.0
                } else {
                    0.0
                },
            });
        }
        if request.action == "fade_stop" {
            let duration_frames = request
                .duration_ms
                .filter(|duration| *duration > 0)
                .map(u64::from)
                .ok_or_else(|| player_platform_error("player.audio.fade_stop", "duration missing"))
                .and_then(duration_ms_to_frames)?;
            let fence = request
                .fence
                .clone()
                .filter(|fence| !fence.is_empty())
                .ok_or_else(|| player_platform_error("player.audio.fade_stop", "fence missing"))?;
            match self.voice_kinds.get(&request.target).map(String::as_str) {
                Some("bgm") => {}
                None if self.known_bgm_targets.contains(&request.target) => {
                    complete_voice(completed_signals, &request.target, "bgm");
                    completed_signals.insert(fence);
                    return Ok(());
                }
                _ => {
                    return Err(player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_FADE_STOP_REQUIRES_BGM",
                    ));
                }
            }
            let bus = self
                .service_ref()?
                .voice_bus(&request.target)
                .map(str::to_owned)
                .ok_or_else(|| player_platform_error("player.audio.fade_stop", "voice missing"))?;
            if let Some(fade_id) = self
                .service_ref()?
                .active_bus_fade_id(&bus)
                .map(str::to_owned)
            {
                if self.pending_fade_stops.contains_key(&fade_id) {
                    return Err(player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_FADE_STOP_CONFLICT",
                    ));
                }
                self.apply(AudioServiceCommand::CancelFade { fade_id })?;
            }
            let fade_id = format!("fade-stop.{}", request.command_id);
            self.apply(AudioServiceCommand::FadeBus {
                fade_id: fade_id.clone(),
                bus,
                target_gain: 0.0,
                duration_frames,
            })?;
            self.pending_fade_stops.insert(
                fade_id,
                NativeVnPendingFadeStop {
                    voice_id: request.target.clone(),
                    fence,
                },
            );
            return Ok(());
        }
        if request.duration_ms.is_some() || request.fence.is_some() {
            return Err(player_platform_error(
                "player.audio.control",
                "ASTRA_PLAYER_AUDIO_CONTROL_UNEXPECTED_TIMING",
            ));
        }
        if request.action == "stop" && self.service_ref()?.voice_bus(&request.target).is_none() {
            if self.voice_kinds.contains_key(&request.target) {
                return Err(player_platform_error(
                    "player.audio.stop",
                    "ASTRA_PLAYER_AUDIO_STOP_OWNER_WITHOUT_VOICE",
                ));
            }
            completed_signals.insert(request.target.clone());
            completed_signals.insert(format!("{}.end", request.target));
            return Ok(());
        }
        let command = match request.action.as_str() {
            "pause" => AudioServiceCommand::Pause {
                voice_id: request.target.clone(),
            },
            "resume" => AudioServiceCommand::Resume {
                voice_id: request.target.clone(),
            },
            "stop" => AudioServiceCommand::Stop {
                voice_id: request.target.clone(),
            },
            _ => {
                return Err(player_platform_error(
                    "player.audio.control",
                    "ASTRA_PLAYER_AUDIO_CONTROL_UNSUPPORTED",
                ));
            }
        };
        let stopped_kind = (request.action == "stop")
            .then(|| self.voice_kinds.get(&request.target).cloned())
            .flatten();
        self.apply(command)?;
        if let Some(kind) = stopped_kind {
            self.voice_kinds.remove(&request.target);
            complete_voice(completed_signals, &request.target, &kind);
        }
        Ok(())
    }

    pub async fn shutdown(
        &mut self,
        _source: &mut crate::NativeVnHostCommandSource,
        executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    ) -> Result<(), PlatformError> {
        if let Some(service) = self.service.as_mut() {
            service
                .poll_backend()
                .map_err(|error| player_platform_error("player.audio.shutdown", error))?;
            self.last_meter = Some(NativeVnAudioMeterSnapshot::from(service.telemetry()));
        }
        drop(self.service.take());
        if let Some(output) = self.output.take() {
            executor.sink().client().close_audio(output).await?;
        }
        self.prepared_assets.clear();
        self.voice_kinds.clear();
        self.known_bgm_targets.clear();
        self.pending_fade_stops.clear();
        self.pending_restore = None;
        Ok(())
    }

    fn service_ref(&self) -> Result<&AudioServiceSession, PlatformError> {
        self.service
            .as_ref()
            .ok_or_else(|| player_platform_error("player.audio.service", "audio service is closed"))
    }

    fn service_mut(&mut self) -> Result<&mut AudioServiceSession, PlatformError> {
        self.service
            .as_mut()
            .ok_or_else(|| player_platform_error("player.audio.service", "audio service is closed"))
    }

    fn apply(&mut self, command: AudioServiceCommand) -> Result<(), PlatformError> {
        self.service_mut()?
            .apply(command)
            .map(|_| ())
            .map_err(|error| player_platform_error("player.audio.command", error))
    }
}

impl From<AudioChunkTelemetry> for NativeVnAudioMeterSnapshot {
    fn from(value: AudioChunkTelemetry) -> Self {
        Self {
            rendered_samples: value.rendered_samples,
            submitted_samples: value.submitted_samples,
            consumed_samples: value.consumed_samples,
            underflow_count: value.underflow_count,
            render_ns: value.render_ns,
            submit_wait_ns: value.submit_wait_ns,
        }
    }
}

fn complete_voice(completed: &mut BTreeSet<String>, voice_id: &str, kind: &str) {
    completed.insert(voice_id.to_owned());
    completed.insert(format!("{voice_id}.end"));
    if kind == "voice" {
        completed.insert("voice_end".into());
    }
}

fn duration_ms_to_frames(duration_ms: u64) -> Result<u64, PlatformError> {
    duration_ms
        .checked_mul(u64::from(CANONICAL_SAMPLE_RATE))
        .and_then(|frames| frames.checked_add(999))
        .map(|frames| frames / 1_000)
        .ok_or_else(|| player_platform_error("player.audio.duration", "duration overflowed"))
}

fn elapsed_ns(started: Option<Instant>, operation: &'static str) -> Result<u64, PlatformError> {
    let Some(started) = started else {
        return Ok(0);
    };
    u64::try_from(started.elapsed().as_nanos())
        .map_err(|_| player_platform_error(operation, "duration overflowed"))
}

fn parse_audio_bool(
    request: &crate::NativeVnAudioRequest,
    key: &str,
    default: bool,
) -> Result<bool, PlatformError> {
    match request.attributes.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        Some(_) => Err(player_platform_error(
            "player.audio.attribute",
            "ASTRA_PLAYER_AUDIO_BOOL_INVALID",
        )),
    }
}

fn parse_audio_f32(
    request: &crate::NativeVnAudioRequest,
    key: &str,
    default: f32,
) -> Result<f32, PlatformError> {
    let value = request
        .attributes
        .get(key)
        .map(|value| value.parse::<f32>())
        .transpose()
        .map_err(|_| {
            player_platform_error("player.audio.attribute", "ASTRA_PLAYER_AUDIO_FLOAT_INVALID")
        })?
        .unwrap_or(default);
    if !value.is_finite() || value < 0.0 {
        return Err(player_platform_error(
            "player.audio.attribute",
            "ASTRA_PLAYER_AUDIO_FLOAT_RANGE",
        ));
    }
    Ok(value)
}

fn player_platform_error(
    operation: &'static str,
    message: impl std::fmt::Display,
) -> PlatformError {
    PlatformError::new(
        astra_platform::PlatformErrorCode::IntegrityMismatch,
        operation,
        message.to_string(),
    )
}
