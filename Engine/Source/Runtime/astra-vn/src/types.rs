use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Hash128, SourceRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::SystemStoryManifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstraSource {
    pub path: String,
    pub text: String,
}

impl AstraSource {
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompiledStory {
    pub schema: String,
    pub story_hash: Hash128,
    pub story_manifest: StoryManifest,
    pub variable_manifest: VariableManifest,
    pub command_manifest: CommandManifest,
    pub system_story_manifest: SystemStoryManifest,
    pub stories: Vec<Story>,
    pub states: BTreeMap<String, State>,
    pub route_graph: RouteGraph,
    pub source_map: BTreeMap<String, SourceRef>,
    pub debug_symbols: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StoryManifest {
    pub schema: String,
    pub stories: Vec<StoryManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StoryManifestEntry {
    pub id: String,
    pub name: String,
    pub states: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VariableManifest {
    pub schema: String,
    pub scopes: BTreeMap<String, VariableScopeManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VariableScopeManifest {
    pub keys: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommandManifest {
    pub schema: String,
    pub commands: Vec<CommandManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommandManifestEntry {
    pub id: String,
    pub kind: String,
    pub story_id: String,
    pub state_id: String,
    pub scene_id: String,
    pub source: Option<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Story {
    pub id: String,
    pub name: String,
    pub states: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct State {
    pub id: String,
    pub name: String,
    pub story_id: String,
    pub scenes: Vec<Scene>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub commands: Vec<CompiledCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum CompiledCommand {
    Dialogue {
        id: String,
        key: String,
        speaker: Option<String>,
        voice: Option<String>,
        window: Option<String>,
    },
    Choice {
        id: String,
        key: String,
        options: Vec<ChoiceOption>,
    },
    Jump {
        id: String,
        target: String,
    },
    Call {
        id: String,
        target: String,
    },
    Return {
        id: String,
    },
    Mutate {
        id: String,
        scope: String,
        key: String,
        op: MutationOp,
        value: i64,
        reason: Option<String>,
    },
    SystemPage {
        id: String,
        page: SystemPageKind,
        policy: Option<String>,
    },
    Presentation {
        id: String,
        command: PresentationCommand,
    },
    Wait {
        id: String,
        fence: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChoiceOption {
    pub id: String,
    pub key: String,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MutationOp {
    Set,
    Add,
    Sub,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SystemPageKind {
    Title,
    Save,
    Load,
    Config,
    Gallery,
    Replay,
    VoiceReplay,
    RouteChart,
    Backlog,
    LocalizationPreview,
    Unknown,
}

impl SystemPageKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "title" => Self::Title,
            "save" => Self::Save,
            "load" => Self::Load,
            "config" => Self::Config,
            "gallery" => Self::Gallery,
            "replay" => Self::Replay,
            "voice_replay" => Self::VoiceReplay,
            "route_chart" | "chart" => Self::RouteChart,
            "backlog" => Self::Backlog,
            "localization_preview" | "locale_preview" => Self::LocalizationPreview,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PresentationCommand {
    Dialogue {
        key: String,
        speaker: Option<String>,
        voice: Option<String>,
        window: Option<String>,
    },
    Choice {
        key: String,
        options: Vec<ChoiceOption>,
    },
    SystemPage {
        page: SystemPageKind,
    },
    Stage {
        command: String,
        attributes: BTreeMap<String, String>,
    },
    Marker {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteGraph {
    pub schema: String,
    pub nodes: Vec<RouteNode>,
    pub edges: Vec<RouteEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteNode {
    pub id: String,
    pub label: String,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteEdge {
    pub from: String,
    pub to: String,
    pub trigger: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnRunConfig {
    pub profile: String,
    pub locale: String,
}

impl VnRunConfig {
    pub fn classic(locale: impl Into<String>) -> Self {
        Self {
            profile: "classic".to_string(),
            locale: locale.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnRuntimeState {
    pub schema: String,
    pub profile: String,
    pub locale: String,
    pub current_story: Option<String>,
    pub current_state: Option<String>,
    pub command_cursor: usize,
    #[serde(default)]
    pub call_stack: Vec<VnCallFrame>,
    #[serde(default)]
    pub system: VnSystemState,
    #[serde(default)]
    pub pending_choice: Option<PendingChoice>,
    #[serde(default)]
    pub variables: BTreeMap<String, BTreeMap<String, i64>>,
    #[serde(default)]
    pub backlog: Vec<BacklogEntry>,
    #[serde(default)]
    pub read_state: BTreeSet<String>,
    #[serde(default)]
    pub voice_replay: BTreeMap<String, VoiceReplayEntry>,
    #[serde(default)]
    pub route_coverage: BTreeSet<String>,
    #[serde(default)]
    pub route_flags: BTreeMap<String, VnRouteFlag>,
    #[serde(default)]
    pub pending_wait: Option<VnWaitState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnRouteFlag {
    pub schema: String,
    pub kind: VnRouteFlagKind,
    pub source: String,
    pub target: String,
    pub count: u32,
}

impl VnRouteFlag {
    pub fn new(
        kind: VnRouteFlagKind,
        source: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            schema: "astra.vn.route_flag.v1".to_string(),
            kind,
            source: source.into(),
            target: target.into(),
            count: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnRouteFlagKind {
    Launch,
    Choice,
    Jump,
    Call,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnSystemState {
    #[serde(default)]
    pub auto_enabled: bool,
    #[serde(default)]
    pub skip_mode: SkipMode,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub gallery_unlocks: BTreeSet<String>,
    #[serde(default)]
    pub replay_unlocks: BTreeSet<String>,
}

impl Default for VnSystemState {
    fn default() -> Self {
        Self {
            auto_enabled: false,
            skip_mode: SkipMode::None,
            config: BTreeMap::new(),
            gallery_unlocks: BTreeSet::new(),
            replay_unlocks: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkipMode {
    #[default]
    None,
    Read,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SystemUnlockKind {
    Gallery,
    Replay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnCallFrame {
    pub story_id: String,
    pub state_id: String,
    pub command_cursor: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PendingChoice {
    pub choice_id: String,
    pub key: String,
    pub options: Vec<ChoiceOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BacklogEntry {
    pub command_id: String,
    pub key: String,
    pub speaker: Option<String>,
    pub voice: Option<String>,
    pub story_id: String,
    pub state_id: String,
    pub route_position: usize,
    pub read: bool,
    pub layout: BacklogLayoutMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BacklogLayoutMetadata {
    pub window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceReplayEntry {
    pub voice: String,
    pub line_key: String,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnReplayUiState {
    pub schema: String,
    pub backlog: Vec<BacklogEntry>,
    pub voice_replay: Vec<VoiceReplayEntry>,
    pub read_count: usize,
    pub unread_count: usize,
}

impl VnReplayUiState {
    pub fn state_hash(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(self).expect("replay UI state must serialize for hashing"),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnWaitState {
    pub schema: String,
    pub kind: VnWaitKind,
    pub fence: String,
    pub command_id: String,
}

impl VnWaitState {
    pub fn new(kind: VnWaitKind, fence: impl Into<String>, command_id: impl Into<String>) -> Self {
        Self {
            schema: "astra.vn.wait_state.v1".to_string(),
            kind,
            fence: fence.into(),
            command_id: command_id.into(),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum VnWaitKind {
    Fence,
    Timer,
    TimelineComplete,
    MovieEnd,
    VoiceEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnCoverage {
    pub reached: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnStepOutput {
    pub schema: String,
    pub presentation: Vec<PresentationCommand>,
    pub coverage: VnCoverage,
    pub state_hash_before_advance: Hash128,
    pub state_hash_after_advance: Hash128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum VnPlayerCommand {
    Launch { story_id: String, state_id: String },
    Advance,
    Choose { option_id: String },
    OpenSystem { page: SystemPageKind },
    ReplayVoice { voice: String },
    SetAuto { enabled: bool },
    SetSkip { mode: SkipMode },
    SetConfig { key: String, value: String },
    Unlock { kind: SystemUnlockKind, id: String },
    CompleteWait { fence: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnSaveBlob {
    pub schema: String,
    pub slot: String,
    pub state_hash: Hash128,
    pub state: VnRuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnProfileManifest {
    pub schema: String,
    pub target: String,
    pub profiles: Vec<String>,
}
