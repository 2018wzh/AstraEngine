use std::collections::BTreeMap;

#[cfg(feature = "ffi")]
use abi_stable::{
    std_types::{ROption, RString, RVec},
    StableAbi,
};

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

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLivePresentationCommand {
    pub sequence: u64,
    pub command: FfiRuntimeLivePresentationKind,
}

#[cfg(feature = "ffi")]
#[repr(u8)]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiRuntimeLivePresentationKind {
    Dialogue {
        key: RString,
        speaker: ROption<RString>,
        voice: ROption<RString>,
        window: ROption<RString>,
    },
    Choice {
        key: RString,
        options: RVec<FfiRuntimeLiveChoiceOption>,
    },
    SystemPage {
        page: FfiRuntimeLiveSystemPage,
    },
    SystemOption {
        option: FfiRuntimeLiveChoiceOption,
    },
    Stage(FfiRuntimeLiveStageCommand),
    Extension(FfiRuntimeLiveExtensionCommand),
    Marker {
        id: RString,
    },
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveChoiceOption {
    pub id: RString,
    pub key: RString,
    pub target: RString,
    pub enabled_when: ROption<FfiRuntimeLiveVariableCondition>,
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveVariableCondition {
    pub scope: RString,
    pub key: RString,
    pub operation: FfiRuntimeLiveComparison,
    pub value: i64,
}

#[cfg(feature = "ffi")]
#[repr(u8)]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeLiveComparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[cfg(feature = "ffi")]
#[repr(u8)]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeLiveSystemPage {
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

#[cfg(feature = "ffi")]
#[repr(u8)]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiRuntimeLiveStageCommand {
    Preload {
        asset: RString,
    },
    Configure {
        width: u32,
        height: u32,
        safe_area_width: u32,
        safe_area_height: u32,
    },
    DeclareLayer {
        id: RString,
        kind: FfiRuntimeLiveStageLayerKind,
        z: i32,
        blend: FfiRuntimeLiveStageBlend,
        clip: ROption<FfiRuntimeLiveStageClip>,
        input: ROption<RString>,
    },
    Background {
        asset: RString,
        layer: RString,
        preset: ROption<RString>,
        duration_ms: u32,
        interrupt: FfiRuntimeLiveInterruptPolicy,
    },
    Show {
        id: RString,
        asset: RString,
        pose: ROption<RString>,
        layer: RString,
        placement: FfiRuntimeLiveStagePlacement,
        fit: FfiRuntimeLiveStageFit,
        opacity_millionths: i64,
        preset: ROption<RString>,
        interrupt: FfiRuntimeLiveInterruptPolicy,
    },
    Hide {
        id: RString,
        preset: ROption<RString>,
        duration_ms: u32,
        interrupt: FfiRuntimeLiveInterruptPolicy,
    },
    ClearLayer {
        layer: RString,
        duration_ms: u32,
        interrupt: FfiRuntimeLiveInterruptPolicy,
    },
    SetLayerVisibility {
        layer: RString,
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
        id: RString,
        x_millionths: i64,
        y_millionths: i64,
        duration_ms: u32,
        preset: ROption<RString>,
        interrupt: FfiRuntimeLiveInterruptPolicy,
    },
    Camera {
        target: RString,
        x_millionths: i64,
        y_millionths: i64,
        zoom_millionths: i64,
        rotation_millionths: i64,
        duration_ms: u32,
        preset: ROption<RString>,
    },
    Movie {
        layer: RString,
        asset: RString,
        alpha_millionths: i64,
        loop_mode: FfiRuntimeLiveMovieLoop,
        end: FfiRuntimeLiveMovieEnd,
        fence: ROption<RString>,
        fallback: ROption<RString>,
        interrupt: FfiRuntimeLiveInterruptPolicy,
    },
    Audio(FfiRuntimeLiveAudioCueCommand),
    AudioControl(FfiRuntimeLiveAudioControl),
    SetAudioBusEnabled {
        bus: super::FfiRuntimeAudioBus,
        enabled: bool,
    },
    Transition {
        preset: RString,
        duration_ms: u32,
        descriptor_id: ROption<RString>,
    },
    Shake {
        target: RString,
        strength_millionths: i64,
        duration_ms: u32,
    },
    Timeline(FfiRuntimeLiveTimelineCommand),
    Effect {
        target: RString,
        lip_sync: bool,
        filter: RString,
        fallback: RString,
        budget_us: u32,
    },
}

#[cfg(feature = "ffi")]
macro_rules! ffi_copy_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, StableAbi)]
        pub enum $name { $($variant),+ }
    };
}

#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveStageLayerKind {
    Background,
    Sprite,
    Video,
    Text,
    Cg,
    Ui,
    Effect
});
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveStageBlend {
    Normal,
    Add,
    Multiply,
    Screen
});
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveStageClip { Stage, SafeArea });
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveStagePlacement {
    Left,
    Center,
    Right
});
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveStageFit {
    ContainHeight,
    Native
});
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveInterruptPolicy {
    Queue,
    ReplaceFromCurrent,
    SnapThenStart,
    Reject
});
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveMovieLoop { Once, Loop });
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveMovieEnd {
    Continue,
    Wait,
    Hold
});
#[cfg(feature = "ffi")]
ffi_copy_enum!(FfiRuntimeLiveTimelineJoin {
    FireAndForget,
    Block,
    ReplaceTarget
});

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveAudioCueCommand {
    pub id: RString,
    pub bus: super::FfiRuntimeAudioBus,
    pub asset: RString,
    pub looped: bool,
    pub fade_ms: u32,
    pub sync: super::FfiRuntimeAudioSync,
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveAudioControl {
    pub id: RString,
    pub action: FfiRuntimeLiveAudioControlAction,
    pub target: RString,
}

#[cfg(feature = "ffi")]
#[repr(u8)]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiRuntimeLiveAudioControlAction {
    Pause,
    Resume,
    Stop,
    FadeStop { duration_ms: u32, fence: RString },
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveTimelineTask {
    pub command_id: RString,
    pub command: FfiRuntimeLiveTimelineCommand,
}

#[cfg(feature = "ffi")]
#[repr(u8)]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiRuntimeLiveTimelineCommand {
    Start(FfiRuntimeLiveTimelineSpec),
    Cancel { id: RString, reason: RString },
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveTimelineSpec {
    pub id: RString,
    pub join: FfiRuntimeLiveTimelineJoin,
    pub tracks: RVec<FfiRuntimeLiveTimelineTrack>,
    pub fence: ROption<RString>,
    pub fallback: ROption<RString>,
    pub budget_us: u32,
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveTimelineTrack {
    pub target: RString,
    pub property: RString,
    pub keyframes: RVec<FfiRuntimeLiveTimelineKeyframe>,
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, Copy, StableAbi)]
pub struct FfiRuntimeLiveTimelineKeyframe {
    pub time_ms: u32,
    pub value_millionths: i64,
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveExtensionCommand {
    pub command: RString,
    pub provider_id: RString,
    pub schema: RString,
    pub fields: RVec<FfiRuntimeLiveExtensionField>,
}

#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveExtensionField {
    pub name: RString,
    pub value: FfiRuntimeLiveExtensionValue,
}

#[cfg(feature = "ffi")]
#[repr(u8)]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiRuntimeLiveExtensionValue {
    String(RString),
    Integer(i64),
    Fixed(i64),
    Boolean(bool),
    Symbol(RString),
    AssetUri(RString),
}

#[cfg(feature = "ffi")]
macro_rules! enum_bridge {
    ($rust:ident, $ffi:ident { $($variant:ident),+ $(,)? }) => {
        impl From<$rust> for $ffi {
            fn from(value: $rust) -> Self {
                match value { $($rust::$variant => Self::$variant),+ }
            }
        }
        impl From<$ffi> for $rust {
            fn from(value: $ffi) -> Self {
                match value { $($ffi::$variant => Self::$variant),+ }
            }
        }
    };
}

#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveComparison,
    FfiRuntimeLiveComparison {
        Equal,
        NotEqual,
        Less,
        LessEqual,
        Greater,
        GreaterEqual
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveSystemPage,
    FfiRuntimeLiveSystemPage {
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
        Unknown
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveStageLayerKind,
    FfiRuntimeLiveStageLayerKind {
        Background,
        Sprite,
        Video,
        Text,
        Cg,
        Ui,
        Effect
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveStageBlend,
    FfiRuntimeLiveStageBlend {
        Normal,
        Add,
        Multiply,
        Screen
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveStageClip,
    FfiRuntimeLiveStageClip { Stage, SafeArea }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveStagePlacement,
    FfiRuntimeLiveStagePlacement {
        Left,
        Center,
        Right
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveStageFit,
    FfiRuntimeLiveStageFit {
        ContainHeight,
        Native
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveInterruptPolicy,
    FfiRuntimeLiveInterruptPolicy {
        Queue,
        ReplaceFromCurrent,
        SnapThenStart,
        Reject
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(RuntimeLiveMovieLoop, FfiRuntimeLiveMovieLoop { Once, Loop });
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveMovieEnd,
    FfiRuntimeLiveMovieEnd {
        Continue,
        Wait,
        Hold
    }
);
#[cfg(feature = "ffi")]
enum_bridge!(
    RuntimeLiveTimelineJoin,
    FfiRuntimeLiveTimelineJoin {
        FireAndForget,
        Block,
        ReplaceTarget
    }
);

#[cfg(feature = "ffi")]
impl RuntimeLivePresentationCommand {
    pub fn into_ffi(self) -> FfiRuntimeLivePresentationCommand {
        FfiRuntimeLivePresentationCommand {
            sequence: self.sequence,
            command: self.command.into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLivePresentationCommand {
    pub fn into_runtime(self) -> Result<RuntimeLivePresentationCommand, String> {
        Ok(RuntimeLivePresentationCommand {
            sequence: self.sequence,
            command: self.command.into_runtime()?,
        })
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLivePresentationKind> for FfiRuntimeLivePresentationKind {
    fn from(value: RuntimeLivePresentationKind) -> Self {
        match value {
            RuntimeLivePresentationKind::Dialogue {
                key,
                speaker,
                voice,
                window,
            } => Self::Dialogue {
                key: key.into(),
                speaker: speaker.map(Into::into).into(),
                voice: voice.map(Into::into).into(),
                window: window.map(Into::into).into(),
            },
            RuntimeLivePresentationKind::Choice { key, options } => Self::Choice {
                key: key.into(),
                options: options
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>()
                    .into(),
            },
            RuntimeLivePresentationKind::SystemPage { page } => {
                Self::SystemPage { page: page.into() }
            }
            RuntimeLivePresentationKind::SystemOption { option } => Self::SystemOption {
                option: option.into(),
            },
            RuntimeLivePresentationKind::Stage(command) => Self::Stage(command.into()),
            RuntimeLivePresentationKind::Extension(command) => Self::Extension(command.into()),
            RuntimeLivePresentationKind::Marker { id } => Self::Marker { id: id.into() },
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLivePresentationKind {
    fn into_runtime(self) -> Result<RuntimeLivePresentationKind, String> {
        Ok(match self {
            Self::Dialogue {
                key,
                speaker,
                voice,
                window,
            } => RuntimeLivePresentationKind::Dialogue {
                key: key.into(),
                speaker: speaker.into_option().map(Into::into),
                voice: voice.into_option().map(Into::into),
                window: window.into_option().map(Into::into),
            },
            Self::Choice { key, options } => RuntimeLivePresentationKind::Choice {
                key: key.into(),
                options: options.into_iter().map(Into::into).collect(),
            },
            Self::SystemPage { page } => {
                RuntimeLivePresentationKind::SystemPage { page: page.into() }
            }
            Self::SystemOption { option } => RuntimeLivePresentationKind::SystemOption {
                option: option.into(),
            },
            Self::Stage(command) => RuntimeLivePresentationKind::Stage(command.into_runtime()?),
            Self::Extension(command) => {
                RuntimeLivePresentationKind::Extension(command.into_runtime()?)
            }
            Self::Marker { id } => RuntimeLivePresentationKind::Marker { id: id.into() },
        })
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveChoiceOption> for FfiRuntimeLiveChoiceOption {
    fn from(value: RuntimeLiveChoiceOption) -> Self {
        Self {
            id: value.id.into(),
            key: value.key.into(),
            target: value.target.into(),
            enabled_when: value.enabled_when.map(Into::into).into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveChoiceOption> for RuntimeLiveChoiceOption {
    fn from(value: FfiRuntimeLiveChoiceOption) -> Self {
        Self {
            id: value.id.into(),
            key: value.key.into(),
            target: value.target.into(),
            enabled_when: value.enabled_when.into_option().map(Into::into),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveVariableCondition> for FfiRuntimeLiveVariableCondition {
    fn from(value: RuntimeLiveVariableCondition) -> Self {
        Self {
            scope: value.scope.into(),
            key: value.key.into(),
            operation: value.operation.into(),
            value: value.value,
        }
    }
}

#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveVariableCondition> for RuntimeLiveVariableCondition {
    fn from(value: FfiRuntimeLiveVariableCondition) -> Self {
        Self {
            scope: value.scope.into(),
            key: value.key.into(),
            operation: value.operation.into(),
            value: value.value,
        }
    }
}

#[cfg(feature = "ffi")]
fn audio_bus_into_ffi(bus: super::RuntimeLiveAudioBus) -> super::FfiRuntimeAudioBus {
    match bus {
        super::RuntimeLiveAudioBus::Voice => super::FfiRuntimeAudioBus::Voice,
        super::RuntimeLiveAudioBus::Bgm => super::FfiRuntimeAudioBus::Bgm,
        super::RuntimeLiveAudioBus::Se => super::FfiRuntimeAudioBus::Se,
        super::RuntimeLiveAudioBus::Movie => super::FfiRuntimeAudioBus::Movie,
    }
}

#[cfg(feature = "ffi")]
fn audio_bus_from_ffi(bus: super::FfiRuntimeAudioBus) -> super::RuntimeLiveAudioBus {
    match bus {
        super::FfiRuntimeAudioBus::Voice => super::RuntimeLiveAudioBus::Voice,
        super::FfiRuntimeAudioBus::Bgm => super::RuntimeLiveAudioBus::Bgm,
        super::FfiRuntimeAudioBus::Se => super::RuntimeLiveAudioBus::Se,
        super::FfiRuntimeAudioBus::Movie => super::RuntimeLiveAudioBus::Movie,
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveStageCommand> for FfiRuntimeLiveStageCommand {
    fn from(value: RuntimeLiveStageCommand) -> Self {
        use RuntimeLiveStageCommand as R;
        match value {
            R::Preload { asset } => Self::Preload {
                asset: asset.into(),
            },
            R::Configure {
                width,
                height,
                safe_area_width,
                safe_area_height,
            } => Self::Configure {
                width,
                height,
                safe_area_width,
                safe_area_height,
            },
            R::DeclareLayer {
                id,
                kind,
                z,
                blend,
                clip,
                input,
            } => Self::DeclareLayer {
                id: id.into(),
                kind: kind.into(),
                z,
                blend: blend.into(),
                clip: clip.map(Into::into).into(),
                input: input.map(Into::into).into(),
            },
            R::Background {
                asset,
                layer,
                preset,
                duration_ms,
                interrupt,
            } => Self::Background {
                asset: asset.into(),
                layer: layer.into(),
                preset: preset.map(Into::into).into(),
                duration_ms,
                interrupt: interrupt.into(),
            },
            R::Show {
                id,
                asset,
                pose,
                layer,
                placement,
                fit,
                opacity_millionths,
                preset,
                interrupt,
            } => Self::Show {
                id: id.into(),
                asset: asset.into(),
                pose: pose.map(Into::into).into(),
                layer: layer.into(),
                placement: placement.into(),
                fit: fit.into(),
                opacity_millionths,
                preset: preset.map(Into::into).into(),
                interrupt: interrupt.into(),
            },
            R::Hide {
                id,
                preset,
                duration_ms,
                interrupt,
            } => Self::Hide {
                id: id.into(),
                preset: preset.map(Into::into).into(),
                duration_ms,
                interrupt: interrupt.into(),
            },
            R::ClearLayer {
                layer,
                duration_ms,
                interrupt,
            } => Self::ClearLayer {
                layer: layer.into(),
                duration_ms,
                interrupt: interrupt.into(),
            },
            R::SetLayerVisibility { layer, visible } => Self::SetLayerVisibility {
                layer: layer.into(),
                visible,
            },
            R::Backdrop { color } => Self::Backdrop { color },
            R::Shade {
                color,
                opacity_millionths,
            } => Self::Shade {
                color,
                opacity_millionths,
            },
            R::SetSkipAllowed { allowed } => Self::SetSkipAllowed { allowed },
            R::Move {
                id,
                x_millionths,
                y_millionths,
                duration_ms,
                preset,
                interrupt,
            } => Self::Move {
                id: id.into(),
                x_millionths,
                y_millionths,
                duration_ms,
                preset: preset.map(Into::into).into(),
                interrupt: interrupt.into(),
            },
            R::Camera {
                target,
                x_millionths,
                y_millionths,
                zoom_millionths,
                rotation_millionths,
                duration_ms,
                preset,
            } => Self::Camera {
                target: target.into(),
                x_millionths,
                y_millionths,
                zoom_millionths,
                rotation_millionths,
                duration_ms,
                preset: preset.map(Into::into).into(),
            },
            R::Movie {
                layer,
                asset,
                alpha_millionths,
                loop_mode,
                end,
                fence,
                fallback,
                interrupt,
            } => Self::Movie {
                layer: layer.into(),
                asset: asset.into(),
                alpha_millionths,
                loop_mode: loop_mode.into(),
                end: end.into(),
                fence: fence.map(Into::into).into(),
                fallback: fallback.map(Into::into).into(),
                interrupt: interrupt.into(),
            },
            R::Audio(command) => Self::Audio(command.into()),
            R::AudioControl(command) => Self::AudioControl(command.into()),
            R::SetAudioBusEnabled { bus, enabled } => Self::SetAudioBusEnabled {
                bus: audio_bus_into_ffi(bus),
                enabled,
            },
            R::Transition {
                preset,
                duration_ms,
                descriptor_id,
            } => Self::Transition {
                preset: preset.into(),
                duration_ms,
                descriptor_id: descriptor_id.map(Into::into).into(),
            },
            R::Shake {
                target,
                strength_millionths,
                duration_ms,
            } => Self::Shake {
                target: target.into(),
                strength_millionths,
                duration_ms,
            },
            R::Timeline(command) => Self::Timeline(command.into()),
            R::Effect {
                target,
                lip_sync,
                filter,
                fallback,
                budget_us,
            } => Self::Effect {
                target: target.into(),
                lip_sync,
                filter: filter.into(),
                fallback: fallback.into(),
                budget_us,
            },
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveStageCommand {
    fn into_runtime(self) -> Result<RuntimeLiveStageCommand, String> {
        use RuntimeLiveStageCommand as R;
        Ok(match self {
            Self::Preload { asset } => R::Preload {
                asset: asset.into(),
            },
            Self::Configure {
                width,
                height,
                safe_area_width,
                safe_area_height,
            } => R::Configure {
                width,
                height,
                safe_area_width,
                safe_area_height,
            },
            Self::DeclareLayer {
                id,
                kind,
                z,
                blend,
                clip,
                input,
            } => R::DeclareLayer {
                id: id.into(),
                kind: kind.into(),
                z,
                blend: blend.into(),
                clip: clip.into_option().map(Into::into),
                input: input.into_option().map(Into::into),
            },
            Self::Background {
                asset,
                layer,
                preset,
                duration_ms,
                interrupt,
            } => R::Background {
                asset: asset.into(),
                layer: layer.into(),
                preset: preset.into_option().map(Into::into),
                duration_ms,
                interrupt: interrupt.into(),
            },
            Self::Show {
                id,
                asset,
                pose,
                layer,
                placement,
                fit,
                opacity_millionths,
                preset,
                interrupt,
            } => R::Show {
                id: id.into(),
                asset: asset.into(),
                pose: pose.into_option().map(Into::into),
                layer: layer.into(),
                placement: placement.into(),
                fit: fit.into(),
                opacity_millionths,
                preset: preset.into_option().map(Into::into),
                interrupt: interrupt.into(),
            },
            Self::Hide {
                id,
                preset,
                duration_ms,
                interrupt,
            } => R::Hide {
                id: id.into(),
                preset: preset.into_option().map(Into::into),
                duration_ms,
                interrupt: interrupt.into(),
            },
            Self::ClearLayer {
                layer,
                duration_ms,
                interrupt,
            } => R::ClearLayer {
                layer: layer.into(),
                duration_ms,
                interrupt: interrupt.into(),
            },
            Self::SetLayerVisibility { layer, visible } => R::SetLayerVisibility {
                layer: layer.into(),
                visible,
            },
            Self::Backdrop { color } => R::Backdrop { color },
            Self::Shade {
                color,
                opacity_millionths,
            } => R::Shade {
                color,
                opacity_millionths,
            },
            Self::SetSkipAllowed { allowed } => R::SetSkipAllowed { allowed },
            Self::Move {
                id,
                x_millionths,
                y_millionths,
                duration_ms,
                preset,
                interrupt,
            } => R::Move {
                id: id.into(),
                x_millionths,
                y_millionths,
                duration_ms,
                preset: preset.into_option().map(Into::into),
                interrupt: interrupt.into(),
            },
            Self::Camera {
                target,
                x_millionths,
                y_millionths,
                zoom_millionths,
                rotation_millionths,
                duration_ms,
                preset,
            } => R::Camera {
                target: target.into(),
                x_millionths,
                y_millionths,
                zoom_millionths,
                rotation_millionths,
                duration_ms,
                preset: preset.into_option().map(Into::into),
            },
            Self::Movie {
                layer,
                asset,
                alpha_millionths,
                loop_mode,
                end,
                fence,
                fallback,
                interrupt,
            } => R::Movie {
                layer: layer.into(),
                asset: asset.into(),
                alpha_millionths,
                loop_mode: loop_mode.into(),
                end: end.into(),
                fence: fence.into_option().map(Into::into),
                fallback: fallback.into_option().map(Into::into),
                interrupt: interrupt.into(),
            },
            Self::Audio(command) => R::Audio(command.into_runtime()?),
            Self::AudioControl(command) => R::AudioControl(command.into_runtime()),
            Self::SetAudioBusEnabled { bus, enabled } => R::SetAudioBusEnabled {
                bus: audio_bus_from_ffi(bus),
                enabled,
            },
            Self::Transition {
                preset,
                duration_ms,
                descriptor_id,
            } => R::Transition {
                preset: preset.into(),
                duration_ms,
                descriptor_id: descriptor_id.into_option().map(Into::into),
            },
            Self::Shake {
                target,
                strength_millionths,
                duration_ms,
            } => R::Shake {
                target: target.into(),
                strength_millionths,
                duration_ms,
            },
            Self::Timeline(command) => R::Timeline(command.into_runtime()),
            Self::Effect {
                target,
                lip_sync,
                filter,
                fallback,
                budget_us,
            } => R::Effect {
                target: target.into(),
                lip_sync,
                filter: filter.into(),
                fallback: fallback.into(),
                budget_us,
            },
        })
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveAudioCueCommand> for FfiRuntimeLiveAudioCueCommand {
    fn from(value: RuntimeLiveAudioCueCommand) -> Self {
        let sync = match value.sync {
            super::RuntimeLiveAudioSync::None => super::FfiRuntimeAudioSync::None,
            super::RuntimeLiveAudioSync::Text => super::FfiRuntimeAudioSync::Text,
            super::RuntimeLiveAudioSync::Fence(fence_id) => super::FfiRuntimeAudioSync::Fence {
                fence_id: fence_id.into(),
            },
        };
        Self {
            id: value.id.into(),
            bus: audio_bus_into_ffi(value.bus),
            asset: value.asset.into(),
            looped: value.looped,
            fade_ms: value.fade_ms,
            sync,
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveAudioCueCommand {
    fn into_runtime(self) -> Result<RuntimeLiveAudioCueCommand, String> {
        let sync = match self.sync {
            super::FfiRuntimeAudioSync::None => super::RuntimeLiveAudioSync::None,
            super::FfiRuntimeAudioSync::Text => super::RuntimeLiveAudioSync::Text,
            super::FfiRuntimeAudioSync::Fence { fence_id } => {
                super::RuntimeLiveAudioSync::Fence(fence_id.into())
            }
        };
        Ok(RuntimeLiveAudioCueCommand {
            id: self.id.into(),
            bus: audio_bus_from_ffi(self.bus),
            asset: self.asset.into(),
            looped: self.looped,
            fade_ms: self.fade_ms,
            sync,
        })
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveAudioControl> for FfiRuntimeLiveAudioControl {
    fn from(value: RuntimeLiveAudioControl) -> Self {
        Self {
            id: value.id.into(),
            action: value.action.into(),
            target: value.target.into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveAudioControl {
    fn into_runtime(self) -> RuntimeLiveAudioControl {
        RuntimeLiveAudioControl {
            id: self.id.into(),
            action: self.action.into(),
            target: self.target.into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveAudioControlAction> for FfiRuntimeLiveAudioControlAction {
    fn from(value: RuntimeLiveAudioControlAction) -> Self {
        match value {
            RuntimeLiveAudioControlAction::Pause => Self::Pause,
            RuntimeLiveAudioControlAction::Resume => Self::Resume,
            RuntimeLiveAudioControlAction::Stop => Self::Stop,
            RuntimeLiveAudioControlAction::FadeStop { duration_ms, fence } => Self::FadeStop {
                duration_ms,
                fence: fence.into(),
            },
        }
    }
}

#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveAudioControlAction> for RuntimeLiveAudioControlAction {
    fn from(value: FfiRuntimeLiveAudioControlAction) -> Self {
        match value {
            FfiRuntimeLiveAudioControlAction::Pause => Self::Pause,
            FfiRuntimeLiveAudioControlAction::Resume => Self::Resume,
            FfiRuntimeLiveAudioControlAction::Stop => Self::Stop,
            FfiRuntimeLiveAudioControlAction::FadeStop { duration_ms, fence } => Self::FadeStop {
                duration_ms,
                fence: fence.into(),
            },
        }
    }
}

#[cfg(feature = "ffi")]
impl RuntimeLiveTimelineTask {
    pub fn into_ffi(self) -> FfiRuntimeLiveTimelineTask {
        FfiRuntimeLiveTimelineTask {
            command_id: self.command_id.into(),
            command: self.command.into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveTimelineTask {
    pub fn into_runtime(self) -> RuntimeLiveTimelineTask {
        RuntimeLiveTimelineTask {
            command_id: self.command_id.into(),
            command: self.command.into_runtime(),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveTimelineCommand> for FfiRuntimeLiveTimelineCommand {
    fn from(value: RuntimeLiveTimelineCommand) -> Self {
        match value {
            RuntimeLiveTimelineCommand::Start(spec) => Self::Start(spec.into()),
            RuntimeLiveTimelineCommand::Cancel { id, reason } => Self::Cancel {
                id: id.into(),
                reason: reason.into(),
            },
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveTimelineCommand {
    fn into_runtime(self) -> RuntimeLiveTimelineCommand {
        match self {
            Self::Start(spec) => RuntimeLiveTimelineCommand::Start(spec.into()),
            Self::Cancel { id, reason } => RuntimeLiveTimelineCommand::Cancel {
                id: id.into(),
                reason: reason.into(),
            },
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveTimelineSpec> for FfiRuntimeLiveTimelineSpec {
    fn from(value: RuntimeLiveTimelineSpec) -> Self {
        Self {
            id: value.id.into(),
            join: value.join.into(),
            tracks: value
                .tracks
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
            fence: value.fence.map(Into::into).into(),
            fallback: value.fallback.map(Into::into).into(),
            budget_us: value.budget_us,
        }
    }
}

#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveTimelineSpec> for RuntimeLiveTimelineSpec {
    fn from(value: FfiRuntimeLiveTimelineSpec) -> Self {
        Self {
            id: value.id.into(),
            join: value.join.into(),
            tracks: value.tracks.into_iter().map(Into::into).collect(),
            fence: value.fence.into_option().map(Into::into),
            fallback: value.fallback.into_option().map(Into::into),
            budget_us: value.budget_us,
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveTimelineTrack> for FfiRuntimeLiveTimelineTrack {
    fn from(value: RuntimeLiveTimelineTrack) -> Self {
        Self {
            target: value.target.into(),
            property: value.property.into(),
            keyframes: value
                .keyframes
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveTimelineTrack> for RuntimeLiveTimelineTrack {
    fn from(value: FfiRuntimeLiveTimelineTrack) -> Self {
        Self {
            target: value.target.into(),
            property: value.property.into(),
            keyframes: value.keyframes.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveTimelineKeyframe> for FfiRuntimeLiveTimelineKeyframe {
    fn from(value: RuntimeLiveTimelineKeyframe) -> Self {
        Self {
            time_ms: value.time_ms,
            value_millionths: value.value_millionths,
        }
    }
}

#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveTimelineKeyframe> for RuntimeLiveTimelineKeyframe {
    fn from(value: FfiRuntimeLiveTimelineKeyframe) -> Self {
        Self {
            time_ms: value.time_ms,
            value_millionths: value.value_millionths,
        }
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveExtensionCommand> for FfiRuntimeLiveExtensionCommand {
    fn from(value: RuntimeLiveExtensionCommand) -> Self {
        Self {
            command: value.command.into(),
            provider_id: value.provider_id.into(),
            schema: value.schema.into(),
            fields: value
                .fields
                .into_iter()
                .map(|(name, value)| FfiRuntimeLiveExtensionField {
                    name: name.into(),
                    value: value.into(),
                })
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveExtensionCommand {
    fn into_runtime(self) -> Result<RuntimeLiveExtensionCommand, String> {
        let mut fields = BTreeMap::new();
        for field in self.fields {
            let name: String = field.name.into();
            if fields.insert(name, field.value.into()).is_some() {
                return Err("ASTRA_RUNTIME_LIVE_EXTENSION_FIELD_DUPLICATE: extension field names must be unique".into());
            }
        }
        Ok(RuntimeLiveExtensionCommand {
            command: self.command.into(),
            provider_id: self.provider_id.into(),
            schema: self.schema.into(),
            fields,
        })
    }
}

#[cfg(feature = "ffi")]
impl From<RuntimeLiveExtensionValue> for FfiRuntimeLiveExtensionValue {
    fn from(value: RuntimeLiveExtensionValue) -> Self {
        match value {
            RuntimeLiveExtensionValue::String(value) => Self::String(value.into()),
            RuntimeLiveExtensionValue::Integer(value) => Self::Integer(value),
            RuntimeLiveExtensionValue::Fixed(value) => Self::Fixed(value),
            RuntimeLiveExtensionValue::Boolean(value) => Self::Boolean(value),
            RuntimeLiveExtensionValue::Symbol(value) => Self::Symbol(value.into()),
            RuntimeLiveExtensionValue::AssetUri(value) => Self::AssetUri(value.into()),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<FfiRuntimeLiveExtensionValue> for RuntimeLiveExtensionValue {
    fn from(value: FfiRuntimeLiveExtensionValue) -> Self {
        match value {
            FfiRuntimeLiveExtensionValue::String(value) => Self::String(value.into()),
            FfiRuntimeLiveExtensionValue::Integer(value) => Self::Integer(value),
            FfiRuntimeLiveExtensionValue::Fixed(value) => Self::Fixed(value),
            FfiRuntimeLiveExtensionValue::Boolean(value) => Self::Boolean(value),
            FfiRuntimeLiveExtensionValue::Symbol(value) => Self::Symbol(value.into()),
            FfiRuntimeLiveExtensionValue::AssetUri(value) => Self::AssetUri(value.into()),
        }
    }
}
