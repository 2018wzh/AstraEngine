use super::*;

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
    SwitchSystemPage { page: SystemPageKind },
    ReturnSystem,
    ReplayVoice { voice: String },
    SetAuto { enabled: bool },
    SetSkip { mode: SkipMode },
    SetReadingMode { mode: ReadingMode },
    SetAudioEnabled { enabled: bool },
    InvokeSystemAction { action_id: String },
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
    pub state: VnRuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnProfileManifest {
    pub schema: String,
    pub target: String,
    pub profiles: Vec<String>,
}
