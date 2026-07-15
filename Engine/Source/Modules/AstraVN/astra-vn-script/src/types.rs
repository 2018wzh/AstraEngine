use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Diagnostic, Hash128, Hash256, SourceRef};
use astra_ui_core::{UiBindingManifest, UiBlueprintBundle, UiThemeManifest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CommandSourceMap;
use crate::VnError;

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
    pub component_ids: BTreeSet<String>,
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
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStoryManifest {
    pub schema: String,
    pub entries: BTreeMap<SystemPageKind, SystemStoryEntry>,
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

impl SystemStoryManifest {
    pub fn empty() -> Self {
        Self {
            schema: "astra.vn.system_story_manifest.v1".to_string(),
            entries: BTreeMap::new(),
        }
    }

    pub fn from_compiled(compiled: &CompiledStory) -> Result<Self, VnError> {
        let mut entries = BTreeMap::new();
        for state in compiled.states.values() {
            for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
                let CompiledCommand::SystemPage { id, page, policy } = command else {
                    continue;
                };
                if *page == SystemPageKind::Unknown {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_SYSTEM_PAGE_UNKNOWN",
                        format!("system page {id} has unknown kind"),
                    ));
                }
                entries.entry(*page).or_insert_with(|| SystemStoryEntry {
                    page: *page,
                    story_id: state.story_id.clone(),
                    state_id: state.id.clone(),
                    source_id: id.clone(),
                    policy: policy.clone(),
                });
            }
        }
        Ok(Self {
            entries,
            ..Self::empty()
        })
    }

    pub fn commercial_required_pages() -> Vec<SystemPageKind> {
        vec![
            SystemPageKind::Title,
            SystemPageKind::Save,
            SystemPageKind::Load,
            SystemPageKind::Config,
            SystemPageKind::Gallery,
            SystemPageKind::Replay,
            SystemPageKind::VoiceReplay,
            SystemPageKind::RouteChart,
            SystemPageKind::Backlog,
            SystemPageKind::LocalizationPreview,
        ]
    }

    pub fn validate_required(&self, required: &[SystemPageKind]) -> SystemStoryValidationReport {
        let mut diagnostics = Vec::new();
        for page in required {
            let Some(entry) = self.entries.get(page) else {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_ENTRY_MISSING",
                        format!("required system page {page:?} is missing"),
                    )
                    .with_field("page", format!("{page:?}")),
                );
                continue;
            };
            if entry
                .policy
                .as_deref()
                .is_none_or(|policy| policy.trim().is_empty())
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VN_SYSTEM_POLICY_MISSING",
                        format!("required system page {page:?} is missing policy binding"),
                    )
                    .with_field("page", format!("{page:?}"))
                    .with_field("state_id", &entry.state_id),
                );
            }
        }
        SystemStoryValidationReport {
            schema: "astra.vn.system_story_validation_report.v1".to_string(),
            status: if diagnostics.is_empty() {
                SystemStoryValidationStatus::Pass
            } else {
                SystemStoryValidationStatus::Blocked
            },
            diagnostics,
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
    SystemOption {
        option: ChoiceOption,
    },
    Stage(crate::StageCommand),
    Extension(crate::ExtensionPresentationCommand),
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
    pub instance_id: String,
    pub profile: String,
    pub locale: String,
    pub cursor: Option<VnCommandCursor>,
    #[serde(default)]
    pub call_stack: Vec<VnCallFrame>,
    #[serde(default)]
    pub system_stack: Vec<VnSystemFrame>,
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
    Branch,
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
    pub return_to: VnCommandCursor,
    pub source_command_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnSystemFrame {
    pub return_to: VnCommandCursor,
    pub return_wait: Option<VnWaitState>,
    pub return_choice: Option<PendingChoice>,
    pub page: SystemPageKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnCommandCursor {
    pub story_id: String,
    pub state_id: String,
    pub scene_id: String,
    pub command_id: String,
    pub ordinal: usize,
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
    #[serde(default)]
    pub await_id: Option<String>,
}

impl VnWaitState {
    pub fn new(kind: VnWaitKind, fence: impl Into<String>, command_id: impl Into<String>) -> Self {
        Self {
            schema: "astra.vn.wait_state.v1".to_string(),
            kind,
            fence: fence.into(),
            command_id: command_id.into(),
            await_id: None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum VnWaitKind {
    Dialogue,
    Choice,
    SystemPage,
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
    pub next_cursor: Option<VnCommandCursor>,
    pub wait: Option<VnWaitState>,
    pub awaits: Vec<String>,
    pub events: Vec<VnEvent>,
    pub presentation: Vec<PresentationCommand>,
    pub audio: Vec<VnAudioCommand>,
    pub timeline_tasks: Vec<VnTimelineTask>,
    pub mutations: Vec<VnMutationRecord>,
    pub coverage: VnCoverage,
    pub state_hash_before_advance: Hash128,
    pub state_hash_after_advance: Hash128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnEvent {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnAudioCommand {
    pub command_id: String,
    pub cue: crate::AudioCue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnTimelineTask {
    pub command_id: String,
    pub command: crate::TimelineCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnMutationRecord {
    pub scope: String,
    pub key: String,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum VnPlayerCommand {
    Launch { story_id: String, state_id: String },
    Advance,
    Choose { option_id: String },
    OpenSystem { page: SystemPageKind },
    ReturnSystem,
    ReplayVoice { voice: String },
    SetAuto { enabled: bool },
    SetSkip { mode: SkipMode },
    SetConfig { key: String, value: String },
    StartReplay { replay_id: String },
    PreviewGallery { item_id: String },
    JumpRoute { node_id: String },
    JumpBacklog { command_id: String },
    SubmitText { input_id: String, value: String },
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
