use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const AUDIO_TIMELINE_SCHEMA: &str = "astra.audio_timeline.v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct AudioAssetRevision {
    pub package_id: String,
    pub uri: String,
    pub revision: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AudioServiceCommand {
    Play {
        voice_id: String,
        bus: String,
        asset: AudioAssetRevision,
        start_frame: u64,
        looping: bool,
    },
    Stop {
        voice_id: String,
    },
    Pause {
        voice_id: String,
    },
    Resume {
        voice_id: String,
    },
    Seek {
        voice_id: String,
        frame: u64,
    },
    SetBusGain {
        bus: String,
        gain: f32,
    },
    FadeBus {
        fade_id: String,
        bus: String,
        target_gain: f32,
        duration_frames: u64,
    },
    CancelFade {
        fade_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioServiceEvent {
    VoiceCompleted { sequence: u64, voice_id: String },
    FadeCompleted { sequence: u64, fade_id: String },
    DeviceLost { sequence: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioVoiceState {
    pub command_sequence: u64,
    pub bus: String,
    pub asset: AudioAssetRevision,
    pub cursor_frames: u64,
    pub looping: bool,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioBusState {
    pub gain: f32,
    pub fade_id: Option<String>,
    pub fade_sequence: u64,
    pub fade_start_gain: Option<f32>,
    pub fade_target_gain: Option<f32>,
    pub fade_total_frames: u64,
    pub fade_rendered_frames: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioTimelineStateV1 {
    pub schema: String,
    pub command_sequence: u64,
    pub device_sample_rate: u32,
    pub device_channels: u16,
    pub consumed_frames: u64,
    pub voices: BTreeMap<String, AudioVoiceState>,
    pub buses: BTreeMap<String, AudioBusState>,
}

impl AudioTimelineStateV1 {
    #[must_use]
    pub fn new(device_sample_rate: u32, device_channels: u16) -> Self {
        Self {
            schema: AUDIO_TIMELINE_SCHEMA.into(),
            command_sequence: 0,
            device_sample_rate,
            device_channels,
            consumed_frames: 0,
            voices: BTreeMap::new(),
            buses: BTreeMap::new(),
        }
    }
}
