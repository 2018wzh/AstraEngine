use crate::SourceSpan;
use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MUSICA_RUNTIME_STATE_SCHEMA: &str = "astra.emu.musica.runtime_state.v15";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaRuntimeState {
    pub schema: String,
    pub script_uri: String,
    pub script_hash: Hash256,
    pub pc_line: u32,
    pub variables: BTreeMap<String, i64>,
    pub global_variables: BTreeMap<String, i64>,
    pub wait: Option<MusicaWaitState>,
    pub message: Option<MusicaMessageState>,
    pub choice: Option<MusicaChoiceState>,
    pub stage: Option<MusicaStageCommand>,
    pub transition: MusicaTransitionState,
    pub effect: Option<MusicaEffectState>,
    pub firefly: Option<MusicaFireflyState>,
    pub secondary_effect: Option<MusicaSecondaryEffectState>,
    pub wscroll2: Option<MusicaWScroll2State>,
    pub scroll_xf: Option<MusicaScrollXfState>,
    pub linear_scroll: Option<MusicaLinearScrollState>,
    pub axis_scroll: Option<MusicaAxisScrollState>,
    pub screen_shake: Option<MusicaScreenShakeState>,
    pub panel: Option<MusicaPanelState>,
    pub audio: BTreeMap<u32, MusicaAudioState>,
    pub movie: Option<MusicaMovieState>,
    pub system_ui: MusicaSystemUiState,
    pub fixed_tick: u64,
    pub session_seed: u64,
    pub random_state: u64,
    pub instruction_count: u64,
    pub effect_sequence: u64,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaWaitState {
    LinearScroll {
        token_id: String,
        milliseconds: u32,
    },
    AxisScroll {
        token_id: String,
        milliseconds: u32,
    },
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
pub struct MusicaMessageState {
    pub source: SourceSpan,
    pub message_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaChoiceState {
    pub source: SourceSpan,
    pub selected_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct MusicaTransitionState {
    pub mode: i32,
    pub resource: Option<String>,
    pub duration_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaEffectState {
    pub kind: MusicaEffectKind,
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
pub struct MusicaPanelState {
    pub mode: u32,
    pub resource_uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaEffectKind {
    CrossFade2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaEffectFrame {
    pub sequence: u64,
    pub current_resource_uri: Option<String>,
    pub next_resource_uri: Option<String>,
    pub alpha_255: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaStageCommand {
    pub resource_sequence: Vec<Option<String>>,
    pub reference_position: Option<[i32; 2]>,
    pub background: Option<MusicaStageLayer>,
    pub stands: Vec<MusicaStandLayer>,
    pub transition: MusicaTransitionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaStageLayer {
    pub resource_uri: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaStandLayer {
    pub resource_uri: String,
    pub position: i32,
    pub resource_parameter: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaAudioState {
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
pub struct MusicaAudioResourceSpec {
    pub resource: String,
    pub volume_percent: u16,
    pub pan_percent: i16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaMovieState {
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
pub enum MusicaSystemPage {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaSystemUiState {
    pub page: MusicaSystemPage,
    pub focus_index: u32,
    pub auto_mode: bool,
    pub skip_mode: bool,
    pub skip_enabled: bool,
    pub control_enabled: bool,
    pub backlog_cursor: Option<u32>,
    pub pending_save_slot: Option<u32>,
    pub pending_load_slot: Option<u32>,
}

impl Default for MusicaSystemUiState {
    fn default() -> Self {
        Self {
            page: MusicaSystemPage::default(),
            focus_index: 0,
            auto_mode: false,
            skip_mode: false,
            skip_enabled: true,
            control_enabled: false,
            backlog_cursor: None,
            pending_save_slot: None,
            pending_load_slot: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MusicaVmEvent {
    Wait(MusicaWaitState),
    Message {
        presentation_sequence: u64,
        capture_sequence: u64,
        text: String,
        speaker: Option<String>,
        wait: MusicaWaitState,
    },
    Audio {
        commands: Vec<MusicaAudioCommand>,
    },
    Stage(MusicaStageCommand),
    Effect(MusicaEffectFrame),
    EffectCleared,
    ScrollXf(MusicaScrollXfFrame),
    WScroll2(MusicaWScroll2Frame),
    Firefly(MusicaFireflyFrame),
    FireflyCleared {
        sequence: u64,
    },
    SecondaryEffect(MusicaSecondaryEffectFrame),
    SecondaryEffectCleared {
        sequence: u64,
    },
    LinearScroll(MusicaLinearScrollFrame),
    AxisScroll(MusicaAxisScrollFrame),
    ScreenShake(MusicaScreenShakeFrame),
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
pub enum MusicaAudioCommand {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaScreenShakeState {
    pub kind: MusicaScreenShakeKind,
    pub amplitude: i32,
    pub interval_ms: u32,
    pub elapsed_ns: u64,
    pub update_index: u64,
    pub offset: [i32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaScreenShakeKind {
    Random,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaScreenShakeFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaAxisScrollState {
    pub axis: MusicaAxisScrollAxis,
    pub start: i32,
    pub target: i32,
    /// Native scroll speed in tenths of a pixel per millisecond.
    pub speed_tenths: i32,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub current: i32,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaAxisScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaAxisScrollFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaLinearScrollState {
    pub start: [i32; 2],
    pub target: [i32; 2],
    pub speed_tenths: u32,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub current: [i32; 2],
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaLinearScrollFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaScrollXfState {
    pub start_extent: [i32; 2],
    pub end_extent: [i32; 2],
    pub start_offset: [i32; 2],
    pub end_offset: [i32; 2],
    pub duration_ms: u32,
    pub easing: u8,
    pub elapsed_ns: u64,
    pub completed: bool,
    pub visible_extent: [i32; 2],
    pub visible_offset: [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaScrollXfFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaWScroll2State {
    pub sync_resource_uri: String,
    pub period_ticks: u32,
    pub speed_tenths: i32,
    pub elapsed_ns: u64,
    pub elapsed_ticks: u64,
    pub foreground_offset: i64,
    pub background_offset: i64,
    pub background_remainder: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaWScroll2Frame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaFireflyState {
    pub resources: [String; 3],
    pub target_count: u32,
    pub duration_ms: u32,
    pub ending: bool,
    pub fade_alpha_256: u16,
    pub fade_elapsed_ns: u64,
    pub particles: Vec<MusicaFireflyParticle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaFireflyParticle {
    pub control_points: [[i32; 2]; 7],
    pub kind: u8,
    pub elapsed_ns: u64,
    pub lifetime_ns: u64,
    pub position: [i32; 2],
    pub opacity_255: u16,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaSecondaryEffectState {
    pub kind: MusicaSecondaryEffectKind,
    pub resources: [String; 3],
    pub ending: bool,
    pub alpha_256: u16,
    pub fade_elapsed_ns: u64,
    pub motion_elapsed_ns: u64,
    pub particles: Vec<MusicaSnowHParticle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaSecondaryEffectKind {
    SnowHorizontal,
    SnowVertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaSnowHParticle {
    pub fixed_position: [i64; 2],
    pub horizontal_velocity: u32,
    pub vertical_velocity: u32,
    pub vertical_positive: bool,
    pub kind: u8,
    pub position: [i32; 2],
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaFireflyFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaSecondaryEffectFrame {
    pub sequence: u64,
}
