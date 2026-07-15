use astra_ui_core::UiValue;
use astra_vn_script::{SkipMode, SystemPageKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnUiAction {
    Advance,
    Choose { option_id: String },
    OpenSystem { page: SystemPageKind },
    ReturnSystem,
    RequestSave { slot_id: String },
    RequestSaveConfirmed { slot_id: String },
    RequestLoad { slot_id: String },
    RequestDeleteSave { slot_id: String },
    SetConfig { key: String, value: UiValue },
    SetAuto { enabled: bool },
    SetSkip { mode: SkipMode },
    ReplayVoice { voice_id: String },
    StartReplay { replay_id: String },
    PreviewGallery { item_id: String },
    RequestRouteJump { node_id: String },
    RequestBacklogJump { command_id: String },
    SubmitText { input_id: String, value: String },
}

impl VnUiAction {
    pub fn stable_target(&self) -> &str {
        match self {
            Self::Choose { option_id } => option_id,
            Self::RequestSave { slot_id }
            | Self::RequestSaveConfirmed { slot_id }
            | Self::RequestLoad { slot_id }
            | Self::RequestDeleteSave { slot_id } => slot_id,
            Self::ReplayVoice { voice_id } => voice_id,
            Self::StartReplay { replay_id } => replay_id,
            Self::PreviewGallery { item_id } => item_id,
            Self::RequestRouteJump { node_id } => node_id,
            Self::RequestBacklogJump { command_id } => command_id,
            Self::SubmitText { input_id, .. } => input_id,
            Self::SetConfig { key, .. } => key,
            Self::Advance
            | Self::OpenSystem { .. }
            | Self::ReturnSystem
            | Self::SetAuto { .. }
            | Self::SetSkip { .. } => "vn",
        }
    }
}
