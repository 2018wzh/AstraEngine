use std::collections::{BTreeMap, BTreeSet};

use astra_media::{
    AudioCommand, PcmAsset, PcmAssetResolver, ProductionAudioMixer, ProductionMixerSnapshot,
    CANONICAL_CHANNELS, CANONICAL_FRAMES_PER_TICK, CANONICAL_SAMPLE_RATE,
};
use astra_player_core::{PlayerDecodedAudio, PlayerHostCommandResult, PlayerMixedAudio};

#[derive(Default)]
pub struct NativeVnProductAudioHost {
    mixer: Option<ProductionAudioMixer>,
    assets: ProductPcmAssets,
    output: Option<astra_player_core::PlayerHostResourceId>,
    next_packet_sequence: u64,
    voice_kinds: BTreeMap<String, String>,
    last_meter: Option<NativeVnAudioMeterSnapshot>,
    submitted_timeline: Vec<f32>,
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
    pub last_meter: Option<NativeVnAudioMeterSnapshot>,
}

impl NativeVnProductAudioHost {
    const BUFFERED_FRAMES: u32 = 4_096;
    const MAX_VOICES: usize = 64;
    const MAX_CONVERTED_SAMPLES: usize = 20_000_000;

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

    pub fn submitted_timeline(&self) -> &[f32] {
        &self.submitted_timeline
    }

    pub fn snapshot(&self) -> NativeVnProductAudioSnapshot {
        NativeVnProductAudioSnapshot {
            schema: "astra.player.native_vn_audio_snapshot.v1".into(),
            mixer: self.mixer.as_ref().map(ProductionAudioMixer::snapshot),
            output: self.output.map(|output| output.0),
            next_packet_sequence: self.next_packet_sequence,
            voice_kinds: self.voice_kinds.clone(),
            last_meter: self.last_meter,
        }
    }

    pub fn restore(
        &mut self,
        snapshot: NativeVnProductAudioSnapshot,
    ) -> Result<(), astra_platform::PlatformError> {
        let has_mixer = snapshot.mixer.is_some();
        let has_output = snapshot.output.is_some();
        if snapshot.schema != "astra.player.native_vn_audio_snapshot.v1"
            || has_mixer != has_output
            || (has_mixer && snapshot.next_packet_sequence == 0)
            || (!has_mixer && !snapshot.voice_kinds.is_empty())
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
        self.mixer = mixer;
        self.output = snapshot.output.map(astra_player_core::PlayerHostResourceId);
        // The mixer timeline is replay state, while packet sequence belongs to the
        // already-open host output and must never move backwards across load.
        self.next_packet_sequence = self.next_packet_sequence.max(snapshot.next_packet_sequence);
        self.voice_kinds = snapshot.voice_kinds;
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
        if !matches!(
            result.as_slice(),
            [PlayerHostCommandResult::AudioFormat {
                sample_rate: CANONICAL_SAMPLE_RATE,
                channels: CANONICAL_CHANNELS
            }]
        ) {
            return Err(player_platform_error(
                "player.audio.format",
                "ASTRA_PLAYER_AUDIO_CANONICAL_FORMAT_REQUIRED",
            ));
        }
        let (output, open) = source
            .prepare_persistent_audio_open(
                CANONICAL_SAMPLE_RATE,
                CANONICAL_CHANNELS,
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
    ) -> Result<(), astra_platform::PlatformError> {
        let looping = parse_audio_bool(request, "loop", request.command == "bgm")?;
        let gain = parse_audio_f32(request, "gain", 1.0)?;
        let bus = request
            .attributes
            .get("bus")
            .cloned()
            .unwrap_or_else(|| request.command.clone());
        let audio = audio
            .convert_to(
                CANONICAL_SAMPLE_RATE,
                CANONICAL_CHANNELS,
                Self::MAX_CONVERTED_SAMPLES,
            )
            .map_err(|error| player_platform_error("player.audio.convert", error))?;
        self.ensure_open(source, executor).await?;
        let asset_id = request.asset_id.clone();
        let bytes = audio
            .samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let asset = PcmAsset::new(
            asset_id.clone(),
            astra_core::Hash256::from_sha256(&bytes),
            audio.samples,
        )
        .map_err(|error| player_platform_error("player.audio.asset", error))?;
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
        tracing::info!(
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
    ) -> Result<(), astra_platform::PlatformError> {
        if self.output.is_none() {
            return Ok(());
        }
        let output = self
            .output
            .ok_or_else(|| player_platform_error("player.audio.pump", "output is missing"))?;
        let query = source
            .prepare_persistent_audio_query(output)
            .map_err(|error| player_platform_error("player.audio.query.prepare", error))?;
        let state = executor
            .execute_batch(query)
            .await
            .map_err(|error| player_platform_error("player.audio.query", error))?;
        match state.as_slice() {
            [PlayerHostCommandResult::AudioState {
                output: actual,
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
            }
            _ => {
                return Err(player_platform_error(
                    "player.audio.query",
                    "ASTRA_PLAYER_AUDIO_QUERY_RESULT",
                ))
            }
        }
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
        let packet = PlayerMixedAudio {
            sample_rate: CANONICAL_SAMPLE_RATE,
            channels: CANONICAL_CHANNELS,
            samples: mixed.samples,
            completed: Vec::new(),
        };
        let submit = source
            .prepare_persistent_audio_submit(output, self.next_packet_sequence, &packet)
            .map_err(|error| player_platform_error("player.audio.submit.prepare", error))?;
        executor
            .execute_batch(submit)
            .await
            .map_err(|error| player_platform_error("player.audio.submit", error))?;
        self.submitted_timeline.extend_from_slice(&packet.samples);
        self.next_packet_sequence = self
            .next_packet_sequence
            .checked_add(1)
            .ok_or_else(|| player_platform_error("player.audio.submit", "sequence overflowed"))?;
        if self.next_packet_sequence % 120 == 0 {
            tracing::info!(
                event = "astra.player.audio.timeline_progress",
                packet_sequence = self.next_packet_sequence,
                active_voice_count = self
                    .mixer
                    .as_ref()
                    .map_or(0, ProductionAudioMixer::active_voice_count),
                "Player production mixer advanced the canonical audio timeline"
            );
        }
        for voice_id in mixed.completed_voices {
            let kind = self.voice_kinds.remove(&voice_id).ok_or_else(|| {
                player_platform_error("player.audio.complete", "completion owner is missing")
            })?;
            completed_signals.insert(voice_id.clone());
            completed_signals.insert(format!("{voice_id}.end"));
            if kind == "voice" {
                completed_signals.insert("voice_end".into());
            }
            tracing::info!(
                event = "astra.player.audio.voice_completed",
                voice_id = %voice_id,
                kind,
                "Player production mixer completed a voice"
            );
        }
        Ok(())
    }

    pub fn control(
        &mut self,
        request: &crate::NativeVnAudioControlRequest,
        completed_signals: &mut BTreeSet<String>,
    ) -> Result<(), astra_platform::PlatformError> {
        let mixer = self
            .mixer
            .as_mut()
            .ok_or_else(|| player_platform_error("player.audio.control", "mixer is missing"))?;
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
            let kind = self.voice_kinds.remove(&request.target).ok_or_else(|| {
                player_platform_error("player.audio.stop", "completion owner is missing")
            })?;
            completed_signals.insert(request.target.clone());
            completed_signals.insert(format!("{}.end", request.target));
            if kind == "voice" {
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
        self.assets.assets.clear();
        self.voice_kinds.clear();
        Ok(())
    }
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
