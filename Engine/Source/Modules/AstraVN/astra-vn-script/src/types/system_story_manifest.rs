use super::*;

impl SystemStoryManifest {
    pub fn empty() -> Self {
        Self {
            schema: "astra.vn.system_story_manifest.v2".to_string(),
            entries: BTreeMap::new(),
            actions: BTreeMap::new(),
        }
    }

    pub fn from_compiled(compiled: &CompiledStory) -> Result<Self, VnError> {
        let mut entries = BTreeMap::new();
        let system_stories = compiled
            .stories
            .iter()
            .filter(|story| story.name == "system")
            .collect::<Vec<_>>();
        if system_stories.len() > 1 {
            return Err(VnError::diagnostic(
                "ASTRA_VN_SYSTEM_STORY_DUPLICATE",
                "multiple stories are named system",
            ));
        }
        let system_story_id = system_stories.first().map(|story| story.id.as_str());
        for state in compiled.states.values() {
            if system_story_id.is_some_and(|story_id| state.story_id != story_id) {
                continue;
            }
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
                let entry = SystemStoryEntry {
                    page: *page,
                    story_id: state.story_id.clone(),
                    state_id: state.id.clone(),
                    source_id: id.clone(),
                    policy: policy.clone(),
                };
                if let Some(previous) = entries.insert(*page, entry) {
                    return Err(VnError::diagnostic(
                        "ASTRA_VN_SYSTEM_PAGE_DUPLICATE",
                        format!(
                            "system page {page:?} is defined by both {} and {id}",
                            previous.source_id
                        ),
                    ));
                }
            }
        }
        Ok(Self {
            entries,
            actions: compiled.system_story_manifest.actions.clone(),
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
    pub revision: u64,
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
    pub wait_sequence: u64,
    #[serde(default)]
    pub pending_wait: Option<VnWaitState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnTextRevealState {
    pub command_id: String,
    pub text_key: String,
    pub text_graphemes: u32,
    pub visible_graphemes: u32,
    pub elapsed_ns: u64,
    pub graphemes_per_second: u16,
}

impl VnTextRevealState {
    pub fn complete(&self) -> bool {
        self.visible_graphemes >= self.text_graphemes
    }
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
    pub reading_mode: ReadingMode,
    pub audio_enabled: bool,
    pub skip_allowed: bool,
}

impl Default for VnSystemState {
    fn default() -> Self {
        Self {
            auto_enabled: false,
            skip_mode: SkipMode::None,
            config: BTreeMap::new(),
            gallery_unlocks: BTreeSet::new(),
            replay_unlocks: BTreeSet::new(),
            reading_mode: ReadingMode::Manual,
            audio_enabled: true,
            skip_allowed: true,
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
    pub enabled_option_ids: BTreeSet<String>,
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
    Input,
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
    pub state_revision_before_advance: u64,
    pub state_revision_after_advance: u64,
}
