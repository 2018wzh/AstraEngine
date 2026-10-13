use super::*;

pub const VN_RUNTIME_STATE_SCHEMA: &str = "astra.vn.runtime_state.v4";

pub const VN_RUNTIME_STATE_SCHEMA_MAJOR: u16 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstraSource {
    pub path: String,
    pub text: String,
    pub role: AstraSourceRole,
}

impl AstraSource {
    pub fn story(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
            role: AstraSourceRole::Story,
        }
    }

    pub fn ui(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
            role: AstraSourceRole::Ui,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstraSourceRole {
    Story,
    Ui,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompiledVnProject {
    pub schema: String,
    pub project_hash: Hash256,
    pub story: CompiledStory,
    pub ui_blueprints: UiBlueprintBundle,
    pub ui_bindings: UiBindingManifest,
    pub ui_source_map: BTreeMap<String, SourceRef>,
    pub controller_ids: BTreeSet<String>,
    pub controller_sources: BTreeMap<String, String>,
    pub theme_ids: BTreeSet<String>,
    pub themes: BTreeMap<String, UiThemeManifest>,
    pub system_ui_profiles: BTreeMap<String, SystemUiProfilePolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemUiProfilePolicy {
    pub profile_id: String,
    pub save_slot_ids: Vec<String>,
    pub quick_slot_id: Option<String>,
    pub allowed_pages: BTreeSet<SystemPageKind>,
    pub reading_modes: BTreeSet<ReadingMode>,
    pub audio_toggle: bool,
    pub save_completion: SaveCompletionPolicy,
    pub custom_action_ids: BTreeSet<String>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SaveCompletionPolicy {
    Stay,
    ReturnSystem,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ReadingMode {
    Hidden,
    Manual,
    FastForward,
}

impl std::ops::Deref for CompiledVnProject {
    type Target = CompiledStory;

    fn deref(&self) -> &Self::Target {
        &self.story
    }
}

impl std::ops::DerefMut for CompiledVnProject {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.story
    }
}

impl From<CompiledVnProject> for CompiledStory {
    fn from(project: CompiledVnProject) -> Self {
        project.story
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
    pub source_map: CommandSourceMap,
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
    Branch {
        id: String,
        scope: String,
        key: String,
        op: BranchOp,
        value: i64,
        then_target: String,
        else_target: String,
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
    InputWait {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChoiceOption {
    pub id: String,
    pub key: String,
    pub target: String,
    #[serde(default)]
    pub enabled_when: Option<VariableCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VariableCondition {
    pub scope: String,
    pub key: String,
    pub op: BranchOp,
    pub value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MutationOp {
    Set,
    Add,
    Sub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BranchOp {
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SystemPageKind {
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

impl SystemPageKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "title" => Self::Title,
            "quick_panel" => Self::QuickPanel,
            "save" => Self::Save,
            "load" => Self::Load,
            "config" => Self::Config,
            "gallery" => Self::Gallery,
            "replay" => Self::Replay,
            "voice_replay" => Self::VoiceReplay,
            "route_chart" | "chart" => Self::RouteChart,
            "backlog" => Self::Backlog,
            "localization_preview" | "locale_preview" => Self::LocalizationPreview,
            "custom" => Self::Custom,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStoryManifest {
    pub schema: String,
    pub entries: BTreeMap<SystemPageKind, SystemStoryEntry>,
    pub actions: BTreeMap<String, SystemActionProgram>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemActionProgram {
    pub id: String,
    pub effects: Vec<SystemActionEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SystemActionEffect {
    Mutate {
        scope: String,
        key: String,
        op: MutationOp,
        value: i64,
    },
    Jump {
        target: String,
    },
    SwitchSystemPage {
        page: SystemPageKind,
    },
    ReturnSystem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStoryEntry {
    pub page: SystemPageKind,
    pub story_id: String,
    pub state_id: String,
    pub source_id: String,
    pub policy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStoryValidationReport {
    pub schema: String,
    pub status: SystemStoryValidationStatus,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SystemStoryValidationStatus {
    Pass,
    Blocked,
}
