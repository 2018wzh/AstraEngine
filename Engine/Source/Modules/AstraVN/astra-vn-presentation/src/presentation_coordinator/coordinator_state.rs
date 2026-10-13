use super::*;

pub const PRESENTATION_COORDINATOR_SCHEMA: &str = "astra.vn.presentation_coordinator.v6";

pub(super) const MAX_REGION_QUEUE: usize = 4_096;

pub(super) const MAX_FRAME_DELTA_NS: u64 = 1_000_000_000;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PresentationRegion {
    Character,
    Background,
    Text,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationCommandEnvelope {
    pub fixed_step: u64,
    pub sequence: u64,
    pub command_id: String,
    pub interrupt: PresentationInterruptPolicy,
    pub fence: Option<String>,
    pub payload: PresentationRegionCommand,
}

impl PresentationCommandEnvelope {
    pub fn region(&self) -> PresentationRegion {
        match self.payload {
            PresentationRegionCommand::Character(_) => PresentationRegion::Character,
            PresentationRegionCommand::Background(_) => PresentationRegion::Background,
            PresentationRegionCommand::Text(_) => PresentationRegion::Text,
            PresentationRegionCommand::Video(_) => PresentationRegion::Video,
        }
    }

    pub(super) fn layer_write(&self) -> Option<&str> {
        match &self.payload {
            PresentationRegionCommand::Character(command) => Some(command.layer.as_str()),
            PresentationRegionCommand::Background(command) => Some(command.layer.as_str()),
            PresentationRegionCommand::Video(command) => Some(command.layer.as_str()),
            PresentationRegionCommand::Text(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresentationRegionCommand {
    Character(CharacterRegionCommand),
    Background(BackgroundRegionCommand),
    Text(TextRegionCommand),
    Video(VideoRegionCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterRegionCommand {
    pub character_id: String,
    pub asset: String,
    pub pose: Option<String>,
    pub layer: String,
    pub visible: bool,
    pub duration_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundRegionCommand {
    pub layer: String,
    pub asset: Option<String>,
    pub duration_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextRegionCommand {
    pub text_key: String,
    pub speaker: Option<String>,
    pub window: Option<String>,
    pub grapheme_count: u32,
    pub graphemes_per_second: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoRegionCommand {
    pub session_id: String,
    pub layer: String,
    pub asset: String,
    pub logical_start_ns: u64,
    pub loop_mode: MovieLoopMode,
    pub end_behavior: VnMovieEndBehavior,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterPresentationState {
    pub command_id: String,
    pub asset: String,
    pub pose: Option<String>,
    pub layer: String,
    pub visible: bool,
    pub elapsed_ns: u64,
    pub duration_ns: u64,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterRegionState {
    pub characters: BTreeMap<String, CharacterPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundPresentationState {
    pub command_id: String,
    pub current: Option<String>,
    pub incoming: Option<String>,
    pub transition_pending: bool,
    pub elapsed_ns: u64,
    pub duration_ns: u64,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundRegionState {
    pub layers: BTreeMap<String, BackgroundPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextPresentationState {
    pub command_id: String,
    pub text_key: String,
    pub speaker: Option<String>,
    pub window: Option<String>,
    pub grapheme_count: u32,
    pub visible_graphemes: u32,
    pub elapsed_ns: u64,
    pub graphemes_per_second: u16,
    pub layout_pending: bool,
    pub auto_timer_ns: Option<u64>,
    pub tasks: TextTaskProgress,
    pub fence: Option<String>,
}

impl TextPresentationState {
    pub fn reveal_complete(&self) -> bool {
        self.visible_graphemes >= self.grapheme_count
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextRegionState {
    pub active: Option<TextPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoPhase {
    Prebuffering,
    Playing,
    Paused,
    Seeking,
    Ended,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoPresentationState {
    pub command_id: String,
    pub layer: String,
    pub asset: String,
    pub phase: VideoPhase,
    pub logical_time_ns: u64,
    pub loop_mode: MovieLoopMode,
    pub end_behavior: VnMovieEndBehavior,
    pub fallback: Option<String>,
    pub fence: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoRegionState {
    pub sessions: BTreeMap<String, VideoPresentationState>,
    pub queued: VecDeque<PresentationCommandEnvelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FenceStatus {
    Pending,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationCoordinatorState {
    pub schema: String,
    pub character: CharacterRegionState,
    pub background: BackgroundRegionState,
    pub text: TextRegionState,
    pub video: VideoRegionState,
    pub fences: BTreeMap<String, FenceStatus>,
    pub fence_tasks: BTreeMap<String, FenceTaskGroup>,
    pub activated_commands: VecDeque<String>,
    pub last_fixed_step: u64,
    pub last_sequence: Option<u64>,
}

impl Default for PresentationCoordinatorState {
    fn default() -> Self {
        Self {
            schema: PRESENTATION_COORDINATOR_SCHEMA.to_string(),
            character: CharacterRegionState {
                characters: BTreeMap::new(),
                queued: VecDeque::new(),
            },
            background: BackgroundRegionState {
                layers: BTreeMap::new(),
                queued: VecDeque::new(),
            },
            text: TextRegionState {
                active: None,
                queued: VecDeque::new(),
            },
            video: VideoRegionState {
                sessions: BTreeMap::new(),
                queued: VecDeque::new(),
            },
            fences: BTreeMap::new(),
            fence_tasks: BTreeMap::new(),
            activated_commands: VecDeque::new(),
            last_fixed_step: 0,
            last_sequence: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationRegionDelta {
    pub region: PresentationRegion,
    pub first_sequence: u64,
    pub applied_commands: Vec<String>,
    pub completed_fences: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextAdvanceDisposition {
    RevealCompleted,
    StoryAdvanceRequested,
    NoActiveText,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationCoordinator {
    pub(super) state: PresentationCoordinatorState,
    pub(super) failed: bool,
}
