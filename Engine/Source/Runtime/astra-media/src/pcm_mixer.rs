use std::{collections::BTreeMap, sync::Arc};

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AudioCommand, AudioGraph, MediaError};

pub const CANONICAL_SAMPLE_RATE: u32 = 48_000;
pub const CANONICAL_CHANNELS: u16 = 2;
pub const CANONICAL_FRAMES_PER_TICK: usize = 800;

#[derive(Debug, Clone)]
pub struct PcmAsset {
    pub identity: String,
    pub hash: Hash256,
    pub samples: Arc<[f32]>,
}

impl PcmAsset {
    pub fn new(
        identity: impl Into<String>,
        hash: Hash256,
        samples: Vec<f32>,
    ) -> Result<Self, MediaError> {
        validate_canonical_samples(&samples)?;
        if canonical_pcm_hash(&samples) != hash {
            return Err(invalid_pcm_asset());
        }
        Ok(Self {
            identity: identity.into(),
            hash,
            samples: samples.into(),
        })
    }

    pub fn from_canonical_samples(
        identity: impl Into<String>,
        samples: Vec<f32>,
    ) -> Result<Self, MediaError> {
        validate_canonical_samples(&samples)?;
        let hash = canonical_pcm_hash(&samples);
        Ok(Self {
            identity: identity.into(),
            hash,
            samples: samples.into(),
        })
    }

    pub fn with_identity(&self, identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            hash: self.hash,
            samples: Arc::clone(&self.samples),
        }
    }
    pub fn frame_count(&self) -> usize {
        self.samples.len() / usize::from(CANONICAL_CHANNELS)
    }
}

fn validate_canonical_samples(samples: &[f32]) -> Result<(), MediaError> {
    if samples.is_empty()
        || !samples
            .len()
            .is_multiple_of(usize::from(CANONICAL_CHANNELS))
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(invalid_pcm_asset());
    }
    Ok(())
}

fn invalid_pcm_asset() -> MediaError {
    mixer_error(
        "ASTRA_AUDIO_ASSET_INVALID",
        "canonical PCM asset is empty, misaligned, non-finite, or hash-mismatched",
    )
}

fn canonical_pcm_hash(samples: &[f32]) -> Hash256 {
    let mut hasher = Sha256::new();
    for sample in samples {
        hasher.update(sample.to_le_bytes());
    }
    Hash256::from_bytes(hasher.finalize().into())
}

pub trait PcmAssetResolver {
    fn resolve_canonical(&self, asset: &str) -> Result<PcmAsset, MediaError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProductionMixerSnapshot {
    pub schema: String,
    pub graph: AudioGraph,
    pub voices: Vec<MixerVoiceSnapshot>,
    pub buses: BTreeMap<String, MixerBusSnapshot>,
    pub rendered_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MixerVoiceSnapshot {
    pub id: String,
    pub bus: String,
    pub asset: String,
    pub asset_hash: String,
    pub cursor_frames: u64,
    pub looping: bool,
    pub paused: bool,
    pub gain: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MixerBusSnapshot {
    pub gain: f32,
    pub fade: Option<MixerFadeSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MixerFadeSnapshot {
    pub id: String,
    pub start: f32,
    pub target: f32,
    pub total_frames: u64,
    pub rendered_frames: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MixedTick {
    pub samples: Vec<f32>,
    pub completed_voices: Vec<String>,
    pub completed_fades: Vec<String>,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

pub struct ProductionAudioMixer {
    graph: AudioGraph,
    voices: BTreeMap<String, Voice>,
    buses: BTreeMap<String, Bus>,
    rendered_ticks: u64,
    max_voices: usize,
}
struct Voice {
    bus: String,
    asset: PcmAsset,
    cursor: usize,
    looping: bool,
    paused: bool,
    gain: f32,
}
struct Bus {
    gain: f32,
    fade: Option<Fade>,
}
struct Fade {
    id: String,
    start: f32,
    target: f32,
    total_frames: u64,
    rendered_frames: u64,
}

impl ProductionAudioMixer {
    pub fn new(max_voices: usize) -> Result<Self, MediaError> {
        if max_voices == 0 {
            return Err(mixer_error(
                "ASTRA_AUDIO_MIXER_BUDGET",
                "mixer voice budget must be non-zero",
            ));
        }
        Ok(Self {
            graph: AudioGraph::default(),
            voices: BTreeMap::new(),
            buses: BTreeMap::new(),
            rendered_ticks: 0,
            max_voices,
        })
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.len()
    }

    pub fn voice_bus(&self, voice_id: &str) -> Option<&str> {
        self.voices.get(voice_id).map(|voice| voice.bus.as_str())
    }

    pub fn active_bus_fade_id(&self, bus: &str) -> Option<&str> {
        self.buses
            .get(bus)
            .and_then(|state| state.fade.as_ref())
            .map(|fade| fade.id.as_str())
    }

    pub fn apply(
        &mut self,
        command: AudioCommand,
        resolver: &dyn PcmAssetResolver,
    ) -> Result<(), MediaError> {
        let mut graph = self.graph.clone();
        graph.apply(command.clone())?;
        match &command {
            AudioCommand::SetBusGain { bus, gain } => {
                self.buses.insert(
                    bus.clone(),
                    Bus {
                        gain: *gain,
                        fade: None,
                    },
                );
            }
            AudioCommand::PlayVoice {
                voice_id,
                bus,
                asset,
                start_ms,
                looping,
                ..
            } => {
                if self.voices.len() >= self.max_voices {
                    return Err(mixer_error(
                        "ASTRA_AUDIO_MIXER_VOICE_BUDGET",
                        "mixer voice budget exceeded",
                    ));
                }
                let pcm = resolver.resolve_canonical(asset)?;
                let cursor = frames_from_ms(*start_ms)?;
                if cursor >= pcm.frame_count() {
                    return Err(mixer_error(
                        "ASTRA_AUDIO_MIXER_ASSET_RANGE",
                        "voice start is outside decoded asset",
                    ));
                }
                self.buses.entry(bus.clone()).or_insert(Bus {
                    gain: 1.0,
                    fade: None,
                });
                self.voices.insert(
                    voice_id.clone(),
                    Voice {
                        bus: bus.clone(),
                        asset: pcm,
                        cursor,
                        looping: *looping,
                        paused: false,
                        gain: 1.0,
                    },
                );
            }
            AudioCommand::PauseVoice { voice_id } => self.voice_mut(voice_id)?.paused = true,
            AudioCommand::ResumeVoice { voice_id } => self.voice_mut(voice_id)?.paused = false,
            AudioCommand::SeekVoice {
                voice_id,
                position_ms,
            } => {
                let cursor = frames_from_ms(*position_ms)?;
                let voice = self.voice_mut(voice_id)?;
                if cursor >= voice.asset.frame_count() {
                    return Err(mixer_error(
                        "ASTRA_AUDIO_MIXER_ASSET_RANGE",
                        "seek is outside decoded asset",
                    ));
                }
                voice.cursor = cursor;
            }
            AudioCommand::StopVoice { voice_id } => {
                self.voices.remove(voice_id).ok_or_else(|| {
                    mixer_error("ASTRA_AUDIO_MIXER_VOICE_UNKNOWN", "voice is not active")
                })?;
            }
            AudioCommand::FadeBus {
                fade_id,
                bus,
                target_gain,
                duration_ms,
            } => {
                let total_frames = u64::try_from(frames_from_ms(*duration_ms)?).map_err(|_| {
                    mixer_error(
                        "ASTRA_AUDIO_MIXER_FADE_RANGE",
                        "fade frame count overflowed",
                    )
                })?;
                let state = self.buses.entry(bus.clone()).or_insert(Bus {
                    gain: 1.0,
                    fade: None,
                });
                state.fade = Some(Fade {
                    id: fade_id.clone(),
                    start: state.gain,
                    target: *target_gain,
                    total_frames,
                    rendered_frames: 0,
                });
            }
            AudioCommand::CancelFade { fade_id } => {
                let bus = self
                    .buses
                    .values_mut()
                    .find(|bus| bus.fade.as_ref().is_some_and(|fade| fade.id == *fade_id))
                    .ok_or_else(|| {
                        mixer_error(
                            "ASTRA_AUDIO_MIXER_FADE_UNKNOWN",
                            "mixer fade id is not active",
                        )
                    })?;
                bus.fade = None;
            }
        }
        self.graph = graph;
        Ok(())
    }

    pub fn render_tick(&mut self) -> Result<MixedTick, MediaError> {
        let mut samples = vec![0.0_f32; CANONICAL_FRAMES_PER_TICK * 2];
        let mut completed = Vec::new();
        let mut completed_fades = Vec::new();
        for frame in 0..CANONICAL_FRAMES_PER_TICK {
            self.advance_fades(&mut completed_fades)?;
            let ids = self.voices.keys().cloned().collect::<Vec<_>>();
            let mut frame_completed = Vec::new();
            for id in ids {
                let voice = self.voices.get_mut(&id).ok_or_else(|| {
                    mixer_error("ASTRA_AUDIO_MIXER_STATE", "voice disappeared during render")
                })?;
                if voice.paused {
                    continue;
                }
                let bus_gain = self.buses.get(&voice.bus).map_or(1.0, |bus| bus.gain);
                let source = voice.cursor * 2;
                let target = frame * 2;
                samples[target] += voice.asset.samples[source] * voice.gain * bus_gain;
                samples[target + 1] += voice.asset.samples[source + 1] * voice.gain * bus_gain;
                voice.cursor += 1;
                if voice.cursor == voice.asset.frame_count() {
                    if voice.looping {
                        voice.cursor = 0;
                    } else {
                        frame_completed.push(id);
                    }
                }
            }
            for id in frame_completed {
                self.voices.remove(&id);
                completed.push(id);
            }
        }
        for sample in &mut samples {
            *sample = sample.clamp(-1.0, 1.0);
        }
        self.rendered_ticks = self.rendered_ticks.checked_add(1).ok_or_else(|| {
            mixer_error(
                "ASTRA_AUDIO_MIXER_TICK_OVERFLOW",
                "rendered tick counter overflowed",
            )
        })?;
        self.graph.tick_ns(16_666_667)?;
        let peak = samples
            .iter()
            .fold(0.0_f32, |value, sample| value.max(sample.abs()));
        let rms = (samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / samples.len() as f64)
            .sqrt() as f32;
        completed.sort();
        completed.dedup();
        completed_fades.sort();
        completed_fades.dedup();
        Ok(MixedTick {
            samples,
            completed_voices: completed,
            completed_fades,
            peak_dbfs: db(peak),
            rms_dbfs: db(rms),
        })
    }

    pub fn snapshot(&self) -> ProductionMixerSnapshot {
        ProductionMixerSnapshot {
            schema: "astra.production_audio_mixer_snapshot.v1".into(),
            graph: self.graph.clone(),
            rendered_ticks: self.rendered_ticks,
            voices: self
                .voices
                .iter()
                .map(|(id, voice)| MixerVoiceSnapshot {
                    id: id.clone(),
                    bus: voice.bus.clone(),
                    asset: voice.asset.identity.clone(),
                    asset_hash: voice.asset.hash.to_string(),
                    cursor_frames: voice.cursor as u64,
                    looping: voice.looping,
                    paused: voice.paused,
                    gain: voice.gain,
                })
                .collect(),
            buses: self
                .buses
                .iter()
                .map(|(id, bus)| {
                    (
                        id.clone(),
                        MixerBusSnapshot {
                            gain: bus.gain,
                            fade: bus.fade.as_ref().map(|fade| MixerFadeSnapshot {
                                id: fade.id.clone(),
                                start: fade.start,
                                target: fade.target,
                                total_frames: fade.total_frames,
                                rendered_frames: fade.rendered_frames,
                            }),
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn restore(
        snapshot: ProductionMixerSnapshot,
        resolver: &dyn PcmAssetResolver,
        max_voices: usize,
    ) -> Result<Self, MediaError> {
        if snapshot.schema != "astra.production_audio_mixer_snapshot.v1"
            || snapshot.voices.len() > max_voices
        {
            return Err(mixer_error(
                "ASTRA_AUDIO_MIXER_SNAPSHOT_INVALID",
                "mixer snapshot schema or budget is invalid",
            ));
        }
        let mut voices = BTreeMap::new();
        for state in snapshot.voices {
            let asset = resolver.resolve_canonical(&state.asset)?;
            if asset.hash.to_string() != state.asset_hash {
                return Err(mixer_error(
                    "ASTRA_AUDIO_MIXER_ASSET_HASH",
                    "restored audio asset hash changed",
                ));
            }
            let cursor = usize::try_from(state.cursor_frames).map_err(|_| {
                mixer_error("ASTRA_AUDIO_MIXER_CURSOR", "voice cursor overflows host")
            })?;
            if cursor >= asset.frame_count() {
                return Err(mixer_error(
                    "ASTRA_AUDIO_MIXER_CURSOR",
                    "voice cursor is outside asset",
                ));
            }
            if voices
                .insert(
                    state.id,
                    Voice {
                        bus: state.bus,
                        asset,
                        cursor,
                        looping: state.looping,
                        paused: state.paused,
                        gain: state.gain,
                    },
                )
                .is_some()
            {
                return Err(mixer_error(
                    "ASTRA_AUDIO_MIXER_VOICE_DUPLICATE",
                    "snapshot repeats voice id",
                ));
            }
        }
        let buses = snapshot
            .buses
            .into_iter()
            .map(|(id, bus)| {
                (
                    id,
                    Bus {
                        gain: bus.gain,
                        fade: bus.fade.map(|fade| Fade {
                            id: fade.id,
                            start: fade.start,
                            target: fade.target,
                            total_frames: fade.total_frames,
                            rendered_frames: fade.rendered_frames,
                        }),
                    },
                )
            })
            .collect();
        Ok(Self {
            graph: snapshot.graph,
            voices,
            buses,
            rendered_ticks: snapshot.rendered_ticks,
            max_voices,
        })
    }

    fn voice_mut(&mut self, id: &str) -> Result<&mut Voice, MediaError> {
        self.voices
            .get_mut(id)
            .ok_or_else(|| mixer_error("ASTRA_AUDIO_MIXER_VOICE_UNKNOWN", "voice is not active"))
    }
    fn advance_fades(&mut self, completed: &mut Vec<String>) -> Result<(), MediaError> {
        for bus in self.buses.values_mut() {
            if let Some(fade) = &mut bus.fade {
                fade.rendered_frames = fade
                    .rendered_frames
                    .checked_add(1)
                    .ok_or_else(|| {
                        mixer_error("ASTRA_AUDIO_MIXER_FADE_OVERFLOW", "fade cursor overflowed")
                    })?
                    .min(fade.total_frames);
                let progress = fade.rendered_frames as f32 / fade.total_frames as f32;
                bus.gain = fade.start + (fade.target - fade.start) * progress;
                if fade.rendered_frames == fade.total_frames {
                    bus.gain = fade.target;
                    completed.push(fade.id.clone());
                    bus.fade = None;
                }
            }
        }
        Ok(())
    }
}

fn frames_from_ms(ms: u64) -> Result<usize, MediaError> {
    usize::try_from(
        ms.checked_mul(u64::from(CANONICAL_SAMPLE_RATE))
            .and_then(|v| v.checked_div(1000))
            .ok_or_else(|| {
                mixer_error(
                    "ASTRA_AUDIO_MIXER_TIME_OVERFLOW",
                    "audio time overflows frame count",
                )
            })?,
    )
    .map_err(|_| {
        mixer_error(
            "ASTRA_AUDIO_MIXER_TIME_OVERFLOW",
            "audio frame count overflows host",
        )
    })
}
fn db(value: f32) -> f32 {
    if value == 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * value.log10()
    }
}
fn mixer_error(code: &'static str, message: &'static str) -> MediaError {
    MediaError::message(format!("{code}: {message}"))
}
