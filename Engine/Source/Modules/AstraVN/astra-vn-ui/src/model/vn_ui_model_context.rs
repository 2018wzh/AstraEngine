use super::*;

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

    pub(super) fn system_page_underlay(
        &self,
    ) -> Result<SystemPageUnderlayViewModel, UiValidationError> {
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

    pub(super) fn config_model(&self) -> Result<ConfigViewModel, UiValidationError> {
        Ok(ConfigViewModel {
            reading_mode: self.runtime.system.reading_mode,
            audio_enabled: self.runtime.system.audio_enabled,
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

    pub(super) fn backlog(&self) -> Vec<BacklogEntryViewModel> {
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
    pub(super) fn none() -> Self {
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

pub(super) fn system_page_underlay_kind(page: SystemPageKind) -> &'static str {
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

pub(super) fn speaker_localization_key(speaker: Option<&str>) -> Option<String> {
    speaker.map(|speaker| format!("speaker.{speaker}"))
}

pub(super) fn config_integer(
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

pub(super) fn config_bool(
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
    pub(super) fn to_ui_value(&self) -> UiValue {
        ui_map([
            ("option_id", UiValue::String(self.option_id.clone())),
            ("text_key", UiValue::String(self.text_key.clone())),
            ("enabled", UiValue::Bool(self.enabled)),
        ])
    }
}

impl SaveSlotViewModel {
    pub(super) fn to_ui_value(&self) -> UiValue {
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
    pub(super) fn to_ui_value(&self) -> UiValue {
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
