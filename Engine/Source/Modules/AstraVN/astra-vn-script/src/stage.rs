use std::collections::BTreeMap;

use astra_core::Diagnostic;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct FixedScalar {
    pub millionths: i64,
}

impl FixedScalar {
    pub const ZERO: Self = Self { millionths: 0 };
    pub const ONE: Self = Self {
        millionths: 1_000_000,
    };

    pub fn as_f32(self) -> f32 {
        self.millionths as f32 / 1_000_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StageViewport {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageLayerKind {
    Background,
    Sprite,
    Video,
    Text,
    Cg,
    Ui,
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageBlendMode {
    Normal,
    Add,
    Multiply,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageClipPolicy {
    Stage,
    SafeArea,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StagePlacement {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MovieLoopMode {
    Once,
    Loop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnMovieEndBehavior {
    Continue,
    Wait,
    Hold,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum VnAudioBus {
    Voice,
    Bgm,
    Se,
    Movie,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnAudioSync {
    None,
    Text,
    Fence(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioCue {
    pub id: String,
    pub bus: VnAudioBus,
    pub asset: String,
    pub looped: bool,
    pub fade_ms: u32,
    pub sync: VnAudioSync,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnAudioControlAction {
    Pause,
    Resume,
    Stop,
    FadeStop { duration_ms: u32, fence: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioControl {
    pub id: String,
    pub action: VnAudioControlAction,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnTimelineJoinPolicy {
    FireAndForget,
    Block,
    ReplaceTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnTimelineKeyframe {
    pub time_ms: u32,
    pub value: FixedScalar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnTimelineTrack {
    pub target: String,
    pub property: String,
    pub keyframes: Vec<VnTimelineKeyframe>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimelineSpec {
    pub id: String,
    pub join: VnTimelineJoinPolicy,
    pub tracks: Vec<VnTimelineTrack>,
    pub fence: Option<String>,
    pub fallback: Option<String>,
    pub budget_us: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimelineCommand {
    Start(TimelineSpec),
    Cancel { id: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageCommand {
    Preload {
        asset: String,
    },
    Configure {
        viewport: StageViewport,
        safe_area: AspectRatio,
    },
    DeclareLayer {
        id: String,
        kind: StageLayerKind,
        z: i32,
        blend: StageBlendMode,
        clip: Option<StageClipPolicy>,
        input: Option<String>,
    },
    Background {
        asset: String,
        layer: String,
        preset: Option<String>,
        duration_ms: u32,
    },
    Show {
        id: String,
        asset: String,
        pose: Option<String>,
        layer: String,
        placement: StagePlacement,
        opacity: FixedScalar,
        preset: Option<String>,
    },
    Hide {
        id: String,
        preset: Option<String>,
        duration_ms: u32,
    },
    ClearLayer {
        layer: String,
        duration_ms: u32,
    },
    SetLayerVisibility {
        layer: String,
        visible: bool,
    },
    Shade {
        opacity: FixedScalar,
    },
    SetSkipAllowed {
        allowed: bool,
    },
    Move {
        id: String,
        x: FixedScalar,
        y: FixedScalar,
        duration_ms: u32,
        preset: Option<String>,
    },
    Camera {
        target: String,
        x: FixedScalar,
        y: FixedScalar,
        zoom: FixedScalar,
        rotation: FixedScalar,
        duration_ms: u32,
        preset: Option<String>,
    },
    Movie {
        layer: String,
        asset: String,
        alpha: FixedScalar,
        loop_mode: MovieLoopMode,
        end: VnMovieEndBehavior,
        fence: Option<String>,
        fallback: Option<String>,
    },
    Audio(AudioCue),
    AudioControl(AudioControl),
    Transition {
        preset: String,
        duration_ms: u32,
    },
    Shake {
        target: String,
        strength: FixedScalar,
        duration_ms: u32,
    },
    Timeline(TimelineCommand),
    Effect {
        target: String,
        lip_sync: bool,
        filter: String,
        fallback: String,
        budget_us: u32,
    },
}

impl StageCommand {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Preload { .. } => "preload",
            Self::Configure { .. } => "stage",
            Self::DeclareLayer { .. } => "layer",
            Self::Background { .. } => "background",
            Self::Show { .. } => "show",
            Self::Hide { .. } => "hide",
            Self::ClearLayer { .. } => "clear_layer",
            Self::SetLayerVisibility { .. } => "layer_visibility",
            Self::Shade { .. } => "shade",
            Self::SetSkipAllowed { .. } => "skip_allowed",
            Self::Move { .. } => "move",
            Self::Camera { .. } => "camera",
            Self::Movie { .. } => "movie",
            Self::Audio(cue) => match cue.bus {
                VnAudioBus::Voice => "voice",
                VnAudioBus::Bgm => "bgm",
                VnAudioBus::Se => "se",
                VnAudioBus::Movie => "movie_audio",
            },
            Self::AudioControl(_) => "audio",
            Self::Transition { .. } => "transition",
            Self::Shake { .. } => "shake",
            Self::Timeline(_) => "timeline",
            Self::Effect { .. } => "effect",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionFieldKind {
    String,
    Integer,
    Fixed,
    Boolean,
    Symbol,
    AssetUri,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionFieldContract {
    pub kind: ExtensionFieldKind,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionCommandDescriptor {
    pub command: String,
    pub provider_id: String,
    pub schema: String,
    pub fields: BTreeMap<String, ExtensionFieldContract>,
}

impl ExtensionCommandDescriptor {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if !is_safe_symbol(&self.command) {
            return Err(Diagnostic::blocking(
                "ASTRA_VN_EXTENSION_COMMAND_ID",
                "extension command id is not a safe symbol",
            ));
        }
        if !is_safe_symbol(&self.provider_id) {
            return Err(Diagnostic::blocking(
                "ASTRA_VN_EXTENSION_PROVIDER_ID",
                "extension provider id is not a safe symbol",
            ));
        }
        if !is_safe_symbol(&self.schema) || !self.schema.contains('.') {
            return Err(Diagnostic::blocking(
                "ASTRA_VN_EXTENSION_SCHEMA",
                "extension command schema is invalid",
            ));
        }
        if let Some(field) = self.fields.keys().find(|field| !is_safe_symbol(field)) {
            return Err(Diagnostic::blocking(
                "ASTRA_VN_EXTENSION_FIELD",
                "extension command field is not a safe symbol",
            )
            .with_field("field", field));
        }
        Ok(())
    }
}

fn is_safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionValue {
    String(String),
    Integer(i64),
    Fixed(FixedScalar),
    Boolean(bool),
    Symbol(String),
    AssetUri(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionPresentationCommand {
    pub command: String,
    pub provider_id: String,
    pub schema: String,
    pub fields: BTreeMap<String, ExtensionValue>,
}
