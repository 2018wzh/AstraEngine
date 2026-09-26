use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLivePresentationCommand {
    pub sequence: u64,
    pub command: RuntimeLivePresentationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLivePresentationKind {
    Dialogue {
        key: String,
        speaker: Option<String>,
        voice: Option<String>,
        window: Option<String>,
    },
    Choice {
        key: String,
        options: Vec<RuntimeLiveChoiceOption>,
    },
    SystemPage {
        page: RuntimeLiveSystemPage,
    },
    SystemOption {
        option: RuntimeLiveChoiceOption,
    },
    Stage(RuntimeLiveStageCommand),
    Extension(RuntimeLiveExtensionCommand),
    Marker {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveChoiceOption {
    pub id: String,
    pub key: String,
    pub target: String,
    pub enabled_when: Option<RuntimeLiveVariableCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVariableCondition {
    pub scope: String,
    pub key: String,
    pub operation: RuntimeLiveComparison,
    pub value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveComparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveSystemPage {
    Title,
    QuickPanel,
    Save,
    Load,
    Config,
    Gallery,
    Replay,
    VoiceReplay,
    RouteChart,
    Backlog,
    LocalizationPreview,
    Custom,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveStageCommand {
    Preload {
        asset: String,
    },
    Configure {
        width: u32,
        height: u32,
        safe_area_width: u32,
        safe_area_height: u32,
    },
    DeclareLayer {
        id: String,
        kind: RuntimeLiveStageLayerKind,
        z: i32,
        blend: RuntimeLiveStageBlend,
        clip: Option<RuntimeLiveStageClip>,
        input: Option<String>,
    },
    Background {
        asset: String,
        layer: String,
        preset: Option<String>,
        duration_ms: u32,
        interrupt: RuntimeLiveInterruptPolicy,
    },
    Show {
        id: String,
        asset: String,
        pose: Option<String>,
        layer: String,
        placement: RuntimeLiveStagePlacement,
        fit: RuntimeLiveStageFit,
        opacity_millionths: i64,
        preset: Option<String>,
        interrupt: RuntimeLiveInterruptPolicy,
    },
    Hide {
        id: String,
        preset: Option<String>,
        duration_ms: u32,
        interrupt: RuntimeLiveInterruptPolicy,
    },
    ClearLayer {
        layer: String,
        duration_ms: u32,
        interrupt: RuntimeLiveInterruptPolicy,
    },
    SetLayerVisibility {
        layer: String,
        visible: bool,
    },
    Backdrop {
        color: [u8; 4],
    },
    Shade {
        color: [u8; 4],
        opacity_millionths: i64,
    },
    SetSkipAllowed {
        allowed: bool,
    },
    Move {
        id: String,
        x_millionths: i64,
        y_millionths: i64,
        duration_ms: u32,
        preset: Option<String>,
        interrupt: RuntimeLiveInterruptPolicy,
    },
    Camera {
        target: String,
        x_millionths: i64,
        y_millionths: i64,
        zoom_millionths: i64,
        rotation_millionths: i64,
        duration_ms: u32,
        preset: Option<String>,
    },
    Movie {
        layer: String,
        asset: String,
        alpha_millionths: i64,
        loop_mode: RuntimeLiveMovieLoop,
        end: RuntimeLiveMovieEnd,
        fence: Option<String>,
        fallback: Option<String>,
        interrupt: RuntimeLiveInterruptPolicy,
    },
    Audio(RuntimeLiveAudioCueCommand),
    AudioControl(RuntimeLiveAudioControl),
    SetAudioBusEnabled {
        bus: super::RuntimeLiveAudioBus,
        enabled: bool,
    },
    Transition {
        preset: String,
        duration_ms: u32,
        descriptor_id: Option<String>,
    },
    Shake {
        target: String,
        strength_millionths: i64,
        duration_ms: u32,
    },
    Timeline(RuntimeLiveTimelineCommand),
    Effect {
        target: String,
        lip_sync: bool,
        filter: String,
        fallback: String,
        budget_us: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveStageLayerKind {
    Background,
    Sprite,
    Video,
    Text,
    Cg,
    Ui,
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveStageBlend {
    Normal,
    Add,
    Multiply,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveStageClip {
    Stage,
    SafeArea,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveStagePlacement {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveStageFit {
    ContainHeight,
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveInterruptPolicy {
    Queue,
    ReplaceFromCurrent,
    SnapThenStart,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveMovieLoop {
    Once,
    Loop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveMovieEnd {
    Continue,
    Wait,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveAudioCueCommand {
    pub id: String,
    pub bus: super::RuntimeLiveAudioBus,
    pub asset: String,
    pub looped: bool,
    pub fade_ms: u32,
    pub sync: super::RuntimeLiveAudioSync,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveAudioControl {
    pub id: String,
    pub action: RuntimeLiveAudioControlAction,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveAudioControlAction {
    Pause,
    Resume,
    Stop,
    FadeStop { duration_ms: u32, fence: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveTimelineTask {
    pub command_id: String,
    pub command: RuntimeLiveTimelineCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveTimelineCommand {
    Start(RuntimeLiveTimelineSpec),
    Cancel { id: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveTimelineSpec {
    pub id: String,
    pub join: RuntimeLiveTimelineJoin,
    pub tracks: Vec<RuntimeLiveTimelineTrack>,
    pub fence: Option<String>,
    pub fallback: Option<String>,
    pub budget_us: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveTimelineJoin {
    FireAndForget,
    Block,
    ReplaceTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveTimelineTrack {
    pub target: String,
    pub property: String,
    pub keyframes: Vec<RuntimeLiveTimelineKeyframe>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLiveTimelineKeyframe {
    pub time_ms: u32,
    pub value_millionths: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveExtensionCommand {
    pub command: String,
    pub provider_id: String,
    pub schema: String,
    pub fields: BTreeMap<String, RuntimeLiveExtensionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveExtensionValue {
    String(String),
    Integer(i64),
    Fixed(i64),
    Boolean(bool),
    Symbol(String),
    AssetUri(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveAudioBus {
    Voice,
    Bgm,
    Se,
    Movie,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveAudioSync {
    None,
    Text,
    Fence(String),
}
