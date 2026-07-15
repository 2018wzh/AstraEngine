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
    pub has_thumbnail: bool,
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
    pub has_voice: bool,
    pub can_jump: bool,
    pub read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UnlockItemViewModel {
    pub item_id: String,
    pub label_key: String,
    pub thumbnail_asset: Option<String>,
    pub has_thumbnail: bool,
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
pub struct ConfigViewModel {
    pub master_volume: i64,
    pub text_speed: i64,
    pub auto_delay_ms: i64,
    pub high_contrast: bool,
    pub locale: String,
    pub available_locales: Vec<String>,
    pub player_name: TextInputViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LocalizationEntryViewModel {
    pub entry_id: String,
    pub text_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnUiPageModel {
    Title {
        can_continue: bool,
    },
    Config {
        config: ConfigViewModel,
    },
    Save {
        slots: Vec<SaveSlotViewModel>,
    },
    Load {
        slots: Vec<SaveSlotViewModel>,
    },
    Backlog {
        entries: Vec<BacklogEntryViewModel>,
    },
    Gallery {
        items: Vec<UnlockItemViewModel>,
    },
    Replay {
        items: Vec<UnlockItemViewModel>,
    },
    VoiceReplay {
        entries: Vec<BacklogEntryViewModel>,
    },
    RouteChart {
        nodes: Vec<RouteNodeViewModel>,
    },
    LocalizationPreview {
        locale: String,
        entries: Vec<LocalizationEntryViewModel>,
    },
    TextInput {
        input: TextInputViewModel,
    },
}

impl VnUiPageModel {
    pub fn to_ui_value(&self) -> Result<UiValue, UiValidationError> {
        let value = match self {
            Self::Title { can_continue } => serde_json::json!({ "can_continue": can_continue }),
            Self::Config { config } => serde_json::to_value(config).map_err(|error| {
                UiValidationError::invalid("ASTRA_VN_UI_CONFIG_ENCODE", error.to_string())
            })?,
            Self::Save { slots } | Self::Load { slots } => serde_json::json!({ "slots": slots }),
            Self::Backlog { entries } | Self::VoiceReplay { entries } => {
                serde_json::json!({ "entries": entries })
            }
            Self::Gallery { items } | Self::Replay { items } => {
                serde_json::json!({ "items": items })
            }
            Self::RouteChart { nodes } => serde_json::json!({ "nodes": nodes }),
            Self::LocalizationPreview { locale, entries } => {
                serde_json::json!({ "locale": locale, "entries": entries })
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

    pub fn build_system_page(
        &self,
        page: SystemPageKind,
    ) -> Result<VnUiPageModel, UiValidationError> {
        Ok(match page {
            SystemPageKind::Title => VnUiPageModel::Title {
                can_continue: self.runtime.cursor.is_some(),
            },
            SystemPageKind::Config => VnUiPageModel::Config {
                config: self.config_model()?,
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
                        has_thumbnail: false,
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
                        has_thumbnail: false,
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
                        has_voice: true,
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
                    entries: self
                        .localization_keys
                        .iter()
                        .enumerate()
                        .map(|(index, key)| LocalizationEntryViewModel {
                            entry_id: format!("locale.{index}"),
                            text_key: key.clone(),
                        })
                        .collect(),
                }
            }
        })
    }

    fn config_model(&self) -> Result<ConfigViewModel, UiValidationError> {
        Ok(ConfigViewModel {
            master_volume: config_integer(
                &self.runtime.system.config,
                "audio.master",
                100,
                0,
                100,
            )?,
            text_speed: config_integer(&self.runtime.system.config, "text.speed", 50, 0, 100)?,
            auto_delay_ms: config_integer(
                &self.runtime.system.config,
                "auto.delay_ms",
                1200,
                100,
                10_000,
            )?,
            high_contrast: config_bool(
                &self.runtime.system.config,
                "display.high_contrast",
                false,
            )?,
            locale: self.runtime.locale.clone(),
            available_locales: vec!["en".into(), "zh-Hans".into(), "ja".into()],
            player_name: TextInputViewModel {
                input_id: "profile.player_name".into(),
                value: self
                    .runtime
                    .system
                    .config
                    .get("profile.player_name")
                    .cloned()
                    .unwrap_or_default(),
                multiline: false,
                max_graphemes: 32,
                character_policy: "single_line".into(),
            },
        })
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
                has_voice: entry.voice.is_some(),
                can_jump: entry.read,
                read: entry.read,
            })
            .collect()
    }
}

fn config_integer(
    values: &BTreeMap<String, String>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, UiValidationError> {
    let Some(raw) = values.get(key) else {
        return Ok(default);
    };
    let value = raw.parse::<i64>().map_err(|_| {
        UiValidationError::invalid(
            "ASTRA_VN_UI_CONFIG_INTEGER",
            format!("config key {key} is not a valid integer"),
        )
    })?;
    if !(min..=max).contains(&value) {
        return Err(UiValidationError::invalid(
            "ASTRA_VN_UI_CONFIG_RANGE",
            format!("config key {key} is outside {min}..={max}"),
        ));
    }
    Ok(value)
}

fn config_bool(
    values: &BTreeMap<String, String>,
    key: &str,
    default: bool,
) -> Result<bool, UiValidationError> {
    let Some(raw) = values.get(key) else {
        return Ok(default);
    };
    raw.parse::<bool>().map_err(|_| {
        UiValidationError::invalid(
            "ASTRA_VN_UI_CONFIG_BOOL",
            format!("config key {key} is not a valid boolean"),
        )
    })
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

#[cfg(test)]
mod tests {
    use super::{config_bool, config_integer};
    use std::collections::BTreeMap;

    #[astra_headless_test::test]
    fn config_values_are_schema_checked_instead_of_silently_clamped() {
        let values = BTreeMap::from([
            ("volume".to_owned(), "101".to_owned()),
            ("contrast".to_owned(), "enabled".to_owned()),
        ]);

        let range = config_integer(&values, "volume", 50, 0, 100).unwrap_err();
        assert_eq!(range.code(), "ASTRA_VN_UI_CONFIG_RANGE");

        let boolean = config_bool(&values, "contrast", false).unwrap_err();
        assert_eq!(boolean.code(), "ASTRA_VN_UI_CONFIG_BOOL");
    }

    #[astra_headless_test::test]
    fn missing_config_values_use_declared_schema_defaults() {
        let values = BTreeMap::new();
        assert_eq!(config_integer(&values, "volume", 50, 0, 100).unwrap(), 50);
        assert!(config_bool(&values, "contrast", true).unwrap());
    }
}
