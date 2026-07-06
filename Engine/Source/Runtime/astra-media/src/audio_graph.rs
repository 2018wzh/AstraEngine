use std::collections::BTreeMap;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MediaError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioCommand {
    SetBusGain {
        bus: String,
        gain: f32,
    },
    PlayBgm {
        bus: String,
        asset: String,
        duration_ms: u64,
        looping: bool,
    },
    FadeBus {
        bus: String,
        target_gain: f32,
        ticks: u32,
    },
}

impl AudioCommand {
    pub fn set_bus_gain(bus: impl Into<String>, gain: f32) -> Self {
        Self::SetBusGain {
            bus: bus.into(),
            gain,
        }
    }

    pub fn play_bgm(
        bus: impl Into<String>,
        asset: impl Into<String>,
        duration_ms: u64,
        looping: bool,
    ) -> Self {
        Self::PlayBgm {
            bus: bus.into(),
            asset: asset.into(),
            duration_ms,
            looping,
        }
    }

    pub fn fade_bus(bus: impl Into<String>, target_gain: f32, ticks: u32) -> Self {
        Self::FadeBus {
            bus: bus.into(),
            target_gain,
            ticks,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioGraph {
    buses: BTreeMap<String, AudioBus>,
    voices: Vec<AudioVoice>,
    fades: Vec<AudioFade>,
    completed_fences: Vec<AudioFence>,
    tick: u64,
}

impl AudioGraph {
    pub fn apply(&mut self, command: AudioCommand) -> Result<(), MediaError> {
        match command {
            AudioCommand::SetBusGain { bus, gain } => {
                self.buses.insert(bus.clone(), AudioBus { id: bus, gain });
            }
            AudioCommand::PlayBgm {
                bus,
                asset,
                duration_ms,
                looping,
            } => {
                self.buses.entry(bus.clone()).or_insert_with(|| AudioBus {
                    id: bus.clone(),
                    gain: 1.0,
                });
                self.voices.push(AudioVoice {
                    bus,
                    asset,
                    duration_ms,
                    looping,
                });
            }
            AudioCommand::FadeBus {
                bus,
                target_gain,
                ticks,
            } => {
                let start_gain = self.buses.get(&bus).map_or(1.0, |bus| bus.gain);
                self.fades.push(AudioFade {
                    bus,
                    start_gain,
                    target_gain,
                    ticks_total: ticks.max(1),
                    ticks_done: 0,
                });
            }
        }
        Ok(())
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        let mut remaining = Vec::new();
        for mut fade in self.fades.drain(..) {
            fade.ticks_done += 1;
            let t = fade.ticks_done as f32 / fade.ticks_total as f32;
            let gain = fade.start_gain + (fade.target_gain - fade.start_gain) * t.min(1.0);
            self.buses
                .entry(fade.bus.clone())
                .or_insert_with(|| AudioBus {
                    id: fade.bus.clone(),
                    gain,
                })
                .gain = gain;
            if fade.ticks_done >= fade.ticks_total {
                self.completed_fences.push(AudioFence {
                    kind: "fade".to_string(),
                    bus: fade.bus,
                    tick: self.tick,
                });
            } else {
                remaining.push(fade);
            }
        }
        self.fades = remaining;
    }

    pub fn completed_fences(&self) -> &[AudioFence] {
        &self.completed_fences
    }

    pub fn voices(&self) -> &[AudioVoice] {
        &self.voices
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioBus {
    pub id: String,
    pub gain: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioVoice {
    pub bus: String,
    pub asset: String,
    pub duration_ms: u64,
    pub looping: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioFence {
    pub kind: String,
    pub bus: String,
    pub tick: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
struct AudioFade {
    bus: String,
    start_gain: f32,
    target_gain: f32,
    ticks_total: u32,
    ticks_done: u32,
}

#[derive(Debug, Clone, Default)]
pub struct AudioMeterProvider;

impl AudioMeterProvider {
    pub fn meter_hash(&self, graph: &AudioGraph) -> Hash256 {
        let payload = serde_json::to_vec(graph).expect("AudioGraph serializes");
        Hash256::from_sha256(&payload)
    }
}

#[cfg(feature = "desktop-audio")]
#[derive(Debug, Clone, Default)]
pub struct KiraAudioOutputProvider;

#[cfg(feature = "desktop-audio")]
impl KiraAudioOutputProvider {
    pub fn provider_id(&self) -> &'static str {
        let _settings = kira::AudioManagerSettings::default();
        "astra.audio.kira"
    }
}
