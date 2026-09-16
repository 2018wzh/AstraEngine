use crate::SourceSpan;
use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MINORI_RUNTIME_STATE_SCHEMA: &str = "astra.emu.minori.runtime_state.v7";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriRuntimeState {
    pub schema: String,
    pub script_uri: String,
    pub script_hash: Hash256,
    pub pc_line: u32,
    pub variables: BTreeMap<String, i64>,
    pub global_variables: BTreeMap<String, i64>,
    pub wait: Option<MinoriWaitState>,
    pub message: Option<MinoriMessageState>,
    pub choice: Option<MinoriChoiceState>,
    pub layers: BTreeMap<u32, MinoriLayerState>,
    pub transition: MinoriTransitionState,
    pub effect: Option<MinoriEffectState>,
    pub panel: Option<MinoriPanelState>,
    pub audio: BTreeMap<u32, MinoriAudioState>,
    pub movie: Option<MinoriMovieState>,
    pub system_ui: MinoriSystemUiState,
    pub fixed_tick: u64,
    pub session_seed: u64,
    pub random_state: u64,
    pub instruction_count: u64,
    pub effect_sequence: u64,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriWaitState {
    Time {
        token_id: String,
        timer_ticks: u32,
        milliseconds: u32,
    },
    Input {
        token_id: String,
    },
    Media {
        token_id: String,
        media_id: String,
    },
    Presentation {
        token_id: String,
        fence_id: String,
    },
    Provider {
        token_id: String,
        request_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriMessageState {
    pub source: SourceSpan,
    pub message_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriChoiceState {
    pub source: SourceSpan,
    pub selected_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriLayerState {
    pub resource_uri: String,
    pub x_milli: i32,
    pub y_milli: i32,
    pub scale_x_milli: i32,
    pub scale_y_milli: i32,
    pub opacity_milli: u16,
    pub blend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct MinoriTransitionState {
    pub mode: i32,
    pub resource: Option<String>,
    pub duration_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriEffectState {
    pub kind: MinoriEffectKind,
    pub resources: Vec<Option<String>>,
    pub current_index: u32,
    pub next_index: u32,
    pub alpha_255: u32,
    pub alpha_step: u32,
    pub interval_ms: u32,
    pub elapsed_ns: u64,
    pub visible_current_index: u32,
    pub visible_next_index: u32,
    pub visible_alpha_255: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriPanelState {
    pub mode: u32,
    pub resource_uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriEffectKind {
    CrossFade2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriEffectFrame {
    pub sequence: u64,
    pub current_resource_uri: Option<String>,
    pub next_resource_uri: Option<String>,
    pub alpha_255: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriStageCommand {
    pub foreground: Option<MinoriStageLayer>,
    pub background: Option<MinoriStageLayer>,
    pub stands: Vec<MinoriStandLayer>,
    pub transition: MinoriTransitionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriStageLayer {
    pub resource_uri: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriStandLayer {
    pub resource_uri: String,
    pub position: i32,
    pub offset: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriAudioState {
    pub bus: String,
    pub resource_uri: String,
    pub looped: bool,
    pub volume_milli: u16,
    pub pan_milli: i16,
    pub playing: bool,
    pub continuation_pts: u64,
}

/// Resource token accepted by the original BGM/SE path. The bracket suffix is
/// family metadata, not part of the archive entry name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriAudioResourceSpec {
    pub resource: String,
    pub volume_percent: u16,
    pub pan_percent: i16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriMovieState {
    pub media_id: String,
    pub resource_uri: String,
    pub width: u32,
    pub height: u32,
    pub skippable: bool,
    pub continuation_pts: u64,
    pub fence_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MinoriSystemPage {
    #[default]
    None,
    Title,
    Load,
    Save,
    Config,
    Backlog,
    GalleryCg,
    GalleryBgm,
    GalleryReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct MinoriSystemUiState {
    pub page: MinoriSystemPage,
    pub focus_index: u32,
    pub auto_mode: bool,
    pub skip_mode: bool,
    pub backlog_cursor: Option<u32>,
    pub pending_save_slot: Option<u32>,
    pub pending_load_slot: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MinoriVmEvent {
    Wait(MinoriWaitState),
    Message {
        presentation_sequence: u64,
        capture_sequence: u64,
        text: String,
        speaker: Option<String>,
        wait: MinoriWaitState,
    },
    Audio {
        commands: Vec<MinoriAudioCommand>,
    },
    Stage(MinoriStageCommand),
    Effect(MinoriEffectFrame),
    EffectCleared,
    Choice,
    Panel {
        sequence: u64,
    },
    Chain {
        target: String,
    },
    Terminal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MinoriAudioCommand {
    LoadResource {
        sequence: u64,
        stream_id: u32,
        resource_uri: String,
    },
    Play {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    },
    Stop {
        sequence: u64,
        stream_id: u32,
        fade_ms: u32,
    },
    SetParams {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
    },
}
