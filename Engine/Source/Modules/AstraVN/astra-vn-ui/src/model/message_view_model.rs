use super::*;

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
    pub reading_mode: ReadingMode,
    pub audio_enabled: bool,
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
            Self::Config { config } => config.to_ui_value(),
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
