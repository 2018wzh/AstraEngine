use std::collections::BTreeMap;

use astra_ui_core::{UiValidationError, UiValue, ValidateUi};
use astra_vn_script::{
    CompiledStory, SkipMode, SystemPageKind, VnRuntimeState, VnTextRevealState, VnWaitKind,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MessageViewModel {
    pub schema: String,
    pub command_id: String,
    pub text_key: String,
    pub speaker_key: Option<String>,
    pub voice_id: Option<String>,
    pub window: Option<String>,
    pub auto_enabled: bool,
    pub skip_mode: SkipMode,
    pub visible_graphemes: u32,
    pub text_graphemes: u32,
    pub reveal_complete: bool,
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
    pub metadata_text: Option<String>,
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
pub struct SystemPageUnderlayViewModel {
    pub kind: String,
    pub title: bool,
    pub message: bool,
    pub choice: bool,
    pub text_key: Option<String>,
    pub speaker_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemPageViewModel {
    pub page: VnUiPageModel,
    pub underlay: SystemPageUnderlayViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VnUiPageModel {
    Title {
        can_continue: bool,
    },
    QuickPanel {
        auto_enabled: bool,
        skip_mode: SkipMode,
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
            Self::Title { can_continue } => {
                ui_map([("can_continue", UiValue::Bool(*can_continue))])
            }
            Self::QuickPanel {
                auto_enabled,
                skip_mode,
            } => ui_map([
                ("auto_enabled", UiValue::Bool(*auto_enabled)),
                (
                    "skip_mode",
                    UiValue::String(skip_mode_name(*skip_mode).into()),
                ),
            ]),
            Self::Config { config } => ui_map([("config", config.to_ui_value())]),
            Self::Save { slots } | Self::Load { slots } => ui_map([(
                "slots",
                UiValue::List(slots.iter().map(SaveSlotViewModel::to_ui_value).collect()),
            )]),
            Self::Backlog { entries } | Self::VoiceReplay { entries } => ui_map([(
                "entries",
                UiValue::List(
                    entries
                        .iter()
                        .map(BacklogEntryViewModel::to_ui_value)
                        .collect(),
                ),
            )]),
            Self::Gallery { items } | Self::Replay { items } => ui_map([(
                "items",
                UiValue::List(items.iter().map(UnlockItemViewModel::to_ui_value).collect()),
            )]),
            Self::RouteChart { nodes } => ui_map([(
                "nodes",
                UiValue::List(nodes.iter().map(RouteNodeViewModel::to_ui_value).collect()),
            )]),
            Self::LocalizationPreview { locale, entries } => ui_map([
                ("locale", UiValue::String(locale.clone())),
                (
                    "entries",
                    UiValue::List(
                        entries
                            .iter()
                            .map(LocalizationEntryViewModel::to_ui_value)
                            .collect(),
                    ),
                ),
            ]),
            Self::TextInput { input } => ui_map([("input", input.to_ui_value())]),
        };
        value.validate()?;
        Ok(value)
    }
}

impl SystemPageViewModel {
    pub fn to_ui_value(&self) -> Result<UiValue, UiValidationError> {
        let mut page = self.page.to_ui_value()?;
        let UiValue::Map(values) = &mut page else {
            return Err(UiValidationError::invalid(
                "ASTRA_VN_UI_SYSTEM_PAGE_MODEL",
                "system page model must encode as a map",
            ));
        };
        values.insert(
            "underlay_kind".into(),
            UiValue::String(self.underlay.kind.clone()),
        );
        values.insert("underlay_title".into(), UiValue::Bool(self.underlay.title));
        values.insert(
            "underlay_message".into(),
            UiValue::Bool(self.underlay.message),
        );
        values.insert(
            "underlay_choice".into(),
            UiValue::Bool(self.underlay.choice),
        );
        values.insert(
            "underlay_text_key".into(),
            self.underlay
                .text_key
                .clone()
                .map(UiValue::String)
                .unwrap_or(UiValue::Null),
        );
        values.insert(
            "underlay_speaker_key".into(),
            self.underlay
                .speaker_key
                .clone()
                .map(UiValue::String)
                .unwrap_or(UiValue::Null),
        );
        Ok(page)
    }
}

pub struct VnUiModelContext<'a> {
    pub runtime: &'a VnRuntimeState,
    pub text_reveal: Option<&'a VnTextRevealState>,
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
            schema: "astra.vn.ui_model.message.v2".to_string(),
            command_id: entry.command_id.clone(),
            text_key: entry.key.clone(),
            speaker_key: speaker_localization_key(entry.speaker.as_deref()),
            voice_id: entry.voice.clone(),
            window: entry.layout.window.clone(),
            auto_enabled: self.runtime.system.auto_enabled,
            skip_mode: self.runtime.system.skip_mode,
            visible_graphemes: self
                .text_reveal
                .as_ref()
                .map_or(u32::MAX, |reveal| reveal.visible_graphemes),
            text_graphemes: self
                .text_reveal
                .as_ref()
                .map_or(0, |reveal| reveal.text_graphemes),
            reveal_complete: self
                .text_reveal
                .as_ref()
                .is_none_or(|reveal| reveal.complete()),
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
                    enabled: choice.enabled_option_ids.contains(&option.id),
                })
                .collect(),
        })
    }

    pub fn build_system_page(
        &self,
        page: SystemPageKind,
    ) -> Result<SystemPageViewModel, UiValidationError> {
        let page = match page {
            SystemPageKind::Title => VnUiPageModel::Title {
                can_continue: self.runtime.cursor.is_some(),
            },
            SystemPageKind::QuickPanel => VnUiPageModel::QuickPanel {
                auto_enabled: self.runtime.system.auto_enabled,
                skip_mode: self.runtime.system.skip_mode,
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
                        speaker_key: speaker_localization_key(entry.speaker.as_deref()),
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
            SystemPageKind::Custom => VnUiPageModel::Title { can_continue: true },
        };
        Ok(SystemPageViewModel {
            page,
            underlay: self.system_page_underlay()?,
        })
    }

    fn system_page_underlay(&self) -> Result<SystemPageUnderlayViewModel, UiValidationError> {
        let Some(frame) = self.runtime.system_stack.last() else {
            return Ok(SystemPageUnderlayViewModel::none());
        };
        if frame.return_choice.is_some() {
            return Ok(SystemPageUnderlayViewModel {
                kind: "choice".into(),
                title: false,
                message: false,
                choice: true,
                text_key: None,
                speaker_key: None,
            });
        }
        let Some(wait) = frame.return_wait.as_ref() else {
            return Ok(SystemPageUnderlayViewModel::none());
        };
        if wait.kind == VnWaitKind::SystemPage {
            let pages = self
                .story
                .system_story_manifest
                .entries
                .values()
                .filter(|entry| {
                    entry.story_id == frame.return_to.story_id
                        && entry.state_id == frame.return_to.state_id
                })
                .map(|entry| entry.page)
                .collect::<Vec<_>>();
            if pages.len() != 1 {
                return Err(UiValidationError::invalid(
                    "ASTRA_VN_UI_SYSTEM_UNDERLAY_PAGE",
                    "system page return cursor must resolve to exactly one authored page",
                ));
            }
            return Ok(SystemPageUnderlayViewModel {
                kind: system_page_underlay_kind(pages[0]).into(),
                title: pages[0] == SystemPageKind::Title,
                message: false,
                choice: false,
                text_key: None,
                speaker_key: None,
            });
        }
        if matches!(wait.kind, VnWaitKind::Dialogue | VnWaitKind::Input) {
            let entry = self.runtime.backlog.last().ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_VN_UI_SYSTEM_UNDERLAY_MESSAGE",
                    "reading-surface system overlay requires a backlog entry",
                )
            })?;
            return Ok(SystemPageUnderlayViewModel {
                kind: "message".into(),
                title: false,
                message: true,
                choice: false,
                text_key: Some(entry.key.clone()),
                speaker_key: speaker_localization_key(entry.speaker.as_deref()),
            });
        }
        Ok(SystemPageUnderlayViewModel::none())
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
                speaker_key: speaker_localization_key(entry.speaker.as_deref()),
                voice_id: entry.voice.clone(),
                has_voice: entry.voice.is_some(),
                can_jump: entry.read,
                read: entry.read,
            })
            .collect()
    }
}

impl SystemPageUnderlayViewModel {
    fn none() -> Self {
        Self {
            kind: "none".into(),
            title: false,
            message: false,
            choice: false,
            text_key: None,
            speaker_key: None,
        }
    }
}

fn system_page_underlay_kind(page: SystemPageKind) -> &'static str {
    match page {
        SystemPageKind::Title => "title",
        SystemPageKind::QuickPanel => "quick_panel",
        SystemPageKind::Save => "save",
        SystemPageKind::Load => "load",
        SystemPageKind::Config => "config",
        SystemPageKind::Gallery => "gallery",
        SystemPageKind::Replay => "replay",
        SystemPageKind::VoiceReplay => "voice_replay",
        SystemPageKind::RouteChart => "route_chart",
        SystemPageKind::Backlog => "backlog",
        SystemPageKind::LocalizationPreview => "localization_preview",
        SystemPageKind::Custom => "custom",
        SystemPageKind::Unknown => "unknown",
    }
}

fn speaker_localization_key(speaker: Option<&str>) -> Option<String> {
    speaker.map(|speaker| format!("speaker.{speaker}"))
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

impl MessageViewModel {
    pub fn to_ui_value(&self) -> Result<UiValue, UiValidationError> {
        let value = ui_map([
            ("schema", UiValue::String(self.schema.clone())),
            ("command_id", UiValue::String(self.command_id.clone())),
            ("text_key", UiValue::String(self.text_key.clone())),
            ("speaker_key", optional_string(&self.speaker_key)),
            ("voice_id", optional_string(&self.voice_id)),
            ("window", optional_string(&self.window)),
            ("auto_enabled", UiValue::Bool(self.auto_enabled)),
            (
                "skip_mode",
                UiValue::String(skip_mode_name(self.skip_mode).into()),
            ),
            (
                "visible_graphemes",
                UiValue::Integer(i64::from(self.visible_graphemes)),
            ),
            (
                "text_graphemes",
                UiValue::Integer(i64::from(self.text_graphemes)),
            ),
            ("reveal_complete", UiValue::Bool(self.reveal_complete)),
        ]);
        value.validate()?;
        Ok(value)
    }
}

impl ChoiceViewModel {
    pub fn to_ui_value(&self) -> Result<UiValue, UiValidationError> {
        let value = ui_map([
            ("schema", UiValue::String(self.schema.clone())),
            ("choice_id", UiValue::String(self.choice_id.clone())),
            ("prompt_key", UiValue::String(self.prompt_key.clone())),
            (
                "options",
                UiValue::List(
                    self.options
                        .iter()
                        .map(ChoiceOptionViewModel::to_ui_value)
                        .collect(),
                ),
            ),
        ]);
        value.validate()?;
        Ok(value)
    }
}

impl ChoiceOptionViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("option_id", UiValue::String(self.option_id.clone())),
            ("text_key", UiValue::String(self.text_key.clone())),
            ("enabled", UiValue::Bool(self.enabled)),
        ])
    }
}

impl SaveSlotViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("slot_id", UiValue::String(self.slot_id.clone())),
            ("occupied", UiValue::Bool(self.occupied)),
            ("thumbnail_asset", optional_string(&self.thumbnail_asset)),
            ("has_thumbnail", UiValue::Bool(self.has_thumbnail)),
            ("title_key", optional_string(&self.title_key)),
            ("timestamp_text", optional_string(&self.timestamp_text)),
            ("playtime_text", optional_string(&self.playtime_text)),
            ("metadata_text", optional_string(&self.metadata_text)),
            ("can_write", UiValue::Bool(self.can_write)),
            ("can_load", UiValue::Bool(self.can_load)),
            (
                "migration_status",
                UiValue::String(self.migration_status.clone()),
            ),
        ])
    }
}

impl BacklogEntryViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("command_id", UiValue::String(self.command_id.clone())),
            ("text_key", UiValue::String(self.text_key.clone())),
            ("speaker_key", optional_string(&self.speaker_key)),
            ("voice_id", optional_string(&self.voice_id)),
            ("has_voice", UiValue::Bool(self.has_voice)),
            ("can_jump", UiValue::Bool(self.can_jump)),
            ("read", UiValue::Bool(self.read)),
        ])
    }
}

impl UnlockItemViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("item_id", UiValue::String(self.item_id.clone())),
            ("label_key", UiValue::String(self.label_key.clone())),
            ("thumbnail_asset", optional_string(&self.thumbnail_asset)),
            ("has_thumbnail", UiValue::Bool(self.has_thumbnail)),
            ("unlocked", UiValue::Bool(self.unlocked)),
        ])
    }
}

impl RouteNodeViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("node_id", UiValue::String(self.node_id.clone())),
            ("label_key", UiValue::String(self.label_key.clone())),
            ("terminal", UiValue::Bool(self.terminal)),
            ("reached", UiValue::Bool(self.reached)),
            ("x_milli", UiValue::Integer(i64::from(self.x_milli))),
            ("y_milli", UiValue::Integer(i64::from(self.y_milli))),
        ])
    }
}

impl TextInputViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("input_id", UiValue::String(self.input_id.clone())),
            ("value", UiValue::String(self.value.clone())),
            ("multiline", UiValue::Bool(self.multiline)),
            (
                "max_graphemes",
                UiValue::Integer(i64::from(self.max_graphemes)),
            ),
            (
                "character_policy",
                UiValue::String(self.character_policy.clone()),
            ),
        ])
    }
}

impl ConfigViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("master_volume", UiValue::Integer(self.master_volume)),
            ("text_speed", UiValue::Integer(self.text_speed)),
            ("auto_delay_ms", UiValue::Integer(self.auto_delay_ms)),
            ("high_contrast", UiValue::Bool(self.high_contrast)),
            ("locale", UiValue::String(self.locale.clone())),
            (
                "available_locales",
                UiValue::List(
                    self.available_locales
                        .iter()
                        .cloned()
                        .map(UiValue::String)
                        .collect(),
                ),
            ),
            ("player_name", self.player_name.to_ui_value()),
        ])
    }
}

impl LocalizationEntryViewModel {
    fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("entry_id", UiValue::String(self.entry_id.clone())),
            ("text_key", UiValue::String(self.text_key.clone())),
        ])
    }
}

fn ui_map<const N: usize>(entries: [(&str, UiValue); N]) -> UiValue {
    UiValue::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn optional_string(value: &Option<String>) -> UiValue {
    value
        .as_ref()
        .cloned()
        .map(UiValue::String)
        .unwrap_or(UiValue::Null)
}

fn skip_mode_name(value: SkipMode) -> &'static str {
    match value {
        SkipMode::None => "none",
        SkipMode::Read => "read",
        SkipMode::All => "all",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        config_bool, config_integer, speaker_localization_key, SystemPageUnderlayViewModel,
        SystemPageViewModel, VnUiPageModel,
    };
    use astra_ui_core::UiValue;
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

    #[astra_headless_test::test]
    fn speaker_ids_are_resolved_through_the_localization_namespace() {
        assert_eq!(
            speaker_localization_key(Some("tsui.speaker.fixture")),
            Some("speaker.tsui.speaker.fixture".to_string())
        );
        assert_eq!(speaker_localization_key(None), None);
    }

    #[astra_headless_test::test]
    fn system_page_model_exposes_a_typed_reconstructible_underlay() {
        let value = SystemPageViewModel {
            page: VnUiPageModel::Title { can_continue: true },
            underlay: SystemPageUnderlayViewModel {
                kind: "message".into(),
                title: false,
                message: true,
                choice: false,
                text_key: Some("line.fixture".into()),
                speaker_key: Some("speaker.fixture".into()),
            },
        }
        .to_ui_value()
        .expect("system page model");
        let UiValue::Map(values) = value else {
            panic!("system page model must be a map");
        };
        assert_eq!(values["underlay_kind"], UiValue::String("message".into()));
        assert_eq!(values["underlay_message"], UiValue::Bool(true));
        assert_eq!(
            values["underlay_text_key"],
            UiValue::String("line.fixture".into())
        );
    }
}
