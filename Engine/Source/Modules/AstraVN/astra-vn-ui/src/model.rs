use std::collections::BTreeMap;

use astra_ui_core::{UiValidationError, UiValue, ValidateUi};
use astra_vn_script::{CompiledStory, SkipMode, SystemPageKind, VnRuntimeState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MessageViewModel {
    pub schema: String,
    pub command_id: String,
    pub text_key: String,
    pub speaker_key: Option<String>,
    pub voice_id: Option<String>,
    pub auto_enabled: bool,
    pub skip_mode: SkipMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChoiceOptionViewModel {
    pub option_id: String,
    pub text_key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChoiceViewModel {
    pub schema: String,
    pub choice_id: String,
    pub prompt_key: String,
    pub options: Vec<ChoiceOptionViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveSlotViewModel {
    pub slot_id: String,
    pub occupied: bool,
    pub thumbnail_asset: Option<String>,
    pub title_key: Option<String>,
    pub timestamp_text: Option<String>,
    pub playtime_text: Option<String>,
    pub can_write: bool,
    pub can_load: bool,
    pub migration_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BacklogEntryViewModel {
    pub command_id: String,
    pub text_key: String,
    pub speaker_key: Option<String>,
    pub voice_id: Option<String>,
    pub can_jump: bool,
    pub read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UnlockItemViewModel {
    pub item_id: String,
    pub label_key: String,
    pub thumbnail_asset: Option<String>,
    pub unlocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteNodeViewModel {
    pub node_id: String,
    pub label_key: String,
    pub terminal: bool,
    pub reached: bool,
    pub x_milli: i32,
    pub y_milli: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextInputViewModel {
    pub input_id: String,
    pub value: String,
    pub multiline: bool,
    pub max_graphemes: u32,
    pub character_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnUiPageModel {
    Title { can_continue: bool },
    Config { values: BTreeMap<String, String> },
    Save { slots: Vec<SaveSlotViewModel> },
    Load { slots: Vec<SaveSlotViewModel> },
    Backlog { entries: Vec<BacklogEntryViewModel> },
    Gallery { items: Vec<UnlockItemViewModel> },
    Replay { items: Vec<UnlockItemViewModel> },
    VoiceReplay { entries: Vec<BacklogEntryViewModel> },
    RouteChart { nodes: Vec<RouteNodeViewModel> },
    LocalizationPreview { locale: String, keys: Vec<String> },
    TextInput { input: TextInputViewModel },
}

impl VnUiPageModel {
    pub fn to_ui_value(&self) -> Result<UiValue, UiValidationError> {
        let value = match self {
            Self::Title { can_continue } => serde_json::json!({ "can_continue": can_continue }),
            Self::Config { values } => serde_json::json!({ "values": values }),
            Self::Save { slots } | Self::Load { slots } => serde_json::json!({ "slots": slots }),
            Self::Backlog { entries } | Self::VoiceReplay { entries } => {
                serde_json::json!({ "entries": entries })
            }
            Self::Gallery { items } | Self::Replay { items } => {
                serde_json::json!({ "items": items })
            }
            Self::RouteChart { nodes } => serde_json::json!({ "nodes": nodes }),
            Self::LocalizationPreview { locale, keys } => {
                serde_json::json!({ "locale": locale, "keys": keys })
            }
            Self::TextInput { input } => serde_json::json!({ "input": input }),
        };
        model_to_ui_value(&value)
    }
}

pub struct VnUiModelContext<'a> {
    pub runtime: &'a VnRuntimeState,
    pub story: &'a CompiledStory,
    pub save_slots: &'a [SaveSlotViewModel],
    pub localization_keys: &'a [String],
}

impl VnUiModelContext<'_> {
    pub fn build_message(&self) -> Result<MessageViewModel, UiValidationError> {
        let entry = self.runtime.backlog.last().ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_VN_UI_MESSAGE_MISSING",
                "message surface requires a current backlog entry",
            )
        })?;
        Ok(MessageViewModel {
            schema: "astra.vn.ui_model.message.v1".to_string(),
            command_id: entry.command_id.clone(),
            text_key: entry.key.clone(),
            speaker_key: entry
                .speaker
                .as_ref()
                .map(|speaker| format!("speaker.{speaker}")),
            voice_id: entry.voice.clone(),
            auto_enabled: self.runtime.system.auto_enabled,
            skip_mode: self.runtime.system.skip_mode,
        })
    }

    pub fn build_choice(&self) -> Result<ChoiceViewModel, UiValidationError> {
        let choice = self.runtime.pending_choice.as_ref().ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_VN_UI_CHOICE_MISSING",
                "choice surface requires a pending choice",
            )
        })?;
        Ok(ChoiceViewModel {
            schema: "astra.vn.ui_model.choice.v1".to_string(),
            choice_id: choice.choice_id.clone(),
            prompt_key: choice.key.clone(),
            options: choice
                .options
                .iter()
                .map(|option| ChoiceOptionViewModel {
                    option_id: option.id.clone(),
                    text_key: option.key.clone(),
                    enabled: true,
                })
                .collect(),
        })
    }

    pub fn build_system_page(&self, page: SystemPageKind) -> VnUiPageModel {
        match page {
            SystemPageKind::Title => VnUiPageModel::Title {
                can_continue: self.runtime.cursor.is_some(),
            },
            SystemPageKind::Config => VnUiPageModel::Config {
                values: self.runtime.system.config.clone(),
            },
            SystemPageKind::Save => VnUiPageModel::Save {
                slots: self.save_slots.to_vec(),
            },
            SystemPageKind::Load => VnUiPageModel::Load {
                slots: self.save_slots.to_vec(),
            },
            SystemPageKind::Backlog => VnUiPageModel::Backlog {
                entries: self.backlog(),
            },
            SystemPageKind::Gallery => VnUiPageModel::Gallery {
                items: self
                    .runtime
                    .system
                    .gallery_unlocks
                    .iter()
                    .map(|id| UnlockItemViewModel {
                        item_id: id.clone(),
                        label_key: id.clone(),
                        thumbnail_asset: None,
                        unlocked: true,
                    })
                    .collect(),
            },
            SystemPageKind::Replay => VnUiPageModel::Replay {
                items: self
                    .runtime
                    .system
                    .replay_unlocks
                    .iter()
                    .map(|id| UnlockItemViewModel {
                        item_id: id.clone(),
                        label_key: id.clone(),
                        thumbnail_asset: None,
                        unlocked: true,
                    })
                    .collect(),
            },
            SystemPageKind::VoiceReplay => VnUiPageModel::VoiceReplay {
                entries: self
                    .runtime
                    .voice_replay
                    .values()
                    .map(|entry| BacklogEntryViewModel {
                        command_id: entry.voice.clone(),
                        text_key: entry.line_key.clone(),
                        speaker_key: entry.speaker.clone(),
                        voice_id: Some(entry.voice.clone()),
                        can_jump: false,
                        read: true,
                    })
                    .collect(),
            },
            SystemPageKind::RouteChart => VnUiPageModel::RouteChart {
                nodes: self
                    .story
                    .route_graph
                    .nodes
                    .iter()
                    .enumerate()
                    .map(|(index, node)| RouteNodeViewModel {
                        node_id: node.id.clone(),
                        label_key: node.id.clone(),
                        terminal: node.terminal,
                        reached: self.runtime.route_coverage.contains(&node.id),
                        x_milli: (index % 8) as i32 * 1000,
                        y_milli: (index / 8) as i32 * 1000,
                    })
                    .collect(),
            },
            SystemPageKind::LocalizationPreview | SystemPageKind::Unknown => {
                VnUiPageModel::LocalizationPreview {
                    locale: self.runtime.locale.clone(),
                    keys: self.localization_keys.to_vec(),
                }
            }
        }
    }

    fn backlog(&self) -> Vec<BacklogEntryViewModel> {
        self.runtime
            .backlog
            .iter()
            .map(|entry| BacklogEntryViewModel {
                command_id: entry.command_id.clone(),
                text_key: entry.key.clone(),
                speaker_key: entry.speaker.clone(),
                voice_id: entry.voice.clone(),
                can_jump: entry.read,
                read: entry.read,
            })
            .collect()
    }
}

pub fn model_to_ui_value<T: Serialize>(model: &T) -> Result<UiValue, UiValidationError> {
    let json = serde_json::to_value(model).map_err(|error| {
        UiValidationError::invalid("ASTRA_VN_UI_MODEL_ENCODE", error.to_string())
    })?;
    let value = json_to_ui_value(json)?;
    value.validate()?;
    Ok(value)
}

fn json_to_ui_value(value: serde_json::Value) -> Result<UiValue, UiValidationError> {
    match value {
        serde_json::Value::Null => Ok(UiValue::Null),
        serde_json::Value::Bool(value) => Ok(UiValue::Bool(value)),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(UiValue::Integer)
            .or_else(|| value.as_f64().map(UiValue::Number))
            .ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_VN_UI_MODEL_NUMBER",
                    "model number cannot be represented by the UI value contract",
                )
            }),
        serde_json::Value::String(value) => Ok(UiValue::String(value)),
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(json_to_ui_value)
            .collect::<Result<Vec<_>, _>>()
            .map(UiValue::List),
        serde_json::Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| Ok((key, json_to_ui_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(UiValue::Map),
    }
}
