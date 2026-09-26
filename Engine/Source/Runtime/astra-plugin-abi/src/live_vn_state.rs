#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnState {
    pub backlog_count: usize,
    pub revision: u64,
    pub instance_id: String,
    pub profile: String,
    pub locale: String,
    pub cursor: Option<RuntimeLiveVnCursor>,
    pub system_stack: Vec<RuntimeLiveVnSystemFrame>,
    pub system: RuntimeLiveVnSystemState,
    pub pending_choice: Option<RuntimeLiveVnPendingChoice>,
    pub backlog: Vec<RuntimeLiveVnBacklogEntry>,
    pub voice_replay: Vec<RuntimeLiveVnVoiceReplayEntry>,
    pub route_coverage: Vec<String>,
    pub route_flags: Vec<RuntimeLiveVnRouteFlag>,
    pub pending_wait: Option<RuntimeLiveVnWait>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnStep {
    pub coverage_reached: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnCursor {
    pub story_id: String,
    pub state_id: String,
    pub scene_id: String,
    pub command_id: String,
    pub ordinal: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnSystemFrame {
    pub return_to: RuntimeLiveVnCursor,
    pub return_wait: Option<RuntimeLiveVnWait>,
    pub return_choice: Option<RuntimeLiveVnPendingChoice>,
    pub page: super::RuntimeLiveSystemPage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnSystemState {
    pub auto_enabled: bool,
    pub skip_mode: RuntimeLiveVnSkipMode,
    pub config: Vec<RuntimeLiveVnStringEntry>,
    pub gallery_unlocks: Vec<String>,
    pub replay_unlocks: Vec<String>,
    pub reading_mode: RuntimeLiveVnReadingMode,
    pub audio_enabled: bool,
    pub skip_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnSkipMode {
    None,
    Read,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnReadingMode {
    Hidden,
    Manual,
    FastForward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnStringEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnPendingChoice {
    pub choice_id: String,
    pub key: String,
    pub options: Vec<super::RuntimeLiveChoiceOption>,
    pub enabled_option_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnBacklogEntry {
    pub command_id: String,
    pub key: String,
    pub speaker: Option<String>,
    pub voice: Option<String>,
    pub story_id: String,
    pub state_id: String,
    pub route_position: usize,
    pub read: bool,
    pub window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnVoiceReplayEntry {
    pub id: String,
    pub voice: String,
    pub line_key: String,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnRouteFlag {
    pub id: String,
    pub kind: RuntimeLiveVnRouteFlagKind,
    pub source: String,
    pub target: String,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnRouteFlagKind {
    Launch,
    Choice,
    Jump,
    Branch,
    Call,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveVnWait {
    pub kind: RuntimeLiveVnWaitKind,
    pub fence: String,
    pub command_id: String,
    pub await_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVnWaitKind {
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
