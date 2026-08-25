use std::collections::BTreeMap;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    script::tokenize_operands, ScCommand, ScControlFlow, ScLineKind, ScOperand, ScScript,
    SourceSpan,
};

pub const MINORI_RUNTIME_STATE_SCHEMA: &str = "astra.emu.minori.runtime_state.v23";

const MINORI_BACKLOG_MAX_ENTRIES: usize = 16_384;
const MINORI_BACKLOG_MAX_ENTRY_BYTES: usize = 64 * 1024;
const MINORI_BACKLOG_MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;

const MINORI_FIREFLY_STAGE_WIDTH: i32 = 1280;
const MINORI_FIREFLY_STAGE_HEIGHT: i32 = 720;
const MINORI_FIREFLY_CONTROL_POINTS: usize = 7;
const MINORI_FIREFLY_MAX_PARTICLES: usize = 256;
const MINORI_FIREFLY_MAX_PARTICLES_U32: u32 = 256;
const MINORI_FIREFLY_MAX_DURATION_MS: u32 = 60_000;
const MINORI_FIREFLY_FADE_SCALE: u16 = 256;
const MINORI_FIREFLY_FADE_STEP_NS: u64 = 16_000_000;
const MINORI_SNOW_H_PARTICLE_COUNT: usize = 50;
const MINORI_SNOW_H_FADE_SCALE: u16 = 256;
const MINORI_SNOW_H_FADE_STEP_NS: u64 = 16_000_000;
const MINORI_SNOW_H_STAGE_WIDTH: i32 = 1280;
const MINORI_SNOW_H_STAGE_HEIGHT: i32 = 720;
const MINORI_SCREEN_SHAKE_MAX_AMPLITUDE: i32 = 1280;
const MINORI_SCREEN_SHAKE_MAX_INTERVAL_MS: u32 = 60_000;
const MINORI_SCROLL_XF_MAX_EXTENT: i32 = 16_384;
const MINORI_SCROLL_XF_MAX_DURATION_MS: u32 = 60_000;
const MINORI_SCROLL_XF_SCALE: u64 = 1_000_000;
pub(crate) const MINORI_ROUTE_CLEAR_FLAGS: [&str; 4] =
    ["TOHKA_CLEAR", "AYAME_CLEAR", "SUI_CLEAR", "REN_CLEAR"];
const MINORI_WSCROLL2_TICKS_PER_SECOND: u64 = 60;
const MINORI_WSCROLL2_MAX_PERIOD_TICKS: u32 = 60_000;
const MINORI_WSCROLL2_MAX_SPEED_TENTHS: i32 = 10_000;
const MINORI_CHARACTER_MAX_SLOTS: usize = 64;
const MINORI_CHARACTER_MAX_SLOT_ID: u32 = 4096;
const MINORI_CHARACTER_RESOURCE_COUNT: usize = 1;
const MINORI_CHARACTER_MAX_COORDINATE: i32 = 65_536;
const MINORI_CHARACTER_MAX_TRANSITION_MS: u32 = 60_000;
const MINORI_AXIS_SCROLL_MAX_COORDINATE: i32 = 65_536;
const MINORI_AXIS_SCROLL_MAX_SPEED_TENTHS: i32 = 10_000;
const MINORI_CONFIG_TEST_BGM_STREAM_ID: u32 = 0xffff_ff00;
const MINORI_CONFIG_TEST_VOICE_STREAM_ID: u32 = 0xffff_ff01;
const MINORI_CONFIG_TEST_SE_STREAM_ID: u32 = 0xffff_ff02;
const MINORI_SAVE_PAGE_COUNT: u32 = 10;
pub(crate) const MINORI_BGM_STREAM_ID: u32 = 0;

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
    pub backlog: Vec<MinoriBacklogEntry>,
    pub backlog_bytes: u64,
    pub choice: Option<MinoriChoiceState>,
    pub stage: Option<MinoriStageCommand>,
    pub characters: BTreeMap<u32, MinoriCharacterState>,
    pub axis_scroll: Option<MinoriAxisScrollState>,
    pub linear_scroll: Option<MinoriLinearScrollState>,
    pub scroll_xf: Option<MinoriScrollXfState>,
    pub wscroll2: Option<MinoriWScroll2State>,
    pub transition: MinoriTransitionState,
    pub effect: Option<MinoriEffectState>,
    pub firefly: Option<MinoriFireflyState>,
    pub secondary_effect: Option<MinoriSecondaryEffectState>,
    pub screen_shake: Option<MinoriScreenShakeState>,
    pub panel: Option<MinoriPanelState>,
    pub audio: BTreeMap<u32, MinoriAudioState>,
    pub movie: Option<MinoriMovieState>,
    pub launch_mode: MinoriLaunchMode,
    pub system_ui: MinoriSystemUiState,
    pub gallery_unlocks: Vec<Hash256>,
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
    AxisScroll {
        token_id: String,
        milliseconds: u32,
    },
    LinearScroll {
        token_id: String,
        milliseconds: u32,
    },
    CharacterTransition {
        token_id: String,
        slot_id: u32,
        milliseconds: u32,
    },
    Input {
        token_id: String,
    },
    Choice {
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
    pub text_hash: Hash256,
    pub speaker_hash: Option<Hash256>,
    pub voice_hash: Option<Hash256>,
    pub voice: Option<MinoriMessageVoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriMessageVoice {
    pub resource_uri: String,
    pub volume_milli: u16,
    pub pan_milli: i16,
}

/// Local-private dialogue history retained by the runtime snapshot. Plaintext is
/// required to reproduce the original backlog after restore, but never enters
/// evidence, reports, diagnostics, or logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriBacklogEntry {
    pub source: SourceSpan,
    pub message_id: i64,
    pub text: String,
    pub speaker: Option<String>,
    pub text_hash: Hash256,
    pub speaker_hash: Option<Hash256>,
    pub voice_hash: Option<Hash256>,
    pub voice: Option<MinoriMessageVoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriChoiceState {
    pub source: SourceSpan,
    pub option_hashes: Vec<Hash256>,
    pub targets: Vec<String>,
    pub selected_index: Option<u32>,
}

pub const MINORI_CHOICE_PRESENTATION_SCHEMA: &str = "astra.emu.minori.choice.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriChoicePresentation {
    pub schema: String,
    pub option_hashes: Vec<Hash256>,
    pub selected_index: u32,
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
pub struct MinoriFireflyState {
    pub resources: [String; 3],
    pub target_count: u32,
    pub duration_ms: u32,
    pub ending: bool,
    pub fade_alpha_256: u16,
    pub fade_elapsed_ns: u64,
    pub particles: Vec<MinoriFireflyParticle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriFireflyParticle {
    pub control_points: [[i32; 2]; MINORI_FIREFLY_CONTROL_POINTS],
    pub kind: u8,
    pub elapsed_ns: u64,
    pub lifetime_ns: u64,
    pub position: [i32; 2],
    pub opacity_255: u16,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriSecondaryEffectState {
    pub kind: MinoriSecondaryEffectKind,
    pub resources: [String; 3],
    pub ending: bool,
    pub alpha_256: u16,
    pub fade_elapsed_ns: u64,
    pub motion_elapsed_ns: u64,
    pub particles: Vec<MinoriSnowHParticle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriSecondaryEffectKind {
    SnowHorizontal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriSnowHParticle {
    pub fixed_position: [i64; 2],
    pub horizontal_velocity: u32,
    pub vertical_velocity: u32,
    pub vertical_positive: bool,
    pub kind: u8,
    pub position: [i32; 2],
    pub active: bool,
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
pub struct MinoriFireflyFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriSecondaryEffectFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriScreenShakeState {
    pub kind: MinoriScreenShakeKind,
    pub amplitude: i32,
    pub interval_ms: u32,
    pub elapsed_ns: u64,
    pub update_index: u64,
    pub offset: [i32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriScreenShakeKind {
    Random,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriScreenShakeFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriStageCommand {
    pub resource_sequence: Vec<Option<String>>,
    pub reference_position: Option<[i32; 2]>,
    pub background: Option<MinoriStageLayer>,
    pub stands: Vec<MinoriStandLayer>,
    pub transition: MinoriTransitionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriStageLayer {
    pub resource_uri: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriStandLayer {
    pub resource_uri: String,
    pub position: i32,
    pub resource_parameter: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriCharacterState {
    /// Original character manager key. The native engine normalizes signed
    /// command ids with `abs` before lookup.
    pub slot_id: u32,
    /// The sign captured by `.char load`. Native CCharLayer stores it on both
    /// sprite nodes and later position commands retain it.
    pub positive_orientation: bool,
    pub resource_uris: Vec<String>,
    /// Native CCharLayer anchor: horizontal center and bottom-relative Y.
    pub anchor_position: [i32; 2],
    pub visible: bool,
    pub opacity_256: u16,
    pub transition: Option<MinoriCharacterTransitionState>,
    /// Native `CCharLayer` one-shot retention flag. `.char keep` sets it;
    /// scene finalization consumes it when deciding which previous-scene
    /// characters survive into the next scene.
    pub keep_once: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriCharacterTransitionState {
    pub start_opacity_256: u16,
    pub target_opacity_256: u16,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriCharacterFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriAxisScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriAxisScrollState {
    pub axis: MinoriAxisScrollAxis,
    pub start: i32,
    pub target: i32,
    /// Native scroll speed in tenths of a pixel per millisecond.
    pub speed_tenths: i32,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub current: i32,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriAxisScrollFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriLinearScrollState {
    pub start: [i32; 2],
    pub target: [i32; 2],
    pub speed_tenths: u32,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub current: [i32; 2],
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriLinearScrollFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriScrollXfState {
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
pub struct MinoriScrollXfFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriWScroll2State {
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
pub struct MinoriWScroll2Frame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriAudioState {
    pub bus: String,
    pub encoding: MinoriAudioEncoding,
    pub resource_uri: String,
    pub looped: bool,
    pub volume_milli: u16,
    pub pan_milli: i16,
    pub playing: bool,
    pub continuation_pts: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriAudioEncoding {
    Ogg,
    Wav,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MinoriLaunchMode {
    #[default]
    DirectEntry,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MinoriSystemPage {
    #[default]
    None,
    Title,
    Load,
    Save,
    Config,
    Backlog,
    Memories,
    GalleryCg,
    GalleryBgm,
    GalleryReplay,
    GalleryMovie,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinoriPlayMode {
    #[default]
    Normal,
    Auto,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriSystemUiState {
    pub page: MinoriSystemPage,
    pub focus_index: u32,
    pub play_mode: MinoriPlayMode,
    pub config: MinoriConfigState,
    pub config_draft: Option<MinoriConfigState>,
    /// Script-owned permission corresponding to the original
    /// `skip_enable`/`skip_disable` pragma state. It defaults to enabled at
    /// scene construction and remains independent from Control input gating.
    pub skip_enabled: bool,
    /// The original Control fast path is explicitly gated by script pragmas.
    /// Keep the physical key state separate from the effective skip state so a
    /// held key survives fixed-tick boundaries without enabling the feature.
    pub control_enabled: bool,
    pub control_pressed: bool,
    pub pointer_x: i32,
    pub pointer_y: i32,
    pub pointer_primary_pressed: bool,
    pub backlog_cursor: Option<u32>,
    pub pending_save_slot: Option<u32>,
    pub pending_load_slot: Option<u32>,
}

impl Default for MinoriSystemUiState {
    fn default() -> Self {
        Self {
            page: MinoriSystemPage::None,
            focus_index: 0,
            play_mode: MinoriPlayMode::Normal,
            config: MinoriConfigState::default(),
            config_draft: None,
            skip_enabled: true,
            control_enabled: false,
            control_pressed: false,
            pointer_x: 0,
            pointer_y: 0,
            pointer_primary_pressed: false,
            backlog_cursor: None,
            pending_save_slot: None,
            pending_load_slot: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriConfigState {
    pub message_speed_unread: u8,
    pub message_speed_read: u8,
    /// Original `messageSpeedAutoPlay` setting in 10 ms units.
    pub message_speed_auto_play: u8,
    pub font_index: u32,
    pub preferred_play_mode: MinoriPlayMode,
    pub fullscreen: bool,
    pub screen_effect: bool,
    pub animation: bool,
    pub text_shadow: bool,
    pub backlog_voice_playback: bool,
    pub stop_voice_at_next_message: bool,
    pub progress_in_background: bool,
    pub bgm_volume: u8,
    pub voice_volume: u8,
    pub se_volume: u8,
    pub bgm_muted: bool,
    pub voice_muted: bool,
    pub se_muted: bool,
    /// Original order: ren, sui, aya, tou, etc.
    pub character_voice_enabled: [bool; 5],
}

impl Default for MinoriConfigState {
    fn default() -> Self {
        Self {
            message_speed_unread: 50,
            message_speed_read: 50,
            message_speed_auto_play: 50,
            font_index: 0,
            preferred_play_mode: MinoriPlayMode::Auto,
            fullscreen: false,
            screen_effect: true,
            animation: true,
            text_shadow: true,
            backlog_voice_playback: true,
            stop_voice_at_next_message: false,
            progress_in_background: false,
            bgm_volume: 100,
            voice_volume: 100,
            se_volume: 100,
            bgm_muted: false,
            voice_muted: false,
            se_muted: false,
            character_voice_enabled: [true; 5],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinoriConfigAudioBus {
    Bgm,
    Voice,
    Se,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinoriConfigControl {
    MessageSpeedUnread(u8),
    MessageSpeedRead(u8),
    MessageSpeedAutoPlay(u8),
    FontPrevious,
    FontNext,
    PreferredPlayMode(MinoriPlayMode),
    Fullscreen(bool),
    ToggleScreenEffect,
    ToggleTextShadow,
    ToggleAnimation,
    ToggleBacklogVoicePlayback,
    ToggleStopVoiceAtNextMessage,
    ToggleProgressInBackground,
    BgmVolume(u8),
    VoiceVolume(u8),
    SeVolume(u8),
    ToggleBgmMute,
    ToggleVoiceMute,
    ToggleSeMute,
    TestAudio(MinoriConfigAudioBus),
    ToggleCharacterVoice(usize),
    Apply,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinoriConfigChange {
    Present,
    AudioParamsChanged,
    TestAudio(MinoriConfigAudioBus),
    Applied,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MinoriVmEvent {
    Wait(MinoriWaitState),
    Message {
        presentation_sequence: u64,
        capture_sequence: u64,
        text: String,
        speaker: Option<String>,
        audio_commands: Vec<MinoriAudioCommand>,
        wait: MinoriWaitState,
    },
    Audio {
        commands: Vec<MinoriAudioCommand>,
    },
    Stage(MinoriStageCommand),
    Character(MinoriCharacterFrame),
    AxisScroll(MinoriAxisScrollFrame),
    LinearScroll(MinoriLinearScrollFrame),
    ScrollXf(MinoriScrollXfFrame),
    WScroll2(MinoriWScroll2Frame),
    Effect(MinoriEffectFrame),
    EffectCleared {
        sequence: u64,
    },
    Firefly(MinoriFireflyFrame),
    FireflyCleared {
        sequence: u64,
    },
    SecondaryEffect(MinoriSecondaryEffectFrame),
    SecondaryEffectCleared {
        sequence: u64,
    },
    ScreenShake(MinoriScreenShakeFrame),
    Panel {
        sequence: u64,
    },
    Choice {
        sequence: u64,
        option_hashes: Vec<Hash256>,
        selected_index: u32,
    },
    Movie(MinoriMovieState),
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
        encoding: MinoriAudioEncoding,
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MinoriRuntimeError {
    #[error("ASTRA_EMU_MINORI_RUNTIME_STATE: runtime state is invalid")]
    State,
    #[error("ASTRA_EMU_MINORI_RUNTIME_PC: program counter is outside the script")]
    ProgramCounter,
    #[error("ASTRA_EMU_MINORI_RUNTIME_LABEL: branch label is missing or duplicated")]
    Label,
    #[error("ASTRA_EMU_MINORI_RUNTIME_OPERAND: command operands do not match the verified schema")]
    Operand,
    #[error(
        "ASTRA_EMU_MINORI_RUNTIME_OPCODE: command `{opcode}` at ordinal {ordinal} is not verified"
    )]
    UnsupportedOpcode { opcode: String, ordinal: u32 },
    #[error("ASTRA_EMU_MINORI_RUNTIME_PRAGMA: pragma is not verified (identity={identity})")]
    UnsupportedPragma { identity: Hash256 },
    #[error("ASTRA_EMU_MINORI_RUNTIME_BUDGET: instruction budget is exhausted")]
    Budget,
    #[error("ASTRA_EMU_MINORI_RUNTIME_WAIT: runtime is awaiting an unresolved token")]
    Waiting,
    #[error("ASTRA_EMU_MINORI_RUNTIME_OVERFLOW: deterministic counter overflowed")]
    Overflow,
    #[error("ASTRA_EMU_MINORI_RUNTIME_SNAPSHOT: runtime snapshot is malformed")]
    Snapshot,
    #[error("ASTRA_EMU_MINORI_RUNTIME_CHAIN: chain target is outside the script mount")]
    ChainTarget,
    #[error("ASTRA_EMU_MINORI_RUNTIME_AUDIO_RESOURCE: audio resource specification is invalid")]
    AudioResource,
    #[error("ASTRA_EMU_MINORI_RUNTIME_EFFECT: effect schema is not verified ({violation:?})")]
    Effect { violation: MinoriEffectViolation },
    #[error("ASTRA_EMU_MINORI_RUNTIME_EFFECT_KIND: effect kind is not implemented (identity={identity})")]
    UnsupportedEffectKind { identity: Hash256 },
    #[error(
        "ASTRA_EMU_MINORI_RUNTIME_PANEL: panel schema is not verified (operand_count={operand_count}, mode={mode:?})"
    )]
    Panel {
        operand_count: u8,
        mode: Option<u32>,
    },
    #[error("ASTRA_EMU_MINORI_RUNTIME_CHOICE: choice schema or selection state is invalid")]
    Choice,
    #[error("ASTRA_EMU_MINORI_RUNTIME_FIREFLY: firefly effect schema or state is invalid")]
    Firefly,
    #[error(
        "ASTRA_EMU_MINORI_RUNTIME_SECONDARY_EFFECT: secondary effect schema or state is invalid"
    )]
    SecondaryEffect,
    #[error("ASTRA_EMU_MINORI_RUNTIME_SCREEN_SHAKE: screen shake schema or state is invalid")]
    ScreenShake,
    #[error("ASTRA_EMU_MINORI_RUNTIME_SCROLL_XF: scrollXF schema or state is invalid")]
    ScrollXf,
    #[error("ASTRA_EMU_MINORI_RUNTIME_WSCROLL2: WScroll2 schema or state is invalid")]
    WScroll2,
    #[error("ASTRA_EMU_MINORI_RUNTIME_CHARACTER: character command schema or state is invalid")]
    Character,
    #[error("ASTRA_EMU_MINORI_RUNTIME_AXIS_SCROLL: axis scroll command or state is invalid")]
    AxisScroll,
    #[error("ASTRA_EMU_MINORI_RUNTIME_LINEAR_SCROLL: linear scroll command or state is invalid")]
    LinearScroll,
    #[error("ASTRA_EMU_MINORI_RUNTIME_BACKLOG: backlog state exceeds its verified bounds")]
    Backlog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinoriEffectViolation {
    Tokenization,
    UnsupportedKind,
    OperandCount { count: u8 },
    ResourceSequence { count: u8 },
    Timing,
    Timeline,
}

/// Reproduces the bounded part of the original audio resource parser:
/// `resource[volume,pan]`, with volume clamped to 0..100 and pan to -100..100.
/// A missing closing bracket leaves the default metadata intact, as observed in
/// the original handler. URI resolution remains a separate fail-closed step.
pub fn parse_audio_resource_spec(
    token: &str,
) -> Result<MinoriAudioResourceSpec, MinoriRuntimeError> {
    if token.is_empty() || token.len() > 4 * 1024 || token.contains('\0') {
        return Err(MinoriRuntimeError::AudioResource);
    }
    let Some(open) = token.find('[') else {
        return Ok(MinoriAudioResourceSpec {
            resource: token.to_owned(),
            volume_percent: 100,
            pan_percent: 0,
        });
    };
    let resource = &token[..open];
    if resource.is_empty() {
        return Err(MinoriRuntimeError::AudioResource);
    }
    let Some(relative_close) = token[open + 1..].find(']') else {
        return Ok(MinoriAudioResourceSpec {
            resource: resource.to_owned(),
            volume_percent: 100,
            pan_percent: 0,
        });
    };
    let close = open + 1 + relative_close;
    let metadata = &token[open + 1..close];
    let (volume, pan) = metadata
        .split_once(',')
        .map_or((metadata, ""), |(volume, pan)| (volume, pan));
    let volume = parse_c_decimal_prefix(volume).unwrap_or(100).clamp(0, 100);
    let pan = parse_c_decimal_prefix(pan).unwrap_or(0).clamp(-100, 100);
    Ok(MinoriAudioResourceSpec {
        resource: resource.to_owned(),
        volume_percent: volume as u16,
        pan_percent: pan as i16,
    })
}

fn parse_c_decimal_prefix(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digit_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == digit_start {
        return None;
    }
    value[..end].parse().ok()
}

pub struct MinoriVm {
    script: ScScript,
    labels: BTreeMap<String, u32>,
    state: MinoriRuntimeState,
    executed_commands: Vec<MinoriExecutedCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinoriExecutedCommand {
    pub script_hash: Hash256,
    pub command_ordinal: u32,
    pub opcode: String,
}

impl MinoriVm {
    pub fn new(
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
        session_seed: u64,
    ) -> Result<Self, MinoriRuntimeError> {
        let labels = build_labels(&script)?;
        let state = MinoriRuntimeState {
            schema: MINORI_RUNTIME_STATE_SCHEMA.into(),
            script_uri,
            script_hash,
            pc_line: 0,
            variables: BTreeMap::new(),
            global_variables: BTreeMap::new(),
            wait: None,
            message: None,
            backlog: Vec::new(),
            backlog_bytes: 0,
            choice: None,
            stage: None,
            characters: BTreeMap::new(),
            axis_scroll: None,
            linear_scroll: None,
            scroll_xf: None,
            wscroll2: None,
            transition: MinoriTransitionState::default(),
            effect: None,
            firefly: None,
            secondary_effect: None,
            screen_shake: None,
            panel: None,
            audio: BTreeMap::new(),
            movie: None,
            launch_mode: MinoriLaunchMode::DirectEntry,
            system_ui: MinoriSystemUiState::default(),
            gallery_unlocks: Vec::new(),
            fixed_tick: 0,
            session_seed,
            random_state: session_seed,
            instruction_count: 0,
            effect_sequence: 0,
            terminal: false,
        };
        Ok(Self {
            script,
            labels,
            state,
            executed_commands: Vec::new(),
        })
    }

    pub fn state(&self) -> &MinoriRuntimeState {
        &self.state
    }

    pub fn merge_verified_gallery_unlocks(
        &mut self,
        unlocks: &[Hash256],
    ) -> Result<(), MinoriRuntimeError> {
        let verified =
            MINORI_ROUTE_CLEAR_FLAGS.map(|flag| (flag, Hash256::from_sha256(flag.as_bytes())));
        if unlocks.len() > verified.len()
            || unlocks.windows(2).any(|pair| pair[0] >= pair[1])
            || unlocks
                .iter()
                .any(|identity| !verified.iter().any(|(_, known)| known == identity))
        {
            return Err(MinoriRuntimeError::State);
        }
        for (flag, identity) in verified {
            if unlocks.contains(&identity) {
                self.state.global_variables.insert(flag.into(), 1);
                if !self.state.gallery_unlocks.contains(&identity) {
                    self.state.gallery_unlocks.push(identity);
                }
            }
        }
        self.state.gallery_unlocks.sort_unstable();
        Ok(())
    }

    pub fn title_variant(&self) -> u8 {
        let is_set = |flag: &str| self.state.global_variables.get(flag) == Some(&1);
        if is_set("TOHKA_CLEAR") {
            2
        } else if ["AYAME_CLEAR", "SUI_CLEAR", "REN_CLEAR"]
            .into_iter()
            .all(is_set)
        {
            1
        } else {
            0
        }
    }

    pub fn advance_provider_tick(&mut self, fixed_tick: u64) -> Result<(), MinoriRuntimeError> {
        if fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 || self.state.terminal {
            return Err(MinoriRuntimeError::State);
        }
        self.executed_commands.clear();
        self.state.fixed_tick = fixed_tick;
        Ok(())
    }

    pub fn begin_title_launch(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.fixed_tick != 0
            || self.state.pc_line != 0
            || self.state.instruction_count != 0
            || self.state.wait.is_some()
            || self.state.terminal
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.launch_mode = MinoriLaunchMode::Title;
        self.state.system_ui.page = MinoriSystemPage::Title;
        self.state.system_ui.focus_index = 0;
        self.state.system_ui.config_draft = None;
        Ok(())
    }

    pub fn open_backlog(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::None
            || self.state.backlog.is_empty()
            || self.state.terminal
        {
            return Err(MinoriRuntimeError::Backlog);
        }
        let cursor =
            u32::try_from(self.state.backlog.len() - 1).map_err(|_| MinoriRuntimeError::Backlog)?;
        self.state.system_ui.page = MinoriSystemPage::Backlog;
        self.state.system_ui.focus_index = 0;
        self.state.system_ui.backlog_cursor = Some(cursor);
        Ok(())
    }

    pub fn move_backlog(&mut self, direction: i32) -> Result<(), MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Backlog || direction == 0 {
            return Err(MinoriRuntimeError::Backlog);
        }
        let last = u32::try_from(
            self.state
                .backlog
                .len()
                .checked_sub(1)
                .ok_or(MinoriRuntimeError::Backlog)?,
        )
        .map_err(|_| MinoriRuntimeError::Backlog)?;
        let cursor = self
            .state
            .system_ui
            .backlog_cursor
            .ok_or(MinoriRuntimeError::Backlog)?;
        self.state.system_ui.backlog_cursor = Some(if direction < 0 {
            cursor.saturating_sub(1)
        } else {
            cursor.saturating_add(1).min(last)
        });
        Ok(())
    }

    pub fn close_backlog(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Backlog {
            return Err(MinoriRuntimeError::Backlog);
        }
        self.state.system_ui.page = MinoriSystemPage::None;
        self.state.system_ui.focus_index = 0;
        self.state.system_ui.backlog_cursor = None;
        Ok(())
    }

    pub fn replay_backlog_voice(&mut self) -> Result<Vec<MinoriAudioCommand>, MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Backlog {
            return Err(MinoriRuntimeError::Backlog);
        }
        let cursor = usize::try_from(
            self.state
                .system_ui
                .backlog_cursor
                .ok_or(MinoriRuntimeError::Backlog)?,
        )
        .map_err(|_| MinoriRuntimeError::Backlog)?;
        let voice = self
            .state
            .backlog
            .get(cursor)
            .ok_or(MinoriRuntimeError::Backlog)?
            .voice
            .clone();
        let voice = voice.filter(|voice| message_voice_enabled(&self.state, voice));
        let mut commands = Vec::new();
        if self
            .state
            .audio
            .get(&VOICE_STREAM_ID)
            .is_some_and(|current| current.playing)
        {
            commands.push(MinoriAudioCommand::Stop {
                sequence: next_effect_sequence(&mut self.state)?,
                stream_id: VOICE_STREAM_ID,
                fade_ms: 0,
            });
        }
        if let Some(voice) = voice {
            append_audio_load_and_play(
                &mut self.state,
                &mut commands,
                VOICE_STREAM_ID,
                &voice.resource_uri,
                voice.volume_milli,
                voice.pan_milli,
                false,
                0,
            )?;
            self.state.audio.insert(
                VOICE_STREAM_ID,
                MinoriAudioState {
                    bus: "voice".into(),
                    encoding: MinoriAudioEncoding::Ogg,
                    resource_uri: voice.resource_uri,
                    looped: false,
                    volume_milli: voice.volume_milli,
                    pan_milli: voice.pan_milli,
                    playing: true,
                    continuation_pts: 0,
                },
            );
        } else if let Some(current) = self.state.audio.get_mut(&VOICE_STREAM_ID) {
            current.playing = false;
        }
        Ok(commands)
    }

    pub fn advance_system_tick(&mut self, fixed_tick: u64) -> Result<(), MinoriRuntimeError> {
        if fixed_tick == 0
            || fixed_tick != self.state.fixed_tick + 1
            || self.state.system_ui.page == MinoriSystemPage::None
            || self.state.terminal
        {
            return Err(MinoriRuntimeError::State);
        }
        self.executed_commands.clear();
        self.state.fixed_tick = fixed_tick;
        Ok(())
    }

    pub fn set_system_page(
        &mut self,
        page: MinoriSystemPage,
        focus_index: u32,
    ) -> Result<(), MinoriRuntimeError> {
        if self.state.launch_mode != MinoriLaunchMode::Title
            || self.state.terminal
            || page == MinoriSystemPage::Config
            || self.state.system_ui.page == MinoriSystemPage::Config
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.system_ui.page = page;
        self.state.system_ui.focus_index = focus_index;
        Ok(())
    }

    pub fn open_save_page(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.terminal
            || self.state.system_ui.page != MinoriSystemPage::None
            || self.state.wait.is_none()
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.system_ui.page = MinoriSystemPage::Save;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }

    pub fn open_load_page(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.terminal
            || self.state.system_ui.page != MinoriSystemPage::None
            || self.state.wait.is_none()
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.system_ui.page = MinoriSystemPage::Load;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }

    pub fn close_gameplay_system_page(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.terminal
            || !matches!(
                self.state.system_ui.page,
                MinoriSystemPage::Save | MinoriSystemPage::Load
            )
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.system_ui.page = MinoriSystemPage::None;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }

    pub fn open_config(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.launch_mode != MinoriLaunchMode::Title
            || self.state.system_ui.page != MinoriSystemPage::Title
            || self.state.system_ui.config_draft.is_some()
            || self.state.terminal
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.system_ui.config_draft = Some(self.state.system_ui.config.clone());
        self.state.system_ui.page = MinoriSystemPage::Config;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }

    pub fn config_for_presentation(&self) -> Result<&MinoriConfigState, MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Config {
            return Err(MinoriRuntimeError::State);
        }
        self.state
            .system_ui
            .config_draft
            .as_ref()
            .ok_or(MinoriRuntimeError::State)
    }

    pub fn apply_config_control(
        &mut self,
        control: MinoriConfigControl,
    ) -> Result<MinoriConfigChange, MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Config
            || self.state.system_ui.config_draft.is_none()
            || self.state.terminal
        {
            return Err(MinoriRuntimeError::State);
        }
        if control == MinoriConfigControl::Apply {
            validate_config_state(
                self.state
                    .system_ui
                    .config_draft
                    .as_ref()
                    .ok_or(MinoriRuntimeError::State)?,
            )?;
            let draft = self
                .state
                .system_ui
                .config_draft
                .take()
                .ok_or(MinoriRuntimeError::State)?;
            self.state.system_ui.config = draft;
            self.state.system_ui.page = MinoriSystemPage::Title;
            self.state.system_ui.focus_index = 0;
            return Ok(MinoriConfigChange::Applied);
        }
        if control == MinoriConfigControl::Cancel {
            self.state.system_ui.config_draft = None;
            self.state.system_ui.page = MinoriSystemPage::Title;
            self.state.system_ui.focus_index = 0;
            return Ok(MinoriConfigChange::Cancelled);
        }
        let draft = self
            .state
            .system_ui
            .config_draft
            .as_mut()
            .ok_or(MinoriRuntimeError::State)?;
        let change = match control {
            MinoriConfigControl::MessageSpeedUnread(value) => {
                draft.message_speed_unread = value;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::MessageSpeedRead(value) => {
                draft.message_speed_read = value;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::MessageSpeedAutoPlay(value) => {
                draft.message_speed_auto_play = value;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::FontPrevious => {
                draft.font_index = 0;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::FontNext => {
                draft.font_index = 0;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::PreferredPlayMode(mode) => {
                if mode == MinoriPlayMode::Normal {
                    return Err(MinoriRuntimeError::State);
                }
                draft.preferred_play_mode = mode;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::Fullscreen(value) => {
                draft.fullscreen = value;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::ToggleScreenEffect => {
                draft.screen_effect = !draft.screen_effect;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::ToggleTextShadow => {
                draft.text_shadow = !draft.text_shadow;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::ToggleAnimation => {
                draft.animation = !draft.animation;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::ToggleBacklogVoicePlayback => {
                draft.backlog_voice_playback = !draft.backlog_voice_playback;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::ToggleStopVoiceAtNextMessage => {
                draft.stop_voice_at_next_message = !draft.stop_voice_at_next_message;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::ToggleProgressInBackground => {
                draft.progress_in_background = !draft.progress_in_background;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::BgmVolume(value) => {
                draft.bgm_volume = value;
                MinoriConfigChange::AudioParamsChanged
            }
            MinoriConfigControl::VoiceVolume(value) => {
                draft.voice_volume = value;
                MinoriConfigChange::AudioParamsChanged
            }
            MinoriConfigControl::SeVolume(value) => {
                draft.se_volume = value;
                MinoriConfigChange::AudioParamsChanged
            }
            MinoriConfigControl::ToggleBgmMute => {
                draft.bgm_muted = !draft.bgm_muted;
                MinoriConfigChange::AudioParamsChanged
            }
            MinoriConfigControl::ToggleVoiceMute => {
                draft.voice_muted = !draft.voice_muted;
                MinoriConfigChange::AudioParamsChanged
            }
            MinoriConfigControl::ToggleSeMute => {
                draft.se_muted = !draft.se_muted;
                MinoriConfigChange::AudioParamsChanged
            }
            MinoriConfigControl::TestAudio(bus) => MinoriConfigChange::TestAudio(bus),
            MinoriConfigControl::ToggleCharacterVoice(index) => {
                let enabled = draft
                    .character_voice_enabled
                    .get_mut(index)
                    .ok_or(MinoriRuntimeError::State)?;
                *enabled = !*enabled;
                MinoriConfigChange::Present
            }
            MinoriConfigControl::Apply | MinoriConfigControl::Cancel => unreachable!(),
        };
        validate_config_state(draft)?;
        Ok(change)
    }

    pub fn config_audio_param_commands(
        &mut self,
    ) -> Result<Vec<MinoriAudioCommand>, MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Config
            && self.state.system_ui.page != MinoriSystemPage::Title
        {
            return Err(MinoriRuntimeError::State);
        }
        let active = self
            .state
            .audio
            .iter()
            .filter(|(_, state)| state.playing)
            .map(|(stream_id, state)| (*stream_id, state.clone()))
            .collect::<Vec<_>>();
        let mut commands = Vec::with_capacity(active.len());
        for (stream_id, state) in active {
            commands.push(MinoriAudioCommand::SetParams {
                sequence: next_effect_sequence(&mut self.state)?,
                stream_id,
                volume: f32::from(state.volume_milli) / 1000.0,
                pan: f32::from(state.pan_milli) / 1000.0,
                repeat: state.looped,
            });
        }
        Ok(commands)
    }

    pub fn close_config_audio_commands(
        &mut self,
    ) -> Result<Vec<MinoriAudioCommand>, MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Title
            || self.state.system_ui.config_draft.is_some()
        {
            return Err(MinoriRuntimeError::State);
        }
        let mut commands = Vec::new();
        for stream_id in [
            MINORI_CONFIG_TEST_BGM_STREAM_ID,
            MINORI_CONFIG_TEST_VOICE_STREAM_ID,
            MINORI_CONFIG_TEST_SE_STREAM_ID,
        ] {
            if self
                .state
                .audio
                .get(&stream_id)
                .is_some_and(|current| current.playing)
            {
                commands.push(MinoriAudioCommand::Stop {
                    sequence: next_effect_sequence(&mut self.state)?,
                    stream_id,
                    fade_ms: 0,
                });
                self.state
                    .audio
                    .get_mut(&stream_id)
                    .ok_or(MinoriRuntimeError::State)?
                    .playing = false;
            }
        }
        commands.extend(self.config_audio_param_commands()?);
        Ok(commands)
    }

    /// Start a verified BGM from the Memories music page. The gallery uses the
    /// same stream contract as `.playbgm`: one looped OGG stream, with the
    /// previous stream stopped before the new resource is loaded. The caller
    /// still validates the resource against the mounted VFS before publishing
    /// the commands.
    pub fn gallery_bgm_play(
        &mut self,
        resource_uri: &str,
    ) -> Result<Vec<MinoriAudioCommand>, MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::GalleryBgm
            || !resource_uri.starts_with("minori:/bgm/")
        {
            return Err(MinoriRuntimeError::State);
        }
        let resource = &resource_uri["minori:/bgm/".len()..];
        validate_audio_relative_path(resource)?;
        let mut commands = Vec::new();
        if self
            .state
            .audio
            .get(&MINORI_BGM_STREAM_ID)
            .is_some_and(|current| current.playing)
        {
            commands.push(MinoriAudioCommand::Stop {
                sequence: next_effect_sequence(&mut self.state)?,
                stream_id: MINORI_BGM_STREAM_ID,
                fade_ms: 0,
            });
        }
        append_audio_load_and_play(
            &mut self.state,
            &mut commands,
            MINORI_BGM_STREAM_ID,
            resource_uri,
            1000,
            0,
            true,
            0,
        )?;
        self.state.audio.insert(
            MINORI_BGM_STREAM_ID,
            MinoriAudioState {
                bus: "bgm".into(),
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: resource_uri.into(),
                looped: true,
                volume_milli: 1000,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        Ok(commands)
    }

    pub fn gallery_bgm_stop(&mut self) -> Result<Vec<MinoriAudioCommand>, MinoriRuntimeError> {
        if !matches!(
            self.state.system_ui.page,
            MinoriSystemPage::GalleryBgm | MinoriSystemPage::Memories
        ) {
            return Err(MinoriRuntimeError::State);
        }
        stop_audio_stream(&mut self.state, MINORI_BGM_STREAM_ID, 0).map(|event| match event {
            Some(MinoriVmEvent::Audio { commands }) => commands,
            Some(_) | None => Vec::new(),
        })
    }

    pub fn config_test_audio_commands(
        &mut self,
        bus: MinoriConfigAudioBus,
    ) -> Result<Vec<MinoriAudioCommand>, MinoriRuntimeError> {
        if self.state.system_ui.page != MinoriSystemPage::Config
            || self.state.system_ui.config_draft.is_none()
        {
            return Err(MinoriRuntimeError::State);
        }
        let (stream_id, bus_name, resource_uri) = match bus {
            MinoriConfigAudioBus::Bgm => (
                MINORI_CONFIG_TEST_BGM_STREAM_ID,
                "bgm",
                "minori:/sys/BGMTest.wav",
            ),
            MinoriConfigAudioBus::Voice => (
                MINORI_CONFIG_TEST_VOICE_STREAM_ID,
                "voice",
                "minori:/sys/VOICEtest.wav",
            ),
            MinoriConfigAudioBus::Se => (
                MINORI_CONFIG_TEST_SE_STREAM_ID,
                "se",
                "minori:/sys/SEtest.wav",
            ),
        };
        let mut commands = Vec::with_capacity(3);
        if self
            .state
            .audio
            .get(&stream_id)
            .is_some_and(|current| current.playing)
        {
            commands.push(MinoriAudioCommand::Stop {
                sequence: next_effect_sequence(&mut self.state)?,
                stream_id,
                fade_ms: 0,
            });
        }
        commands.push(MinoriAudioCommand::LoadResource {
            sequence: next_effect_sequence(&mut self.state)?,
            stream_id,
            encoding: MinoriAudioEncoding::Wav,
            resource_uri: resource_uri.into(),
        });
        commands.push(MinoriAudioCommand::Play {
            sequence: next_effect_sequence(&mut self.state)?,
            stream_id,
            volume: 1.0,
            pan: 0.0,
            repeat: false,
            fade_in_ms: 0,
        });
        self.state.audio.insert(
            stream_id,
            MinoriAudioState {
                bus: bus_name.into(),
                encoding: MinoriAudioEncoding::Wav,
                resource_uri: resource_uri.into(),
                looped: false,
                volume_milli: 1000,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        Ok(commands)
    }

    pub fn move_system_focus(
        &mut self,
        direction: i32,
        item_count: u32,
    ) -> Result<(), MinoriRuntimeError> {
        if self.state.system_ui.page == MinoriSystemPage::None || item_count == 0 || direction == 0
        {
            return Err(MinoriRuntimeError::State);
        }
        let current = self.state.system_ui.focus_index % item_count;
        self.state.system_ui.focus_index = if direction < 0 {
            current.checked_sub(1).unwrap_or(item_count - 1)
        } else {
            current.checked_add(1).unwrap_or(0) % item_count
        };
        Ok(())
    }

    pub fn set_system_focus(
        &mut self,
        focus_index: u32,
        item_count: u32,
    ) -> Result<(), MinoriRuntimeError> {
        if self.state.system_ui.page == MinoriSystemPage::None
            || item_count == 0
            || focus_index >= item_count
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.system_ui.focus_index = focus_index;
        Ok(())
    }

    pub fn move_save_page(&mut self, direction: i32) -> Result<(), MinoriRuntimeError> {
        if !matches!(
            self.state.system_ui.page,
            MinoriSystemPage::Save | MinoriSystemPage::Load
        ) || direction == 0
        {
            return Err(MinoriRuntimeError::State);
        }
        let page = self.state.system_ui.focus_index / 10;
        let slot = self.state.system_ui.focus_index % 10;
        let page = if direction < 0 {
            page.checked_sub(1).unwrap_or(MINORI_SAVE_PAGE_COUNT - 1)
        } else {
            page.checked_add(1).unwrap_or(0) % MINORI_SAVE_PAGE_COUNT
        };
        self.state.system_ui.focus_index = page * 10 + slot;
        Ok(())
    }

    pub fn set_save_focus(&mut self, slot: u32) -> Result<(), MinoriRuntimeError> {
        if !matches!(
            self.state.system_ui.page,
            MinoriSystemPage::Save | MinoriSystemPage::Load
        ) || slot >= MINORI_SAVE_PAGE_COUNT * 10
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.system_ui.focus_index = slot;
        Ok(())
    }

    pub fn terminate_system_session(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.launch_mode != MinoriLaunchMode::Title
            || self.state.system_ui.page != MinoriSystemPage::Title
            || self.state.terminal
        {
            return Err(MinoriRuntimeError::State);
        }
        self.state.terminal = true;
        Ok(())
    }

    pub fn take_executed_commands(&mut self) -> Vec<MinoriExecutedCommand> {
        std::mem::take(&mut self.executed_commands)
    }

    pub fn state_hash(&self) -> Result<Hash256, MinoriRuntimeError> {
        let bytes = postcard::to_allocvec(&self.state).map_err(|_| MinoriRuntimeError::Snapshot)?;
        Ok(Hash256::from_sha256(&bytes))
    }

    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, MinoriRuntimeError> {
        postcard::to_allocvec(&self.state).map_err(|_| MinoriRuntimeError::Snapshot)
    }

    pub fn decode_snapshot(bytes: &[u8]) -> Result<MinoriRuntimeState, MinoriRuntimeError> {
        let state: MinoriRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MinoriRuntimeError::Snapshot)?;
        if state.schema != MINORI_RUNTIME_STATE_SCHEMA {
            return Err(MinoriRuntimeError::State);
        }
        validate_runtime_state(&state)?;
        Ok(state)
    }

    pub fn replace_script(
        &mut self,
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
    ) -> Result<(), MinoriRuntimeError> {
        let labels = build_labels(&script)?;
        self.script = script;
        self.labels = labels;
        self.state.script_uri = script_uri;
        self.state.script_hash = script_hash;
        self.state.pc_line = 0;
        self.state.variables.clear();
        self.state.wait = None;
        self.state.message = None;
        self.state.choice = None;
        self.state.effect = None;
        self.state.firefly = None;
        self.state.secondary_effect = None;
        self.state.screen_shake = None;
        self.state.axis_scroll = None;
        self.state.linear_scroll = None;
        self.state.wscroll2 = None;
        self.state.terminal = false;
        Ok(())
    }

    pub fn restore_state(&mut self, bytes: &[u8]) -> Result<(), MinoriRuntimeError> {
        let restored: MinoriRuntimeState =
            postcard::from_bytes(bytes).map_err(|_| MinoriRuntimeError::Snapshot)?;
        if restored.schema != MINORI_RUNTIME_STATE_SCHEMA
            || restored.script_uri != self.state.script_uri
            || restored.script_hash != self.state.script_hash
            || restored.session_seed != self.state.session_seed
            || restored.pc_line as usize > self.script.lines.len()
        {
            return Err(MinoriRuntimeError::State);
        }
        validate_runtime_state(&restored)?;
        self.state = restored;
        Ok(())
    }

    pub fn resolve_wait(&mut self, token_id: &str) -> Result<(), MinoriRuntimeError> {
        let current = self
            .state
            .wait
            .as_ref()
            .ok_or(MinoriRuntimeError::Waiting)?;
        let expected = match current {
            MinoriWaitState::Time { token_id, .. }
            | MinoriWaitState::AxisScroll { token_id, .. }
            | MinoriWaitState::LinearScroll { token_id, .. }
            | MinoriWaitState::CharacterTransition { token_id, .. }
            | MinoriWaitState::Input { token_id }
            | MinoriWaitState::Choice { token_id }
            | MinoriWaitState::Media { token_id, .. }
            | MinoriWaitState::Presentation { token_id, .. }
            | MinoriWaitState::Provider { token_id, .. } => token_id,
        };
        if expected != token_id {
            return Err(MinoriRuntimeError::Waiting);
        }
        let completes_media = matches!(current, MinoriWaitState::Media { .. });
        let completes_axis_scroll = matches!(current, MinoriWaitState::AxisScroll { .. });
        let completes_linear_scroll = matches!(current, MinoriWaitState::LinearScroll { .. });
        let completes_character_transition =
            matches!(current, MinoriWaitState::CharacterTransition { .. });
        if completes_media {
            self.state.movie = None;
        }
        if completes_axis_scroll {
            self.complete_axis_scroll()?;
        }
        if completes_linear_scroll {
            complete_linear_scroll_state(&mut self.state)?;
        }
        if completes_character_transition {
            complete_character_transition_state(&mut self.state)?;
        }
        self.state.wait = None;
        Ok(())
    }

    /// Update the physical Control key state. Text/timer fast-forward additionally
    /// requires `.pragma enable_control`; a modal movie uses its own script-owned
    /// `skippable` flag before the provider may turn the held key into a stop.
    pub fn set_control_pressed(&mut self, pressed: bool) {
        self.state.system_ui.control_pressed = pressed;
    }

    pub fn set_control_enabled(&mut self, enabled: bool) {
        self.state.system_ui.control_enabled = enabled;
    }

    pub fn set_pointer_axis(&mut self, axis: char, value: f32) -> Result<(), MinoriRuntimeError> {
        if !value.is_finite() || value.fract() != 0.0 {
            return Err(MinoriRuntimeError::State);
        }
        let value = value as i32;
        match axis {
            'x' if (0..=1280).contains(&value) => self.state.system_ui.pointer_x = value,
            'y' if (0..=720).contains(&value) => self.state.system_ui.pointer_y = value,
            _ => return Err(MinoriRuntimeError::State),
        }
        Ok(())
    }

    pub fn set_pointer_primary_pressed(&mut self, pressed: bool) {
        self.state.system_ui.pointer_primary_pressed = pressed;
    }

    pub fn toggle_preferred_play_mode(&mut self) -> Result<bool, MinoriRuntimeError> {
        let target = self.state.system_ui.config.preferred_play_mode;
        if target == MinoriPlayMode::Normal {
            return Err(MinoriRuntimeError::State);
        }
        let next_mode = if self.state.system_ui.play_mode == target {
            MinoriPlayMode::Normal
        } else {
            target
        };
        self.state.system_ui.play_mode = next_mode;

        self.rebind_active_message_wait()
    }

    pub fn rebind_active_message_wait(&mut self) -> Result<bool, MinoriRuntimeError> {
        let Some(wait) = self.state.wait.clone() else {
            return Ok(false);
        };

        let token_id = match &wait {
            MinoriWaitState::Input { token_id } | MinoriWaitState::Time { token_id, .. }
                if token_id.starts_with("minori.message.") =>
            {
                token_id.clone()
            }
            _ => return Ok(false),
        };
        let rebound = message_wait_for_current_mode(&self.state, token_id)?;
        if rebound == wait {
            return Ok(false);
        }
        self.state.wait = Some(rebound);
        Ok(true)
    }

    pub fn fast_forward_active(&self) -> bool {
        self.state.system_ui.skip_enabled
            && (self.state.system_ui.play_mode == MinoriPlayMode::Skip
                || (self.state.system_ui.control_enabled && self.state.system_ui.control_pressed))
    }

    /// Allocate an effect sequence for a host-side presentation update that
    /// does not execute a script command (for example, choice cursor motion).
    /// Keeping this counter inside the VM makes those updates part of the
    /// deterministic snapshot and prevents the next command from reusing the
    /// same sequence.
    pub fn allocate_effect_sequence(&mut self) -> Result<u64, MinoriRuntimeError> {
        next_effect_sequence(&mut self.state)
    }

    pub fn move_choice(&mut self, direction: i32) -> Result<(), MinoriRuntimeError> {
        let choice = self
            .state
            .choice
            .as_mut()
            .ok_or(MinoriRuntimeError::Choice)?;
        let len = choice.option_hashes.len();
        if !(1..=4).contains(&len) {
            return Err(MinoriRuntimeError::Choice);
        }
        let current = usize::try_from(choice.selected_index.unwrap_or(0))
            .map_err(|_| MinoriRuntimeError::Choice)?;
        if current >= len {
            return Err(MinoriRuntimeError::Choice);
        }
        let next = if direction < 0 {
            if current == 0 {
                len - 1
            } else {
                current - 1
            }
        } else if direction > 0 {
            (current + 1) % len
        } else {
            current
        };
        choice.selected_index = Some(u32::try_from(next).map_err(|_| MinoriRuntimeError::Choice)?);
        Ok(())
    }

    /// Reconstruct the current choice labels from the immutable parsed script.
    /// Plaintext remains outside snapshot state; the stored source span, target
    /// list, and hashes prove that the reconstructed labels are the same choice.
    pub fn choice_display_texts(&self) -> Result<Vec<String>, MinoriRuntimeError> {
        let choice = self
            .state
            .choice
            .as_ref()
            .ok_or(MinoriRuntimeError::Choice)?;
        let command = self
            .script
            .lines
            .iter()
            .find_map(|line| match &line.kind {
                ScLineKind::Command { command } if command.span == choice.source => Some(command),
                _ => None,
            })
            .ok_or(MinoriRuntimeError::Choice)?;
        if command.opcode != "select" {
            return Err(MinoriRuntimeError::Choice);
        }
        let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
            .map_err(|_| MinoriRuntimeError::Choice)?;
        if tokens.len() != choice.targets.len() || tokens.len() != choice.option_hashes.len() {
            return Err(MinoriRuntimeError::Choice);
        }
        tokens
            .iter()
            .zip(&choice.targets)
            .zip(&choice.option_hashes)
            .map(|((token, expected_target), expected_hash)| {
                let (display, target) = token
                    .split_once(':')
                    .filter(|(display, target)| !display.is_empty() && !target.is_empty())
                    .ok_or(MinoriRuntimeError::Choice)?;
                if target != expected_target
                    || Hash256::from_sha256(display.as_bytes()) != *expected_hash
                {
                    return Err(MinoriRuntimeError::Choice);
                }
                Ok(display.to_owned())
            })
            .collect()
    }

    pub fn commit_choice(&mut self) -> Result<(), MinoriRuntimeError> {
        if self.state.wait.is_some() {
            return Err(MinoriRuntimeError::Choice);
        }
        let choice = self.state.choice.take().ok_or(MinoriRuntimeError::Choice)?;
        let index = usize::try_from(choice.selected_index.unwrap_or(0))
            .map_err(|_| MinoriRuntimeError::Choice)?;
        let target = choice
            .targets
            .get(index)
            .ok_or(MinoriRuntimeError::Choice)?;
        self.state.pc_line = *self.labels.get(target).ok_or(MinoriRuntimeError::Label)?;
        Ok(())
    }

    pub fn advance_waiting_tick(&mut self, fixed_tick: u64) -> Result<(), MinoriRuntimeError> {
        if self.state.wait.is_none() || fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MinoriRuntimeError::State);
        }
        self.state.fixed_tick = fixed_tick;
        Ok(())
    }

    pub fn advance_effect_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriEffectFrame>, MinoriRuntimeError> {
        let Some(effect) = self.state.effect.as_mut() else {
            return Ok(None);
        };
        // The original CrossFade2 object only enables its transition path
        // after it has resolved at least two source frames. A single source
        // remains a static presentation and must not be wrapped onto itself.
        if effect.resources.len() < 2 {
            return Ok(None);
        }
        effect.elapsed_ns = effect
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let interval_ns = u64::from(effect.interval_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        if effect.elapsed_ns < interval_ns {
            return Ok(None);
        }
        // The original handler performs at most one update per render call and
        // resets its time origin to the current clock value.
        effect.elapsed_ns = 0;
        if effect.alpha_255 >= 255 {
            effect.current_index = effect.next_index;
            effect.next_index = next_effect_index(effect.next_index, effect.resources.len())?;
            effect.alpha_255 = 0;
        }
        let mut frame = effect_frame(effect)?;
        effect.visible_current_index = effect.current_index;
        effect.visible_next_index = effect.next_index;
        effect.visible_alpha_255 = frame.alpha_255;
        effect.alpha_255 = effect
            .alpha_255
            .checked_add(effect.alpha_step)
            .ok_or(MinoriRuntimeError::Overflow)?;
        next_effect_sequence(&mut self.state)?;
        frame.sequence = self.state.effect_sequence;
        Ok(Some(frame))
    }

    pub fn advance_axis_scroll_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriAxisScrollFrame>, MinoriRuntimeError> {
        let (axis_scroll, stage) = (&mut self.state.axis_scroll, &mut self.state.stage);
        let Some(scroll) = axis_scroll.as_mut() else {
            return Ok(None);
        };
        if scroll.completed {
            return Ok(None);
        }
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let previous = scroll.current;
        scroll.elapsed_ns = scroll
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?
            .min(duration_ns);
        scroll.current = axis_scroll_visible_position(scroll)?;
        scroll.completed = scroll.elapsed_ns == duration_ns;
        set_stage_axis_position(
            stage.as_mut().ok_or(MinoriRuntimeError::AxisScroll)?,
            scroll.axis,
            scroll.current,
        )?;
        if scroll.current == previous && !scroll.completed {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriAxisScrollFrame { sequence }))
    }

    pub fn advance_character_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriCharacterFrame>, MinoriRuntimeError> {
        let Some((_, character)) = self
            .state
            .characters
            .iter_mut()
            .find(|(_, character)| character.transition.is_some())
        else {
            return Ok(None);
        };
        let transition = character
            .transition
            .as_mut()
            .ok_or(MinoriRuntimeError::Character)?;
        if transition.completed {
            return Ok(None);
        }
        let duration_ns = u64::from(transition.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        transition.elapsed_ns = transition
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?
            .min(duration_ns);
        character.opacity_256 = interpolate_character_opacity(transition, duration_ns)?;
        transition.completed = transition.elapsed_ns == duration_ns;
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriCharacterFrame { sequence }))
    }

    pub fn advance_linear_scroll_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriLinearScrollFrame>, MinoriRuntimeError> {
        let (linear_scroll, stage) = (&mut self.state.linear_scroll, &mut self.state.stage);
        let Some(scroll) = linear_scroll.as_mut() else {
            return Ok(None);
        };
        if scroll.completed {
            return Ok(None);
        }
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let previous = scroll.current;
        scroll.elapsed_ns = scroll
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?
            .min(duration_ns);
        scroll.current = linear_scroll_visible_position(scroll)?;
        scroll.completed = scroll.elapsed_ns == duration_ns;
        set_stage_position(
            stage.as_mut().ok_or(MinoriRuntimeError::LinearScroll)?,
            scroll.current,
        )?;
        if scroll.current == previous && !scroll.completed {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriLinearScrollFrame { sequence }))
    }

    fn complete_axis_scroll(&mut self) -> Result<(), MinoriRuntimeError> {
        complete_axis_scroll_state(&mut self.state)
    }

    pub fn advance_scroll_xf_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriScrollXfFrame>, MinoriRuntimeError> {
        let Some(scroll) = self.state.scroll_xf.as_mut() else {
            return Ok(None);
        };
        if scroll.completed {
            return Ok(None);
        }
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        scroll.elapsed_ns = scroll
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?
            .min(duration_ns);
        let (visible_extent, visible_offset) = scroll_xf_visible_state(scroll)?;
        scroll.visible_extent = visible_extent;
        scroll.visible_offset = visible_offset;
        scroll.completed = scroll.elapsed_ns == duration_ns;
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriScrollXfFrame { sequence }))
    }

    pub fn advance_wscroll2_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriWScroll2Frame>, MinoriRuntimeError> {
        let Some(scroll) = self.state.wscroll2.as_mut() else {
            return Ok(None);
        };
        let previous_ticks = scroll.elapsed_ticks;
        scroll.elapsed_ns = scroll
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let (elapsed_ticks, foreground_offset, background_offset, background_remainder) =
            wscroll2_visible_state(scroll.elapsed_ns, scroll.speed_tenths)?;
        scroll.elapsed_ticks = elapsed_ticks;
        scroll.foreground_offset = foreground_offset;
        scroll.background_offset = background_offset;
        scroll.background_remainder = background_remainder;
        if elapsed_ticks == previous_ticks {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriWScroll2Frame { sequence }))
    }

    /// Advances the verified Firefly screen effect.  The original effect keeps
    /// a bounded particle pool alive until the script issues `.effect end`; a
    /// particle that reaches its individual lifetime is re-seeded in place.
    /// All random state is owned by the VM so a replay or snapshot continuation
    /// cannot depend on a process-global RNG.
    pub fn advance_firefly_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
        let mut random_state = self.state.random_state;
        let mut clear = false;
        let Some(firefly) = self.state.firefly.as_mut() else {
            return Ok(None);
        };
        firefly.fade_elapsed_ns = firefly
            .fade_elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let fade_steps = firefly.fade_elapsed_ns / MINORI_FIREFLY_FADE_STEP_NS;
        firefly.fade_elapsed_ns %= MINORI_FIREFLY_FADE_STEP_NS;
        let fade_steps = u16::try_from(fade_steps.min(u64::from(u16::MAX)))
            .map_err(|_| MinoriRuntimeError::Overflow)?;
        if firefly.ending {
            firefly.fade_alpha_256 = firefly.fade_alpha_256.saturating_sub(fade_steps);
            if firefly.fade_alpha_256 == 0 {
                clear = true;
            }
        } else {
            firefly.fade_alpha_256 = firefly
                .fade_alpha_256
                .saturating_add(fade_steps)
                .min(MINORI_FIREFLY_FADE_SCALE);
        }
        if !clear {
            for particle in &mut firefly.particles {
                particle.elapsed_ns = particle
                    .elapsed_ns
                    .checked_add(delta_ns)
                    .ok_or(MinoriRuntimeError::Overflow)?;
                if particle.elapsed_ns >= particle.lifetime_ns {
                    respawn_firefly_particle(particle, firefly.duration_ms, &mut random_state)?;
                }
                let t = fixed_firefly_parameter(particle.elapsed_ns, particle.lifetime_ns)?;
                particle.position = firefly_curve_position(&particle.control_points, t)?;
                particle.opacity_255 = firefly_particle_opacity(t);
            }
        }
        self.state.random_state = random_state;
        if clear {
            self.state.firefly = None;
            let sequence = next_effect_sequence(&mut self.state)?;
            return Ok(Some(MinoriVmEvent::FireflyCleared { sequence }));
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriVmEvent::Firefly(MinoriFireflyFrame {
            sequence,
        })))
    }

    /// Advances the independently composited `.effect2 SnowH` slot. Native
    /// Musica keeps this slot separate from the primary effect pointer; its
    /// fade and particle pool therefore survive primary stage/effect changes.
    pub fn advance_secondary_effect_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
        let mut random_state = self.state.random_state;
        let mut clear = false;
        let mut changed = false;
        let Some(effect) = self.state.secondary_effect.as_mut() else {
            return Ok(None);
        };
        effect.fade_elapsed_ns = effect
            .fade_elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let fade_steps = effect.fade_elapsed_ns / MINORI_SNOW_H_FADE_STEP_NS;
        effect.fade_elapsed_ns %= MINORI_SNOW_H_FADE_STEP_NS;
        if fade_steps != 0 {
            changed = true;
            let fade_steps = u16::try_from(fade_steps.min(u64::from(u16::MAX)))
                .map_err(|_| MinoriRuntimeError::Overflow)?;
            if effect.ending {
                effect.alpha_256 = effect.alpha_256.saturating_sub(fade_steps);
                clear = effect.alpha_256 == 0;
            } else {
                effect.alpha_256 = effect
                    .alpha_256
                    .saturating_add(fade_steps)
                    .min(MINORI_SNOW_H_FADE_SCALE);
            }
        }
        effect.motion_elapsed_ns = effect
            .motion_elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let elapsed_ms = effect.motion_elapsed_ns / 1_000_000;
        effect.motion_elapsed_ns %= 1_000_000;
        if elapsed_ms != 0 && !clear {
            changed = true;
            for particle in &mut effect.particles {
                advance_snow_h_particle(particle, elapsed_ms)?;
                if particle.position[0] >= MINORI_SNOW_H_STAGE_WIDTH {
                    initialize_snow_h_particle(particle, &mut random_state)?;
                }
            }
        }
        self.state.random_state = random_state;
        if clear {
            self.state.secondary_effect = None;
            let sequence = next_effect_sequence(&mut self.state)?;
            return Ok(Some(MinoriVmEvent::SecondaryEffectCleared { sequence }));
        }
        if !changed {
            return Ok(None);
        }
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriVmEvent::SecondaryEffect(
            MinoriSecondaryEffectFrame { sequence },
        )))
    }

    /// Advances the native screen-copy shake mode. Musica performs at most
    /// one displacement update per render pass and resets the interval origin
    /// to the current clock; retaining excess elapsed time would therefore
    /// fabricate updates that the original engine never displayed.
    pub fn advance_screen_shake_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MinoriScreenShakeFrame>, MinoriRuntimeError> {
        let Some(shake) = self.state.screen_shake.as_mut() else {
            return Ok(None);
        };
        shake.elapsed_ns = shake
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let interval_ns = u64::from(shake.interval_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        if shake.elapsed_ns < interval_ns {
            return Ok(None);
        }
        shake.elapsed_ns = 0;
        let amplitude = shake.amplitude;
        shake.offset = match shake.kind {
            MinoriScreenShakeKind::Vertical => {
                if shake.update_index.is_multiple_of(2) {
                    [0, -amplitude]
                } else {
                    [0, amplitude]
                }
            }
            MinoriScreenShakeKind::Random => {
                let pattern = next_native_random_15(&mut self.state.random_state) % 8;
                random_screen_shake_offset(pattern, amplitude)?
            }
        };
        shake.update_index = shake
            .update_index
            .checked_add(1)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let sequence = next_effect_sequence(&mut self.state)?;
        Ok(Some(MinoriScreenShakeFrame { sequence }))
    }

    pub fn step(
        &mut self,
        fixed_tick: u64,
        max_instructions: u32,
    ) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
        if fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MinoriRuntimeError::State);
        }
        if self.state.wait.is_some() {
            return Err(MinoriRuntimeError::Waiting);
        }
        if self.state.terminal {
            return Ok(Some(MinoriVmEvent::Terminal));
        }
        self.executed_commands.clear();
        self.state.fixed_tick = fixed_tick;
        for _ in 0..max_instructions {
            let line_index = self.state.pc_line as usize;
            let line = self
                .script
                .lines
                .get(line_index)
                .ok_or(MinoriRuntimeError::ProgramCounter)?;
            self.state.pc_line = self
                .state
                .pc_line
                .checked_add(1)
                .ok_or(MinoriRuntimeError::Overflow)?;
            let ScLineKind::Command { command } = &line.kind else {
                continue;
            };
            self.executed_commands.push(MinoriExecutedCommand {
                script_hash: self.state.script_hash,
                command_ordinal: self.state.pc_line - 1,
                opcode: command.opcode.clone(),
            });
            self.state.instruction_count = self
                .state
                .instruction_count
                .checked_add(1)
                .ok_or(MinoriRuntimeError::Overflow)?;
            if let Some(event) = execute_control(command, &self.labels, &mut self.state)? {
                return Ok(Some(event));
            }
        }
        Err(MinoriRuntimeError::Budget)
    }
}

fn execute_control(
    command: &ScCommand,
    labels: &BTreeMap<String, u32>,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    match command.opcode.as_str() {
        "pragma" => execute_pragma(command, state),
        "label" => Ok(None),
        "set" | "setglobal" => {
            let (key, value) = evaluate_assignment(&command.operands, state)?;
            if command.opcode == "set" {
                state.variables.insert(key.to_owned(), value);
            } else {
                state.global_variables.insert(key.to_owned(), value);
                record_verified_route_clear(state, key, value);
            }
            Ok(None)
        }
        "goto" => {
            let target = branch_target(&command.control_flow)?;
            state.pc_line = *labels.get(target).ok_or(MinoriRuntimeError::Label)?;
            Ok(None)
        }
        "if" => {
            let [ScOperand::Symbol { value: key }, ScOperand::Operator { value: operator }, ScOperand::Integer { value }, ScOperand::Symbol { value: target }] =
                command.operands.as_slice()
            else {
                return Err(MinoriRuntimeError::Operand);
            };
            let current = state
                .variables
                .get(key)
                .or_else(|| state.global_variables.get(key))
                .copied()
                .unwrap_or(0);
            if compare(current, operator, *value)? {
                state.pc_line = *labels.get(target).ok_or(MinoriRuntimeError::Label)?;
            }
            Ok(None)
        }
        "wait" => {
            let [ScOperand::Integer { value }] = command.operands.as_slice() else {
                return Err(MinoriRuntimeError::Operand);
            };
            let timer_ticks = u32::try_from(*value).map_err(|_| MinoriRuntimeError::Operand)?;
            // The original Control fast path bypasses timing work only while
            // the script has enabled it explicitly. Keeping this at command
            // execution time avoids fabricating an await completion in the
            // host and preserves one deterministic fixed tick per call.
            if state.system_ui.skip_enabled
                && (state.system_ui.play_mode == MinoriPlayMode::Skip
                    || (state.system_ui.control_enabled && state.system_ui.control_pressed))
            {
                return Ok(None);
            }
            let milliseconds = timer_ticks
                .checked_mul(10)
                .ok_or(MinoriRuntimeError::Overflow)?;
            let token_id = format!("minori.wait.{}", state.instruction_count);
            let wait = MinoriWaitState::Time {
                token_id,
                timer_ticks,
                milliseconds,
            };
            state.wait = Some(wait.clone());
            Ok(Some(MinoriVmEvent::Wait(wait)))
        }
        "message" => execute_message(command, state),
        "select" => execute_select(command, labels, state),
        "playbgm" => execute_play_bgm(command, state),
        "playse" => execute_play_se(command, state, 1, "se"),
        "playse2" => execute_play_se(command, state, 2, "se2"),
        "playse3" => execute_play_se(command, state, 3, "se3"),
        "playvoice" => execute_play_voice(command, state),
        "transition" => execute_transition(command, state),
        "stage" => execute_stage(command, state),
        "char" => execute_character(command, state),
        "hscroll" => execute_axis_scroll(command, state, MinoriAxisScrollAxis::Horizontal),
        "vscroll" => execute_axis_scroll(command, state, MinoriAxisScrollAxis::Vertical),
        "scroll" => execute_linear_scroll(command, state),
        "scrollxf" => execute_scroll_xf(command, state),
        "endscroll" => execute_end_scroll(command, state),
        "effect" => execute_effect(command, state),
        "effect2" => execute_secondary_effect(command, state),
        "shakescreen" => execute_screen_shake(command, state),
        "panel" => execute_panel(command, state),
        "movie" => execute_movie(command, state),
        "chain" => {
            let ScControlFlow::Chain { target } = &command.control_flow else {
                return Err(MinoriRuntimeError::Operand);
            };
            validate_chain_target(target)?;
            Ok(Some(MinoriVmEvent::Chain {
                target: target.clone(),
            }))
        }
        "end" => {
            // The original CommandEnd returns a title-launched game scene to
            // SceneMainMenu. Direct-entry sessions remain bounded command-line
            // executions and therefore terminate at the same script boundary.
            if state.launch_mode == MinoriLaunchMode::Title {
                state.wait = None;
                state.message = None;
                state.choice = None;
                state.movie = None;
                state.system_ui.page = MinoriSystemPage::Title;
                state.system_ui.focus_index = 0;
                state.system_ui.backlog_cursor = None;
                state.system_ui.config_draft = None;
            } else {
                state.terminal = true;
            }
            Ok(Some(MinoriVmEvent::Terminal))
        }
        _ => Err(MinoriRuntimeError::UnsupportedOpcode {
            opcode: command.opcode.clone(),
            ordinal: command.ordinal,
        }),
    }
}

fn record_verified_route_clear(state: &mut MinoriRuntimeState, key: &str, value: i64) {
    if value != 1 || !MINORI_ROUTE_CLEAR_FLAGS.contains(&key) {
        return;
    }
    let identity = Hash256::from_sha256(key.as_bytes());
    if !state.gallery_unlocks.contains(&identity) {
        state.gallery_unlocks.push(identity);
        state.gallery_unlocks.sort_unstable();
    }
}

fn execute_pragma(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let identity = Hash256::from_sha256(&command.raw_operands);
    let [pragma] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::UnsupportedPragma { identity });
    };
    match pragma.as_str() {
        "enable_control" => {
            state.system_ui.control_enabled = true;
            Ok(None)
        }
        "disable_control" => {
            state.system_ui.control_enabled = false;
            Ok(None)
        }
        "skip_enable" => {
            state.system_ui.skip_enabled = true;
            Ok(None)
        }
        "skip_disable" => {
            state.system_ui.skip_enabled = false;
            Ok(None)
        }
        _ => Err(MinoriRuntimeError::UnsupportedPragma { identity }),
    }
}

fn execute_character(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Character)?;
    let mode = tokens
        .first()
        .map(|value| value.to_ascii_lowercase())
        .ok_or(MinoriRuntimeError::Character)?;
    match mode.as_str() {
        "load" => {
            let [_, slot, resource] = tokens.as_slice() else {
                return Err(MinoriRuntimeError::Character);
            };
            let signed_slot = parse_character_slot(slot)?;
            let slot_id = signed_slot.unsigned_abs();
            if !state.characters.contains_key(&slot_id)
                && state.characters.len() >= MINORI_CHARACTER_MAX_SLOTS
            {
                return Err(MinoriRuntimeError::Character);
            }
            validate_scene_filename(resource).map_err(|_| MinoriRuntimeError::Character)?;
            let resource_uris = vec![format!("minori:/st/{resource}")];
            state.characters.insert(
                slot_id,
                MinoriCharacterState {
                    slot_id,
                    positive_orientation: signed_slot >= 0,
                    resource_uris,
                    anchor_position: [0, 0],
                    visible: true,
                    opacity_256: 256,
                    transition: None,
                    keep_once: false,
                },
            );
        }
        "pos" => {
            let [_, slot, x, y] = tokens.as_slice() else {
                return Err(MinoriRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            let x = parse_character_coordinate(x)?;
            let y = parse_character_coordinate(y)?;
            let character = state
                .characters
                .get_mut(&slot_id)
                .ok_or(MinoriRuntimeError::Character)?;
            character.anchor_position = [x, y];
        }
        "trans" => {
            let [_, slot, duration, opacity] = tokens.as_slice() else {
                return Err(MinoriRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            let duration_ms = parse_character_transition_duration(duration)?;
            let target_opacity_256 = parse_character_opacity(opacity)?;
            let character = state
                .characters
                .get_mut(&slot_id)
                .ok_or(MinoriRuntimeError::Character)?;
            if character.transition.is_some() {
                return Err(MinoriRuntimeError::Character);
            }
            if duration_ms == 0 {
                character.opacity_256 = target_opacity_256;
            } else {
                character.transition = Some(MinoriCharacterTransitionState {
                    start_opacity_256: character.opacity_256,
                    target_opacity_256,
                    duration_ms,
                    elapsed_ns: 0,
                    completed: false,
                });
                let wait = MinoriWaitState::CharacterTransition {
                    token_id: format!("minori.character.{slot_id}.{}", state.instruction_count),
                    slot_id,
                    milliseconds: duration_ms,
                };
                state.wait = Some(wait.clone());
                return Ok(Some(MinoriVmEvent::Wait(wait)));
            }
        }
        "vis" => {
            let [_, slot, visible] = tokens.as_slice() else {
                return Err(MinoriRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            let visible = parse_native_bool(visible);
            let character = state
                .characters
                .get_mut(&slot_id)
                .ok_or(MinoriRuntimeError::Character)?;
            character.visible = visible;
        }
        "keep" => {
            let [_, slot] = tokens.as_slice() else {
                return Err(MinoriRuntimeError::Character);
            };
            let slot_id = parse_character_slot(slot)?.unsigned_abs();
            if let Some(character) = state.characters.get_mut(&slot_id) {
                character.keep_once = true;
            }
        }
        _ => return Err(MinoriRuntimeError::Character),
    }
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::Character(MinoriCharacterFrame {
        sequence,
    })))
}

fn parse_character_slot(token: &str) -> Result<i32, MinoriRuntimeError> {
    let slot = token
        .parse::<i32>()
        .ok()
        .filter(|slot| *slot != 0 && slot.unsigned_abs() <= MINORI_CHARACTER_MAX_SLOT_ID)
        .ok_or(MinoriRuntimeError::Character)?;
    Ok(slot)
}

fn parse_character_coordinate(token: &str) -> Result<i32, MinoriRuntimeError> {
    token
        .parse::<i32>()
        .ok()
        .filter(|value| value.unsigned_abs() <= MINORI_CHARACTER_MAX_COORDINATE as u32)
        .ok_or(MinoriRuntimeError::Character)
}

fn parse_character_transition_duration(token: &str) -> Result<u32, MinoriRuntimeError> {
    token
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= MINORI_CHARACTER_MAX_TRANSITION_MS)
        .ok_or(MinoriRuntimeError::Character)
}

fn parse_character_opacity(token: &str) -> Result<u16, MinoriRuntimeError> {
    token
        .parse::<u16>()
        .ok()
        .filter(|value| *value <= 255)
        .ok_or(MinoriRuntimeError::Character)
}

fn parse_native_bool(token: &str) -> bool {
    token
        .as_bytes()
        .first()
        .is_some_and(|value| matches!(value, b'1'..=b'9' | b't' | b'T'))
}

fn execute_movie(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let [movie_id, resource, width, height, skippable] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::Operand);
    };
    let movie_id = movie_id
        .parse::<u32>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or(MinoriRuntimeError::Operand)?;
    validate_scene_filename(resource)?;
    let width = width
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=8192).contains(value))
        .ok_or(MinoriRuntimeError::Operand)?;
    let height = height
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=8192).contains(value))
        .ok_or(MinoriRuntimeError::Operand)?;
    let skippable = match skippable.as_str() {
        "t" => true,
        "f" => false,
        _ => return Err(MinoriRuntimeError::Operand),
    };
    if state.movie.is_some() {
        return Err(MinoriRuntimeError::State);
    }
    let media_id = format!("minori.movie.{movie_id}");
    let token_id = format!("minori.wait.movie.{}", state.instruction_count);
    let movie = MinoriMovieState {
        media_id: media_id.clone(),
        resource_uri: format!("minori:/mov/{resource}"),
        width,
        height,
        skippable,
        continuation_pts: 0,
        fence_id: token_id.clone(),
    };
    state.movie = Some(movie.clone());
    state.wait = Some(MinoriWaitState::Media { token_id, media_id });
    Ok(Some(MinoriVmEvent::Movie(movie)))
}

fn execute_effect(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens =
        tokenize_operands(&command.raw_operands, command.span.offset as usize).map_err(|_| {
            MinoriRuntimeError::Effect {
                violation: MinoriEffectViolation::Tokenization,
            }
        })?;
    if tokens.is_empty() || tokens.len() > 5 {
        return Err(MinoriRuntimeError::Effect {
            violation: MinoriEffectViolation::OperandCount {
                count: u8::try_from(tokens.len()).map_err(|_| MinoriRuntimeError::Overflow)?,
            },
        });
    }
    if tokens[0] == "*" {
        state.effect = None;
        state.firefly = None;
        state.wscroll2 = None;
        next_effect_sequence(state)?;
        return Ok(Some(MinoriVmEvent::EffectCleared {
            sequence: state.effect_sequence,
        }));
    }
    if tokens[0] == "end" {
        if tokens.len() != 1 {
            return Err(MinoriRuntimeError::Firefly);
        }
        if let Some(firefly) = state.firefly.as_mut() {
            firefly.ending = true;
            next_effect_sequence(state)?;
            return Ok(Some(MinoriVmEvent::Firefly(MinoriFireflyFrame {
                sequence: state.effect_sequence,
            })));
        }
        state.effect = None;
        state.wscroll2 = None;
        next_effect_sequence(state)?;
        return Ok(Some(MinoriVmEvent::EffectCleared {
            sequence: state.effect_sequence,
        }));
    }
    if tokens[0] == "Firefly" {
        if tokens.len() != 4 {
            return Err(MinoriRuntimeError::Firefly);
        }
        validate_scene_filename(&tokens[1]).map_err(|_| MinoriRuntimeError::Firefly)?;
        let target_count = tokens[2]
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=MINORI_FIREFLY_MAX_PARTICLES_U32).contains(value))
            .ok_or(MinoriRuntimeError::Firefly)?;
        let duration_ms = tokens[3]
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=MINORI_FIREFLY_MAX_DURATION_MS).contains(value))
            .ok_or(MinoriRuntimeError::Firefly)?;
        state.effect = None;
        state.wscroll2 = None;
        let firefly = new_firefly_state(
            &tokens[1],
            target_count,
            duration_ms,
            &mut state.random_state,
        )?;
        state.firefly = Some(firefly);
        next_effect_sequence(state)?;
        return Ok(Some(MinoriVmEvent::Firefly(MinoriFireflyFrame {
            sequence: state.effect_sequence,
        })));
    }
    if tokens[0] == "WScroll2" {
        if tokens.len() != 4 || state.stage.is_none() || state.scroll_xf.is_some() {
            return Err(MinoriRuntimeError::WScroll2);
        }
        let sync_resource = tokens[1]
            .strip_prefix("sync:")
            .ok_or(MinoriRuntimeError::WScroll2)?;
        validate_scene_filename(sync_resource).map_err(|_| MinoriRuntimeError::WScroll2)?;
        let period_ticks = tokens[2]
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=MINORI_WSCROLL2_MAX_PERIOD_TICKS).contains(value))
            .ok_or(MinoriRuntimeError::WScroll2)?;
        let speed_tenths = tokens[3]
            .parse::<i32>()
            .ok()
            .filter(|value| value.unsigned_abs() <= MINORI_WSCROLL2_MAX_SPEED_TENTHS as u32)
            .ok_or(MinoriRuntimeError::WScroll2)?;
        state.effect = None;
        state.firefly = None;
        state.wscroll2 = Some(MinoriWScroll2State {
            sync_resource_uri: format!("minori:/st/{sync_resource}"),
            period_ticks,
            speed_tenths,
            elapsed_ns: 0,
            elapsed_ticks: 0,
            foreground_offset: 0,
            background_offset: 0,
            background_remainder: 0,
        });
        let sequence = next_effect_sequence(state)?;
        return Ok(Some(MinoriVmEvent::WScroll2(MinoriWScroll2Frame {
            sequence,
        })));
    }
    if tokens[0] != "CrossFade2" {
        tracing::info!(
            target: "astra_emu_minori::runtime",
            event = "astra_emu_minori_effect_kind_unsupported",
            effect_identity = %Hash256::from_sha256(tokens[0].as_bytes()),
            operand_count = tokens.len(),
            "Minori effect kind is not implemented"
        );
        return Err(MinoriRuntimeError::UnsupportedEffectKind {
            identity: Hash256::from_sha256(tokens[0].as_bytes()),
        });
    }
    if tokens.len() != 1 && tokens.len() != 4 {
        return Err(MinoriRuntimeError::Effect {
            violation: MinoriEffectViolation::ResourceSequence {
                count: u8::try_from(tokens.len()).map_err(|_| MinoriRuntimeError::Overflow)?,
            },
        });
    }
    // The native command parser binds operand zero to the effect kind.  The
    // exercised original-script form supplies only that operand, so the
    // CrossFade2 object receives an empty resource specification and default
    // numeric fields.  It replaces the active effect but resolves no resource
    // frame.  Preserve that zero-frame state rather than inventing a frame or
    // a self-crossfade.
    if tokens.len() == 1 {
        state.effect = None;
        state.firefly = None;
        state.wscroll2 = None;
        next_effect_sequence(state)?;
        return Ok(Some(MinoriVmEvent::EffectCleared {
            sequence: state.effect_sequence,
        }));
    }
    // `SECrossFade2` tokenizes this field before resource resolution.  A
    // standalone `*` is therefore a valid empty resource selection, not a
    // filename.  Its resource lookup produces no frame objects, so the new
    // effect replaces the old primary slot without emitting a presentation.
    // Keep it distinct from a malformed sequence and do not turn it into a
    // guessed background URI.
    if tokens[1] == "*" {
        state.effect = None;
        state.firefly = None;
        state.wscroll2 = None;
        next_effect_sequence(state)?;
        return Ok(Some(MinoriVmEvent::EffectCleared {
            sequence: state.effect_sequence,
        }));
    }
    let resources = tokens[1]
        .split(':')
        .map(|resource| {
            if resource == "*" {
                return Ok(None);
            }
            validate_scene_filename(resource)?;
            Ok(Some(format!("minori:/bg/{resource}")))
        })
        .collect::<Result<Vec<_>, MinoriRuntimeError>>()?;
    if resources.len() < 2 || resources.len() > 64 {
        return Err(MinoriRuntimeError::Effect {
            violation: MinoriEffectViolation::ResourceSequence { count: 4 },
        });
    }
    let alpha_step = tokens[2]
        .parse::<u32>()
        .map_err(|_| MinoriRuntimeError::Effect {
            violation: MinoriEffectViolation::Timing,
        })?;
    let interval_ms = tokens[3]
        .parse::<u32>()
        .map_err(|_| MinoriRuntimeError::Effect {
            violation: MinoriEffectViolation::Timing,
        })?;
    if alpha_step == 0 || interval_ms == 0 {
        return Err(MinoriRuntimeError::Effect {
            violation: MinoriEffectViolation::Timing,
        });
    }
    state.firefly = None;
    state.wscroll2 = None;
    state.effect = Some(MinoriEffectState {
        kind: MinoriEffectKind::CrossFade2,
        resources,
        current_index: 0,
        next_index: 1,
        alpha_255: 0,
        alpha_step,
        interval_ms,
        elapsed_ns: 0,
        visible_current_index: 0,
        visible_next_index: 1,
        visible_alpha_255: 0,
    });
    next_effect_sequence(state)?;
    let mut frame = effect_frame(state.effect.as_ref().ok_or(MinoriRuntimeError::Effect {
        violation: MinoriEffectViolation::Timeline,
    })?)?;
    frame.sequence = state.effect_sequence;
    Ok(Some(MinoriVmEvent::Effect(frame)))
}

fn execute_secondary_effect(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::SecondaryEffect)?;
    let [kind] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::SecondaryEffect);
    };
    match kind.as_str() {
        "SnowH" => {
            state.secondary_effect = Some(new_snow_h_state(&mut state.random_state)?);
        }
        "fadeout" => {
            let effect = state
                .secondary_effect
                .as_mut()
                .ok_or(MinoriRuntimeError::SecondaryEffect)?;
            if effect.kind != MinoriSecondaryEffectKind::SnowHorizontal {
                return Err(MinoriRuntimeError::SecondaryEffect);
            }
            effect.ending = true;
        }
        _ => {
            tracing::info!(
                target: "astra_emu_minori::runtime",
                event = "astra_emu_minori_secondary_effect_kind_unsupported",
                effect_identity = %Hash256::from_sha256(kind.as_bytes()),
                "Minori secondary effect kind is not implemented"
            );
            return Err(MinoriRuntimeError::UnsupportedEffectKind {
                identity: Hash256::from_sha256(kind.as_bytes()),
            });
        }
    }
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::SecondaryEffect(
        MinoriSecondaryEffectFrame { sequence },
    )))
}

fn execute_screen_shake(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::ScreenShake)?;
    let [kind, amplitude, interval_ms] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::ScreenShake);
    };
    let kind = match kind.as_str() {
        "R" | "r" => MinoriScreenShakeKind::Random,
        "V" | "v" => MinoriScreenShakeKind::Vertical,
        _ => return Err(MinoriRuntimeError::ScreenShake),
    };
    let amplitude = amplitude
        .parse::<i32>()
        .ok()
        .filter(|value| (1..=MINORI_SCREEN_SHAKE_MAX_AMPLITUDE).contains(value))
        .ok_or(MinoriRuntimeError::ScreenShake)?;
    let interval_ms = interval_ms
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=MINORI_SCREEN_SHAKE_MAX_INTERVAL_MS).contains(value))
        .ok_or(MinoriRuntimeError::ScreenShake)?;
    state.screen_shake = Some(MinoriScreenShakeState {
        kind,
        amplitude,
        interval_ms,
        elapsed_ns: 0,
        update_index: 0,
        offset: [0, 0],
    });
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::ScreenShake(MinoriScreenShakeFrame {
        sequence,
    })))
}

fn execute_panel(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens =
        tokenize_operands(&command.raw_operands, command.span.offset as usize).map_err(|_| {
            MinoriRuntimeError::Panel {
                operand_count: 0,
                mode: None,
            }
        })?;
    let mode = tokens.first().and_then(|value| value.parse::<u32>().ok());
    let operand_count = u8::try_from(tokens.len()).map_err(|_| MinoriRuntimeError::Overflow)?;
    let [mode] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::Panel {
            operand_count,
            mode,
        });
    };
    let mode = mode.parse::<u32>().map_err(|_| MinoriRuntimeError::Panel {
        operand_count,
        mode: None,
    })?;
    // The original CMessagePanel switch has an asset-free case 0 and loads
    // `msgPanel.png` only in case 1. Case 0 therefore clears the currently
    // visible panel; it is not a request to draw a default panel.
    state.panel = match mode {
        0 => None,
        1 => Some(MinoriPanelState {
            mode,
            resource_uri: "minori:/sys/msgPanel.png".into(),
        }),
        _ => {
            return Err(MinoriRuntimeError::Panel {
                operand_count,
                mode: Some(mode),
            });
        }
    };
    next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::Panel {
        sequence: state.effect_sequence,
    }))
}

fn next_effect_index(current: u32, len: usize) -> Result<u32, MinoriRuntimeError> {
    let len = u32::try_from(len).map_err(|_| MinoriRuntimeError::Overflow)?;
    current
        .checked_add(1)
        .map(|next| next % len)
        .ok_or(MinoriRuntimeError::Overflow)
}

fn effect_frame(effect: &MinoriEffectState) -> Result<MinoriEffectFrame, MinoriRuntimeError> {
    let current =
        usize::try_from(effect.current_index).map_err(|_| MinoriRuntimeError::Effect {
            violation: MinoriEffectViolation::Timeline,
        })?;
    let next = if effect.resources.len() > 1 {
        Some(
            usize::try_from(effect.next_index).map_err(|_| MinoriRuntimeError::Effect {
                violation: MinoriEffectViolation::Timeline,
            })?,
        )
    } else {
        None
    };
    Ok(MinoriEffectFrame {
        sequence: 0,
        current_resource_uri: effect
            .resources
            .get(current)
            .ok_or(MinoriRuntimeError::Effect {
                violation: MinoriEffectViolation::Timeline,
            })?
            .clone(),
        next_resource_uri: next
            .map(|next| {
                effect
                    .resources
                    .get(next)
                    .ok_or(MinoriRuntimeError::Effect {
                        violation: MinoriEffectViolation::Timeline,
                    })
                    .cloned()
            })
            .transpose()?
            .flatten(),
        alpha_255: effect.alpha_255.min(255) as u16,
    })
}

fn next_firefly_random(state: &mut u64) -> u32 {
    // SplitMix64 is small, deterministic on every supported target, and keeps
    // the adapter independent from the process-global C rand() state used by
    // the original executable.
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (value ^ (value >> 31)) as u32
}

fn next_native_random_15(state: &mut u64) -> u32 {
    next_firefly_random(state) & 0x7fff
}

fn random_screen_shake_offset(
    pattern: u32,
    amplitude: i32,
) -> Result<[i32; 2], MinoriRuntimeError> {
    let offset = match pattern {
        // These eight cases preserve the native switch fall-through. Cases
        // 2, 4, and 6 apply two buffer-copy helpers and therefore produce a
        // diagonal displacement rather than a single-axis approximation.
        0 => [0, -amplitude],
        1 => [-amplitude, 0],
        2 => [amplitude, amplitude],
        3 => [amplitude, 0],
        4 => [-amplitude, amplitude],
        5 => [0, amplitude],
        6 => [amplitude, -amplitude],
        7 => [0, -amplitude],
        _ => return Err(MinoriRuntimeError::ScreenShake),
    };
    Ok(offset)
}

fn new_snow_h_state(
    random_state: &mut u64,
) -> Result<MinoriSecondaryEffectState, MinoriRuntimeError> {
    let mut particles = Vec::with_capacity(MINORI_SNOW_H_PARTICLE_COUNT);
    for _ in 0..MINORI_SNOW_H_PARTICLE_COUNT {
        let mut particle = MinoriSnowHParticle {
            fixed_position: [0, 0],
            horizontal_velocity: 0,
            vertical_velocity: 0,
            vertical_positive: false,
            kind: 0,
            position: [0, 0],
            active: false,
        };
        initialize_snow_h_particle(&mut particle, random_state)?;
        particles.push(particle);
    }
    Ok(MinoriSecondaryEffectState {
        kind: MinoriSecondaryEffectKind::SnowHorizontal,
        resources: [
            "minori:/sys/snowS.png".into(),
            "minori:/sys/snowM.png".into(),
            "minori:/sys/snowL.png".into(),
        ],
        ending: false,
        alpha_256: 0,
        fade_elapsed_ns: 0,
        motion_elapsed_ns: 0,
        particles,
    })
}

fn initialize_snow_h_particle(
    particle: &mut MinoriSnowHParticle,
    random_state: &mut u64,
) -> Result<(), MinoriRuntimeError> {
    let x = next_native_random_15(random_state)
        % u32::try_from(MINORI_SNOW_H_STAGE_WIDTH + 100)
            .map_err(|_| MinoriRuntimeError::Overflow)?;
    let y = next_native_random_15(random_state)
        % u32::try_from(MINORI_SNOW_H_STAGE_HEIGHT + 60)
            .map_err(|_| MinoriRuntimeError::Overflow)?;
    let horizontal_velocity = next_native_random_15(random_state)
        .checked_mul(2)
        .and_then(|value| value.checked_add(0x4000))
        .ok_or(MinoriRuntimeError::Overflow)?;
    let vertical_velocity = next_native_random_15(random_state) % 0x2000;
    let vertical_positive = next_native_random_15(random_state) > 0x3fff;
    let kind = if horizontal_velocity < 0x5000 {
        0
    } else if horizontal_velocity < 0xa000 {
        1
    } else {
        2
    };
    particle.fixed_position = [i64::from(x) << 16, i64::from(y) << 16];
    particle.horizontal_velocity = horizontal_velocity;
    particle.vertical_velocity = vertical_velocity;
    particle.vertical_positive = vertical_positive;
    particle.kind = kind;
    particle.position = snow_h_visible_position(particle.fixed_position)?;
    particle.active = true;
    Ok(())
}

fn advance_snow_h_particle(
    particle: &mut MinoriSnowHParticle,
    elapsed_ms: u64,
) -> Result<(), MinoriRuntimeError> {
    if !particle.active {
        return Err(MinoriRuntimeError::SecondaryEffect);
    }
    let horizontal_delta = u64::from(particle.horizontal_velocity)
        .checked_mul(elapsed_ms)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(MinoriRuntimeError::Overflow)?;
    let vertical_delta = u64::from(particle.vertical_velocity)
        .checked_mul(elapsed_ms)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(MinoriRuntimeError::Overflow)?;
    particle.fixed_position[0] = particle.fixed_position[0]
        .checked_add(horizontal_delta)
        .ok_or(MinoriRuntimeError::Overflow)?;
    particle.fixed_position[1] = if particle.vertical_positive {
        particle.fixed_position[1].checked_add(vertical_delta)
    } else {
        particle.fixed_position[1].checked_sub(vertical_delta)
    }
    .ok_or(MinoriRuntimeError::Overflow)?;
    particle.position = snow_h_visible_position(particle.fixed_position)?;
    Ok(())
}

fn snow_h_visible_position(fixed: [i64; 2]) -> Result<[i32; 2], MinoriRuntimeError> {
    let x = (fixed[0] >> 16)
        .checked_sub(50)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(MinoriRuntimeError::Overflow)?;
    let y = (fixed[1] >> 16)
        .checked_sub(30)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(MinoriRuntimeError::Overflow)?;
    Ok([x, y])
}

fn firefly_random_bounded(state: &mut u64, bound: u32) -> Result<i32, MinoriRuntimeError> {
    if bound == 0 {
        return Err(MinoriRuntimeError::Firefly);
    }
    Ok((next_firefly_random(state) % bound) as i32)
}

fn respawn_firefly_particle(
    particle: &mut MinoriFireflyParticle,
    duration_ms: u32,
    random_state: &mut u64,
) -> Result<(), MinoriRuntimeError> {
    if duration_ms == 0 {
        return Err(MinoriRuntimeError::Firefly);
    }
    for point in &mut particle.control_points {
        let x = firefly_random_bounded(
            random_state,
            u32::try_from(MINORI_FIREFLY_STAGE_WIDTH * 2)
                .map_err(|_| MinoriRuntimeError::Overflow)?,
        )? - MINORI_FIREFLY_STAGE_WIDTH / 2;
        let y = firefly_random_bounded(
            random_state,
            u32::try_from(MINORI_FIREFLY_STAGE_HEIGHT * 2)
                .map_err(|_| MinoriRuntimeError::Overflow)?,
        )? - MINORI_FIREFLY_STAGE_HEIGHT / 2;
        *point = [x, y];
    }
    let lifetime_offset = next_firefly_random(random_state) % duration_ms;
    let lifetime_ms = duration_ms
        .checked_add(lifetime_offset)
        .and_then(|value| value.checked_sub(duration_ms / 2))
        .ok_or(MinoriRuntimeError::Overflow)?;
    particle.kind = (next_firefly_random(random_state) % 3) as u8;
    particle.elapsed_ns = 0;
    particle.lifetime_ns = u64::from(lifetime_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    particle.position = particle.control_points[0];
    particle.opacity_255 = 0;
    particle.active = true;
    Ok(())
}

fn fixed_firefly_parameter(elapsed_ns: u64, lifetime_ns: u64) -> Result<u32, MinoriRuntimeError> {
    if lifetime_ns == 0 {
        return Err(MinoriRuntimeError::Firefly);
    }
    let numerator = u128::from(elapsed_ns.min(lifetime_ns))
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    u32::try_from(numerator / u128::from(lifetime_ns)).map_err(|_| MinoriRuntimeError::Overflow)
}

fn firefly_curve_position(
    control_points: &[[i32; 2]; MINORI_FIREFLY_CONTROL_POINTS],
    parameter: u32,
) -> Result<[i32; 2], MinoriRuntimeError> {
    const SCALE: i128 = 1_000_000;
    if parameter > SCALE as u32 {
        return Err(MinoriRuntimeError::Firefly);
    }
    let t = i128::from(parameter);
    let one_minus_t = SCALE - t;
    let mut points = control_points.map(|point| [i128::from(point[0]), i128::from(point[1])]);
    // De Casteljau is the exact degree-six Bernstein curve used by the
    // original seven control-point particle path, evaluated in fixed-point so
    // the snapshot hash is independent of a platform's floating-point mode.
    for level in 1..MINORI_FIREFLY_CONTROL_POINTS {
        for index in 0..(MINORI_FIREFLY_CONTROL_POINTS - level) {
            let (left, right) = points.split_at_mut(index + 1);
            let current = &mut left[index];
            let next = &right[0];
            for (value, next_value) in current.iter_mut().zip(next.iter()) {
                *value = ((*value)
                    .checked_mul(one_minus_t)
                    .and_then(|left| {
                        next_value
                            .checked_mul(t)
                            .and_then(|right| left.checked_add(right))
                    })
                    .ok_or(MinoriRuntimeError::Overflow)?)
                    / SCALE;
            }
        }
    }
    Ok([
        i32::try_from(points[0][0]).map_err(|_| MinoriRuntimeError::Overflow)?,
        i32::try_from(points[0][1]).map_err(|_| MinoriRuntimeError::Overflow)?,
    ])
}

fn firefly_particle_opacity(parameter: u32) -> u16 {
    let value = if parameter < 255_000 {
        parameter.saturating_mul(255) / 255_000
    } else if parameter > 755_000 {
        (1_000_000u32.saturating_sub(parameter)).saturating_mul(255) / 245_000
    } else {
        255
    };
    value.min(255) as u16
}

fn validate_runtime_state(state: &MinoriRuntimeState) -> Result<(), MinoriRuntimeError> {
    validate_backlog_state(state)?;
    if let Some(message) = state.message.as_ref() {
        if message.voice.is_some() != message.voice_hash.is_some() {
            return Err(MinoriRuntimeError::AudioResource);
        }
        if let Some(voice) = message.voice.as_ref() {
            validate_message_voice(voice)?;
        }
    }
    validate_config_state(&state.system_ui.config)?;
    if let Some(draft) = state.system_ui.config_draft.as_ref() {
        validate_config_state(draft)?;
    }
    if (state.system_ui.page == MinoriSystemPage::Config) != state.system_ui.config_draft.is_some()
        || !(0..=1280).contains(&state.system_ui.pointer_x)
        || !(0..=720).contains(&state.system_ui.pointer_y)
    {
        return Err(MinoriRuntimeError::State);
    }
    let verified_unlocks = MINORI_ROUTE_CLEAR_FLAGS
        .map(|flag| Hash256::from_sha256(flag.as_bytes()))
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if state.gallery_unlocks.len() > verified_unlocks.len()
        || state
            .gallery_unlocks
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || state
            .gallery_unlocks
            .iter()
            .any(|identity| !verified_unlocks.contains(identity))
    {
        return Err(MinoriRuntimeError::State);
    }
    if let Some(stage) = state.stage.as_ref() {
        validate_stage_state(stage)?;
    }
    validate_character_state(&state.characters)?;
    let transition_slot = state
        .characters
        .iter()
        .find_map(|(slot_id, character)| character.transition.as_ref().map(|_| *slot_id));
    match (&state.wait, transition_slot) {
        (Some(MinoriWaitState::CharacterTransition { slot_id, .. }), Some(transition_slot))
            if *slot_id == transition_slot => {}
        (Some(MinoriWaitState::CharacterTransition { .. }), _) | (_, Some(_)) => {
            return Err(MinoriRuntimeError::Character)
        }
        _ => {}
    }
    if let Some(scroll) = state.axis_scroll.as_ref() {
        validate_axis_scroll_state(
            scroll,
            state.stage.as_ref().ok_or(MinoriRuntimeError::AxisScroll)?,
        )?;
        if state.linear_scroll.is_some() || state.scroll_xf.is_some() || state.wscroll2.is_some() {
            return Err(MinoriRuntimeError::AxisScroll);
        }
    }
    if matches!(state.wait, Some(MinoriWaitState::AxisScroll { .. })) && state.axis_scroll.is_none()
    {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    if let Some(scroll) = state.linear_scroll.as_ref() {
        validate_linear_scroll_state(
            scroll,
            state
                .stage
                .as_ref()
                .ok_or(MinoriRuntimeError::LinearScroll)?,
        )?;
        if state.axis_scroll.is_some() || state.scroll_xf.is_some() || state.wscroll2.is_some() {
            return Err(MinoriRuntimeError::LinearScroll);
        }
    }
    if matches!(state.wait, Some(MinoriWaitState::LinearScroll { .. }))
        && state.linear_scroll.is_none()
    {
        return Err(MinoriRuntimeError::LinearScroll);
    }
    if let Some(scroll) = state.scroll_xf.as_ref() {
        validate_scroll_xf_state(scroll)?;
        if state.stage.is_none() {
            return Err(MinoriRuntimeError::ScrollXf);
        }
    }
    if let Some(scroll) = state.wscroll2.as_ref() {
        validate_wscroll2_state(scroll)?;
        if state.stage.is_none() || state.scroll_xf.is_some() {
            return Err(MinoriRuntimeError::WScroll2);
        }
    }
    if usize::from(state.effect.is_some())
        + usize::from(state.firefly.is_some())
        + usize::from(state.wscroll2.is_some())
        > 1
    {
        return Err(MinoriRuntimeError::State);
    }
    if let Some(effect) = state.secondary_effect.as_ref() {
        validate_secondary_effect_state(effect)?;
    }
    if let Some(shake) = state.screen_shake.as_ref() {
        validate_screen_shake_state(shake)?;
    }
    let Some(firefly) = state.firefly.as_ref() else {
        return Ok(());
    };
    if state.effect.is_some()
        || !(1..=MINORI_FIREFLY_MAX_PARTICLES_U32).contains(&firefly.target_count)
        || usize::try_from(firefly.target_count).map_err(|_| MinoriRuntimeError::Overflow)?
            != firefly.particles.len()
        || firefly.particles.len() > MINORI_FIREFLY_MAX_PARTICLES
        || !(1..=MINORI_FIREFLY_MAX_DURATION_MS).contains(&firefly.duration_ms)
        || firefly.fade_alpha_256 > MINORI_FIREFLY_FADE_SCALE
        || firefly.fade_elapsed_ns >= MINORI_FIREFLY_FADE_STEP_NS
    {
        return Err(MinoriRuntimeError::Firefly);
    }

    let suffixes = ["S.png", "M.png", "L.png"];
    let mut common_prefix: Option<&str> = None;
    for (resource, suffix) in firefly.resources.iter().zip(suffixes) {
        let filename = resource
            .strip_prefix("minori:/sys/")
            .ok_or(MinoriRuntimeError::Firefly)?;
        validate_scene_filename(filename).map_err(|_| MinoriRuntimeError::Firefly)?;
        let prefix = filename
            .strip_suffix(suffix)
            .filter(|prefix| !prefix.is_empty())
            .ok_or(MinoriRuntimeError::Firefly)?;
        if common_prefix
            .replace(prefix)
            .is_some_and(|value| value != prefix)
        {
            return Err(MinoriRuntimeError::Firefly);
        }
    }

    let min_x = -MINORI_FIREFLY_STAGE_WIDTH / 2;
    let max_x = MINORI_FIREFLY_STAGE_WIDTH + MINORI_FIREFLY_STAGE_WIDTH / 2 - 1;
    let min_y = -MINORI_FIREFLY_STAGE_HEIGHT / 2;
    let max_y = MINORI_FIREFLY_STAGE_HEIGHT + MINORI_FIREFLY_STAGE_HEIGHT / 2 - 1;
    for particle in &firefly.particles {
        if !particle.active
            || particle.kind >= 3
            || particle.lifetime_ns == 0
            || particle.elapsed_ns >= particle.lifetime_ns
            || particle.opacity_255 > 255
            || !(min_x..=max_x).contains(&particle.position[0])
            || !(min_y..=max_y).contains(&particle.position[1])
            || particle.control_points.iter().any(|point| {
                !(min_x..=max_x).contains(&point[0]) || !(min_y..=max_y).contains(&point[1])
            })
        {
            return Err(MinoriRuntimeError::Firefly);
        }
    }
    Ok(())
}

fn validate_config_state(config: &MinoriConfigState) -> Result<(), MinoriRuntimeError> {
    if config.message_speed_unread > 100
        || config.message_speed_read > 100
        || config.message_speed_auto_play > 100
        || config.font_index != 0
        || config.preferred_play_mode == MinoriPlayMode::Normal
        || config.bgm_volume > 100
        || config.voice_volume > 100
        || config.se_volume > 100
    {
        return Err(MinoriRuntimeError::State);
    }
    Ok(())
}

fn validate_backlog_state(state: &MinoriRuntimeState) -> Result<(), MinoriRuntimeError> {
    if state.backlog.len() > MINORI_BACKLOG_MAX_ENTRIES {
        return Err(MinoriRuntimeError::Backlog);
    }
    let mut total_bytes = 0usize;
    for entry in &state.backlog {
        if entry.text.len() > MINORI_BACKLOG_MAX_ENTRY_BYTES
            || entry
                .speaker
                .as_ref()
                .is_some_and(|speaker| speaker.len() > MINORI_BACKLOG_MAX_ENTRY_BYTES)
            || Hash256::from_sha256(entry.text.as_bytes()) != entry.text_hash
            || entry
                .speaker
                .as_ref()
                .map(|speaker| Hash256::from_sha256(speaker.as_bytes()))
                != entry.speaker_hash
            || entry.voice.is_some() != entry.voice_hash.is_some()
            || entry
                .voice
                .as_ref()
                .is_some_and(|voice| validate_message_voice(voice).is_err())
        {
            return Err(MinoriRuntimeError::Backlog);
        }
        total_bytes = total_bytes
            .checked_add(entry.text.len())
            .and_then(|bytes| bytes.checked_add(entry.speaker.as_ref().map_or(0, String::len)))
            .filter(|bytes| *bytes <= MINORI_BACKLOG_MAX_TOTAL_BYTES)
            .ok_or(MinoriRuntimeError::Backlog)?;
    }
    if state.backlog_bytes != u64::try_from(total_bytes).map_err(|_| MinoriRuntimeError::Backlog)? {
        return Err(MinoriRuntimeError::Backlog);
    }
    match state.system_ui.page {
        MinoriSystemPage::Backlog => {
            let cursor = usize::try_from(
                state
                    .system_ui
                    .backlog_cursor
                    .ok_or(MinoriRuntimeError::Backlog)?,
            )
            .map_err(|_| MinoriRuntimeError::Backlog)?;
            if cursor >= state.backlog.len() {
                return Err(MinoriRuntimeError::Backlog);
            }
        }
        _ if state.system_ui.backlog_cursor.is_some() => {
            return Err(MinoriRuntimeError::Backlog);
        }
        _ => {}
    }
    Ok(())
}

fn validate_message_voice(voice: &MinoriMessageVoice) -> Result<(), MinoriRuntimeError> {
    let resource = voice
        .resource_uri
        .strip_prefix("minori:/voice/")
        .ok_or(MinoriRuntimeError::AudioResource)?;
    validate_audio_relative_path(resource)?;
    if voice.volume_milli > 1000 || !(-1000..=1000).contains(&voice.pan_milli) {
        return Err(MinoriRuntimeError::AudioResource);
    }
    Ok(())
}

fn validate_secondary_effect_state(
    effect: &MinoriSecondaryEffectState,
) -> Result<(), MinoriRuntimeError> {
    let expected_resources = [
        "minori:/sys/snowS.png",
        "minori:/sys/snowM.png",
        "minori:/sys/snowL.png",
    ];
    if effect.kind != MinoriSecondaryEffectKind::SnowHorizontal
        || effect
            .resources
            .iter()
            .map(String::as_str)
            .ne(expected_resources)
        || effect.particles.len() != MINORI_SNOW_H_PARTICLE_COUNT
        || effect.alpha_256 > MINORI_SNOW_H_FADE_SCALE
        || effect.fade_elapsed_ns >= MINORI_SNOW_H_FADE_STEP_NS
        || effect.motion_elapsed_ns >= 1_000_000
    {
        return Err(MinoriRuntimeError::SecondaryEffect);
    }
    for particle in &effect.particles {
        let expected_kind = if particle.horizontal_velocity < 0x5000 {
            0
        } else if particle.horizontal_velocity < 0xa000 {
            1
        } else {
            2
        };
        let visible = snow_h_visible_position(particle.fixed_position)
            .map_err(|_| MinoriRuntimeError::SecondaryEffect)?;
        if !particle.active
            || !(0x4000..=0x13ffe).contains(&particle.horizontal_velocity)
            || particle.vertical_velocity >= 0x2000
            || particle.kind != expected_kind
            || particle.position != visible
            || !(-50..MINORI_SNOW_H_STAGE_WIDTH).contains(&particle.position[0])
            || !(-2048..=2048).contains(&particle.position[1])
        {
            return Err(MinoriRuntimeError::SecondaryEffect);
        }
    }
    Ok(())
}

fn validate_screen_shake_state(shake: &MinoriScreenShakeState) -> Result<(), MinoriRuntimeError> {
    let interval_ns = u64::from(shake.interval_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::ScreenShake)?;
    let expected_offset = match shake.kind {
        MinoriScreenShakeKind::Vertical if shake.update_index == 0 => Some([0, 0]),
        MinoriScreenShakeKind::Vertical if (shake.update_index - 1).is_multiple_of(2) => {
            Some([0, -shake.amplitude])
        }
        MinoriScreenShakeKind::Vertical => Some([0, shake.amplitude]),
        MinoriScreenShakeKind::Random if shake.update_index == 0 => Some([0, 0]),
        MinoriScreenShakeKind::Random => None,
    };
    let random_offset_is_valid = (0..8).any(|pattern| {
        random_screen_shake_offset(pattern, shake.amplitude)
            .is_ok_and(|offset| offset == shake.offset)
    });
    if !(1..=MINORI_SCREEN_SHAKE_MAX_AMPLITUDE).contains(&shake.amplitude)
        || !(1..=MINORI_SCREEN_SHAKE_MAX_INTERVAL_MS).contains(&shake.interval_ms)
        || shake.elapsed_ns >= interval_ns
        || shake
            .offset
            .iter()
            .any(|value| value.unsigned_abs() > shake.amplitude as u32)
        || expected_offset.is_some_and(|expected| shake.offset != expected)
        || (shake.kind == MinoriScreenShakeKind::Random
            && shake.update_index != 0
            && !random_offset_is_valid)
    {
        return Err(MinoriRuntimeError::ScreenShake);
    }
    Ok(())
}

fn validate_character_state(
    characters: &BTreeMap<u32, MinoriCharacterState>,
) -> Result<(), MinoriRuntimeError> {
    if characters.len() > MINORI_CHARACTER_MAX_SLOTS {
        return Err(MinoriRuntimeError::Character);
    }
    let mut transition_count = 0usize;
    for (slot_id, character) in characters {
        if *slot_id == 0
            || *slot_id > MINORI_CHARACTER_MAX_SLOT_ID
            || character.slot_id != *slot_id
            || character.resource_uris.len() != MINORI_CHARACTER_RESOURCE_COUNT
            || character.opacity_256 > 256
            || character
                .anchor_position
                .iter()
                .any(|value| value.unsigned_abs() > MINORI_CHARACTER_MAX_COORDINATE as u32)
        {
            return Err(MinoriRuntimeError::Character);
        }
        if let Some(transition) = character.transition.as_ref() {
            transition_count = transition_count
                .checked_add(1)
                .ok_or(MinoriRuntimeError::Overflow)?;
            let duration_ns = u64::from(transition.duration_ms)
                .checked_mul(1_000_000)
                .ok_or(MinoriRuntimeError::Overflow)?;
            if transition.duration_ms == 0
                || transition.duration_ms > MINORI_CHARACTER_MAX_TRANSITION_MS
                || transition.start_opacity_256 > 256
                || transition.target_opacity_256 > 255
                || transition.elapsed_ns > duration_ns
                || transition.completed != (transition.elapsed_ns == duration_ns)
                || character.opacity_256 != interpolate_character_opacity(transition, duration_ns)?
            {
                return Err(MinoriRuntimeError::Character);
            }
        }
        for resource_uri in &character.resource_uris {
            validate_scene_uri(resource_uri, "minori:/st/")
                .map_err(|_| MinoriRuntimeError::Character)?;
        }
    }
    if transition_count > 1 {
        return Err(MinoriRuntimeError::Character);
    }
    Ok(())
}

fn interpolate_character_opacity(
    transition: &MinoriCharacterTransitionState,
    duration_ns: u64,
) -> Result<u16, MinoriRuntimeError> {
    if duration_ns == 0 || transition.elapsed_ns > duration_ns {
        return Err(MinoriRuntimeError::Character);
    }
    let start = i128::from(transition.start_opacity_256);
    let delta = i128::from(transition.target_opacity_256) - start;
    let elapsed = i128::from(transition.elapsed_ns);
    let duration = i128::from(duration_ns);
    let value = start
        .checked_add(
            delta
                .checked_mul(elapsed)
                .ok_or(MinoriRuntimeError::Overflow)?
                / duration,
        )
        .ok_or(MinoriRuntimeError::Overflow)?;
    u16::try_from(value).map_err(|_| MinoriRuntimeError::Character)
}

fn complete_character_transition_state(
    state: &mut MinoriRuntimeState,
) -> Result<(), MinoriRuntimeError> {
    let slot_id = match state.wait.as_ref() {
        Some(MinoriWaitState::CharacterTransition { slot_id, .. }) => *slot_id,
        _ => return Err(MinoriRuntimeError::Character),
    };
    let character = state
        .characters
        .get_mut(&slot_id)
        .ok_or(MinoriRuntimeError::Character)?;
    let transition = character
        .transition
        .take()
        .ok_or(MinoriRuntimeError::Character)?;
    character.opacity_256 = transition.target_opacity_256;
    Ok(())
}

fn validate_stage_state(stage: &MinoriStageCommand) -> Result<(), MinoriRuntimeError> {
    if stage.resource_sequence.is_empty()
        || stage.resource_sequence.len() > 2
        || stage.stands.len() > 10
    {
        return Err(MinoriRuntimeError::Operand);
    }
    for resource in stage.resource_sequence.iter().flatten() {
        validate_scene_uri(resource, "minori:/bg/")?;
    }
    if let Some(background) = stage.background.as_ref() {
        validate_scene_uri(&background.resource_uri, "minori:/bg/")?;
    }
    for stand in &stage.stands {
        validate_scene_uri(&stand.resource_uri, "minori:/st/")?;
        if !stand.resource_uri.to_ascii_lowercase().ends_with(".png") {
            return Err(MinoriRuntimeError::Operand);
        }
    }
    if let Some(resource) = stage.transition.resource.as_deref() {
        validate_scene_filename(resource)?;
    }
    Ok(())
}

fn validate_scene_uri(value: &str, prefix: &str) -> Result<(), MinoriRuntimeError> {
    let filename = value
        .strip_prefix(prefix)
        .ok_or(MinoriRuntimeError::Operand)?;
    validate_scene_filename(filename)
}

fn stage_axis_position(
    stage: &MinoriStageCommand,
    axis: MinoriAxisScrollAxis,
) -> Result<i32, MinoriRuntimeError> {
    let background = stage
        .background
        .as_ref()
        .ok_or(MinoriRuntimeError::AxisScroll)?;
    Ok(match axis {
        MinoriAxisScrollAxis::Horizontal => background.x,
        MinoriAxisScrollAxis::Vertical => background.y,
    })
}

fn set_stage_axis_position(
    stage: &mut MinoriStageCommand,
    axis: MinoriAxisScrollAxis,
    value: i32,
) -> Result<(), MinoriRuntimeError> {
    let background = stage
        .background
        .as_mut()
        .ok_or(MinoriRuntimeError::AxisScroll)?;
    match axis {
        MinoriAxisScrollAxis::Horizontal => background.x = value,
        MinoriAxisScrollAxis::Vertical => background.y = value,
    }
    Ok(())
}

fn stage_position(stage: &MinoriStageCommand) -> Result<[i32; 2], MinoriRuntimeError> {
    let background = stage
        .background
        .as_ref()
        .ok_or(MinoriRuntimeError::LinearScroll)?;
    Ok([background.x, background.y])
}

fn set_stage_position(
    stage: &mut MinoriStageCommand,
    value: [i32; 2],
) -> Result<(), MinoriRuntimeError> {
    let background = stage
        .background
        .as_mut()
        .ok_or(MinoriRuntimeError::LinearScroll)?;
    background.x = value[0];
    background.y = value[1];
    Ok(())
}

fn axis_scroll_duration_ms(
    start: i32,
    target: i32,
    speed_tenths: i32,
) -> Result<u32, MinoriRuntimeError> {
    if speed_tenths == 0 {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    let distance = (i64::from(target) - i64::from(start)).unsigned_abs();
    if distance == 0 {
        return Ok(0);
    }
    let numerator = distance
        .checked_mul(10)
        .ok_or(MinoriRuntimeError::Overflow)?;
    let speed = u64::from(speed_tenths.unsigned_abs());
    u32::try_from(numerator.div_ceil(speed)).map_err(|_| MinoriRuntimeError::Overflow)
}

fn linear_scroll_duration_ms(
    start: [i32; 2],
    target: [i32; 2],
    speed_tenths: u32,
) -> Result<u32, MinoriRuntimeError> {
    if speed_tenths == 0 {
        return Err(MinoriRuntimeError::LinearScroll);
    }
    let distance = start
        .iter()
        .zip(target)
        .map(|(start, target)| (i64::from(target) - i64::from(*start)).unsigned_abs())
        .max()
        .unwrap_or(0);
    if distance == 0 {
        return Ok(0);
    }
    let numerator = distance
        .checked_mul(10)
        .ok_or(MinoriRuntimeError::Overflow)?;
    u32::try_from(numerator.div_ceil(u64::from(speed_tenths)))
        .map_err(|_| MinoriRuntimeError::Overflow)
}

fn linear_scroll_visible_position(
    scroll: &MinoriLinearScrollState,
) -> Result<[i32; 2], MinoriRuntimeError> {
    let distance = scroll
        .start
        .iter()
        .zip(scroll.target)
        .map(|(start, target)| (i64::from(target) - i64::from(*start)).unsigned_abs())
        .max()
        .unwrap_or(0);
    if distance == 0 {
        return Ok(scroll.target);
    }
    let elapsed_ms = scroll.elapsed_ns / 1_000_000;
    let travelled = elapsed_ms
        .checked_mul(u64::from(scroll.speed_tenths))
        .ok_or(MinoriRuntimeError::Overflow)?
        / 10;
    let travelled = travelled.min(distance);
    let interpolate = |start: i32, target: i32| {
        let delta = i128::from(target) - i128::from(start);
        let offset = delta
            .checked_mul(i128::from(travelled))
            .ok_or(MinoriRuntimeError::Overflow)?
            / i128::from(distance);
        i32::try_from(i128::from(start) + offset).map_err(|_| MinoriRuntimeError::Overflow)
    };
    Ok([
        interpolate(scroll.start[0], scroll.target[0])?,
        interpolate(scroll.start[1], scroll.target[1])?,
    ])
}

fn axis_scroll_visible_position(scroll: &MinoriAxisScrollState) -> Result<i32, MinoriRuntimeError> {
    if scroll.start == scroll.target {
        return Ok(scroll.target);
    }
    let elapsed_ms =
        i64::try_from(scroll.elapsed_ns / 1_000_000).map_err(|_| MinoriRuntimeError::Overflow)?;
    let delta = elapsed_ms
        .checked_mul(i64::from(scroll.speed_tenths))
        .ok_or(MinoriRuntimeError::Overflow)?
        / 10;
    let raw = i64::from(scroll.start)
        .checked_add(delta)
        .ok_or(MinoriRuntimeError::Overflow)?;
    let clamped = if scroll.speed_tenths > 0 {
        raw.min(i64::from(scroll.target))
    } else {
        raw.max(i64::from(scroll.target))
    };
    i32::try_from(clamped).map_err(|_| MinoriRuntimeError::Overflow)
}

fn complete_axis_scroll_state(state: &mut MinoriRuntimeState) -> Result<(), MinoriRuntimeError> {
    let (axis_scroll, stage) = (&mut state.axis_scroll, &mut state.stage);
    let scroll = axis_scroll.as_mut().ok_or(MinoriRuntimeError::AxisScroll)?;
    scroll.elapsed_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    scroll.current = scroll.target;
    scroll.completed = true;
    set_stage_axis_position(
        stage.as_mut().ok_or(MinoriRuntimeError::AxisScroll)?,
        scroll.axis,
        scroll.target,
    )
}

fn complete_linear_scroll_state(state: &mut MinoriRuntimeState) -> Result<(), MinoriRuntimeError> {
    let (linear_scroll, stage) = (&mut state.linear_scroll, &mut state.stage);
    let scroll = linear_scroll
        .as_mut()
        .ok_or(MinoriRuntimeError::LinearScroll)?;
    scroll.elapsed_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    scroll.current = scroll.target;
    scroll.completed = true;
    set_stage_position(
        stage.as_mut().ok_or(MinoriRuntimeError::LinearScroll)?,
        scroll.target,
    )
}

fn validate_axis_scroll_state(
    scroll: &MinoriAxisScrollState,
    stage: &MinoriStageCommand,
) -> Result<(), MinoriRuntimeError> {
    if scroll
        .start
        .unsigned_abs()
        .max(scroll.target.unsigned_abs())
        .max(scroll.current.unsigned_abs())
        > MINORI_AXIS_SCROLL_MAX_COORDINATE as u32
        || scroll.speed_tenths == 0
        || scroll.speed_tenths.unsigned_abs() > MINORI_AXIS_SCROLL_MAX_SPEED_TENTHS as u32
        || (scroll.start < scroll.target && scroll.speed_tenths < 0)
        || (scroll.start > scroll.target && scroll.speed_tenths > 0)
        || scroll.duration_ms
            != axis_scroll_duration_ms(scroll.start, scroll.target, scroll.speed_tenths)?
    {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    if scroll.elapsed_ns > duration_ns
        || scroll.completed != (scroll.elapsed_ns == duration_ns)
        || scroll.current != axis_scroll_visible_position(scroll)?
        || stage_axis_position(stage, scroll.axis)? != scroll.current
    {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    Ok(())
}

fn validate_linear_scroll_state(
    scroll: &MinoriLinearScrollState,
    stage: &MinoriStageCommand,
) -> Result<(), MinoriRuntimeError> {
    if scroll
        .start
        .iter()
        .chain(&scroll.target)
        .chain(&scroll.current)
        .any(|value| value.unsigned_abs() > MINORI_AXIS_SCROLL_MAX_COORDINATE as u32)
        || scroll.speed_tenths == 0
        || scroll.speed_tenths > MINORI_AXIS_SCROLL_MAX_SPEED_TENTHS as u32
        || scroll.duration_ms
            != linear_scroll_duration_ms(scroll.start, scroll.target, scroll.speed_tenths)?
    {
        return Err(MinoriRuntimeError::LinearScroll);
    }
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    if scroll.elapsed_ns > duration_ns
        || scroll.completed != (scroll.elapsed_ns == duration_ns)
        || scroll.current != linear_scroll_visible_position(scroll)?
        || stage_position(stage)? != scroll.current
    {
        return Err(MinoriRuntimeError::LinearScroll);
    }
    Ok(())
}

fn validate_scroll_xf_state(scroll: &MinoriScrollXfState) -> Result<(), MinoriRuntimeError> {
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    if !(1..=MINORI_SCROLL_XF_MAX_DURATION_MS).contains(&scroll.duration_ms)
        || scroll.easing > 2
        || scroll.elapsed_ns > duration_ns
        || scroll.completed != (scroll.elapsed_ns == duration_ns)
        || scroll
            .start_extent
            .iter()
            .chain(scroll.end_extent.iter())
            .chain(scroll.start_offset.iter())
            .chain(scroll.end_offset.iter())
            .any(|value| !(0..=MINORI_SCROLL_XF_MAX_EXTENT).contains(value))
    {
        return Err(MinoriRuntimeError::ScrollXf);
    }
    let (visible_extent, visible_offset) = scroll_xf_visible_state(scroll)?;
    if scroll.visible_extent != visible_extent || scroll.visible_offset != visible_offset {
        return Err(MinoriRuntimeError::ScrollXf);
    }
    Ok(())
}

fn scroll_xf_visible_state(
    scroll: &MinoriScrollXfState,
) -> Result<([i32; 2], [i32; 2]), MinoriRuntimeError> {
    let duration_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    let linear = u64::try_from(
        u128::from(scroll.elapsed_ns)
            .checked_mul(u128::from(MINORI_SCROLL_XF_SCALE))
            .ok_or(MinoriRuntimeError::Overflow)?
            / u128::from(duration_ns),
    )
    .map_err(|_| MinoriRuntimeError::Overflow)?
    .min(MINORI_SCROLL_XF_SCALE);
    let squared = u64::try_from(
        u128::from(linear)
            .checked_mul(u128::from(linear))
            .ok_or(MinoriRuntimeError::Overflow)?
            / u128::from(MINORI_SCROLL_XF_SCALE),
    )
    .map_err(|_| MinoriRuntimeError::Overflow)?;
    let eased = match scroll.easing {
        0 => linear,
        1 => squared,
        2 => linear
            .checked_mul(2)
            .and_then(|value| value.checked_sub(squared))
            .ok_or(MinoriRuntimeError::Overflow)?,
        _ => return Err(MinoriRuntimeError::ScrollXf),
    };
    Ok((
        interpolate_scroll_pair(scroll.start_extent, scroll.end_extent, eased)?,
        interpolate_scroll_pair(scroll.start_offset, scroll.end_offset, eased)?,
    ))
}

fn validate_wscroll2_state(scroll: &MinoriWScroll2State) -> Result<(), MinoriRuntimeError> {
    validate_scene_uri(&scroll.sync_resource_uri, "minori:/st/")
        .map_err(|_| MinoriRuntimeError::WScroll2)?;
    if !(1..=MINORI_WSCROLL2_MAX_PERIOD_TICKS).contains(&scroll.period_ticks)
        || scroll.speed_tenths.unsigned_abs() > MINORI_WSCROLL2_MAX_SPEED_TENTHS as u32
    {
        return Err(MinoriRuntimeError::WScroll2);
    }
    let (elapsed_ticks, foreground_offset, background_offset, background_remainder) =
        wscroll2_visible_state(scroll.elapsed_ns, scroll.speed_tenths)?;
    if scroll.elapsed_ticks != elapsed_ticks
        || scroll.foreground_offset != foreground_offset
        || scroll.background_offset != background_offset
        || scroll.background_remainder != background_remainder
    {
        return Err(MinoriRuntimeError::WScroll2);
    }
    Ok(())
}

fn wscroll2_visible_state(
    elapsed_ns: u64,
    speed_tenths: i32,
) -> Result<(u64, i64, i64, i64), MinoriRuntimeError> {
    let elapsed_ticks = u64::try_from(
        u128::from(elapsed_ns)
            .checked_mul(u128::from(MINORI_WSCROLL2_TICKS_PER_SECOND))
            .ok_or(MinoriRuntimeError::Overflow)?
            / 1_000_000_000u128,
    )
    .map_err(|_| MinoriRuntimeError::Overflow)?;
    let foreground_offset = i64::try_from(
        i128::from(elapsed_ticks)
            .checked_mul(i128::from(speed_tenths))
            .ok_or(MinoriRuntimeError::Overflow)?
            / 10,
    )
    .map_err(|_| MinoriRuntimeError::Overflow)?;
    let background_offset = if foreground_offset == 0 {
        0
    } else {
        (foreground_offset - foreground_offset.signum()) / 5
    };
    let background_remainder = foreground_offset
        .checked_sub(
            background_offset
                .checked_mul(5)
                .ok_or(MinoriRuntimeError::Overflow)?,
        )
        .ok_or(MinoriRuntimeError::Overflow)?;
    Ok((
        elapsed_ticks,
        foreground_offset,
        background_offset,
        background_remainder,
    ))
}

fn interpolate_scroll_pair(
    start: [i32; 2],
    end: [i32; 2],
    t: u64,
) -> Result<[i32; 2], MinoriRuntimeError> {
    let interpolate = |start: i32, end: i32| {
        let delta = i64::from(end) - i64::from(start);
        let scaled = i128::from(delta)
            .checked_mul(i128::from(t))
            .ok_or(MinoriRuntimeError::Overflow)?
            / i128::from(MINORI_SCROLL_XF_SCALE);
        i32::try_from(i128::from(start) + scaled).map_err(|_| MinoriRuntimeError::Overflow)
    };
    Ok([
        interpolate(start[0], end[0])?,
        interpolate(start[1], end[1])?,
    ])
}

fn new_firefly_state(
    prefix: &str,
    target_count: u32,
    duration_ms: u32,
    random_state: &mut u64,
) -> Result<MinoriFireflyState, MinoriRuntimeError> {
    if !(1..=MINORI_FIREFLY_MAX_PARTICLES_U32).contains(&target_count)
        || !(1..=MINORI_FIREFLY_MAX_DURATION_MS).contains(&duration_ms)
    {
        return Err(MinoriRuntimeError::Firefly);
    }
    let resources = [
        format!("minori:/sys/{prefix}S.png"),
        format!("minori:/sys/{prefix}M.png"),
        format!("minori:/sys/{prefix}L.png"),
    ];
    let mut particles = Vec::with_capacity(target_count as usize);
    for _ in 0..target_count {
        let mut particle = MinoriFireflyParticle {
            control_points: [[0; 2]; MINORI_FIREFLY_CONTROL_POINTS],
            kind: 0,
            elapsed_ns: 0,
            lifetime_ns: 0,
            position: [0, 0],
            opacity_255: 0,
            active: false,
        };
        respawn_firefly_particle(&mut particle, duration_ms, random_state)?;
        particles.push(particle);
    }
    Ok(MinoriFireflyState {
        resources,
        target_count,
        duration_ms,
        ending: false,
        fade_alpha_256: 0,
        fade_elapsed_ns: 0,
        particles,
    })
}

fn execute_transition(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let [mode, resource, duration] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::Operand);
    };
    let mode = mode
        .parse::<i32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let duration_ticks = duration
        .parse::<u32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let resource = if resource == "*" {
        None
    } else {
        validate_scene_filename(resource)?;
        Some(resource.clone())
    };
    state.transition = MinoriTransitionState {
        mode,
        resource,
        duration_ticks,
    };
    // Both commands replace the same native presentation-mode slot. A new
    // transition therefore terminates an active shake instead of allowing a
    // private adapter effect to leak into later scenes.
    state.screen_shake = None;
    Ok(None)
}

fn execute_stage(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.len() < 4 || tokens.len() > 26 {
        return Err(MinoriRuntimeError::Operand);
    }
    let resource_sequence = parse_stage_resource_sequence(&tokens[0])?;
    let mut cursor = 1usize;
    let mut reference_position = None;
    if tokens.len() >= 6 && !tokens[1].contains('.') && !tokens[2].contains('.') {
        if let (Ok(x), Ok(y)) = (tokens[1].parse::<i32>(), tokens[2].parse::<i32>()) {
            reference_position = Some([x, y]);
            cursor = 3;
        }
    }
    if cursor + 3 > tokens.len() || !(tokens.len() - (cursor + 3)).is_multiple_of(2) {
        return Err(MinoriRuntimeError::Operand);
    }
    let background_name = &tokens[cursor];
    let background_x = tokens[cursor + 1]
        .parse::<i32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let background_y = tokens[cursor + 2]
        .parse::<i32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    cursor += 3;

    let background = stage_layer("bg", background_name, background_x, background_y)?;
    let mut stands = Vec::with_capacity((tokens.len() - cursor) / 2);
    while cursor < tokens.len() {
        let filename = &tokens[cursor];
        validate_scene_filename(filename)?;
        let (position, resource_parameter) = parse_stand_position_spec(&tokens[cursor + 1])?;
        stands.push(MinoriStandLayer {
            resource_uri: format!("minori:/st/{filename}"),
            position,
            resource_parameter,
        });
        cursor += 2;
    }
    if stands.len() > 10 {
        return Err(MinoriRuntimeError::Operand);
    }

    // Native CCharLayer finalizes its transient character table at the scene
    // boundary. Only slots explicitly marked by `.char keep` survive into the
    // next stage, and the marker is consumed exactly once. Without this
    // boundary the runtime retains every character loaded by the route and the
    // shared renderer atlas eventually fills with historical stand textures.
    state.characters.retain(|_, character| {
        let retained = character.keep_once;
        character.keep_once = false;
        retained
    });
    state.scroll_xf = None;
    state.axis_scroll = None;
    state.linear_scroll = None;
    next_effect_sequence(state)?;
    let stage = MinoriStageCommand {
        resource_sequence,
        reference_position,
        background,
        stands,
        transition: state.transition.clone(),
    };
    validate_stage_state(&stage)?;
    state.stage = Some(stage.clone());
    Ok(Some(MinoriVmEvent::Stage(stage)))
}

fn execute_axis_scroll(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
    axis: MinoriAxisScrollAxis,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::AxisScroll)?;
    if tokens.len() > 2 {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    if state.linear_scroll.is_some()
        || state.scroll_xf.is_some()
        || state.wscroll2.is_some()
        || state
            .axis_scroll
            .as_ref()
            .is_some_and(|scroll| !scroll.completed)
    {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    let target = tokens
        .first()
        .map_or(Ok(0), |value| value.parse::<i32>())
        .map_err(|_| MinoriRuntimeError::AxisScroll)?;
    let speed_tenths = tokens
        .get(1)
        .map_or(Ok(10), |value| value.parse::<i32>())
        .map_err(|_| MinoriRuntimeError::AxisScroll)?;
    if !(-MINORI_AXIS_SCROLL_MAX_COORDINATE..=MINORI_AXIS_SCROLL_MAX_COORDINATE).contains(&target)
        || speed_tenths == 0
        || !(-MINORI_AXIS_SCROLL_MAX_SPEED_TENTHS..=MINORI_AXIS_SCROLL_MAX_SPEED_TENTHS)
            .contains(&speed_tenths)
    {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    let stage = state.stage.as_ref().ok_or(MinoriRuntimeError::AxisScroll)?;
    let start = stage_axis_position(stage, axis)?;
    if !(-MINORI_AXIS_SCROLL_MAX_COORDINATE..=MINORI_AXIS_SCROLL_MAX_COORDINATE).contains(&start)
        || (start < target && speed_tenths < 0)
        || (start > target && speed_tenths > 0)
    {
        return Err(MinoriRuntimeError::AxisScroll);
    }
    let duration_ms = axis_scroll_duration_ms(start, target, speed_tenths)?;
    let completed = start == target;
    let scroll = MinoriAxisScrollState {
        axis,
        start,
        target,
        speed_tenths,
        duration_ms,
        elapsed_ns: 0,
        current: start,
        completed,
    };
    validate_axis_scroll_state(&scroll, stage)?;
    state.axis_scroll = Some(scroll);
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::AxisScroll(MinoriAxisScrollFrame {
        sequence,
    })))
}

fn execute_linear_scroll(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::LinearScroll)?;
    let [target_x, target_y, speed_tenths] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::LinearScroll);
    };
    if state.axis_scroll.is_some()
        || state.scroll_xf.is_some()
        || state.wscroll2.is_some()
        || state
            .linear_scroll
            .as_ref()
            .is_some_and(|scroll| !scroll.completed)
    {
        return Err(MinoriRuntimeError::LinearScroll);
    }
    let parse_coordinate = |value: &str| {
        value
            .parse::<i32>()
            .ok()
            .filter(|value| value.unsigned_abs() <= MINORI_AXIS_SCROLL_MAX_COORDINATE as u32)
            .ok_or(MinoriRuntimeError::LinearScroll)
    };
    let target = [parse_coordinate(target_x)?, parse_coordinate(target_y)?];
    let speed_tenths = speed_tenths
        .parse::<u32>()
        .ok()
        .filter(|value| *value != 0 && *value <= MINORI_AXIS_SCROLL_MAX_SPEED_TENTHS as u32)
        .ok_or(MinoriRuntimeError::LinearScroll)?;
    let stage = state
        .stage
        .as_ref()
        .ok_or(MinoriRuntimeError::LinearScroll)?;
    let start = stage_position(stage)?;
    let duration_ms = linear_scroll_duration_ms(start, target, speed_tenths)?;
    let completed = start == target;
    let scroll = MinoriLinearScrollState {
        start,
        target,
        speed_tenths,
        duration_ms,
        elapsed_ns: 0,
        current: start,
        completed,
    };
    validate_linear_scroll_state(&scroll, stage)?;
    state.linear_scroll = Some(scroll);
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::LinearScroll(MinoriLinearScrollFrame {
        sequence,
    })))
}

fn execute_scroll_xf(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::ScrollXf)?;
    let [start_width, start_height, end_width, end_height, start_x, start_y, end_x, end_y, duration, easing] =
        tokens.as_slice()
    else {
        return Err(MinoriRuntimeError::ScrollXf);
    };
    if state.stage.is_none()
        || state.linear_scroll.is_some()
        || state.wscroll2.is_some()
        || state
            .axis_scroll
            .as_ref()
            .is_some_and(|scroll| !scroll.completed)
    {
        return Err(MinoriRuntimeError::ScrollXf);
    }
    state.axis_scroll = None;
    let parse_extent = |value: &str| {
        value
            .parse::<i32>()
            .ok()
            .filter(|value| (0..=MINORI_SCROLL_XF_MAX_EXTENT).contains(value))
            .ok_or(MinoriRuntimeError::ScrollXf)
    };
    let duration_ms = duration
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=MINORI_SCROLL_XF_MAX_DURATION_MS).contains(value))
        .ok_or(MinoriRuntimeError::ScrollXf)?;
    let easing = easing
        .parse::<u8>()
        .ok()
        .filter(|value| *value <= 2)
        .ok_or(MinoriRuntimeError::ScrollXf)?;
    let scroll = MinoriScrollXfState {
        start_extent: [parse_extent(start_width)?, parse_extent(start_height)?],
        end_extent: [parse_extent(end_width)?, parse_extent(end_height)?],
        start_offset: [parse_extent(start_x)?, parse_extent(start_y)?],
        end_offset: [parse_extent(end_x)?, parse_extent(end_y)?],
        duration_ms,
        easing,
        elapsed_ns: 0,
        completed: false,
        visible_extent: [parse_extent(start_width)?, parse_extent(start_height)?],
        visible_offset: [parse_extent(start_x)?, parse_extent(start_y)?],
    };
    validate_scroll_xf_state(&scroll)?;
    state.scroll_xf = Some(scroll);
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::ScrollXf(MinoriScrollXfFrame {
        sequence,
    })))
}

fn execute_end_scroll(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::ScrollXf)?;
    let [finish] = tokens.as_slice() else {
        return Err(MinoriRuntimeError::ScrollXf);
    };
    let force_finish = finish
        .as_bytes()
        .first()
        .is_some_and(|value| matches!(value, b'1'..=b'9' | b't' | b'T'));
    if state
        .axis_scroll
        .as_ref()
        .is_some_and(|scroll| !scroll.completed)
    {
        if force_finish {
            complete_axis_scroll_state(state)?;
            let sequence = next_effect_sequence(state)?;
            return Ok(Some(MinoriVmEvent::AxisScroll(MinoriAxisScrollFrame {
                sequence,
            })));
        }
        let scroll = state
            .axis_scroll
            .as_ref()
            .ok_or(MinoriRuntimeError::AxisScroll)?;
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let remaining_ns = duration_ns
            .checked_sub(scroll.elapsed_ns)
            .filter(|remaining| *remaining > 0)
            .ok_or(MinoriRuntimeError::AxisScroll)?;
        let milliseconds = u32::try_from(remaining_ns.div_ceil(1_000_000))
            .map_err(|_| MinoriRuntimeError::Overflow)?;
        let wait = MinoriWaitState::AxisScroll {
            token_id: format!("minori.scroll.{}", state.instruction_count),
            milliseconds,
        };
        state.wait = Some(wait.clone());
        return Ok(Some(MinoriVmEvent::Wait(wait)));
    }
    if state
        .linear_scroll
        .as_ref()
        .is_some_and(|scroll| !scroll.completed)
    {
        if force_finish {
            complete_linear_scroll_state(state)?;
            let sequence = next_effect_sequence(state)?;
            return Ok(Some(MinoriVmEvent::LinearScroll(MinoriLinearScrollFrame {
                sequence,
            })));
        }
        let scroll = state
            .linear_scroll
            .as_ref()
            .ok_or(MinoriRuntimeError::LinearScroll)?;
        let duration_ns = u64::from(scroll.duration_ms)
            .checked_mul(1_000_000)
            .ok_or(MinoriRuntimeError::Overflow)?;
        let remaining_ns = duration_ns
            .checked_sub(scroll.elapsed_ns)
            .filter(|remaining| *remaining > 0)
            .ok_or(MinoriRuntimeError::LinearScroll)?;
        let milliseconds = u32::try_from(remaining_ns.div_ceil(1_000_000))
            .map_err(|_| MinoriRuntimeError::Overflow)?;
        let wait = MinoriWaitState::LinearScroll {
            token_id: format!("minori.scroll.{}", state.instruction_count),
            milliseconds,
        };
        state.wait = Some(wait.clone());
        return Ok(Some(MinoriVmEvent::Wait(wait)));
    }
    let Some(scroll) = state.scroll_xf.as_mut() else {
        return Ok(None);
    };
    if !force_finish || scroll.completed {
        return Ok(None);
    }
    scroll.elapsed_ns = u64::from(scroll.duration_ms)
        .checked_mul(1_000_000)
        .ok_or(MinoriRuntimeError::Overflow)?;
    scroll.completed = true;
    scroll.visible_extent = scroll.end_extent;
    scroll.visible_offset = scroll.end_offset;
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::ScrollXf(MinoriScrollXfFrame {
        sequence,
    })))
}

fn parse_stage_resource_sequence(value: &str) -> Result<Vec<Option<String>>, MinoriRuntimeError> {
    let tokens = value.split(':').collect::<Vec<_>>();
    if tokens.is_empty() || tokens.len() > 2 || tokens.iter().any(|token| token.is_empty()) {
        return Err(MinoriRuntimeError::Operand);
    }
    tokens
        .into_iter()
        .map(|token| {
            if token == "*" {
                Ok(None)
            } else {
                validate_scene_filename(token)?;
                Ok(Some(format!("minori:/bg/{token}")))
            }
        })
        .collect()
}

fn parse_stand_position_spec(value: &str) -> Result<(i32, i32), MinoriRuntimeError> {
    let mut components = value.split(',');
    let position = components
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(MinoriRuntimeError::Operand)?
        .parse::<i32>()
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let resource_parameter = components
        .next()
        .map(|value| {
            if value.is_empty() {
                return Err(MinoriRuntimeError::Operand);
            }
            value
                .parse::<i32>()
                .map_err(|_| MinoriRuntimeError::Operand)
        })
        .transpose()?
        .unwrap_or_default();
    if components.next().is_some() {
        return Err(MinoriRuntimeError::Operand);
    }
    Ok((position, resource_parameter))
}

fn stage_layer(
    role: &str,
    filename: &str,
    x: i32,
    y: i32,
) -> Result<Option<MinoriStageLayer>, MinoriRuntimeError> {
    if filename == "*" {
        return Ok(None);
    }
    validate_scene_filename(filename)?;
    Ok(Some(MinoriStageLayer {
        resource_uri: format!("minori:/{role}/{filename}"),
        x,
        y,
    }))
}

fn validate_scene_filename(value: &str) -> Result<(), MinoriRuntimeError> {
    if value.is_empty()
        || value.len() > 256
        || value.starts_with('/')
        || value.contains(['/', '\\', ':', '\0'])
        || value == "."
        || value == ".."
    {
        return Err(MinoriRuntimeError::Operand);
    }
    Ok(())
}

fn execute_play_se(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
    loop_stream_id: u32,
    bus: &str,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MinoriRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource == "*" {
        let fade_out_ms = parse_optional_command_integer(tokens.get(3), 2, 2)?;
        return stop_audio_stream(state, loop_stream_id, fade_out_ms);
    }
    validate_audio_relative_path(&spec.resource)?;
    let repeat = tokens
        .get(1)
        .and_then(|token| token.as_bytes().first())
        .is_some_and(|byte| *byte == b't');
    let fade_in_ms = parse_optional_command_integer(tokens.get(2), 2, 2)?;
    let fade_out_ms = parse_optional_command_integer(tokens.get(3), 2, 2)?;
    let volume_milli = spec.volume_percent * 10;
    let pan_milli = spec.pan_percent * 10;
    let resource_uri = format!("minori:/se/{}", spec.resource);
    let stream_id = if repeat {
        loop_stream_id
    } else {
        let ordinal =
            u32::try_from(state.instruction_count).map_err(|_| MinoriRuntimeError::Overflow)?;
        0x1000_0000u32
            .checked_add(
                ordinal
                    .checked_mul(4)
                    .and_then(|value| value.checked_add(loop_stream_id))
                    .ok_or(MinoriRuntimeError::Overflow)?,
            )
            .ok_or(MinoriRuntimeError::Overflow)?
    };
    let mut commands = Vec::new();
    if repeat {
        match state.audio.get(&loop_stream_id).cloned() {
            Some(current) if current.playing && current.resource_uri == resource_uri => {
                if current.volume_milli != volume_milli || current.pan_milli != pan_milli {
                    commands.push(MinoriAudioCommand::SetParams {
                        sequence: next_effect_sequence(state)?,
                        stream_id,
                        volume: f32::from(volume_milli) / 1000.0,
                        pan: f32::from(pan_milli) / 1000.0,
                        repeat,
                    });
                }
            }
            Some(current) if current.playing => {
                commands.push(MinoriAudioCommand::Stop {
                    sequence: next_effect_sequence(state)?,
                    stream_id,
                    fade_ms: fade_out_ms,
                });
                append_audio_load_and_play(
                    state,
                    &mut commands,
                    stream_id,
                    &resource_uri,
                    volume_milli,
                    pan_milli,
                    repeat,
                    fade_in_ms,
                )?;
            }
            _ => append_audio_load_and_play(
                state,
                &mut commands,
                stream_id,
                &resource_uri,
                volume_milli,
                pan_milli,
                repeat,
                fade_in_ms,
            )?,
        }
    } else {
        append_audio_load_and_play(
            state,
            &mut commands,
            stream_id,
            &resource_uri,
            volume_milli,
            pan_milli,
            repeat,
            fade_in_ms,
        )?;
    }
    state.audio.insert(
        stream_id,
        MinoriAudioState {
            bus: bus.into(),
            encoding: MinoriAudioEncoding::Ogg,
            resource_uri,
            looped: repeat,
            volume_milli,
            pan_milli,
            playing: true,
            continuation_pts: 0,
        },
    );
    Ok(Some(MinoriVmEvent::Audio { commands }))
}

#[allow(clippy::too_many_arguments)]
fn append_audio_load_and_play(
    state: &mut MinoriRuntimeState,
    commands: &mut Vec<MinoriAudioCommand>,
    stream_id: u32,
    resource_uri: &str,
    volume_milli: u16,
    pan_milli: i16,
    repeat: bool,
    fade_in_ms: u32,
) -> Result<(), MinoriRuntimeError> {
    commands.push(MinoriAudioCommand::LoadResource {
        sequence: next_effect_sequence(state)?,
        stream_id,
        encoding: MinoriAudioEncoding::Ogg,
        resource_uri: resource_uri.into(),
    });
    commands.push(MinoriAudioCommand::Play {
        sequence: next_effect_sequence(state)?,
        stream_id,
        volume: f32::from(volume_milli) / 1000.0,
        pan: f32::from(pan_milli) / 1000.0,
        repeat,
        fade_in_ms,
    });
    Ok(())
}

fn execute_play_bgm(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    const BGM_STREAM_ID: u32 = 0;
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MinoriRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource == "*" {
        let fade_out_ms = parse_optional_command_integer(tokens.get(2), 1, 2)?;
        return stop_audio_stream(state, BGM_STREAM_ID, fade_out_ms);
    }
    validate_audio_relative_path(&spec.resource)?;
    let fade_in_ms = parse_optional_command_integer(tokens.get(1), 1, 2)?;
    let fade_out_ms = parse_optional_command_integer(tokens.get(2), 1, 2)?;
    let command_volume = parse_optional_command_integer(tokens.get(3), 100, 100)?;
    if !(0..=400).contains(&command_volume) {
        return Err(MinoriRuntimeError::Operand);
    }
    let volume_milli =
        u16::try_from(i64::from(command_volume) * i64::from(spec.volume_percent) * 1000 / 10_000)
            .map_err(|_| MinoriRuntimeError::Overflow)?;
    let pan_milli = spec.pan_percent * 10;
    let resource_uri = format!("minori:/bgm/{}", spec.resource);
    let mut commands = Vec::new();
    match state.audio.get(&BGM_STREAM_ID).cloned() {
        Some(current) if current.playing && current.resource_uri == resource_uri => {
            if current.volume_milli != volume_milli || current.pan_milli != pan_milli {
                commands.push(MinoriAudioCommand::SetParams {
                    sequence: next_effect_sequence(state)?,
                    stream_id: BGM_STREAM_ID,
                    volume: f32::from(volume_milli) / 1000.0,
                    pan: f32::from(pan_milli) / 1000.0,
                    repeat: true,
                });
            }
        }
        Some(current) if current.playing => {
            commands.push(MinoriAudioCommand::Stop {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                fade_ms: fade_out_ms,
            });
            commands.push(MinoriAudioCommand::LoadResource {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: resource_uri.clone(),
            });
            commands.push(MinoriAudioCommand::Play {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                volume: f32::from(volume_milli) / 1000.0,
                pan: f32::from(pan_milli) / 1000.0,
                repeat: true,
                fade_in_ms,
            });
        }
        _ => {
            commands.push(MinoriAudioCommand::LoadResource {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: resource_uri.clone(),
            });
            commands.push(MinoriAudioCommand::Play {
                sequence: next_effect_sequence(state)?,
                stream_id: BGM_STREAM_ID,
                volume: f32::from(volume_milli) / 1000.0,
                pan: f32::from(pan_milli) / 1000.0,
                repeat: true,
                fade_in_ms,
            });
        }
    }
    state.audio.insert(
        BGM_STREAM_ID,
        MinoriAudioState {
            bus: "bgm".into(),
            encoding: MinoriAudioEncoding::Ogg,
            resource_uri,
            looped: true,
            volume_milli,
            pan_milli,
            playing: true,
            continuation_pts: 0,
        },
    );
    Ok(Some(MinoriVmEvent::Audio { commands }))
}

fn execute_play_voice(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    const VOICE_STREAM_ID: u32 = 4;
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    if tokens.is_empty() || tokens.len() > 4 {
        return Err(MinoriRuntimeError::Operand);
    }
    let spec = parse_audio_resource_spec(&tokens[0])?;
    if spec.resource != "*" {
        // The authorized script census only contains the verified stop form.
        // Message-bound voice lookup is a separate command path and remains
        // fail-closed until its archive key mapping is proven.
        return Err(MinoriRuntimeError::UnsupportedOpcode {
            opcode: "playvoice.resource".into(),
            ordinal: command.ordinal,
        });
    }
    let fade_out_ms = parse_optional_command_integer(tokens.get(3), 2, 2)?;
    stop_audio_stream(state, VOICE_STREAM_ID, fade_out_ms)
}

fn stop_audio_stream(
    state: &mut MinoriRuntimeState,
    stream_id: u32,
    fade_ms: u32,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let mut commands = Vec::new();
    if state
        .audio
        .get(&stream_id)
        .is_some_and(|current| current.playing)
    {
        commands.push(MinoriAudioCommand::Stop {
            sequence: next_effect_sequence(state)?,
            stream_id,
            fade_ms,
        });
        if let Some(current) = state.audio.get_mut(&stream_id) {
            current.playing = false;
        }
    }
    Ok(Some(MinoriVmEvent::Audio { commands }))
}

fn parse_optional_command_integer(
    token: Option<&String>,
    missing: i32,
    malformed: i32,
) -> Result<u32, MinoriRuntimeError> {
    let value = token
        .map(|token| parse_c_decimal_prefix(token).unwrap_or(malformed))
        .unwrap_or(missing);
    u32::try_from(value).map_err(|_| MinoriRuntimeError::Operand)
}

fn validate_audio_relative_path(value: &str) -> Result<(), MinoriRuntimeError> {
    if value.len() > 256
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
    {
        return Err(MinoriRuntimeError::AudioResource);
    }
    Ok(())
}

fn next_effect_sequence(state: &mut MinoriRuntimeState) -> Result<u64, MinoriRuntimeError> {
    state.effect_sequence = state
        .effect_sequence
        .checked_add(1)
        .ok_or(MinoriRuntimeError::Overflow)?;
    Ok(state.effect_sequence)
}

fn execute_message(
    command: &ScCommand,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Operand)?;
    let (message_id, voice, speaker, text) = if tokens.len() >= 4 {
        let message_id = tokens[0]
            .parse::<i64>()
            .map_err(|_| MinoriRuntimeError::Operand)?;
        (
            message_id,
            (!tokens[1].is_empty()).then(|| tokens[1].clone()),
            (!tokens[2].is_empty()).then(|| tokens[2].clone()),
            tokens[3..].join(" "),
        )
    } else {
        // The original CommandMessage parser leaves constructor defaults intact when fewer
        // than four operands are present, then still executes the empty message update.
        (-1, None, None, String::new())
    };
    let text_hash = Hash256::from_sha256(text.as_bytes());
    let speaker_hash = speaker
        .as_ref()
        .map(|value| Hash256::from_sha256(value.as_bytes()));
    let voice_hash = voice
        .as_ref()
        .map(|value| Hash256::from_sha256(value.as_bytes()));
    let voice = voice.as_deref().map(parse_message_voice).transpose()?;
    let voice_playback_enabled = voice
        .as_ref()
        .is_none_or(|voice| message_voice_enabled(state, voice));
    let mut audio_commands = Vec::new();
    let voice_is_playing = state
        .audio
        .get(&VOICE_STREAM_ID)
        .is_some_and(|current| current.playing);
    if voice_is_playing {
        audio_commands.push(MinoriAudioCommand::Stop {
            sequence: next_effect_sequence(state)?,
            stream_id: VOICE_STREAM_ID,
            fade_ms: 0,
        });
    }
    if let Some(voice) = voice.as_ref().filter(|_| voice_playback_enabled) {
        append_audio_load_and_play(
            state,
            &mut audio_commands,
            VOICE_STREAM_ID,
            &voice.resource_uri,
            voice.volume_milli,
            voice.pan_milli,
            false,
            0,
        )?;
        state.audio.insert(
            VOICE_STREAM_ID,
            MinoriAudioState {
                bus: "voice".into(),
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: voice.resource_uri.clone(),
                looped: false,
                volume_milli: voice.volume_milli,
                pan_milli: voice.pan_milli,
                playing: true,
                continuation_pts: 0,
            },
        );
    } else if voice.is_none()
        || !voice_playback_enabled
        || state.system_ui.config.stop_voice_at_next_message
    {
        if let Some(current) = state.audio.get_mut(&VOICE_STREAM_ID) {
            current.playing = false;
        }
    }
    append_backlog_entry(
        state,
        MinoriBacklogEntry {
            source: command.span,
            message_id,
            text: text.clone(),
            speaker: speaker.clone(),
            text_hash,
            speaker_hash,
            voice_hash,
            voice: voice.clone(),
        },
    )?;
    state.message = Some(MinoriMessageState {
        source: command.span,
        message_id,
        text_hash,
        speaker_hash,
        voice_hash,
        voice,
    });
    let presentation_sequence = next_effect_sequence(state)?;
    let capture_sequence = next_effect_sequence(state)?;
    let token_id = format!("minori.message.{}", state.instruction_count);
    let wait = message_wait_for_current_mode(state, token_id)?;
    state.wait = Some(wait.clone());
    Ok(Some(MinoriVmEvent::Message {
        presentation_sequence,
        capture_sequence,
        text,
        speaker,
        audio_commands,
        wait,
    }))
}

fn message_auto_wait(
    token_id: String,
    configured_units: u8,
) -> Result<MinoriWaitState, MinoriRuntimeError> {
    let timer_ticks = u32::from(configured_units).max(1);
    Ok(MinoriWaitState::Time {
        token_id,
        timer_ticks,
        milliseconds: timer_ticks
            .checked_mul(10)
            .ok_or(MinoriRuntimeError::Overflow)?,
    })
}

fn message_wait_for_current_mode(
    state: &MinoriRuntimeState,
    token_id: String,
) -> Result<MinoriWaitState, MinoriRuntimeError> {
    if state.system_ui.skip_enabled
        && (state.system_ui.play_mode == MinoriPlayMode::Skip
            || (state.system_ui.control_enabled && state.system_ui.control_pressed))
    {
        return message_auto_wait(token_id, 0);
    }
    if state.system_ui.play_mode == MinoriPlayMode::Auto {
        return message_auto_wait(token_id, state.system_ui.config.message_speed_auto_play);
    }
    Ok(MinoriWaitState::Input { token_id })
}

const VOICE_STREAM_ID: u32 = 4;

fn parse_message_voice(token: &str) -> Result<MinoriMessageVoice, MinoriRuntimeError> {
    let spec = parse_audio_resource_spec(token)?;
    validate_audio_relative_path(&spec.resource)?;
    Ok(MinoriMessageVoice {
        resource_uri: format!("minori:/voice/{}", spec.resource),
        volume_milli: spec.volume_percent * 10,
        pan_milli: spec.pan_percent * 10,
    })
}

fn message_voice_enabled(state: &MinoriRuntimeState, voice: &MinoriMessageVoice) -> bool {
    let Some(name) = voice.resource_uri.strip_prefix("minori:/voice/") else {
        return true;
    };
    let prefix = name.split('-').next().unwrap_or_default();
    let index = match prefix {
        "ren" => 0,
        "sui" => 1,
        "aya" => 2,
        "tou" => 3,
        "mot" => 4,
        _ => return true,
    };
    state
        .system_ui
        .config
        .character_voice_enabled
        .get(index)
        .copied()
        .unwrap_or(true)
}

fn append_backlog_entry(
    state: &mut MinoriRuntimeState,
    entry: MinoriBacklogEntry,
) -> Result<(), MinoriRuntimeError> {
    if state.backlog.len() >= MINORI_BACKLOG_MAX_ENTRIES
        || entry.text.len() > MINORI_BACKLOG_MAX_ENTRY_BYTES
        || entry
            .speaker
            .as_ref()
            .is_some_and(|speaker| speaker.len() > MINORI_BACKLOG_MAX_ENTRY_BYTES)
    {
        return Err(MinoriRuntimeError::Backlog);
    }
    let entry_bytes = entry
        .text
        .len()
        .checked_add(entry.speaker.as_ref().map_or(0, String::len))
        .ok_or(MinoriRuntimeError::Backlog)?;
    let backlog_bytes = usize::try_from(state.backlog_bytes)
        .map_err(|_| MinoriRuntimeError::Backlog)?
        .checked_add(entry_bytes)
        .filter(|bytes| *bytes <= MINORI_BACKLOG_MAX_TOTAL_BYTES)
        .ok_or(MinoriRuntimeError::Backlog)?;
    state.backlog.push(entry);
    state.backlog_bytes = u64::try_from(backlog_bytes).map_err(|_| MinoriRuntimeError::Backlog)?;
    Ok(())
}

fn execute_select(
    command: &ScCommand,
    labels: &BTreeMap<String, u32>,
    state: &mut MinoriRuntimeState,
) -> Result<Option<MinoriVmEvent>, MinoriRuntimeError> {
    let targets = match &command.control_flow {
        ScControlFlow::Choice { targets } if (1..=4).contains(&targets.len()) => targets,
        _ => return Err(MinoriRuntimeError::Choice),
    };
    if state.choice.is_some() || state.wait.is_some() {
        return Err(MinoriRuntimeError::Choice);
    }
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MinoriRuntimeError::Choice)?;
    if tokens.len() != targets.len() {
        return Err(MinoriRuntimeError::Choice);
    }
    let mut option_hashes = Vec::with_capacity(tokens.len());
    for (token, target) in tokens.iter().zip(targets) {
        let (display, parsed_target) = token
            .split_once(':')
            .filter(|(display, parsed_target)| !display.is_empty() && !parsed_target.is_empty())
            .ok_or(MinoriRuntimeError::Choice)?;
        if parsed_target != target || !labels.contains_key(target) {
            return Err(MinoriRuntimeError::Choice);
        }
        option_hashes.push(Hash256::from_sha256(display.as_bytes()));
    }
    let selected_index = 0u32;
    state.choice = Some(MinoriChoiceState {
        source: command.span,
        option_hashes: option_hashes.clone(),
        targets: targets.clone(),
        selected_index: Some(selected_index),
    });
    let token_id = format!("minori.choice.{}", state.instruction_count);
    state.wait = Some(MinoriWaitState::Choice { token_id });
    let sequence = next_effect_sequence(state)?;
    Ok(Some(MinoriVmEvent::Choice {
        sequence,
        option_hashes,
        selected_index,
    }))
}

fn validate_chain_target(target: &str) -> Result<(), MinoriRuntimeError> {
    if target.is_empty()
        || target.len() > 256
        || !target.to_ascii_lowercase().ends_with(".sc")
        || target.contains('/')
        || target.contains('\\')
        || target == "."
        || target.contains("..")
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(MinoriRuntimeError::ChainTarget);
    }
    Ok(())
}

fn evaluate_assignment<'a>(
    operands: &'a [ScOperand],
    state: &MinoriRuntimeState,
) -> Result<(&'a str, i64), MinoriRuntimeError> {
    match operands {
        [ScOperand::Symbol { value: key }, ScOperand::Operator { value: assign }, rhs]
            if assign == "=" =>
        {
            Ok((key, resolve_integer(rhs, state)?))
        }
        [ScOperand::Symbol { value: key }, ScOperand::Operator { value: assign }, left, ScOperand::Operator { value: operator }, right]
            if assign == "=" =>
        {
            let left = resolve_integer(left, state)?;
            let right = resolve_integer(right, state)?;
            let value = match operator.as_str() {
                "|" => left | right,
                "&" => left & right,
                "+" => left
                    .checked_add(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "-" => left
                    .checked_sub(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "*" => left
                    .checked_mul(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "/" if right != 0 => left
                    .checked_div(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                "%" if right != 0 => left
                    .checked_rem(right)
                    .ok_or(MinoriRuntimeError::Overflow)?,
                _ => return Err(MinoriRuntimeError::Operand),
            };
            Ok((key, value))
        }
        _ => Err(MinoriRuntimeError::Operand),
    }
}

fn resolve_integer(
    operand: &ScOperand,
    state: &MinoriRuntimeState,
) -> Result<i64, MinoriRuntimeError> {
    match operand {
        ScOperand::Integer { value } => Ok(*value),
        ScOperand::Symbol { value } => Ok(state
            .variables
            .get(value)
            .or_else(|| state.global_variables.get(value))
            .copied()
            .unwrap_or(0)),
        _ => Err(MinoriRuntimeError::Operand),
    }
}

fn compare(left: i64, operator: &str, right: i64) -> Result<bool, MinoriRuntimeError> {
    Ok(match operator {
        "==" => left == right,
        "!=" => left != right,
        "<" => left < right,
        "<=" => left <= right,
        ">" => left > right,
        ">=" => left >= right,
        _ => return Err(MinoriRuntimeError::Operand),
    })
}

fn branch_target(control_flow: &ScControlFlow) -> Result<&str, MinoriRuntimeError> {
    match control_flow {
        ScControlFlow::Jump { target } | ScControlFlow::ConditionalJump { target } => Ok(target),
        _ => Err(MinoriRuntimeError::Operand),
    }
}

fn build_labels(script: &ScScript) -> Result<BTreeMap<String, u32>, MinoriRuntimeError> {
    let mut labels = BTreeMap::new();
    for (line_index, line) in script.lines.iter().enumerate() {
        let ScLineKind::Command { command } = &line.kind else {
            continue;
        };
        if let ScControlFlow::Label { id } = &command.control_flow {
            let target = u32::try_from(line_index + 1).map_err(|_| MinoriRuntimeError::Overflow)?;
            if labels.insert(id.clone(), target).is_some() {
                return Err(MinoriRuntimeError::Label);
            }
        }
    }
    Ok(labels)
}

#[cfg(test)]
mod tests {
    use crate::{parse_sc, ScOpcodeCatalog};

    use super::*;

    fn firefly_vm(source: &[u8], seed: u64) -> MinoriVm {
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        MinoriVm::new(
            "minori:/scr/firefly-fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            seed,
        )
        .unwrap()
    }

    #[test]
    fn gallery_bgm_playback_uses_the_shared_looped_ogg_stream() {
        let mut vm = firefly_vm(b".end\r\n", 7);
        vm.begin_title_launch().unwrap();
        vm.set_system_page(MinoriSystemPage::GalleryBgm, 0).unwrap();

        let first = vm.gallery_bgm_play("minori:/bgm/BGM001.ogg").unwrap();
        assert!(matches!(first.as_slice(), [
            MinoriAudioCommand::LoadResource {
                stream_id: MINORI_BGM_STREAM_ID,
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri,
                ..
            },
            MinoriAudioCommand::Play {
                stream_id: MINORI_BGM_STREAM_ID,
                repeat: true,
                ..
            }
        ] if resource_uri == "minori:/bgm/BGM001.ogg"));
        assert_eq!(
            vm.state().audio[&MINORI_BGM_STREAM_ID].resource_uri,
            "minori:/bgm/BGM001.ogg"
        );

        let second = vm.gallery_bgm_play("minori:/bgm/BGM002.ogg").unwrap();
        assert!(matches!(
            second.first(),
            Some(MinoriAudioCommand::Stop {
                stream_id: MINORI_BGM_STREAM_ID,
                ..
            })
        ));
        assert_eq!(
            vm.state().audio[&MINORI_BGM_STREAM_ID].resource_uri,
            "minori:/bgm/BGM002.ogg"
        );

        let stopped = vm.gallery_bgm_stop().unwrap();
        assert!(matches!(
            stopped.as_slice(),
            [MinoriAudioCommand::Stop {
                stream_id: MINORI_BGM_STREAM_ID,
                ..
            }]
        ));
        assert!(!vm.state().audio[&MINORI_BGM_STREAM_ID].playing);

        vm.set_system_page(MinoriSystemPage::Memories, 0).unwrap();
        let stopped_from_parent = vm.gallery_bgm_stop().unwrap();
        assert!(stopped_from_parent.is_empty());
    }

    #[test]
    fn character_load_and_position_are_snapshot_safe_and_preserve_signed_orientation() {
        let source = b".char load -11 WALK.png\r\n.char pos -11 727 -1685\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        assert!(matches!(
            vm.step(1, 1).unwrap(),
            Some(MinoriVmEvent::Character(_))
        ));
        let loaded = vm.state().characters.get(&11).unwrap();
        assert!(!loaded.positive_orientation);
        assert_eq!(loaded.resource_uris, ["minori:/st/WALK.png"]);
        assert_eq!(loaded.anchor_position, [0, 0]);
        assert!(!loaded.keep_once);

        assert!(matches!(
            vm.step(2, 1).unwrap(),
            Some(MinoriVmEvent::Character(_))
        ));
        assert_eq!(
            vm.state().characters.get(&11).unwrap().anchor_position,
            [727, -1685]
        );
        let snapshot = vm.snapshot_bytes().unwrap();
        let snapshot_hash = vm.state_hash().unwrap();
        vm.state.characters.get_mut(&11).unwrap().anchor_position = [1, 2];
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state_hash().unwrap(), snapshot_hash);
        assert_eq!(vm.step(3, 1).unwrap(), Some(MinoriVmEvent::Terminal));
    }

    #[test]
    fn character_keep_sets_the_native_one_shot_retention_flag() {
        let source = b".char load -100 WALK.png\r\n.char keep 100\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        vm.step(1, 1).unwrap();
        assert!(matches!(
            vm.step(2, 1).unwrap(),
            Some(MinoriVmEvent::Character(_))
        ));
        assert!(vm.state().characters.get(&100).unwrap().keep_once);
        let snapshot = vm.snapshot_bytes().unwrap();
        vm.restore_state(&snapshot).unwrap();
        assert!(vm.state().characters.get(&100).unwrap().keep_once);

        let missing = b".char keep 100\r\n.end\r\n";
        let mut missing_vm = firefly_vm(missing, 7);
        assert!(matches!(
            missing_vm.step(1, 1).unwrap(),
            Some(MinoriVmEvent::Character(_))
        ));
        assert!(missing_vm.state().characters.is_empty());
    }

    #[test]
    fn character_transition_is_linear_blocking_and_snapshot_safe() {
        let source =
            b".char load 11 WALK.png\r\n.char trans 11 100 0\r\n.char vis 11 false\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        vm.step(1, 1).unwrap();
        let Some(MinoriVmEvent::Wait(MinoriWaitState::CharacterTransition {
            token_id,
            slot_id,
            milliseconds,
        })) = vm.step(2, 1).unwrap()
        else {
            panic!("expected character transition wait");
        };
        assert_eq!(slot_id, 11);
        assert_eq!(milliseconds, 100);
        assert_eq!(vm.state().characters[&11].opacity_256, 256);

        let frame = vm.advance_character_clock(50_000_000).unwrap().unwrap();
        assert_ne!(frame.sequence, 0);
        assert_eq!(vm.state().characters[&11].opacity_256, 128);
        let snapshot = vm.snapshot_bytes().unwrap();
        vm.advance_character_clock(50_000_000).unwrap();
        assert_eq!(vm.state().characters[&11].opacity_256, 0);
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state().characters[&11].opacity_256, 128);

        vm.advance_character_clock(50_000_000).unwrap();
        vm.resolve_wait(&token_id).unwrap();
        assert!(vm.state().characters[&11].transition.is_none());
        assert_eq!(vm.state().characters[&11].opacity_256, 0);
        assert!(matches!(
            vm.step(3, 1).unwrap(),
            Some(MinoriVmEvent::Character(_))
        ));
        assert!(!vm.state().characters[&11].visible);
    }

    #[test]
    fn character_transition_rejects_invalid_duration_opacity_and_overlap() {
        for source in [
            b".char load 1 A.png\r\n.char trans 1 60001 0\r\n".as_slice(),
            b".char load 1 A.png\r\n.char trans 1 1 256\r\n".as_slice(),
            b".char trans 1 1 0\r\n".as_slice(),
        ] {
            let mut vm = firefly_vm(source, 7);
            if source.starts_with(b".char load") {
                vm.step(1, 1).unwrap();
                assert_eq!(vm.step(2, 1).unwrap_err(), MinoriRuntimeError::Character);
            } else {
                assert_eq!(vm.step(1, 1).unwrap_err(), MinoriRuntimeError::Character);
            }
        }
    }

    #[test]
    fn stage_consumes_character_keep_and_discards_unmarked_slots() {
        let source = b".char load 11 KEEP.png\r\n.char load 12 DROP.png\r\n.char keep 11\r\n.stage * BG.png 0 0\r\n.stage * BG2.png 0 0\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);

        for tick in 1..=4 {
            vm.step(tick, 1).unwrap();
        }
        assert_eq!(vm.state().characters.len(), 1);
        let retained = vm.state().characters.get(&11).unwrap();
        assert!(!retained.keep_once);
        assert!(!vm.state().characters.contains_key(&12));

        vm.step(5, 1).unwrap();
        assert!(vm.state().characters.is_empty());
        assert!(matches!(
            vm.step(6, 1).unwrap(),
            Some(MinoriVmEvent::Terminal)
        ));
    }

    #[test]
    fn character_commands_reject_unverified_modes_missing_slots_and_invalid_bounds() {
        for source in [
            b".char pos 11 1 2\r\n".as_slice(),
            b".char load 0 A.png\r\n".as_slice(),
            b".char load 4097 A.png\r\n".as_slice(),
            b".char load 1 ../A.png\r\n".as_slice(),
            b".char load 1 A.png B.png\r\n".as_slice(),
            b".char load 1 A.png B.png C.png D.png\r\n".as_slice(),
            b".char pos 1 65537 0\r\n".as_slice(),
            b".char move 1 0 0 1 1 10\r\n".as_slice(),
        ] {
            assert_eq!(
                firefly_vm(source, 7).step(1, 1).unwrap_err(),
                MinoriRuntimeError::Character
            );
        }
    }

    #[test]
    fn firefly_is_deterministic_snapshot_safe_and_fades_after_end() {
        let source = b".effect Firefly Firefly_c 3 1000\r\n.wait 20\r\n.effect end\r\n.end\r\n";
        let mut first = firefly_vm(source, 7);
        let mut second = firefly_vm(source, 7);
        let mut different_seed = firefly_vm(source, 8);

        assert!(matches!(
            first.step(1, 16).unwrap(),
            Some(MinoriVmEvent::Firefly(_))
        ));
        second.step(1, 16).unwrap();
        different_seed.step(1, 16).unwrap();
        assert_eq!(first.state_hash().unwrap(), second.state_hash().unwrap());
        assert_ne!(
            first.state_hash().unwrap(),
            different_seed.state_hash().unwrap()
        );

        let firefly = first.state().firefly.as_ref().unwrap();
        assert_eq!(firefly.resources[0], "minori:/sys/Firefly_cS.png");
        assert_eq!(firefly.resources[1], "minori:/sys/Firefly_cM.png");
        assert_eq!(firefly.resources[2], "minori:/sys/Firefly_cL.png");
        assert_eq!(firefly.target_count, 3);
        assert_eq!(firefly.particles.len(), 3);
        assert_eq!(firefly.fade_alpha_256, 0);

        first.advance_firefly_clock(16_000_000).unwrap();
        second.advance_firefly_clock(16_000_000).unwrap();
        assert_eq!(first.state_hash().unwrap(), second.state_hash().unwrap());
        assert_eq!(first.state().firefly.as_ref().unwrap().fade_alpha_256, 1);
        let snapshot = first.snapshot_bytes().unwrap();
        let snapshot_hash = first.state_hash().unwrap();
        first.advance_firefly_clock(32_000_000).unwrap();
        first.restore_state(&snapshot).unwrap();
        assert_eq!(first.state_hash().unwrap(), snapshot_hash);

        let wait = first.step(2, 16).unwrap().unwrap();
        let MinoriVmEvent::Wait(MinoriWaitState::Time { token_id, .. }) = wait else {
            panic!("expected Firefly fixture wait")
        };
        first.resolve_wait(&token_id).unwrap();
        assert!(matches!(
            first.step(3, 16).unwrap(),
            Some(MinoriVmEvent::Firefly(_))
        ));
        assert!(first.state().firefly.as_ref().unwrap().ending);
        assert!(matches!(
            first.advance_firefly_clock(16_000_000).unwrap(),
            Some(MinoriVmEvent::FireflyCleared { .. })
        ));
        assert!(first.state().firefly.is_none());
        assert_eq!(first.step(4, 16).unwrap(), Some(MinoriVmEvent::Terminal));
    }

    #[test]
    fn firefly_rejects_invalid_operands_and_corrupt_snapshot_state() {
        for source in [
            b".effect Firefly Firefly_c 0 1000\r\n".as_slice(),
            b".effect Firefly Firefly_c 257 1000\r\n".as_slice(),
            b".effect Firefly Firefly_c 1 0\r\n".as_slice(),
            b".effect Firefly Firefly_c 1 60001\r\n".as_slice(),
            b".effect Firefly Firefly_c 1\r\n".as_slice(),
            b".effect end unexpected\r\n".as_slice(),
        ] {
            assert!(matches!(
                firefly_vm(source, 7).step(1, 8),
                Err(MinoriRuntimeError::Firefly)
            ));
        }

        let source = b".effect Firefly Firefly_c 1 1000\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        vm.step(1, 8).unwrap();
        let mut state = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        state.firefly.as_mut().unwrap().particles[0].kind = 3;
        let corrupt = postcard::to_allocvec(&state).unwrap();
        assert!(matches!(
            MinoriVm::decode_snapshot(&corrupt),
            Err(MinoriRuntimeError::Firefly)
        ));
        assert!(matches!(
            vm.restore_state(&corrupt),
            Err(MinoriRuntimeError::Firefly)
        ));
    }

    #[test]
    fn deterministic_control_flow_wait_and_restore_round_trip() {
        let source = b".setglobal route = 1\r\n.label loop\r\n.set count = count + 1\r\n.if count < 3 loop\r\n.wait 20\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            7,
        )
        .unwrap();
        let event = vm.step(1, 32).unwrap().unwrap();
        let MinoriVmEvent::Wait(MinoriWaitState::Time {
            token_id,
            timer_ticks,
            milliseconds,
        }) = event
        else {
            panic!("expected time wait")
        };
        assert_eq!(timer_ticks, 20);
        assert_eq!(milliseconds, 200);
        assert_eq!(vm.state().variables.get("count"), Some(&3));
        let snapshot = vm.snapshot_bytes().unwrap();
        let hash = vm.state_hash().unwrap();
        vm.resolve_wait(&token_id).unwrap();
        assert_eq!(vm.step(2, 4).unwrap(), Some(MinoriVmEvent::Terminal));
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state_hash().unwrap(), hash);
    }

    #[test]
    fn verified_route_clear_is_a_stable_snapshot_unlock_without_name_guessing() {
        let source = b".setGlobal OP_CLEAR = 1\r\n.setGlobal REN_CLEAR = 1\r\n.setGlobal REN_CLEAR = 1\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        assert_eq!(vm.step(1, 16).unwrap(), Some(MinoriVmEvent::Terminal));
        assert_eq!(
            vm.state().gallery_unlocks,
            [Hash256::from_sha256(b"REN_CLEAR")]
        );
        let snapshot = vm.snapshot_bytes().unwrap();
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(
            vm.state().gallery_unlocks,
            [Hash256::from_sha256(b"REN_CLEAR")]
        );

        let mut invalid = MinoriVm::decode_snapshot(&snapshot).unwrap();
        invalid
            .gallery_unlocks
            .push(Hash256::from_sha256(b"UNKNOWN_CLEAR"));
        assert_eq!(
            MinoriVm::decode_snapshot(&postcard::to_allocvec(&invalid).unwrap()).unwrap_err(),
            MinoriRuntimeError::State
        );
    }

    #[test]
    fn verified_clear_flags_select_only_the_original_title_variants() {
        let mut vm = firefly_vm(b".end\r\n", 7);
        assert_eq!(vm.title_variant(), 0);
        let mut first_three = vec![
            Hash256::from_sha256(b"AYAME_CLEAR"),
            Hash256::from_sha256(b"REN_CLEAR"),
            Hash256::from_sha256(b"SUI_CLEAR"),
        ];
        first_three.sort_unstable();
        vm.merge_verified_gallery_unlocks(&first_three).unwrap();
        assert_eq!(vm.title_variant(), 1);
        vm.merge_verified_gallery_unlocks(&[Hash256::from_sha256(b"TOHKA_CLEAR")])
            .unwrap();
        assert_eq!(vm.title_variant(), 2);
    }

    #[test]
    fn config_draft_applies_or_cancels_atomically_and_round_trips() {
        let mut vm = firefly_vm(b".end\r\n", 7);
        vm.begin_title_launch().unwrap();
        vm.open_config().unwrap();
        assert_eq!(
            vm.apply_config_control(MinoriConfigControl::MessageSpeedAutoPlay(7)),
            Ok(MinoriConfigChange::Present)
        );
        assert_eq!(
            vm.apply_config_control(MinoriConfigControl::BgmVolume(35)),
            Ok(MinoriConfigChange::AudioParamsChanged)
        );
        vm.apply_config_control(MinoriConfigControl::ToggleBgmMute)
            .unwrap();
        vm.apply_config_control(MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Skip))
            .unwrap();
        vm.apply_config_control(MinoriConfigControl::ToggleCharacterVoice(3))
            .unwrap();
        assert_eq!(vm.state().system_ui.config.bgm_volume, 100);
        assert_eq!(vm.config_for_presentation().unwrap().bgm_volume, 35);

        let snapshot = vm.snapshot_bytes().unwrap();
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(
            vm.config_for_presentation()
                .unwrap()
                .message_speed_auto_play,
            7
        );
        assert_eq!(
            vm.apply_config_control(MinoriConfigControl::Cancel),
            Ok(MinoriConfigChange::Cancelled)
        );
        assert_eq!(vm.state().system_ui.page, MinoriSystemPage::Title);
        assert_eq!(vm.state().system_ui.config, MinoriConfigState::default());

        vm.open_config().unwrap();
        vm.apply_config_control(MinoriConfigControl::BgmVolume(35))
            .unwrap();
        vm.apply_config_control(MinoriConfigControl::PreferredPlayMode(MinoriPlayMode::Skip))
            .unwrap();
        assert_eq!(
            vm.apply_config_control(MinoriConfigControl::Apply),
            Ok(MinoriConfigChange::Applied)
        );
        assert_eq!(vm.state().system_ui.config.bgm_volume, 35);
        assert_eq!(
            vm.state().system_ui.config.preferred_play_mode,
            MinoriPlayMode::Skip
        );
        assert!(vm.state().system_ui.config_draft.is_none());
    }

    #[test]
    fn config_test_audio_is_explicit_wav_and_uses_a_dedicated_bus_stream() {
        let mut vm = firefly_vm(b".end\r\n", 7);
        vm.begin_title_launch().unwrap();
        vm.open_config().unwrap();
        let commands = vm
            .config_test_audio_commands(MinoriConfigAudioBus::Voice)
            .unwrap();
        assert!(matches!(
            commands.as_slice(),
            [
                MinoriAudioCommand::LoadResource {
                    stream_id: MINORI_CONFIG_TEST_VOICE_STREAM_ID,
                    encoding: MinoriAudioEncoding::Wav,
                    resource_uri,
                    ..
                },
                MinoriAudioCommand::Play {
                    stream_id: MINORI_CONFIG_TEST_VOICE_STREAM_ID,
                    repeat: false,
                    ..
                }
            ] if resource_uri == "minori:/sys/VOICEtest.wav"
        ));
        let audio = vm
            .state()
            .audio
            .get(&MINORI_CONFIG_TEST_VOICE_STREAM_ID)
            .unwrap();
        assert_eq!(audio.bus, "voice");
        assert_eq!(audio.encoding, MinoriAudioEncoding::Wav);
        vm.apply_config_control(MinoriConfigControl::Cancel)
            .unwrap();
        let close = vm.close_config_audio_commands().unwrap();
        assert!(matches!(
            close.as_slice(),
            [MinoriAudioCommand::Stop {
                stream_id: MINORI_CONFIG_TEST_VOICE_STREAM_ID,
                fade_ms: 0,
                ..
            }]
        ));
        assert!(
            !vm.state()
                .audio
                .get(&MINORI_CONFIG_TEST_VOICE_STREAM_ID)
                .unwrap()
                .playing
        );
    }

    #[test]
    fn control_pragma_enables_only_the_held_key_fast_path() {
        let source = b".pragma enable_control\r\n.wait 500\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();

        vm.set_control_pressed(true);
        assert!(!vm.state().system_ui.control_enabled);
        assert!(!vm.fast_forward_active());
        assert_eq!(vm.step(1, 16).unwrap(), Some(MinoriVmEvent::Terminal));
        assert!(vm.state().system_ui.control_enabled);
        assert!(vm.state().system_ui.control_pressed);
        assert!(vm.fast_forward_active());
        assert_eq!(vm.state().wait, None);

        vm.set_control_pressed(false);
        assert!(!vm.fast_forward_active());
    }

    #[test]
    fn play_mode_is_mutually_exclusive_snapshot_safe_and_drives_auto_wait() {
        let source =
            b".message 1 voice speaker first\r\n.message 2 voice speaker second\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script.clone(),
            1,
        )
        .unwrap();

        let Some(MinoriVmEvent::Message { wait, .. }) = vm.step(1, 16).unwrap() else {
            panic!("expected message")
        };
        assert!(matches!(wait, MinoriWaitState::Input { .. }));
        assert!(vm.toggle_preferred_play_mode().unwrap());
        assert_eq!(vm.state().system_ui.play_mode, MinoriPlayMode::Auto);
        assert!(matches!(
            vm.state().wait,
            Some(MinoriWaitState::Time {
                timer_ticks: 50,
                milliseconds: 500,
                ..
            })
        ));
        let snapshot = vm.snapshot_bytes().unwrap();
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state().system_ui.play_mode, MinoriPlayMode::Auto);

        let token = match vm.state().wait.as_ref().unwrap() {
            MinoriWaitState::Time { token_id, .. } => token_id.clone(),
            _ => panic!("expected rebound time wait"),
        };
        vm.resolve_wait(&token).unwrap();
        let Some(MinoriVmEvent::Message { wait, .. }) = vm.step(2, 16).unwrap() else {
            panic!("expected auto message")
        };
        assert!(matches!(
            wait,
            MinoriWaitState::Time {
                timer_ticks: 50,
                milliseconds: 500,
                ..
            }
        ));

        assert!(vm.toggle_preferred_play_mode().unwrap());
        assert_eq!(vm.state().system_ui.play_mode, MinoriPlayMode::Normal);
        assert!(matches!(
            vm.state().wait,
            Some(MinoriWaitState::Input { .. })
        ));
        vm.state.system_ui.config.preferred_play_mode = MinoriPlayMode::Skip;
        assert!(vm.toggle_preferred_play_mode().unwrap());
        assert_eq!(vm.state().system_ui.play_mode, MinoriPlayMode::Skip);
        assert!(vm.fast_forward_active());
        assert!(matches!(
            vm.state().wait,
            Some(MinoriWaitState::Time {
                timer_ticks: 1,
                milliseconds: 10,
                ..
            })
        ));
    }

    #[test]
    fn fastest_auto_config_maps_to_one_positive_timing_unit() {
        assert!(matches!(
            message_auto_wait("minori.message.1".into(), 0).unwrap(),
            MinoriWaitState::Time {
                timer_ticks: 1,
                milliseconds: 10,
                ..
            }
        ));
    }

    #[test]
    fn held_control_rebinds_only_an_active_message_wait_while_the_gates_are_open() {
        let source = b".pragma enable_control\r\n.message 1 voice speaker first\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();

        let Some(MinoriVmEvent::Message { wait, .. }) = vm.step(1, 16).unwrap() else {
            panic!("expected message")
        };
        assert!(matches!(wait, MinoriWaitState::Input { .. }));
        vm.set_control_pressed(true);
        assert!(vm.rebind_active_message_wait().unwrap());
        assert!(matches!(
            vm.state().wait,
            Some(MinoriWaitState::Time {
                timer_ticks: 1,
                milliseconds: 10,
                ..
            })
        ));
        vm.set_control_pressed(false);
        assert!(vm.rebind_active_message_wait().unwrap());
        assert!(matches!(
            vm.state().wait,
            Some(MinoriWaitState::Input { .. })
        ));
    }

    #[test]
    fn skip_and_control_pragmas_are_independent_fast_path_gates() {
        let source = b".pragma skip_disable\r\n.pragma enable_control\r\n.wait 1\r\n.pragma skip_enable\r\n.wait 1\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();

        vm.set_control_pressed(true);
        let first = vm.step(1, 16).unwrap().unwrap();
        assert!(matches!(
            first,
            MinoriVmEvent::Wait(MinoriWaitState::Time { .. })
        ));
        assert!(!vm.state().system_ui.skip_enabled);
        assert!(vm.state().system_ui.control_enabled);
        assert!(vm.state().system_ui.control_pressed);
        assert!(!vm.fast_forward_active());

        let token = match vm.state().wait.as_ref().unwrap() {
            MinoriWaitState::Time { token_id, .. } => token_id.clone(),
            _ => panic!("expected time wait"),
        };
        vm.resolve_wait(&token).unwrap();
        assert_eq!(vm.step(2, 16).unwrap(), Some(MinoriVmEvent::Terminal));
        assert!(vm.state().system_ui.skip_enabled);
        assert!(vm.fast_forward_active());
        assert!(vm.state().wait.is_none());
    }

    #[test]
    fn skip_gate_round_trips_in_the_runtime_snapshot() {
        let source = b".pragma skip_disable\r\n.wait 1\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script.clone(),
            1,
        )
        .unwrap();
        vm.set_control_pressed(true);
        vm.set_control_enabled(true);
        assert!(matches!(vm.step(1, 16), Ok(Some(MinoriVmEvent::Wait(_)))));
        assert!(!vm.state().system_ui.skip_enabled);
        assert!(!vm.fast_forward_active());

        let snapshot = vm.snapshot_bytes().unwrap();
        let mut restored = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        restored.restore_state(&snapshot).unwrap();
        assert!(!restored.state().system_ui.skip_enabled);
        assert!(restored.state().system_ui.control_enabled);
        assert!(restored.state().system_ui.control_pressed);
        assert!(!restored.fast_forward_active());
    }

    #[test]
    fn unknown_control_pragma_is_a_stable_blocking_error() {
        let source = b".pragma unknown_control\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();

        assert!(matches!(
            vm.step(1, 4),
            Err(MinoriRuntimeError::UnsupportedPragma { .. })
        ));
    }

    #[test]
    fn malformed_screen_shake_blocks_without_advancing_silently() {
        let source = b".shakescreen 0\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1, 4), Err(MinoriRuntimeError::ScreenShake));
    }

    #[test]
    fn panel_mode_one_uses_the_verified_message_panel_resource() {
        let source = b".panel 1\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Panel { sequence }) = vm.step(1, 4).unwrap() else {
            panic!("expected panel event")
        };
        assert_eq!(sequence, 1);
        assert_eq!(
            vm.state().panel,
            Some(MinoriPanelState {
                mode: 1,
                resource_uri: "minori:/sys/msgPanel.png".into(),
            })
        );
        let snapshot = vm.snapshot_bytes().unwrap();
        assert_eq!(
            MinoriVm::decode_snapshot(&snapshot).unwrap().panel,
            vm.state().panel
        );

        let clear_source = b".panel 1\r\n.panel 0\r\n";
        let clear_script = parse_sc(clear_source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut clear_vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(clear_source),
            clear_script,
            1,
        )
        .unwrap();
        assert!(matches!(
            clear_vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::Panel { .. })
        ));
        assert!(clear_vm.state().panel.is_some());
        assert!(matches!(
            clear_vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::Panel { .. })
        ));
        assert_eq!(clear_vm.state().panel, None);

        let (source, operand_count, mode) = (b".panel 1 -1\r\n".as_slice(), 2, Some(1));
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 4).unwrap_err(),
            MinoriRuntimeError::Panel {
                operand_count,
                mode,
            }
        );
    }

    #[test]
    fn crossfade2_without_resource_configuration_replaces_the_active_effect() {
        let source = b".effect CrossFade2\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::EffectCleared { sequence: 1 })
        );
        assert_eq!(vm.state().effect, None);
        assert_eq!(vm.state().effect_sequence, 1);
        assert_eq!(vm.advance_effect_clock(1_000_000).unwrap(), None);
        let snapshot = vm.snapshot_bytes().unwrap();
        let restored = MinoriVm::decode_snapshot(&snapshot).unwrap();
        assert_eq!(restored.effect, vm.state().effect);
        assert_eq!(restored.effect_sequence, vm.state().effect_sequence);
    }

    #[test]
    fn star_effect_kind_clears_the_active_effect() {
        let source = b".effect *\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::EffectCleared { sequence: 1 })
        );
        assert_eq!(vm.state().effect, None);
    }

    #[test]
    fn movie_opens_a_modal_media_wait_and_clears_it_on_completion() {
        let source = b".movie 9989 op.avi 1280 720 t\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Movie(movie)) = vm.step(1, 4).unwrap() else {
            panic!("expected movie event");
        };
        assert_eq!(movie.resource_uri, "minori:/mov/op.avi");
        assert!(movie.skippable);
        vm.resolve_wait(&movie.fence_id).unwrap();
        assert_eq!(vm.state().movie, None);
        assert_eq!(vm.state().wait, None);
    }

    #[test]
    fn select_preserves_option_targets_and_commits_the_selected_label() {
        let source = b".select first:label1 second:label2 third:label3\r\n.label label1\r\n.end\r\n.label label2\r\n.end\r\n.label label3\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Choice {
            sequence,
            option_hashes,
            selected_index,
        }) = vm.step(1, 4).unwrap()
        else {
            panic!("expected choice event")
        };
        assert_eq!(sequence, 1);
        assert_eq!(option_hashes.len(), 3);
        assert_eq!(selected_index, 0);
        assert_eq!(
            vm.choice_display_texts().unwrap(),
            ["first", "second", "third"]
        );
        assert_eq!(
            vm.state().choice.as_ref().unwrap().targets,
            ["label1", "label2", "label3"]
        );
        let token = match vm.state().wait.as_ref().unwrap() {
            MinoriWaitState::Choice { token_id } => token_id.clone(),
            _ => panic!("expected choice wait"),
        };
        vm.move_choice(1).unwrap();
        assert_eq!(vm.state().choice.as_ref().unwrap().selected_index, Some(1));
        vm.resolve_wait(&token).unwrap();
        vm.commit_choice().unwrap();
        assert_eq!(vm.state().pc_line, 4);
        assert!(matches!(
            vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::Terminal)
        ));
    }

    #[test]
    fn crossfade2_star_resource_selection_replaces_the_active_effect_without_a_frame() {
        let source = b".effect CrossFade2 * 320 100\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();

        assert_eq!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::EffectCleared { sequence: 1 })
        );
        assert_eq!(vm.state().effect, None);
        assert_eq!(vm.state().effect_sequence, 1);
        assert_eq!(vm.advance_effect_clock(1_000_000).unwrap(), None);
    }

    #[test]
    fn crossfade2_preserves_an_empty_slot_inside_a_resource_sequence() {
        let source = b".effect CrossFade2 first.png:* 320 100\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();

        assert!(matches!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::Effect(_))
        ));
        let effect = vm.state().effect.as_ref().expect("CrossFade2 effect state");
        assert_eq!(effect.resources.len(), 2);
        assert!(effect.resources[0].is_some());
        assert_eq!(effect.resources[1], None);
    }

    #[test]
    fn crossfade2_rejects_unknown_kinds_and_unverified_resource_configuration() {
        let unsupported_source = b".effect CrossFade first.png:second.png 320 100\r\n";
        let unsupported_script =
            parse_sc(unsupported_source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut unsupported_vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(unsupported_source),
            unsupported_script,
            1,
        )
        .unwrap();
        assert_eq!(
            unsupported_vm.step(1, 4).unwrap_err(),
            MinoriRuntimeError::UnsupportedEffectKind {
                identity: Hash256::from_sha256(b"CrossFade"),
            }
        );

        for (source, violation) in [
            (
                b".effect CrossFade2 first.png:second.png 0 100\r\n".as_slice(),
                MinoriEffectViolation::Timing,
            ),
            (
                b".effect CrossFade2 first.png:second.png 320 0\r\n".as_slice(),
                MinoriEffectViolation::Timing,
            ),
            (
                b".effect CrossFade2 first.png:second.png 320 100 1\r\n".as_slice(),
                MinoriEffectViolation::ResourceSequence { count: 5 },
            ),
        ] {
            let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
            let mut vm = MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(source),
                script,
                1,
            )
            .unwrap();
            assert_eq!(
                vm.step(1, 4).unwrap_err(),
                MinoriRuntimeError::Effect { violation }
            );
        }
    }

    #[test]
    fn crossfade2_rejects_missing_or_additional_operands() {
        let cases = [
            (
                b".effect\r\n".as_slice(),
                MinoriEffectViolation::OperandCount { count: 0 },
            ),
            (
                b".effect CrossFade2 * 320 100 1\r\n".as_slice(),
                MinoriEffectViolation::ResourceSequence { count: 5 },
            ),
        ];
        for (source, violation) in cases {
            let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
            let mut vm = MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(source),
                script,
                1,
            )
            .unwrap();
            assert_eq!(
                vm.step(1, 4).unwrap_err(),
                MinoriRuntimeError::Effect { violation }
            );
        }
    }

    #[test]
    fn effect2_rejects_unverified_secondary_kinds() {
        let source = b".effect2 CrossFade2\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 4).unwrap_err(),
            MinoriRuntimeError::UnsupportedEffectKind {
                identity: Hash256::from_sha256(b"CrossFade2"),
            }
        );
    }

    #[test]
    fn snow_h_uses_an_independent_secondary_slot() {
        let source = b".effect CrossFade2 first.png:second.png 32 16\r\n.effect2 SnowH\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            7,
        )
        .unwrap();
        assert!(matches!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::Effect(_))
        ));
        let primary = vm.state().effect.clone().unwrap();
        assert!(matches!(
            vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::SecondaryEffect(_))
        ));
        assert_eq!(vm.state().effect.as_ref(), Some(&primary));
        let secondary = vm.state().secondary_effect.as_ref().unwrap();
        assert_eq!(secondary.resources[0], "minori:/sys/snowS.png");
        assert_eq!(secondary.resources[1], "minori:/sys/snowM.png");
        assert_eq!(secondary.resources[2], "minori:/sys/snowL.png");
        assert_eq!(secondary.particles.len(), 50);
        assert!(secondary.particles.iter().all(|particle| particle.active));
    }

    #[test]
    fn snow_h_motion_is_deterministic_and_snapshot_validated() {
        let source = b".effect2 SnowH\r\n.end\r\n";
        let make_vm = |seed| {
            MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(source),
                parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap(),
                seed,
            )
            .unwrap()
        };
        let mut first = make_vm(7);
        let mut second = make_vm(7);
        let mut different = make_vm(8);
        first.step(1, 4).unwrap();
        second.step(1, 4).unwrap();
        different.step(1, 4).unwrap();
        assert_eq!(
            first.state().secondary_effect,
            second.state().secondary_effect
        );
        assert_ne!(
            first.state().secondary_effect,
            different.state().secondary_effect
        );

        let before = first.state().secondary_effect.as_ref().unwrap().particles[0].clone();
        assert!(matches!(
            first.advance_secondary_effect_clock(16_000_000).unwrap(),
            Some(MinoriVmEvent::SecondaryEffect(_))
        ));
        second.advance_secondary_effect_clock(16_000_000).unwrap();
        assert_eq!(
            first.state().secondary_effect,
            second.state().secondary_effect
        );
        let after = &first.state().secondary_effect.as_ref().unwrap().particles[0];
        assert_eq!(
            after.fixed_position[0],
            before.fixed_position[0] + i64::from(before.horizontal_velocity) * 16
        );
        let vertical_delta = i64::from(before.vertical_velocity) * 16;
        assert_eq!(
            after.fixed_position[1],
            if before.vertical_positive {
                before.fixed_position[1] + vertical_delta
            } else {
                before.fixed_position[1] - vertical_delta
            }
        );
        assert_eq!(
            first.state().secondary_effect.as_ref().unwrap().alpha_256,
            1
        );

        let snapshot = first.snapshot_bytes().unwrap();
        assert_eq!(
            MinoriVm::decode_snapshot(&snapshot).unwrap(),
            *first.state()
        );
        let mut corrupt = first.state().clone();
        corrupt.secondary_effect.as_mut().unwrap().particles[0].kind = 3;
        let corrupt = postcard::to_allocvec(&corrupt).unwrap();
        assert_eq!(
            MinoriVm::decode_snapshot(&corrupt),
            Err(MinoriRuntimeError::SecondaryEffect)
        );
    }

    #[test]
    fn snow_h_fadeout_clears_only_the_secondary_slot() {
        let source = b".effect2 SnowH\r\n.effect2 fadeout\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            7,
        )
        .unwrap();
        vm.step(1, 4).unwrap();
        vm.advance_secondary_effect_clock(32_000_000).unwrap();
        assert_eq!(vm.state().secondary_effect.as_ref().unwrap().alpha_256, 2);
        assert!(matches!(
            vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::SecondaryEffect(_))
        ));
        assert!(vm.state().secondary_effect.as_ref().unwrap().ending);
        assert!(matches!(
            vm.advance_secondary_effect_clock(32_000_000).unwrap(),
            Some(MinoriVmEvent::SecondaryEffectCleared { .. })
        ));
        assert!(vm.state().secondary_effect.is_none());
        assert!(vm.state().effect.is_none());
    }

    #[test]
    fn screen_shake_vertical_mode_uses_native_interval_and_alternation() {
        let source = b".shakeScreen V 10 30\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        assert!(matches!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::ScreenShake(_))
        ));
        assert_eq!(vm.state().screen_shake.as_ref().unwrap().offset, [0, 0]);
        assert_eq!(vm.advance_screen_shake_clock(29_000_000).unwrap(), None);
        let first = vm.advance_screen_shake_clock(1_000_000).unwrap().unwrap();
        assert!(first.sequence > 0);
        assert_eq!(vm.state().screen_shake.as_ref().unwrap().offset, [0, -10]);
        // Musica resets the interval origin instead of carrying excess time.
        assert!(vm.advance_screen_shake_clock(31_000_000).unwrap().is_some());
        assert_eq!(vm.state().screen_shake.as_ref().unwrap().offset, [0, 10]);

        let snapshot = vm.snapshot_bytes().unwrap();
        assert_eq!(MinoriVm::decode_snapshot(&snapshot).unwrap(), *vm.state());
        let mut corrupt = vm.state().clone();
        corrupt.screen_shake.as_mut().unwrap().offset = [1, 10];
        assert_eq!(
            MinoriVm::decode_snapshot(&postcard::to_allocvec(&corrupt).unwrap()),
            Err(MinoriRuntimeError::ScreenShake)
        );
    }

    #[test]
    fn screen_shake_random_mode_is_deterministic_and_preserves_switch_fallthrough() {
        let source = b".shakeScreen R 50 100\r\n.end\r\n";
        let mut first = firefly_vm(source, 11);
        let mut second = firefly_vm(source, 11);
        first.step(1, 4).unwrap();
        second.step(1, 4).unwrap();
        for _ in 0..32 {
            first.advance_screen_shake_clock(100_000_000).unwrap();
            second.advance_screen_shake_clock(100_000_000).unwrap();
            assert_eq!(first.state().screen_shake, second.state().screen_shake);
            let offset = first.state().screen_shake.as_ref().unwrap().offset;
            assert!([
                [0, -50],
                [-50, 0],
                [50, 50],
                [50, 0],
                [-50, 50],
                [0, 50],
                [50, -50],
            ]
            .contains(&offset));
        }
    }

    #[test]
    fn screen_shake_rejects_unverified_modes_and_is_replaced_by_transition() {
        let invalid_source = b".shakeScreen H 10 10\r\n";
        let mut invalid = firefly_vm(invalid_source, 1);
        assert_eq!(invalid.step(1, 4), Err(MinoriRuntimeError::ScreenShake));

        let source = b".shakeScreen V 10 10\r\n.transition 0 * 10\r\n.end\r\n";
        let mut vm = firefly_vm(source, 1);
        vm.step(1, 1).unwrap();
        assert!(vm.state().screen_shake.is_some());
        assert_eq!(vm.step(2, 2).unwrap(), Some(MinoriVmEvent::Terminal));
        assert!(vm.state().screen_shake.is_none());
    }

    #[test]
    fn transition_configures_the_following_stage_without_guessing_star_as_a_resource() {
        let source = b".transition 0 * 10\r\n.stage * BLACK.png 0 0\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Stage(stage)) = vm.step(1, 4).unwrap() else {
            panic!("expected stage event")
        };
        assert_eq!(stage.resource_sequence, vec![None]);
        assert_eq!(stage.reference_position, None);
        assert_eq!(
            stage.background,
            Some(MinoriStageLayer {
                resource_uri: "minori:/bg/BLACK.png".into(),
                x: 0,
                y: 0,
            })
        );
        assert_eq!(stage.transition.mode, 0);
        assert_eq!(stage.transition.resource, None);
        assert_eq!(stage.transition.duration_ticks, 10);
    }

    #[test]
    fn stage_preserves_resource_sequence_and_verified_stand_pair_contract() {
        let source =
            b".stage PRELOAD_A.png:PRELOAD_B.png BG.png 0 0 STAND.png 727,1685\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Stage(stage)) = vm.step(1, 4).unwrap() else {
            panic!("expected stage event")
        };
        assert_eq!(
            stage.resource_sequence,
            vec![
                Some("minori:/bg/PRELOAD_A.png".into()),
                Some("minori:/bg/PRELOAD_B.png".into()),
            ]
        );
        assert_eq!(stage.reference_position, None);
        assert_eq!(
            stage.stands,
            vec![MinoriStandLayer {
                resource_uri: "minori:/st/STAND.png".into(),
                position: 727,
                resource_parameter: 1685,
            }]
        );
        assert_eq!(vm.state().stage.as_ref(), Some(&stage));

        let snapshot = vm.snapshot_bytes().unwrap();
        let restored = MinoriVm::decode_snapshot(&snapshot).unwrap();
        assert_eq!(restored.stage, Some(stage));
    }

    #[test]
    fn stage_defaults_missing_resource_parameter_and_rejects_malformed_pairs() {
        let source = b".stage * BG.png 0 0 STAND.png 727\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Stage(stage)) = vm.step(1, 4).unwrap() else {
            panic!("expected stage event")
        };
        assert_eq!(stage.stands[0].resource_parameter, 0);

        for operands in [
            ".stage * BG.png 0 0 STAND.png 727,1685,1\r\n",
            ".stage * BG.png 0 0 STAND.png 727,\r\n",
            ".stage A.png:B.png:C.png BG.png 0 0\r\n",
        ] {
            let script = parse_sc(
                format!("{operands}.end\r\n").as_bytes(),
                &ScOpcodeCatalog::observed_minori(),
            )
            .unwrap();
            let mut vm = MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(operands.as_bytes()),
                script,
                1,
            )
            .unwrap();
            assert_eq!(vm.step(1, 4).unwrap_err(), MinoriRuntimeError::Operand);
        }
    }

    #[test]
    fn horizontal_scroll_waits_for_the_native_linear_clock_and_round_trips() {
        let source = b".stage * BG.png 461 0\r\n.hscroll 0 -10\r\n.endscroll 0\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        vm.step(1, 4).unwrap();
        assert!(matches!(
            vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::AxisScroll(_))
        ));
        let started = vm.state().axis_scroll.as_ref().unwrap();
        assert_eq!(started.axis, MinoriAxisScrollAxis::Horizontal);
        assert_eq!(started.start, 461);
        assert_eq!(started.target, 0);
        assert_eq!(started.speed_tenths, -10);
        assert_eq!(started.duration_ms, 461);

        assert!(vm.advance_axis_scroll_clock(200_000_000).unwrap().is_some());
        assert_eq!(vm.state().axis_scroll.as_ref().unwrap().current, 261);
        assert_eq!(
            vm.state()
                .stage
                .as_ref()
                .unwrap()
                .background
                .as_ref()
                .unwrap()
                .x,
            261
        );
        let Some(MinoriVmEvent::Wait(MinoriWaitState::AxisScroll {
            token_id,
            milliseconds,
        })) = vm.step(3, 4).unwrap()
        else {
            panic!("expected axis-scroll completion wait")
        };
        assert_eq!(milliseconds, 261);
        let snapshot = vm.snapshot_bytes().unwrap();
        let hash = vm.state_hash().unwrap();
        vm.advance_axis_scroll_clock(261_000_000).unwrap();
        assert!(vm.state().axis_scroll.as_ref().unwrap().completed);
        assert_eq!(
            vm.state()
                .stage
                .as_ref()
                .unwrap()
                .background
                .as_ref()
                .unwrap()
                .x,
            0
        );
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state_hash().unwrap(), hash);
        vm.resolve_wait(&token_id).unwrap();
        assert!(vm.state().axis_scroll.as_ref().unwrap().completed);
        assert_eq!(
            vm.state()
                .stage
                .as_ref()
                .unwrap()
                .background
                .as_ref()
                .unwrap()
                .x,
            0
        );
        assert_eq!(vm.step(4, 4).unwrap(), Some(MinoriVmEvent::Terminal));
    }

    #[test]
    fn linear_scroll_interpolates_both_axes_and_ends_through_a_time_wait() {
        let source = b".stage * BG.png 192 723\r\n.scroll 0 0 10\r\n.endScroll f\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        vm.step(1, 4).unwrap();
        assert!(matches!(
            vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::LinearScroll(_))
        ));
        let started = vm.state().linear_scroll.as_ref().unwrap();
        assert_eq!(started.start, [192, 723]);
        assert_eq!(started.target, [0, 0]);
        assert_eq!(started.speed_tenths, 10);
        assert_eq!(started.duration_ms, 723);

        assert!(vm
            .advance_linear_scroll_clock(100_000_000)
            .unwrap()
            .is_some());
        assert_eq!(
            vm.state().linear_scroll.as_ref().unwrap().current,
            [166, 623]
        );
        assert_eq!(
            stage_position(vm.state().stage.as_ref().unwrap()).unwrap(),
            [166, 623]
        );
        let Some(MinoriVmEvent::Wait(MinoriWaitState::LinearScroll {
            token_id,
            milliseconds,
        })) = vm.step(3, 4).unwrap()
        else {
            panic!("expected linear-scroll completion wait")
        };
        assert_eq!(milliseconds, 623);
        let snapshot = vm.snapshot_bytes().unwrap();
        assert_eq!(MinoriVm::decode_snapshot(&snapshot).unwrap(), *vm.state());
        vm.resolve_wait(&token_id).unwrap();
        let completed = vm.state().linear_scroll.as_ref().unwrap();
        assert!(completed.completed);
        assert_eq!(completed.current, [0, 0]);
        assert_eq!(
            stage_position(vm.state().stage.as_ref().unwrap()).unwrap(),
            [0, 0]
        );
        assert_eq!(vm.step(4, 4).unwrap(), Some(MinoriVmEvent::Terminal));
    }

    #[test]
    fn vertical_scroll_force_finish_updates_the_stage_at_the_command_boundary() {
        let source = b".stage * BG.png 0 605\r\n.vscroll 0 -10\r\n.endscroll true\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        vm.step(1, 4).unwrap();
        vm.step(2, 4).unwrap();
        vm.advance_axis_scroll_clock(100_000_000).unwrap();
        assert_eq!(vm.state().axis_scroll.as_ref().unwrap().current, 505);
        assert!(matches!(
            vm.step(3, 4).unwrap(),
            Some(MinoriVmEvent::AxisScroll(_))
        ));
        let completed = vm.state().axis_scroll.as_ref().unwrap();
        assert_eq!(completed.axis, MinoriAxisScrollAxis::Vertical);
        assert!(completed.completed);
        assert_eq!(completed.current, 0);
        assert_eq!(
            vm.state()
                .stage
                .as_ref()
                .unwrap()
                .background
                .as_ref()
                .unwrap()
                .y,
            0
        );
    }

    #[test]
    fn axis_scroll_rejects_direction_conflicts_and_corrupt_snapshot_state() {
        for source in [
            b".hscroll 0 -10\r\n".as_slice(),
            b".stage * BG.png 461 0\r\n.hscroll 0 0\r\n".as_slice(),
            b".stage * BG.png 461 0\r\n.hscroll 0 10\r\n".as_slice(),
            b".stage * BG.png 461 0\r\n.hscroll 65537 10\r\n".as_slice(),
        ] {
            let mut vm = firefly_vm(source, 7);
            if source.starts_with(b".stage") {
                vm.step(1, 4).unwrap();
                assert_eq!(vm.step(2, 4).unwrap_err(), MinoriRuntimeError::AxisScroll);
            } else {
                assert_eq!(vm.step(1, 4).unwrap_err(), MinoriRuntimeError::AxisScroll);
            }
        }

        let source = b".stage * BG.png 461 0\r\n.hscroll 0 -10\r\n.end\r\n";
        let mut vm = firefly_vm(source, 7);
        vm.step(1, 4).unwrap();
        vm.step(2, 4).unwrap();
        let mut corrupt = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        corrupt.axis_scroll.as_mut().unwrap().current -= 1;
        let corrupt = postcard::to_allocvec(&corrupt).unwrap();
        assert_eq!(
            MinoriVm::decode_snapshot(&corrupt).unwrap_err(),
            MinoriRuntimeError::AxisScroll
        );
    }

    #[test]
    fn scroll_xf_advances_deterministically_and_round_trips_snapshot_state() {
        let source = b".stage * BG.png 0 0\r\n.scrollXF 0 720 1280 720 0 0 1280 0 1200 0\r\n.endScroll f\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert!(matches!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::Stage(_))
        ));
        assert!(matches!(
            vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::ScrollXf(_))
        ));
        let started = vm.state().scroll_xf.as_ref().unwrap();
        assert_eq!(started.visible_extent, [0, 720]);
        assert_eq!(started.visible_offset, [0, 0]);

        assert!(vm.advance_scroll_xf_clock(600_000_000).unwrap().is_some());
        let halfway = vm.state().scroll_xf.as_ref().unwrap();
        assert_eq!(halfway.visible_extent, [640, 720]);
        assert_eq!(halfway.visible_offset, [640, 0]);
        assert!(!halfway.completed);

        let snapshot = vm.snapshot_bytes().unwrap();
        let snapshot_hash = vm.state_hash().unwrap();
        vm.advance_scroll_xf_clock(600_000_000).unwrap();
        let completed = vm.state().scroll_xf.as_ref().unwrap();
        assert_eq!(completed.visible_extent, [1280, 720]);
        assert_eq!(completed.visible_offset, [1280, 0]);
        assert!(completed.completed);
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state_hash().unwrap(), snapshot_hash);

        assert_eq!(vm.step(3, 4).unwrap(), Some(MinoriVmEvent::Terminal));
        assert_eq!(
            vm.state().scroll_xf.as_ref().unwrap().visible_extent,
            [640, 720]
        );
    }

    #[test]
    fn scroll_xf_easing_force_finish_and_corrupt_state_are_fail_closed() {
        let source = b".stage * BG.png 0 0\r\n.scrollXF 0 100 100 100 0 0 100 0 1000 2\r\n.endScroll t\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        vm.step(1, 4).unwrap();
        vm.step(2, 4).unwrap();
        vm.advance_scroll_xf_clock(500_000_000).unwrap();
        assert_eq!(
            vm.state().scroll_xf.as_ref().unwrap().visible_extent,
            [75, 100]
        );
        assert!(matches!(
            vm.step(3, 4).unwrap(),
            Some(MinoriVmEvent::ScrollXf(_))
        ));
        let completed = vm.state().scroll_xf.as_ref().unwrap();
        assert_eq!(completed.visible_extent, [100, 100]);
        assert_eq!(completed.visible_offset, [100, 0]);
        assert!(completed.completed);

        let mut state = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        state.scroll_xf.as_mut().unwrap().visible_extent = [99, 100];
        let corrupt = postcard::to_allocvec(&state).unwrap();
        assert!(matches!(
            MinoriVm::decode_snapshot(&corrupt),
            Err(MinoriRuntimeError::ScrollXf)
        ));

        for command in [
            ".scrollXF 0 1 1 1 0 0 1 0 0 0",
            ".scrollXF 0 1 1 1 0 0 1 0 1000 3",
            ".scrollXF 0 1 1 1 0 0 1 0 1000",
        ] {
            let invalid_source = format!(".stage * BG.png 0 0\r\n{command}\r\n.end\r\n");
            let script = parse_sc(
                invalid_source.as_bytes(),
                &ScOpcodeCatalog::observed_minori(),
            )
            .unwrap();
            let mut vm = MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(invalid_source.as_bytes()),
                script,
                1,
            )
            .unwrap();
            vm.step(1, 4).unwrap();
            assert_eq!(vm.step(2, 4).unwrap_err(), MinoriRuntimeError::ScrollXf);
        }
    }

    #[test]
    fn wscroll2_uses_verified_sync_profile_and_deterministic_parallax_state() {
        let source =
            b".stage * FAR.png 0 0 NEAR.png 0\r\n.effect WScroll2 sync:walk.txt 60 -8\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        vm.step(1, 4).unwrap();
        assert!(matches!(
            vm.step(2, 4).unwrap(),
            Some(MinoriVmEvent::WScroll2(_))
        ));
        let started = vm.state().wscroll2.as_ref().unwrap();
        assert_eq!(started.sync_resource_uri, "minori:/st/walk.txt");
        assert_eq!(started.period_ticks, 60);
        assert_eq!(started.speed_tenths, -8);

        assert!(vm.advance_wscroll2_clock(1_000_000_000).unwrap().is_some());
        let advanced = vm.state().wscroll2.as_ref().unwrap();
        assert_eq!(advanced.elapsed_ticks, 60);
        assert_eq!(advanced.foreground_offset, -48);
        assert_eq!(advanced.background_offset, -9);
        assert_eq!(advanced.background_remainder, -3);
        let snapshot = vm.snapshot_bytes().unwrap();
        let hash = vm.state_hash().unwrap();
        vm.advance_wscroll2_clock(1_000_000_000).unwrap();
        vm.restore_state(&snapshot).unwrap();
        assert_eq!(vm.state_hash().unwrap(), hash);

        let mut corrupt = MinoriVm::decode_snapshot(&snapshot).unwrap();
        corrupt.wscroll2.as_mut().unwrap().foreground_offset -= 1;
        let corrupt = postcard::to_allocvec(&corrupt).unwrap();
        assert!(matches!(
            MinoriVm::decode_snapshot(&corrupt),
            Err(MinoriRuntimeError::WScroll2)
        ));
    }

    #[test]
    fn wscroll2_rejects_unverified_resource_modes_and_bounds() {
        for effect in [
            ".effect WScroll2 char:walk 60 -8",
            ".effect WScroll2 sync:walk.txt 0 -8",
            ".effect WScroll2 sync:walk.txt 60 10001",
            ".effect WScroll2 sync:walk.txt 60",
        ] {
            let source = format!(".stage * FAR.png 0 0 NEAR.png 0\r\n{effect}\r\n.end\r\n");
            let script = parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_minori()).unwrap();
            let mut vm = MinoriVm::new(
                "minori:/scr/fixture.sc".into(),
                Hash256::from_sha256(source.as_bytes()),
                script,
                1,
            )
            .unwrap();
            vm.step(1, 4).unwrap();
            assert_eq!(vm.step(2, 4).unwrap_err(), MinoriRuntimeError::WScroll2);
        }
    }

    #[test]
    fn message_uses_the_verified_four_operand_and_joined_tail_contract() {
        let source = b".message 42 voice speaker hello world\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Message {
            presentation_sequence,
            capture_sequence,
            text,
            speaker,
            audio_commands,
            wait: MinoriWaitState::Input { token_id },
            ..
        }) = vm.step(1, 4).unwrap()
        else {
            panic!("expected message input wait")
        };
        assert_eq!(text, "hello world");
        assert_eq!(speaker.as_deref(), Some("speaker"));
        assert_eq!(presentation_sequence, 3);
        assert_eq!(capture_sequence, 4);
        assert_eq!(audio_commands.len(), 2);
        assert!(matches!(
            &audio_commands[0],
            MinoriAudioCommand::LoadResource {
                stream_id: VOICE_STREAM_ID,
                resource_uri,
                ..
            } if resource_uri == "minori:/voice/voice"
        ));
        assert!(matches!(
            &audio_commands[1],
            MinoriAudioCommand::Play {
                stream_id: VOICE_STREAM_ID,
                volume,
                pan,
                repeat: false,
                ..
            } if *volume == 1.0 && *pan == 0.0
        ));
        assert_eq!(token_id, "minori.message.1");
        let state = vm.state().message.as_ref().unwrap();
        assert_eq!(state.message_id, 42);
        assert_eq!(state.text_hash, Hash256::from_sha256(b"hello world"));
        assert_eq!(state.voice_hash, Some(Hash256::from_sha256(b"voice")));
        assert_eq!(
            state.voice,
            Some(MinoriMessageVoice {
                resource_uri: "minori:/voice/voice".into(),
                volume_milli: 1000,
                pan_milli: 0,
            })
        );
        assert_eq!(vm.state().backlog.len(), 1);
        assert_eq!(vm.state().backlog_bytes, 18);
        let backlog = &vm.state().backlog[0];
        assert_eq!(backlog.message_id, 42);
        assert_eq!(backlog.text, "hello world");
        assert_eq!(backlog.speaker.as_deref(), Some("speaker"));
        assert_eq!(backlog.text_hash, state.text_hash);
        assert_eq!(backlog.speaker_hash, state.speaker_hash);
        assert_eq!(backlog.voice_hash, state.voice_hash);
        assert_eq!(backlog.voice, state.voice);

        let snapshot = vm.snapshot_bytes().unwrap();
        let mut corrupt = MinoriVm::decode_snapshot(&snapshot).unwrap();
        corrupt.backlog[0].text.push('!');
        let corrupt = postcard::to_allocvec(&corrupt).unwrap();
        assert_eq!(
            MinoriVm::decode_snapshot(&corrupt).unwrap_err(),
            MinoriRuntimeError::Backlog
        );

        vm.open_backlog().unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(0));
        vm.advance_system_tick(2).unwrap();
        vm.move_backlog(-1).unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(0));
        vm.close_backlog().unwrap();
        assert_eq!(vm.state().system_ui.page, MinoriSystemPage::None);
        assert!(vm.state().wait.is_some());
    }

    #[test]
    fn message_preserves_empty_voice_and_speaker_positions() {
        let source = b".message 42   body words\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Message { text, speaker, .. }) = vm.step(1, 4).unwrap() else {
            panic!("expected message input wait")
        };
        assert_eq!(text, "body words");
        assert!(speaker.is_none());
        let state = vm.state().message.as_ref().unwrap();
        assert_eq!(state.message_id, 42);
        assert_eq!(state.text_hash, Hash256::from_sha256(b"body words"));
        assert!(state.voice_hash.is_none());
        assert!(state.speaker_hash.is_none());
    }

    #[test]
    fn backlog_cursor_moves_across_multiple_retained_messages_and_clamps() {
        let source = b".message 1  speaker first\r\n.message 2  speaker second\r\n.message 3  speaker third\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();

        for tick in 1..=3 {
            let Some(MinoriVmEvent::Message { wait, .. }) = vm.step(tick, 4).unwrap() else {
                panic!("expected retained message {tick}")
            };
            if tick < 3 {
                let token_id = match wait {
                    MinoriWaitState::Input { token_id } => token_id,
                    _ => panic!("expected input wait"),
                };
                vm.resolve_wait(&token_id).unwrap();
            }
        }
        assert_eq!(
            vm.state()
                .backlog
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third"]
        );

        vm.open_backlog().unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(2));
        vm.move_backlog(-1).unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(1));
        vm.move_backlog(-1).unwrap();
        vm.move_backlog(-1).unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(0));
        vm.move_backlog(1).unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(1));
        vm.move_backlog(1).unwrap();
        vm.move_backlog(1).unwrap();
        assert_eq!(vm.state().system_ui.backlog_cursor, Some(2));
    }

    #[test]
    fn message_voice_uses_the_verified_suffix_and_stops_the_previous_stream() {
        let source = b".message 1 first[25,-50] speaker one\r\n.message 2 second speaker two\r\n.message 3  speaker three\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Message { audio_commands, .. }) = vm.step(1, 4).unwrap() else {
            panic!("expected first voiced message")
        };
        assert!(matches!(
            audio_commands.as_slice(),
            [
                MinoriAudioCommand::LoadResource { resource_uri, .. },
                MinoriAudioCommand::Play {
                    volume,
                    pan,
                    repeat: false,
                    ..
                }
            ] if resource_uri == "minori:/voice/first" && *volume == 0.25 && *pan == -0.5
        ));
        vm.resolve_wait("minori.message.1").unwrap();
        let Some(MinoriVmEvent::Message { audio_commands, .. }) = vm.step(2, 4).unwrap() else {
            panic!("expected replacement voice")
        };
        assert!(matches!(
            audio_commands.as_slice(),
            [
                MinoriAudioCommand::Stop { stream_id: VOICE_STREAM_ID, fade_ms: 0, .. },
                MinoriAudioCommand::LoadResource { resource_uri, .. },
                MinoriAudioCommand::Play { repeat: false, .. }
            ] if resource_uri == "minori:/voice/second"
        ));
        vm.resolve_wait("minori.message.2").unwrap();
        let Some(MinoriVmEvent::Message { audio_commands, .. }) = vm.step(3, 4).unwrap() else {
            panic!("expected silent message")
        };
        assert!(matches!(
            audio_commands.as_slice(),
            [MinoriAudioCommand::Stop {
                stream_id: VOICE_STREAM_ID,
                fade_ms: 0,
                ..
            }]
        ));
        assert!(!vm.state().audio[&VOICE_STREAM_ID].playing);
    }

    #[test]
    fn character_voice_toggle_keeps_backlog_identity_but_suppresses_playback() {
        let source = b".message 1 aya-A02-0003 speaker one\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let mut state = MinoriVm::decode_snapshot(&vm.snapshot_bytes().unwrap()).unwrap();
        state.system_ui.config.character_voice_enabled[2] = false;
        vm.restore_state(&postcard::to_allocvec(&state).unwrap())
            .unwrap();
        let Some(MinoriVmEvent::Message { audio_commands, .. }) = vm.step(1, 4).unwrap() else {
            panic!("expected message")
        };
        assert!(audio_commands.is_empty());
        assert_eq!(
            vm.state()
                .message
                .as_ref()
                .unwrap()
                .voice
                .as_ref()
                .unwrap()
                .resource_uri,
            "minori:/voice/aya-A02-0003"
        );
    }

    #[test]
    fn message_preserves_empty_voice_before_speaker() {
        let source = b".message 42  speaker body\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Message { text, speaker, .. }) = vm.step(1, 4).unwrap() else {
            panic!("expected message input wait")
        };
        assert_eq!(text, "body");
        assert_eq!(speaker.as_deref(), Some("speaker"));
        assert!(vm.state().message.as_ref().unwrap().voice_hash.is_none());
    }

    #[test]
    fn short_message_executes_the_observed_constructor_defaults() {
        let source = b".message 42 incomplete\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Message { text, speaker, .. }) = vm.step(1, 1).unwrap() else {
            panic!("expected empty message update")
        };
        assert!(text.is_empty());
        assert!(speaker.is_none());
        assert_eq!(vm.state().message.as_ref().unwrap().message_id, -1);
    }

    #[test]
    fn audio_resource_suffix_matches_the_observed_volume_pan_contract() {
        assert_eq!(
            parse_audio_resource_spec("theme.ogg[75,-120]").unwrap(),
            MinoriAudioResourceSpec {
                resource: "theme.ogg".into(),
                volume_percent: 75,
                pan_percent: -100,
            }
        );
        assert_eq!(
            parse_audio_resource_spec("theme.ogg[120,25suffix]").unwrap(),
            MinoriAudioResourceSpec {
                resource: "theme.ogg".into(),
                volume_percent: 100,
                pan_percent: 25,
            }
        );
        assert_eq!(
            parse_audio_resource_spec("theme.ogg[broken").unwrap(),
            MinoriAudioResourceSpec {
                resource: "theme.ogg".into(),
                volume_percent: 100,
                pan_percent: 0,
            }
        );
        assert_eq!(
            parse_audio_resource_spec("[50,0]").unwrap_err(),
            MinoriRuntimeError::AudioResource
        );
    }

    #[test]
    fn play_bgm_emits_stable_uri_audio_commands_with_observed_defaults() {
        let source = b".playBGM theme.ogg[50,-25] * * 80\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Audio { commands }) = vm.step(1, 4).unwrap() else {
            panic!("expected BGM commands")
        };
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0],
            MinoriAudioCommand::LoadResource {
                sequence: 1,
                stream_id: 0,
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: "minori:/bgm/theme.ogg".into(),
            }
        );
        assert_eq!(
            commands[1],
            MinoriAudioCommand::Play {
                sequence: 2,
                stream_id: 0,
                volume: 0.4,
                pan: -0.25,
                repeat: true,
                fade_in_ms: 2,
            }
        );
        let state = vm.state().audio.get(&0).unwrap();
        assert_eq!(state.volume_milli, 400);
        assert_eq!(state.pan_milli, -250);
    }

    #[test]
    fn audio_control_token_stops_the_bound_bus_with_fade_out() {
        let source = b".playBGM theme.ogg\r\n.playBGM * * 25\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert!(matches!(
            vm.step(1, 1).unwrap(),
            Some(MinoriVmEvent::Audio { .. })
        ));
        let Some(MinoriVmEvent::Audio { commands }) = vm.step(2, 1).unwrap() else {
            panic!("expected BGM stop command")
        };
        assert_eq!(
            commands,
            vec![MinoriAudioCommand::Stop {
                sequence: 3,
                stream_id: 0,
                fade_ms: 25,
            }]
        );
        assert!(!vm.state().audio.get(&0).unwrap().playing);
    }

    #[test]
    fn play_voice_control_token_is_a_bounded_noop_without_active_voice() {
        let source = b".playVoice * false 2 30\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 1).unwrap(),
            Some(MinoriVmEvent::Audio {
                commands: Vec::new()
            })
        );
    }

    #[test]
    fn play_se_preserves_repeat_bus_and_resource_metadata() {
        let source = b".playSE click.ogg[75,20] true * 30\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        let Some(MinoriVmEvent::Audio { commands }) = vm.step(1, 4).unwrap() else {
            panic!("expected SE commands")
        };
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0],
            MinoriAudioCommand::LoadResource {
                sequence: 1,
                stream_id: 1,
                encoding: MinoriAudioEncoding::Ogg,
                resource_uri: "minori:/se/click.ogg".into(),
            }
        );
        assert_eq!(
            commands[1],
            MinoriAudioCommand::Play {
                sequence: 2,
                stream_id: 1,
                volume: 0.75,
                pan: 0.2,
                repeat: true,
                fade_in_ms: 2,
            }
        );
        let state = vm.state().audio.get(&1).unwrap();
        assert_eq!(state.bus, "se");
        assert!(state.looped);
    }

    #[test]
    fn chain_is_a_bounded_tail_transfer_without_a_return_frame() {
        let source = b".set local = 1\r\n.chain K01.sc\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(
            vm.step(1, 4).unwrap(),
            Some(MinoriVmEvent::Chain {
                target: "K01.sc".into()
            })
        );

        let next = b".end\r\n";
        vm.replace_script(
            "minori:/scr/K01.sc".into(),
            Hash256::from_sha256(next),
            parse_sc(next, &ScOpcodeCatalog::observed_minori()).unwrap(),
        )
        .unwrap();
        assert!(vm.state().variables.is_empty());
        assert_eq!(vm.step(2, 2).unwrap(), Some(MinoriVmEvent::Terminal));
    }

    #[test]
    fn chain_rejects_path_escape() {
        let source = b".chain ../outside.sc\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1, 1).unwrap_err(), MinoriRuntimeError::ChainTarget);
    }

    #[test]
    fn assignment_uses_verified_three_and_five_token_forms() {
        let source = b".set base = 6\r\n.set sum = base + 4\r\n.set bits = sum | 1\r\n.set rem = sum % 4\r\n.end\r\n";
        let script = parse_sc(source, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1, 8).unwrap(), Some(MinoriVmEvent::Terminal));
        assert_eq!(vm.state().variables.get("sum"), Some(&10));
        assert_eq!(vm.state().variables.get("bits"), Some(&11));
        assert_eq!(vm.state().variables.get("rem"), Some(&2));

        let unsupported = b".set count += 1\r\n";
        let script = parse_sc(unsupported, &ScOpcodeCatalog::observed_minori()).unwrap();
        let mut vm = MinoriVm::new(
            "minori:/scr/test.sc".into(),
            Hash256::from_sha256(unsupported),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1, 1).unwrap_err(), MinoriRuntimeError::Operand);
    }
}
