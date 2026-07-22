use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use astra_media::{
    AudioCommand, MixedTick, PcmAsset, PcmAssetResolver, ProductionAudioMixer,
    ProductionMixerSnapshot, CANONICAL_CHANNELS, CANONICAL_FRAMES_PER_TICK, CANONICAL_SAMPLE_RATE,
};
use astra_player_core::{PlayerDecodedAudio, PlayerHostCommandResult, PlayerMixedAudio};

pub struct NativeVnProductAudioHost {
    mixer: Option<ProductionAudioMixer>,
    assets: ProductPcmAssets,
    output: Option<astra_player_core::PlayerHostResourceId>,
    output_sample_rate: u32,
    output_channels: u16,
    next_packet_sequence: u64,
    voice_kinds: BTreeMap<String, String>,
    known_bgm_targets: BTreeSet<String>,
    pending_fade_stops: BTreeMap<String, NativeVnPendingFadeStop>,
    last_meter: Option<NativeVnAudioMeterSnapshot>,
    submitted_timeline: SubmittedAudioTimeline,
    retain_submitted_timeline: bool,
}

/// Retains Headless review audio without repeatedly relocating the full run.
///
/// A single growing `Vec` copies every previously submitted sample whenever its
/// capacity expands. On long 120 Hz runs that copy happens on the audio tick and
/// can consume an entire frame deadline. Fixed-size chunks keep append cost
/// bounded; the review artifact is materialized only when capture is requested.
#[derive(Default)]
struct SubmittedAudioTimeline {
    chunks: Vec<Vec<f32>>,
    sample_count: usize,
}

impl SubmittedAudioTimeline {
    const CHUNK_SAMPLES: usize = CANONICAL_FRAMES_PER_TICK * CANONICAL_CHANNELS as usize * 120;

    fn clear(&mut self) {
        self.chunks.clear();
        self.sample_count = 0;
    }

    fn extend_from_slice(&mut self, mut samples: &[f32]) {
        while !samples.is_empty() {
            let needs_chunk = self
                .chunks
                .last()
                .is_none_or(|chunk| chunk.len() == Self::CHUNK_SAMPLES);
            if needs_chunk {
                self.chunks.push(Vec::with_capacity(Self::CHUNK_SAMPLES));
            }
            let chunk = self
                .chunks
                .last_mut()
                .expect("a submitted-audio chunk was just allocated");
            let count = samples.len().min(Self::CHUNK_SAMPLES - chunk.len());
            chunk.extend_from_slice(&samples[..count]);
            self.sample_count = self.sample_count.saturating_add(count);
            samples = &samples[count..];
        }
    }

    fn to_vec(&self) -> Vec<f32> {
        let mut samples = Vec::with_capacity(self.sample_count);
        for chunk in &self.chunks {
            samples.extend_from_slice(chunk);
        }
        debug_assert_eq!(samples.len(), self.sample_count);
        samples
    }
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

#[derive(Default)]
struct ProductPcmAssets {
    assets: BTreeMap<String, PcmAsset>,
}

impl PcmAssetResolver for ProductPcmAssets {
    fn resolve_canonical(&self, asset: &str) -> Result<PcmAsset, astra_media::MediaError> {
        self.assets
            .get(asset)
            .cloned()
            .ok_or_else(|| astra_media::MediaError::message("ASTRA_PLAYER_AUDIO_ASSET_MISSING"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeVnAudioMeterSnapshot {
    pub callback_count: u64,
    pub submitted_samples: u64,
    pub consumed_samples: u64,
    pub underflow_count: u64,
    pub peak_dbfs_bits: u32,
    pub rms_dbfs_bits: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NativeVnProductAudioSnapshot {
    pub schema: String,
    pub mixer: Option<ProductionMixerSnapshot>,
    pub output: Option<u64>,
    pub next_packet_sequence: u64,
    pub voice_kinds: BTreeMap<String, String>,
    #[serde(default)]
    pub known_bgm_targets: BTreeSet<String>,
    pub pending_fade_stops: BTreeMap<String, NativeVnPendingFadeStop>,
    pub last_meter: Option<NativeVnAudioMeterSnapshot>,
    #[serde(default)]
    pub output_sample_rate: u32,
    #[serde(default)]
    pub output_channels: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeVnPendingFadeStop {
    pub voice_id: String,
    pub fence: String,
}

impl NativeVnProductAudioHost {
    const BUFFERED_FRAMES: u32 = 4_096;
    const MAX_VOICES: usize = 64;
    pub(crate) const MAX_CONVERTED_SAMPLES: usize = 20_000_000;

    pub fn new(retain_submitted_timeline: bool) -> Self {
        Self {
            mixer: None,
            assets: ProductPcmAssets::default(),
            output: None,
            output_sample_rate: 0,
            output_channels: 0,
            next_packet_sequence: 0,
            voice_kinds: BTreeMap::new(),
            known_bgm_targets: BTreeSet::new(),
            pending_fade_stops: BTreeMap::new(),
            last_meter: None,
            submitted_timeline: SubmittedAudioTimeline::default(),
            retain_submitted_timeline,
        }
    }

    pub fn is_active(&self) -> bool {
        self.mixer
            .as_ref()
            .is_some_and(|mixer| mixer.active_voice_count() > 0)
    }

    pub fn has_active_voice(&self) -> bool {
        self.voice_kinds.values().any(|kind| kind == "voice")
    }

    pub fn last_meter(&self) -> Option<NativeVnAudioMeterSnapshot> {
        self.last_meter
    }

    pub fn submitted_timeline(&self) -> Vec<f32> {
        self.submitted_timeline.to_vec()
    }

    pub fn snapshot(&self) -> NativeVnProductAudioSnapshot {
        NativeVnProductAudioSnapshot {
            schema: "astra.player.native_vn_audio_snapshot.v4".into(),
            mixer: self.mixer.as_ref().map(ProductionAudioMixer::snapshot),
            output: self.output.map(|output| output.0),
            next_packet_sequence: self.next_packet_sequence,
            voice_kinds: self.voice_kinds.clone(),
            known_bgm_targets: self.known_bgm_targets.clone(),
            pending_fade_stops: self.pending_fade_stops.clone(),
            last_meter: self.last_meter,
            output_sample_rate: self.output_sample_rate,
            output_channels: self.output_channels,
        }
    }

    pub fn restore(
        &mut self,
        snapshot: NativeVnProductAudioSnapshot,
    ) -> Result<(), astra_platform::PlatformError> {
        let has_mixer = snapshot.mixer.is_some();
        let has_output = snapshot.output.is_some();
        if !matches!(
            snapshot.schema.as_str(),
            "astra.player.native_vn_audio_snapshot.v2"
                | "astra.player.native_vn_audio_snapshot.v3"
                | "astra.player.native_vn_audio_snapshot.v4"
        ) || has_mixer != has_output
            || (has_mixer && snapshot.next_packet_sequence == 0)
            || (!has_mixer && !snapshot.voice_kinds.is_empty())
            || (!has_mixer && !snapshot.pending_fade_stops.is_empty())
            || snapshot.pending_fade_stops.values().any(|pending| {
                !snapshot.voice_kinds.contains_key(&pending.voice_id) || pending.fence.is_empty()
            })
            || (matches!(
                snapshot.schema.as_str(),
                "astra.player.native_vn_audio_snapshot.v3"
                    | "astra.player.native_vn_audio_snapshot.v4"
            ) && snapshot.voice_kinds.iter().any(|(voice_id, kind)| {
                kind == "bgm" && !snapshot.known_bgm_targets.contains(voice_id)
            }))
            || (snapshot.schema == "astra.player.native_vn_audio_snapshot.v4"
                && if has_output {
                    snapshot.output_sample_rate != CANONICAL_SAMPLE_RATE
                        || !(CANONICAL_CHANNELS..=32).contains(&snapshot.output_channels)
                } else {
                    snapshot.output_sample_rate != 0 || snapshot.output_channels != 0
                })
            || snapshot.pending_fade_stops.iter().any(|(fade_id, _)| {
                !snapshot.mixer.as_ref().is_some_and(|mixer| {
                    mixer
                        .buses
                        .values()
                        .any(|bus| bus.fade.as_ref().is_some_and(|fade| fade.id == *fade_id))
                })
            })
        {
            return Err(player_platform_error(
                "player.audio.restore",
                "ASTRA_PLAYER_AUDIO_SNAPSHOT_INVALID",
            ));
        }
        let mixer = snapshot
            .mixer
            .map(|snapshot| ProductionAudioMixer::restore(snapshot, &self.assets, Self::MAX_VOICES))
            .transpose()
            .map_err(|error| player_platform_error("player.audio.restore", error))?;
        let mut known_bgm_targets = snapshot.known_bgm_targets;
        known_bgm_targets.extend(
            snapshot
                .voice_kinds
                .iter()
                .filter(|(_, kind)| kind.as_str() == "bgm")
                .map(|(voice_id, _)| voice_id.clone()),
        );
        self.mixer = mixer;
        self.output = snapshot.output.map(astra_player_core::PlayerHostResourceId);
        let has_restored_output = self.output.is_some();
        self.output_sample_rate = match (has_restored_output, snapshot.output_sample_rate) {
            (false, _) => 0,
            (true, 0) => CANONICAL_SAMPLE_RATE,
            (true, sample_rate) => sample_rate,
        };
        self.output_channels = match (has_restored_output, snapshot.output_channels) {
            (false, _) => 0,
            (true, 0) => CANONICAL_CHANNELS,
            (true, channels) => channels,
        };
        // The mixer timeline is replay state, while packet sequence belongs to the
        // already-open host output and must never move backwards across load.
        self.next_packet_sequence = self.next_packet_sequence.max(snapshot.next_packet_sequence);
        self.voice_kinds = snapshot.voice_kinds;
        self.known_bgm_targets = known_bgm_targets;
        self.pending_fade_stops = snapshot.pending_fade_stops;
        self.last_meter = snapshot.last_meter;
        Ok(())
    }

    pub async fn ensure_open(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
    ) -> Result<(), astra_platform::PlatformError> {
        if self.mixer.is_some() {
            return Ok(());
        }
        let query = source
            .prepare_audio_output_format_query()
            .map_err(|error| player_platform_error("player.audio.format.prepare", error))?;
        let result = executor
            .execute_batch(query)
            .await
            .map_err(|error| player_platform_error("player.audio.format", error))?;
        let (output_sample_rate, output_channels) = match result.as_slice() {
            [PlayerHostCommandResult::AudioFormat {
                sample_rate,
                channels,
            }] if *sample_rate == CANONICAL_SAMPLE_RATE && *channels >= CANONICAL_CHANNELS => {
                (*sample_rate, *channels)
            }
            _ => {
                return Err(player_platform_error(
                    "player.audio.format",
                    "ASTRA_PLAYER_AUDIO_CANONICAL_RATE_OR_STEREO_OUTPUT_REQUIRED",
                ));
            }
        };
        if output_channels > 32 {
            return Err(player_platform_error(
                "player.audio.format",
                "ASTRA_PLAYER_AUDIO_OUTPUT_CHANNEL_LIMIT",
            ));
        }
        let (output, open) = source
            .prepare_persistent_audio_open(
                output_sample_rate,
                output_channels,
                Self::BUFFERED_FRAMES,
            )
            .map_err(|error| player_platform_error("player.audio.open.prepare", error))?;
        let opened = executor
            .execute_batch(open)
            .await
            .map_err(|error| player_platform_error("player.audio.open", error))?;
        if !matches!(opened.as_slice(), [PlayerHostCommandResult::AudioOpened { output: actual }] if *actual == output)
        {
            return Err(player_platform_error(
                "player.audio.open",
                "ASTRA_PLAYER_AUDIO_OPEN_RESULT",
            ));
        }
        self.output = Some(output);
        self.output_sample_rate = output_sample_rate;
        self.output_channels = output_channels;
        self.mixer = Some(
            ProductionAudioMixer::new(Self::MAX_VOICES)
                .map_err(|error| player_platform_error("player.audio.mixer.create", error))?,
        );
        self.next_packet_sequence = 1;
        self.submitted_timeline.clear();
        Ok(())
    }

    pub async fn start(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
        request: &crate::NativeVnAudioRequest,
        audio: PlayerDecodedAudio,
        completed_signals: &mut BTreeSet<String>,
    ) -> Result<(), astra_platform::PlatformError> {
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
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
        request: &crate::NativeVnAudioRequest,
        asset: PcmAsset,
        completed_signals: &mut BTreeSet<String>,
    ) -> Result<(), astra_platform::PlatformError> {
        let looping = parse_audio_bool(request, "loop", request.command == "bgm")?;
        let gain = parse_audio_f32(request, "gain", 1.0)?;
        let bus = request
            .attributes
            .get("bus")
            .cloned()
            .unwrap_or_else(|| request.command.clone());
        completed_signals.remove(&request.command_id);
        completed_signals.remove(&format!("{}.end", request.command_id));
        if request.command == "voice" {
            completed_signals.remove("voice_end");
        }
        if let Some(fence) = request.attributes.get("fence") {
            completed_signals.remove(fence);
        }
        self.ensure_open(source, executor).await?;
        let asset_id = request.asset_id.clone();
        if asset.identity != asset_id {
            return Err(player_platform_error(
                "player.audio.asset",
                "ASTRA_PLAYER_AUDIO_ASSET_IDENTITY_MISMATCH",
            ));
        }
        let frame_count = asset.frame_count();
        let duration_ms = (frame_count as u64)
            .checked_mul(1_000)
            .and_then(|value| value.checked_add(u64::from(CANONICAL_SAMPLE_RATE) - 1))
            .map(|value| value / u64::from(CANONICAL_SAMPLE_RATE))
            .ok_or_else(|| player_platform_error("player.audio.duration", "duration overflowed"))?;
        self.assets.assets.insert(asset_id.clone(), asset);
        let mixer = self
            .mixer
            .as_mut()
            .ok_or_else(|| player_platform_error("player.audio.mixer", "mixer is missing"))?;
        if let Some(fade_id) = mixer.active_bus_fade_id(&bus).map(str::to_owned) {
            if self.pending_fade_stops.contains_key(&fade_id) {
                return Err(player_platform_error(
                    "player.audio.replace",
                    "ASTRA_PLAYER_AUDIO_START_DURING_FADE_STOP",
                ));
            }
            mixer
                .apply(AudioCommand::CancelFade { fade_id }, &self.assets)
                .map_err(|error| player_platform_error("player.audio.replace.fade", error))?;
        }
        if let Some(existing_bus) = mixer.voice_bus(&request.command_id).map(str::to_owned) {
            mixer
                .apply(
                    AudioCommand::StopVoice {
                        voice_id: request.command_id.clone(),
                    },
                    &self.assets,
                )
                .map_err(|error| player_platform_error("player.audio.replace.stop", error))?;
            self.voice_kinds
                .remove(&request.command_id)
                .ok_or_else(|| {
                    player_platform_error(
                        "player.audio.replace",
                        "ASTRA_PLAYER_AUDIO_REPLACEMENT_OWNER_MISSING",
                    )
                })?;
            tracing::debug!(
                event = "astra.player.audio.voice_replaced",
                command_id = %request.command_id,
                bus = %existing_bus,
                "Player atomically replaced an active authored audio voice"
            );
        }
        mixer
            .apply(
                AudioCommand::SetBusGain {
                    bus: bus.clone(),
                    gain,
                },
                &self.assets,
            )
            .and_then(|_| {
                mixer.apply(
                    AudioCommand::PlayVoice {
                        voice_id: request.command_id.clone(),
                        bus: bus.clone(),
                        asset: asset_id,
                        duration_ms: duration_ms.max(1),
                        start_ms: 0,
                        looping,
                    },
                    &self.assets,
                )
            })
            .map_err(|error| player_platform_error("player.audio.start", error))?;
        if let Some(fade_ms) = request.attributes.get("fade") {
            let duration_ms = fade_ms.parse::<u64>().map_err(|_| {
                player_platform_error("player.audio.fade", "ASTRA_PLAYER_AUDIO_FADE_INVALID")
            })?;
            if duration_ms > 0 {
                mixer
                    .apply(
                        AudioCommand::SetBusGain {
                            bus: bus.clone(),
                            gain: 0.0,
                        },
                        &self.assets,
                    )
                    .and_then(|_| {
                        mixer.apply(
                            AudioCommand::FadeBus {
                                fade_id: format!("fade.{}", request.command_id),
                                bus,
                                target_gain: gain,
                                duration_ms,
                            },
                            &self.assets,
                        )
                    })
                    .map_err(|error| player_platform_error("player.audio.fade", error))?;
            }
        }
        self.voice_kinds
            .insert(request.command_id.clone(), request.command.clone());
        if request.command == "bgm" {
            self.known_bgm_targets.insert(request.command_id.clone());
        }
        tracing::debug!(
            event = "astra.player.audio.voice_bound",
            command_id = %request.command_id,
            frame_count,
            duration_ms,
            looping,
            "Player bound decoded PCM to the production mixer"
        );
        Ok(())
    }

    pub async fn pump(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
        completed_signals: &mut BTreeSet<String>,
        profile: bool,
    ) -> Result<NativeVnAudioPerformanceSample, astra_platform::PlatformError> {
        let mut performance = NativeVnAudioPerformanceSample::default();
        if self.output.is_none() {
            return Ok(performance);
        }
        let output = self
            .output
            .ok_or_else(|| player_platform_error("player.audio.pump", "output is missing"))?;
        let query_started = profile.then(Instant::now);
        let query = source
            .prepare_persistent_audio_query(output)
            .map_err(|error| player_platform_error("player.audio.query.prepare", error))?;
        let state = executor
            .execute_batch(query)
            .await
            .map_err(|error| player_platform_error("player.audio.query", error))?;
        let queued_frames = match state.as_slice() {
            [PlayerHostCommandResult::AudioState {
                output: actual,
                queued_frames,
                callback_count,
                submitted_samples,
                consumed_samples,
                underflow_count,
                peak_dbfs_bits,
                rms_dbfs_bits,
                ..
            }] if *actual == output => {
                self.last_meter = Some(NativeVnAudioMeterSnapshot {
                    callback_count: *callback_count,
                    submitted_samples: *submitted_samples,
                    consumed_samples: *consumed_samples,
                    underflow_count: *underflow_count,
                    peak_dbfs_bits: *peak_dbfs_bits,
                    rms_dbfs_bits: *rms_dbfs_bits,
                });
                *queued_frames
            }
            _ => {
                return Err(player_platform_error(
                    "player.audio.query",
                    "ASTRA_PLAYER_AUDIO_QUERY_RESULT",
                ))
            }
        };
        performance.query_ns = elapsed_ns(query_started, "player.audio.performance.query")?;
        let tick_frames = u64::try_from(CANONICAL_FRAMES_PER_TICK)
            .map_err(|_| player_platform_error("player.audio.pump", "tick frame overflowed"))?;
        if queued_frames.saturating_add(tick_frames) > u64::from(Self::BUFFERED_FRAMES) {
            tracing::trace!(
                event = "astra.player.audio.buffer_sufficient",
                queued_frames,
                buffered_frames = Self::BUFFERED_FRAMES,
                "Player deferred mixer advancement until the hardware queue has capacity"
            );
            return Ok(performance);
        }
        let render_started = profile.then(Instant::now);
        let mixed = self
            .mixer
            .as_mut()
            .ok_or_else(|| player_platform_error("player.audio.pump", "mixer is missing"))?
            .render_tick()
            .map_err(|error| player_platform_error("player.audio.render", error))?;
        if mixed.samples.len() != CANONICAL_FRAMES_PER_TICK * usize::from(CANONICAL_CHANNELS) {
            return Err(player_platform_error(
                "player.audio.render",
                "ASTRA_PLAYER_AUDIO_TICK_FRAME_COUNT",
            ));
        }
        let output_channels = self.output_channels;
        if self.output_sample_rate != CANONICAL_SAMPLE_RATE || output_channels < CANONICAL_CHANNELS
        {
            return Err(player_platform_error(
                "player.audio.render",
                "ASTRA_PLAYER_AUDIO_OUTPUT_FORMAT_STATE_INVALID",
            ));
        }
        let MixedTick {
            samples,
            completed_voices,
            ..
        } = mixed;
        if self.retain_submitted_timeline {
            self.submitted_timeline.extend_from_slice(&samples);
        }
        let samples = upmix_canonical_stereo(samples, output_channels)?;
        let packet = PlayerMixedAudio {
            sample_rate: self.output_sample_rate,
            channels: output_channels,
            samples,
            completed: Vec::new(),
        };
        performance.render_ns = elapsed_ns(render_started, "player.audio.performance.render")?;
        let submit_started = profile.then(Instant::now);
        let submit = source
            .prepare_persistent_audio_submit(output, self.next_packet_sequence, packet)
            .map_err(|error| player_platform_error("player.audio.submit.prepare", error))?;
        executor
            .execute_batch(submit)
            .await
            .map_err(|error| player_platform_error("player.audio.submit", error))?;
        performance.submit_ns = elapsed_ns(submit_started, "player.audio.performance.submit")?;
        let completion_started = profile.then(Instant::now);
        self.next_packet_sequence = self
            .next_packet_sequence
            .checked_add(1)
            .ok_or_else(|| player_platform_error("player.audio.submit", "sequence overflowed"))?;
        if self.next_packet_sequence.is_multiple_of(120) {
            tracing::trace!(
                event = "astra.player.audio.timeline_progress",
                packet_sequence = self.next_packet_sequence,
                active_voice_count = self
                    .mixer
                    .as_ref()
                    .map_or(0, ProductionAudioMixer::active_voice_count),
                "Player production mixer advanced the canonical audio timeline"
            );
        }
        for voice_id in completed_voices {
            let kind = self.voice_kinds.remove(&voice_id).ok_or_else(|| {
                player_platform_error("player.audio.complete", "completion owner is missing")
            })?;
            completed_signals.insert(voice_id.clone());
            completed_signals.insert(format!("{voice_id}.end"));
            if kind == "voice" {
                completed_signals.insert("voice_end".into());
            }
            tracing::debug!(
                event = "astra.player.audio.voice_completed",
                voice_id = %voice_id,
                kind,
                "Player production mixer completed a voice"
            );
        }
        for fade_id in mixed.completed_fades {
            let Some(pending) = self.pending_fade_stops.remove(&fade_id) else {
                if !fade_id.starts_with("fade.") {
                    return Err(player_platform_error(
                        "player.audio.fade.complete",
                        "completed fade has no recognized owner",
                    ));
                }
                tracing::debug!(
                    event = "astra.player.audio.start_fade_completed",
                    fade_id,
                    "Player completed an authored audio start fade"
                );
                continue;
            };
            self.mixer
                .as_mut()
                .ok_or_else(|| player_platform_error("player.audio.mixer", "mixer is missing"))?
                .apply(
                    AudioCommand::StopVoice {
                        voice_id: pending.voice_id.clone(),
                    },
                    &self.assets,
                )
                .map_err(|error| player_platform_error("player.audio.fade_stop.stop", error))?;
            let kind = self.voice_kinds.remove(&pending.voice_id).ok_or_else(|| {
                player_platform_error(
                    "player.audio.fade_stop.complete",
                    "completion owner is missing",
                )
            })?;
            completed_signals.insert(pending.voice_id.clone());
            completed_signals.insert(format!("{}.end", pending.voice_id));
            completed_signals.insert(pending.fence.clone());
            if kind == "voice" {
                completed_signals.insert("voice_end".into());
            }
            tracing::debug!(
                event = "astra.player.audio.fade_stop_completed",
                fade_id,
                voice_id = %pending.voice_id,
                fence = %pending.fence,
                "Player completed a sample-accurate fade-stop"
            );
        }
        performance.completion_ns =
            elapsed_ns(completion_started, "player.audio.performance.completion")?;
        Ok(performance)
    }

    pub fn control(
        &mut self,
        request: &crate::NativeVnAudioControlRequest,
        completed_signals: &mut BTreeSet<String>,
    ) -> Result<(), astra_platform::PlatformError> {
        if matches!(request.action.as_str(), "enable_bus" | "disable_bus") {
            if !matches!(request.target.as_str(), "bgm" | "se")
                || request.duration_ms.is_some()
                || request.fence.is_some()
            {
                return Err(player_platform_error(
                    "player.audio.bus_enabled",
                    "ASTRA_PLAYER_AUDIO_BUS_ENABLED_CONTRACT",
                ));
            }
            let mixer = self.mixer.as_mut().ok_or_else(|| {
                player_platform_error("player.audio.bus_enabled", "mixer is missing")
            })?;
            if let Some(active_fade_id) =
                mixer.active_bus_fade_id(&request.target).map(str::to_owned)
            {
                mixer
                    .apply(
                        AudioCommand::CancelFade {
                            fade_id: active_fade_id,
                        },
                        &self.assets,
                    )
                    .map_err(|error| {
                        player_platform_error("player.audio.bus_enabled.cancel_fade", error)
                    })?;
            }
            mixer
                .apply(
                    AudioCommand::set_bus_gain(
                        request.target.clone(),
                        if request.action == "enable_bus" {
                            1.0
                        } else {
                            0.0
                        },
                    ),
                    &self.assets,
                )
                .map_err(|error| player_platform_error("player.audio.bus_enabled", error))?;
            tracing::debug!(
                event = "astra.player.audio.bus_enabled",
                bus = %request.target,
                enabled = request.action == "enable_bus",
                "Player applied a typed VN audio bus enabled state"
            );
            return Ok(());
        }
        if request.action == "fade_stop" {
            let duration_ms = request
                .duration_ms
                .filter(|duration| *duration > 0)
                .ok_or_else(|| {
                    player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_FADE_STOP_DURATION",
                    )
                })?;
            let fence = request
                .fence
                .clone()
                .filter(|fence| !fence.is_empty())
                .ok_or_else(|| {
                    player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_FADE_STOP_FENCE",
                    )
                })?;
            match self.voice_kinds.get(&request.target).map(String::as_str) {
                Some("bgm") => {}
                Some(_) => {
                    return Err(player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_FADE_STOP_REQUIRES_BGM",
                    ));
                }
                None if self.known_bgm_targets.contains(&request.target) => {
                    completed_signals.insert(request.target.clone());
                    completed_signals.insert(format!("{}.end", request.target));
                    completed_signals.insert(fence.clone());
                    tracing::debug!(
                        event = "astra.player.audio.fade_stop_already_complete",
                        command_id = %request.command_id,
                        target = %request.target,
                        fence = %fence,
                        "Player preserved an authored idempotent BGM fade-stop"
                    );
                    return Ok(());
                }
                None => {
                    return Err(player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_CONTROL_TARGET_UNKNOWN",
                    ));
                }
            }
            let mixer = self
                .mixer
                .as_mut()
                .ok_or_else(|| player_platform_error("player.audio.control", "mixer is missing"))?;
            completed_signals.remove(&request.target);
            completed_signals.remove(&format!("{}.end", request.target));
            completed_signals.remove(&fence);
            let bus = mixer
                .voice_bus(&request.target)
                .map(str::to_string)
                .ok_or_else(|| {
                    player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_CONTROL_TARGET_UNKNOWN",
                    )
                })?;
            if let Some(active_fade_id) = mixer.active_bus_fade_id(&bus).map(str::to_owned) {
                if self.pending_fade_stops.contains_key(&active_fade_id) {
                    return Err(player_platform_error(
                        "player.audio.fade_stop",
                        "ASTRA_PLAYER_AUDIO_FADE_STOP_CONFLICT",
                    ));
                }
                mixer
                    .apply(
                        AudioCommand::CancelFade {
                            fade_id: active_fade_id,
                        },
                        &self.assets,
                    )
                    .map_err(|error| {
                        player_platform_error("player.audio.fade_stop.cancel_start", error)
                    })?;
            }
            let fade_id = format!("fade-stop.{}", request.command_id);
            if self.pending_fade_stops.contains_key(&fade_id) {
                return Err(player_platform_error(
                    "player.audio.fade_stop",
                    "ASTRA_PLAYER_AUDIO_FADE_STOP_DUPLICATE",
                ));
            }
            mixer
                .apply(
                    AudioCommand::FadeBus {
                        fade_id: fade_id.clone(),
                        bus,
                        target_gain: 0.0,
                        duration_ms: u64::from(duration_ms),
                    },
                    &self.assets,
                )
                .map_err(|error| player_platform_error("player.audio.fade_stop", error))?;
            tracing::debug!(
                event = "astra.player.audio.fade_stop_scheduled",
                command_id = %request.command_id,
                target = %request.target,
                duration_ms,
                fence = %fence,
                fade_id,
                "Player scheduled a sample-accurate fade-stop"
            );
            self.pending_fade_stops.insert(
                fade_id,
                NativeVnPendingFadeStop {
                    voice_id: request.target.clone(),
                    fence,
                },
            );
            return Ok(());
        }
        let mixer = self
            .mixer
            .as_mut()
            .ok_or_else(|| player_platform_error("player.audio.control", "mixer is missing"))?;
        if request.duration_ms.is_some() || request.fence.is_some() {
            return Err(player_platform_error(
                "player.audio.control",
                "ASTRA_PLAYER_AUDIO_CONTROL_UNEXPECTED_TIMING",
            ));
        }
        if request.action == "stop" && mixer.voice_bus(&request.target).is_none() {
            if self.voice_kinds.contains_key(&request.target) {
                return Err(player_platform_error(
                    "player.audio.stop",
                    "ASTRA_PLAYER_AUDIO_STOP_OWNER_WITHOUT_VOICE",
                ));
            }
            completed_signals.insert(request.target.clone());
            completed_signals.insert(format!("{}.end", request.target));
            tracing::debug!(
                event = "astra.player.audio.stop_already_complete",
                command_id = %request.command_id,
                target = %request.target,
                "Player preserved the authored idempotent stop state"
            );
            return Ok(());
        }
        let stopped_kind = if request.action == "stop" {
            Some(
                self.voice_kinds
                    .get(&request.target)
                    .cloned()
                    .ok_or_else(|| {
                        player_platform_error(
                            "player.audio.stop",
                            "ASTRA_PLAYER_AUDIO_STOP_VOICE_WITHOUT_OWNER",
                        )
                    })?,
            )
        } else {
            None
        };
        let command = match request.action.as_str() {
            "pause" => AudioCommand::PauseVoice {
                voice_id: request.target.clone(),
            },
            "resume" => AudioCommand::ResumeVoice {
                voice_id: request.target.clone(),
            },
            "stop" => AudioCommand::StopVoice {
                voice_id: request.target.clone(),
            },
            _ => {
                return Err(player_platform_error(
                    "player.audio.control",
                    "ASTRA_PLAYER_AUDIO_CONTROL_UNSUPPORTED",
                ))
            }
        };
        mixer
            .apply(command, &self.assets)
            .map_err(|error| player_platform_error("player.audio.control", error))?;
        if request.action == "stop" {
            self.voice_kinds.remove(&request.target);
            completed_signals.insert(request.target.clone());
            completed_signals.insert(format!("{}.end", request.target));
            if stopped_kind.as_deref() == Some("voice") {
                completed_signals.insert("voice_end".into());
            }
        }
        Ok(())
    }

    pub async fn shutdown(
        &mut self,
        source: &mut crate::NativeVnHostCommandSource,
        executor: &mut astra_player_core::PlayerHostCommandExecutor<
            astra_player_core::PlatformCommandSink,
        >,
    ) -> Result<(), astra_platform::PlatformError> {
        let Some(output) = self.output.take() else {
            return Ok(());
        };
        let drain = source
            .prepare_persistent_audio_drain(output)
            .map_err(|error| player_platform_error("player.audio.drain.prepare", error))?;
        let drained = executor
            .execute_batch(drain)
            .await
            .map_err(|error| player_platform_error("player.audio.drain", error))?;
        let (sample_count, peak_dbfs_bits, rms_dbfs_bits) = match drained.as_slice() {
            [PlayerHostCommandResult::AudioDrained {
                output: actual,
                sample_count,
                peak_dbfs_bits,
                rms_dbfs_bits,
            }] if *actual == output => (*sample_count, *peak_dbfs_bits, *rms_dbfs_bits),
            _ => {
                return Err(player_platform_error(
                    "player.audio.drain",
                    "ASTRA_PLAYER_AUDIO_DRAIN_RESULT",
                ));
            }
        };
        let previous = self.last_meter.unwrap_or(NativeVnAudioMeterSnapshot {
            callback_count: 0,
            submitted_samples: sample_count,
            consumed_samples: 0,
            underflow_count: 0,
            peak_dbfs_bits,
            rms_dbfs_bits,
        });
        self.last_meter = Some(NativeVnAudioMeterSnapshot {
            consumed_samples: sample_count,
            peak_dbfs_bits,
            rms_dbfs_bits,
            ..previous
        });
        let close = source
            .prepare_persistent_audio_close(output)
            .map_err(|error| player_platform_error("player.audio.close.prepare", error))?;
        let closed = executor
            .execute_batch(close)
            .await
            .map_err(|error| player_platform_error("player.audio.close", error))?;
        if !matches!(closed.as_slice(), [PlayerHostCommandResult::AudioClosed { output: actual }] if *actual == output)
        {
            return Err(player_platform_error(
                "player.audio.close",
                "ASTRA_PLAYER_AUDIO_CLOSE_RESULT",
            ));
        }
        self.mixer = None;
        self.output = None;
        self.output_sample_rate = 0;
        self.output_channels = 0;
        self.assets.assets.clear();
        self.voice_kinds.clear();
        self.known_bgm_targets.clear();
        Ok(())
    }
}

fn elapsed_ns(
    started: Option<Instant>,
    operation: &'static str,
) -> Result<u64, astra_platform::PlatformError> {
    let Some(started) = started else {
        return Ok(0);
    };
    u64::try_from(started.elapsed().as_nanos()).map_err(|_| {
        player_platform_error(
            operation,
            "ASTRA_PLAYER_AUDIO_PERFORMANCE_DURATION_OVERFLOW",
        )
    })
}

fn upmix_canonical_stereo(
    canonical: Vec<f32>,
    output_channels: u16,
) -> Result<Vec<f32>, astra_platform::PlatformError> {
    if output_channels < CANONICAL_CHANNELS
        || !canonical
            .len()
            .is_multiple_of(usize::from(CANONICAL_CHANNELS))
    {
        return Err(player_platform_error(
            "player.audio.upmix",
            "ASTRA_PLAYER_AUDIO_UPMIX_FORMAT_INVALID",
        ));
    }
    if output_channels == CANONICAL_CHANNELS {
        return Ok(canonical);
    }
    let frame_count = canonical.len() / usize::from(CANONICAL_CHANNELS);
    let sample_count = frame_count
        .checked_mul(usize::from(output_channels))
        .ok_or_else(|| {
            player_platform_error(
                "player.audio.upmix",
                "ASTRA_PLAYER_AUDIO_UPMIX_SIZE_OVERFLOW",
            )
        })?;
    let mut output = vec![0.0; sample_count];
    for (input, output) in canonical
        .chunks_exact(usize::from(CANONICAL_CHANNELS))
        .zip(output.chunks_exact_mut(usize::from(output_channels)))
    {
        output[0] = input[0];
        output[1] = input[1];
    }
    Ok(output)
}

fn parse_audio_bool(
    request: &crate::NativeVnAudioRequest,
    key: &str,
    default: bool,
) -> Result<bool, astra_platform::PlatformError> {
    match request.attributes.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(player_platform_error(
            "player.audio.command",
            "ASTRA_PLAYER_AUDIO_BOOL_INVALID",
        )),
    }
}

fn parse_audio_f32(
    request: &crate::NativeVnAudioRequest,
    key: &str,
    default: f32,
) -> Result<f32, astra_platform::PlatformError> {
    let value = match request.attributes.get(key) {
        Some(value) => value.parse::<f32>().map_err(|_| {
            player_platform_error("player.audio.command", "ASTRA_PLAYER_AUDIO_GAIN_INVALID")
        })?,
        None => default,
    };
    if !value.is_finite() || !(0.0..=4.0).contains(&value) {
        return Err(player_platform_error(
            "player.audio.command",
            "ASTRA_PLAYER_AUDIO_GAIN_INVALID",
        ));
    }
    Ok(value)
}

fn player_platform_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> astra_platform::PlatformError {
    astra_platform::PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        upmix_canonical_stereo, NativeVnProductAudioHost, SubmittedAudioTimeline,
        CANONICAL_SAMPLE_RATE,
    };

    #[astra_headless_test::test]
    fn submitted_audio_timeline_uses_bounded_chunks_without_changing_capture_order() {
        let mut timeline = SubmittedAudioTimeline::default();
        let input = (0..SubmittedAudioTimeline::CHUNK_SAMPLES + 17)
            .map(|sample| sample as f32)
            .collect::<Vec<_>>();

        timeline.extend_from_slice(&input[..31]);
        timeline.extend_from_slice(&input[31..]);

        assert_eq!(timeline.chunks.len(), 2);
        assert_eq!(
            timeline.chunks[0].len(),
            SubmittedAudioTimeline::CHUNK_SAMPLES
        );
        assert_eq!(timeline.chunks[1].len(), 17);
        assert_eq!(timeline.to_vec(), input);

        timeline.clear();
        assert!(timeline.chunks.is_empty());
        assert!(timeline.to_vec().is_empty());
    }

    #[astra_headless_test::test]
    fn canonical_stereo_upmix_preserves_front_channels_and_silences_surround_channels() {
        let output = upmix_canonical_stereo(vec![0.25, -0.5, 0.75, -1.0], 8).unwrap();

        assert_eq!(output.len(), 16);
        assert_eq!(&output[..8], &[0.25, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(&output[8..], &[0.75, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[astra_headless_test::test]
    fn version_four_snapshot_rejects_output_format_without_a_live_output() {
        let mut host = NativeVnProductAudioHost::new(false);
        let mut snapshot = host.snapshot();
        snapshot.output_sample_rate = CANONICAL_SAMPLE_RATE;

        let error = host.restore(snapshot).unwrap_err();

        assert_eq!(error.operation, "player.audio.restore");
    }

    #[astra_headless_test::test]
    fn legacy_empty_snapshot_restores_to_a_valid_version_four_snapshot() {
        let mut host = NativeVnProductAudioHost::new(false);
        let mut snapshot = host.snapshot();
        snapshot.schema = "astra.player.native_vn_audio_snapshot.v3".into();

        host.restore(snapshot).unwrap();

        let restored = host.snapshot();
        assert_eq!(restored.output, None);
        assert_eq!(restored.output_sample_rate, 0);
        assert_eq!(restored.output_channels, 0);
    }
}
