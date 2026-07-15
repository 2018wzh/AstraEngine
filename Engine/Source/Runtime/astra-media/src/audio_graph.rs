use std::collections::BTreeMap;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MediaError;

pub const AUDIO_GRAPH_SCHEMA: &str = "astra.audio_graph.v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioGraphConfig {
    pub max_buses: usize,
    pub max_voices: usize,
    pub max_fades: usize,
    pub max_completed_fences: usize,
    pub max_tick_delta_ms: u32,
}

impl AudioGraphConfig {
    pub const fn production_defaults() -> Self {
        Self {
            max_buses: 128,
            max_voices: 2048,
            max_fades: 128,
            max_completed_fences: 8192,
            max_tick_delta_ms: 1_000,
        }
    }
}

impl Default for AudioGraphConfig {
    fn default() -> Self {
        Self::production_defaults()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioCommand {
    SetBusGain {
        bus: String,
        gain: f32,
    },
    PlayVoice {
        voice_id: String,
        bus: String,
        asset: String,
        duration_ms: u64,
        start_ms: u64,
        looping: bool,
    },
    PauseVoice {
        voice_id: String,
    },
    ResumeVoice {
        voice_id: String,
    },
    SeekVoice {
        voice_id: String,
        position_ms: u64,
    },
    StopVoice {
        voice_id: String,
    },
    FadeBus {
        fade_id: String,
        bus: String,
        target_gain: f32,
        duration_ms: u64,
    },
    CancelFade {
        fade_id: String,
    },
}

impl AudioCommand {
    pub fn set_bus_gain(bus: impl Into<String>, gain: f32) -> Self {
        Self::SetBusGain {
            bus: bus.into(),
            gain,
        }
    }

    pub fn play_voice(
        voice_id: impl Into<String>,
        bus: impl Into<String>,
        asset: impl Into<String>,
        duration_ms: u64,
        looping: bool,
    ) -> Self {
        Self::PlayVoice {
            voice_id: voice_id.into(),
            bus: bus.into(),
            asset: asset.into(),
            duration_ms,
            start_ms: 0,
            looping,
        }
    }

    pub fn fade_bus(
        fade_id: impl Into<String>,
        bus: impl Into<String>,
        target_gain: f32,
        duration_ms: u64,
    ) -> Self {
        Self::FadeBus {
            fade_id: fade_id.into(),
            bus: bus.into(),
            target_gain,
            duration_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioGraph {
    schema: String,
    config: AudioGraphConfig,
    buses: BTreeMap<String, AudioBus>,
    voices: BTreeMap<String, AudioVoice>,
    fades: BTreeMap<String, AudioFade>,
    completed_fences: Vec<AudioFence>,
    tick: u64,
    elapsed_ms: u64,
    #[serde(default)]
    elapsed_ns: u64,
    #[serde(default)]
    sub_ms_ns: u32,
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new(AudioGraphConfig::production_defaults()).expect("default audio config is valid")
    }
}

impl AudioGraph {
    pub fn new(config: AudioGraphConfig) -> Result<Self, MediaError> {
        if config.max_buses == 0
            || config.max_voices == 0
            || config.max_fades == 0
            || config.max_completed_fences == 0
            || config.max_tick_delta_ms == 0
        {
            return Err(audio_error(
                "ASTRA_AUDIO_CONFIG",
                "every AudioGraph budget must be non-zero",
            ));
        }
        Ok(Self {
            schema: AUDIO_GRAPH_SCHEMA.into(),
            config,
            buses: BTreeMap::new(),
            voices: BTreeMap::new(),
            fades: BTreeMap::new(),
            completed_fences: Vec::new(),
            tick: 0,
            elapsed_ms: 0,
            elapsed_ns: 0,
            sub_ms_ns: 0,
        })
    }

    pub fn apply(&mut self, command: AudioCommand) -> Result<(), MediaError> {
        let mut next = self.clone();
        next.apply_inner(command)?;
        *self = next;
        Ok(())
    }

    fn apply_inner(&mut self, command: AudioCommand) -> Result<(), MediaError> {
        tracing::trace!(
            event = "media.audio_graph.command.apply",
            tick = self.tick,
            voice_count = self.voices.len(),
            fade_count = self.fades.len(),
            "audio graph command applied"
        );
        match command {
            AudioCommand::SetBusGain { bus, gain } => {
                validate_symbol(&bus, "ASTRA_AUDIO_BUS_ID")?;
                validate_gain(gain)?;
                if !self.buses.contains_key(&bus) && self.buses.len() == self.config.max_buses {
                    return Err(audio_error(
                        "ASTRA_AUDIO_BUS_BUDGET",
                        "audio bus count exceeds the configured budget",
                    ));
                }
                self.buses.insert(bus.clone(), AudioBus { id: bus, gain });
            }
            AudioCommand::PlayVoice {
                voice_id,
                bus,
                asset,
                duration_ms,
                start_ms,
                looping,
            } => {
                validate_symbol(&voice_id, "ASTRA_AUDIO_VOICE_ID")?;
                validate_symbol(&bus, "ASTRA_AUDIO_BUS_ID")?;
                validate_asset(&asset)?;
                if duration_ms == 0 || start_ms >= duration_ms {
                    return Err(audio_error(
                        "ASTRA_AUDIO_VOICE_RANGE",
                        "voice duration and start position are invalid",
                    ));
                }
                if self.voices.contains_key(&voice_id) {
                    return Err(audio_error(
                        "ASTRA_AUDIO_VOICE_DUPLICATE",
                        "voice id is already active",
                    ));
                }
                if self.voices.len() == self.config.max_voices {
                    return Err(audio_error(
                        "ASTRA_AUDIO_VOICE_BUDGET",
                        "active voice count exceeds the configured budget",
                    ));
                }
                if !self.buses.contains_key(&bus) {
                    if self.buses.len() == self.config.max_buses {
                        return Err(audio_error(
                            "ASTRA_AUDIO_BUS_BUDGET",
                            "audio bus count exceeds the configured budget",
                        ));
                    }
                    self.buses.insert(
                        bus.clone(),
                        AudioBus {
                            id: bus.clone(),
                            gain: 1.0,
                        },
                    );
                }
                self.voices.insert(
                    voice_id.clone(),
                    AudioVoice {
                        id: voice_id,
                        bus,
                        asset,
                        duration_ms,
                        position_ms: start_ms,
                        looping,
                        state: AudioVoiceState::Playing,
                        loop_count: 0,
                    },
                );
            }
            AudioCommand::PauseVoice { voice_id } => {
                let voice = self.voice_mut(&voice_id)?;
                if voice.state != AudioVoiceState::Playing {
                    return Err(audio_error(
                        "ASTRA_AUDIO_VOICE_STATE",
                        "only a playing voice can be paused",
                    ));
                }
                voice.state = AudioVoiceState::Paused;
            }
            AudioCommand::ResumeVoice { voice_id } => {
                let voice = self.voice_mut(&voice_id)?;
                if voice.state != AudioVoiceState::Paused {
                    return Err(audio_error(
                        "ASTRA_AUDIO_VOICE_STATE",
                        "only a paused voice can be resumed",
                    ));
                }
                voice.state = AudioVoiceState::Playing;
            }
            AudioCommand::SeekVoice {
                voice_id,
                position_ms,
            } => {
                let voice = self.voice_mut(&voice_id)?;
                if position_ms >= voice.duration_ms {
                    return Err(audio_error(
                        "ASTRA_AUDIO_VOICE_RANGE",
                        "seek position is outside the voice duration",
                    ));
                }
                voice.position_ms = position_ms;
            }
            AudioCommand::StopVoice { voice_id } => {
                validate_symbol(&voice_id, "ASTRA_AUDIO_VOICE_ID")?;
                let voice = self.voices.remove(&voice_id).ok_or_else(|| {
                    audio_error("ASTRA_AUDIO_VOICE_UNKNOWN", "voice id is not active")
                })?;
                self.push_fence("voice_stopped", &voice.id)?;
            }
            AudioCommand::FadeBus {
                fade_id,
                bus,
                target_gain,
                duration_ms,
            } => {
                validate_symbol(&fade_id, "ASTRA_AUDIO_FADE_ID")?;
                validate_symbol(&bus, "ASTRA_AUDIO_BUS_ID")?;
                validate_gain(target_gain)?;
                if duration_ms == 0 {
                    return Err(audio_error(
                        "ASTRA_AUDIO_FADE_RANGE",
                        "fade duration must be non-zero",
                    ));
                }
                if self.fades.contains_key(&fade_id) {
                    return Err(audio_error(
                        "ASTRA_AUDIO_FADE_DUPLICATE",
                        "fade id is already active",
                    ));
                }
                if self.fades.values().any(|fade| fade.bus == bus) {
                    return Err(audio_error(
                        "ASTRA_AUDIO_FADE_CONFLICT",
                        "an audio bus cannot have multiple authoritative fades",
                    ));
                }
                if self.fades.len() == self.config.max_fades {
                    return Err(audio_error(
                        "ASTRA_AUDIO_FADE_BUDGET",
                        "active fade count exceeds the configured budget",
                    ));
                }
                let start_gain = self.buses.get(&bus).map_or(1.0, |value| value.gain);
                self.fades.insert(
                    fade_id.clone(),
                    AudioFade {
                        id: fade_id,
                        bus,
                        start_gain,
                        target_gain,
                        duration_ms,
                        elapsed_ms: 0,
                    },
                );
            }
            AudioCommand::CancelFade { fade_id } => {
                validate_symbol(&fade_id, "ASTRA_AUDIO_FADE_ID")?;
                self.fades.remove(&fade_id).ok_or_else(|| {
                    audio_error("ASTRA_AUDIO_FADE_UNKNOWN", "fade id is not active")
                })?;
                self.push_fence("fade_cancelled", &fade_id)?;
            }
        }
        Ok(())
    }

    pub fn tick(&mut self, delta_ms: u32) -> Result<(), MediaError> {
        if delta_ms == 0 || delta_ms > self.config.max_tick_delta_ms {
            return Err(audio_error(
                "ASTRA_AUDIO_TICK_DELTA",
                "audio tick delta is outside the configured fixed-step budget",
            ));
        }
        self.tick_ns(u64::from(delta_ms) * 1_000_000)
    }

    pub fn tick_ns(&mut self, delta_ns: u64) -> Result<(), MediaError> {
        if delta_ns == 0 || delta_ns > u64::from(self.config.max_tick_delta_ms) * 1_000_000 {
            return Err(audio_error(
                "ASTRA_AUDIO_TICK_DELTA",
                "audio tick delta is outside the configured fixed-step budget",
            ));
        }
        let mut next = self.clone();
        next.tick = next
            .tick
            .checked_add(1)
            .ok_or_else(|| audio_error("ASTRA_AUDIO_TICK_OVERFLOW", "audio tick overflowed"))?;
        next.elapsed_ns = next.elapsed_ns.checked_add(delta_ns).ok_or_else(|| {
            audio_error("ASTRA_AUDIO_TIME_OVERFLOW", "audio elapsed time overflowed")
        })?;
        let accumulated_ns = u64::from(next.sub_ms_ns)
            .checked_add(delta_ns)
            .ok_or_else(|| {
                audio_error("ASTRA_AUDIO_TIME_OVERFLOW", "audio remainder overflowed")
            })?;
        let delta_ms = accumulated_ns / 1_000_000;
        next.sub_ms_ns = (accumulated_ns % 1_000_000) as u32;
        next.elapsed_ms = next.elapsed_ns / 1_000_000;
        if delta_ms != 0 {
            next.advance_fades(delta_ms)?;
            next.advance_voices(delta_ms)?;
        }
        *self = next;
        Ok(())
    }

    fn advance_fades(&mut self, delta_ms: u64) -> Result<(), MediaError> {
        let ids = self.fades.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let (bus, gain, complete) = {
                let fade = self.fades.get_mut(&id).ok_or_else(|| {
                    audio_error("ASTRA_AUDIO_GRAPH_STATE", "active fade disappeared")
                })?;
                fade.elapsed_ms = fade
                    .elapsed_ms
                    .saturating_add(delta_ms)
                    .min(fade.duration_ms);
                let progress = fade.elapsed_ms as f64 / fade.duration_ms as f64;
                let gain = f64::from(fade.start_gain)
                    + (f64::from(fade.target_gain) - f64::from(fade.start_gain)) * progress;
                (
                    fade.bus.clone(),
                    gain as f32,
                    fade.elapsed_ms == fade.duration_ms,
                )
            };
            self.buses
                .entry(bus.clone())
                .or_insert(AudioBus { id: bus, gain })
                .gain = gain;
            if complete {
                self.fades.remove(&id);
                self.push_fence("fade_completed", &id)?;
            }
        }
        Ok(())
    }

    fn advance_voices(&mut self, delta_ms: u64) -> Result<(), MediaError> {
        let ids = self.voices.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let complete = {
                let voice = self.voices.get_mut(&id).ok_or_else(|| {
                    audio_error("ASTRA_AUDIO_GRAPH_STATE", "active voice disappeared")
                })?;
                if voice.state == AudioVoiceState::Paused {
                    false
                } else {
                    let advanced = voice.position_ms.checked_add(delta_ms).ok_or_else(|| {
                        audio_error("ASTRA_AUDIO_TIME_OVERFLOW", "voice position overflowed")
                    })?;
                    if voice.looping {
                        let loops = advanced / voice.duration_ms;
                        voice.loop_count =
                            voice.loop_count.checked_add(loops).ok_or_else(|| {
                                audio_error(
                                    "ASTRA_AUDIO_LOOP_OVERFLOW",
                                    "voice loop count overflowed",
                                )
                            })?;
                        voice.position_ms = advanced % voice.duration_ms;
                        false
                    } else if advanced >= voice.duration_ms {
                        true
                    } else {
                        voice.position_ms = advanced;
                        false
                    }
                }
            };
            if complete {
                self.voices.remove(&id);
                self.push_fence("voice_completed", &id)?;
            }
        }
        Ok(())
    }

    fn voice_mut(&mut self, voice_id: &str) -> Result<&mut AudioVoice, MediaError> {
        validate_symbol(voice_id, "ASTRA_AUDIO_VOICE_ID")?;
        self.voices
            .get_mut(voice_id)
            .ok_or_else(|| audio_error("ASTRA_AUDIO_VOICE_UNKNOWN", "voice id is not active"))
    }

    fn push_fence(&mut self, kind: &str, resource_id: &str) -> Result<(), MediaError> {
        if self.completed_fences.len() == self.config.max_completed_fences {
            return Err(audio_error(
                "ASTRA_AUDIO_FENCE_BUDGET",
                "completed audio fence count exceeds the configured budget",
            ));
        }
        self.completed_fences.push(AudioFence {
            kind: kind.into(),
            resource_id: resource_id.into(),
            tick: self.tick,
            elapsed_ms: self.elapsed_ms,
        });
        Ok(())
    }

    pub fn completed_fences(&self) -> &[AudioFence] {
        &self.completed_fences
    }

    pub fn voices(&self) -> &BTreeMap<String, AudioVoice> {
        &self.voices
    }

    pub fn buses(&self) -> &BTreeMap<String, AudioBus> {
        &self.buses
    }

    pub fn deterministic_hash(&self) -> Result<Hash256, MediaError> {
        let payload = postcard::to_allocvec(self)
            .map_err(|error| MediaError::message(format!("serialize AudioGraph: {error}")))?;
        Ok(Hash256::from_sha256(&payload))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioBus {
    pub id: String,
    pub gain: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioVoiceState {
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioVoice {
    pub id: String,
    pub bus: String,
    pub asset: String,
    pub duration_ms: u64,
    pub position_ms: u64,
    pub looping: bool,
    pub state: AudioVoiceState,
    pub loop_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioFence {
    pub kind: String,
    pub resource_id: String,
    pub tick: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
struct AudioFade {
    id: String,
    bus: String,
    start_gain: f32,
    target_gain: f32,
    duration_ms: u64,
    elapsed_ms: u64,
}

fn validate_symbol(value: &str, code: &str) -> Result<(), MediaError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(audio_error(code, "audio identifier is invalid"));
    }
    Ok(())
}

fn validate_asset(asset: &str) -> Result<(), MediaError> {
    if !asset.starts_with("asset:/") || asset.contains("..") || asset.contains('\\') {
        return Err(audio_error(
            "ASTRA_AUDIO_ASSET",
            "audio voice asset must be an engine VFS URI",
        ));
    }
    Ok(())
}

fn validate_gain(gain: f32) -> Result<(), MediaError> {
    if !gain.is_finite() || !(0.0..=4.0).contains(&gain) {
        return Err(audio_error(
            "ASTRA_AUDIO_GAIN",
            "audio gain must be finite and between zero and four",
        ));
    }
    Ok(())
}

fn audio_error(code: &str, message: &str) -> MediaError {
    MediaError::message(format!("{code}: {message}"))
}
