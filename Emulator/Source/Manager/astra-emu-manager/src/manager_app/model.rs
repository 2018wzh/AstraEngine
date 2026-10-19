use super::*;
use astra_emu_manager_ui_slint::{
    describe_error, AboutViewModel, BangumiViewModel, CompatViewModel, EngineActionViewModel,
    GameCardViewModel, LibraryViewModel, MatchViewModel, NoticeViewModel, PlaySessionViewModel,
    ReleaseViewModel, SelectionViewModel, SessionViewModel, SettingsViewModel,
    TranslationViewModel,
};
impl AstraEmuManagerController {
    pub(super) fn model(&self) -> Result<ManagerViewModel, String> {
        let now_ms = unix_time_ms()?;
        let query = self.search_query.trim().to_ascii_lowercase();
        let astra_emu_manager_core::LibrarySnapshot {
            games,
            play_stats,
            identities,
            metadata: metadata_snapshots,
        } = self
            .library
            .library_snapshot()
            .map_err(|error| error.to_string())?;
        let mut selected = None;
        let mut metadata = None;
        let startup_busy = self.startup_busy();
        let mut rows = Vec::new();
        for mut game in games {
            let linked = identities
                .get(&game.game_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let record = Self::metadata_from_snapshot(&game.game_id, linked, &metadata_snapshots)?;
            let has_metadata = record.is_some();
            if self.selected_case_id.as_deref() == Some(game.game_id.as_str()) {
                if let Some(record) = record {
                    game.title.clone_from(&record.title);
                    metadata = Some(record);
                }
                selected = Some(game.clone());
            } else if let Some(record) = record {
                game.title = record.title;
            }
            if !query.is_empty()
                && !game.display_title().to_ascii_lowercase().contains(&query)
                && !game.game_id.to_ascii_lowercase().contains(&query)
            {
                continue;
            }
            let stats = play_stats
                .get(&game.game_id)
                .ok_or("ASTRA_EMU_LIBRARY_SNAPSHOT_STATS")?;
            let cover_uri = if has_metadata {
                self.data_dir
                    .join("covers")
                    .join(&game.game_id)
                    .to_str()
                    .ok_or("ASTRA_EMU_METADATA_COVER_PATH_UTF8")?
                    .to_owned()
            } else {
                String::new()
            };
            let compatibility_status = self
                .compatibility_for_identities(&game.game_id, linked)
                .map(|entry| entry.status.as_str().to_owned())
                .unwrap_or_default();
            if self.compatibility_filter != "all"
                && (self.compatibility_filter == "unknown" && !compatibility_status.is_empty()
                    || self.compatibility_filter != "unknown"
                        && self.compatibility_filter != compatibility_status)
            {
                continue;
            }
            rows.push((
                game,
                stats.total_duration_ms,
                stats.last_played_unix_ms,
                cover_uri,
                compatibility_status,
            ));
        }
        match self.library_sort.as_str() {
            "recent" => rows.sort_by(|left, right| {
                right
                    .2
                    .unwrap_or(i64::MIN)
                    .cmp(&left.2.unwrap_or(i64::MIN))
                    .then_with(|| {
                        left.0
                            .display_title()
                            .to_ascii_lowercase()
                            .cmp(&right.0.display_title().to_ascii_lowercase())
                    })
            }),
            "play_time" => rows.sort_by(|left, right| {
                right
                    .1
                    .cmp(&left.1)
                    .then_with(|| left.0.game_id.cmp(&right.0.game_id))
            }),
            _ => rows.sort_by(|left, right| {
                left.0
                    .display_title()
                    .to_ascii_lowercase()
                    .cmp(&right.0.display_title().to_ascii_lowercase())
                    .then_with(|| left.0.game_id.cmp(&right.0.game_id))
            }),
        }
        if self.selected_case_id.is_some() && selected.is_none() {
            return Err("ASTRA_EMU_GAME_NOT_FOUND".into());
        }
        let cards = rows
            .into_iter()
            .map(
                |(mut game, duration_ms, last_played_ms, cover_uri, compatibility_status)| {
                    let family = game
                        .family_id
                        .as_deref()
                        .map(Self::engine_label)
                        .unwrap_or("未识别引擎")
                        .to_owned();
                    let problem = self.game_problem(&game, startup_busy);
                    let title = game.user_title.take().unwrap_or(game.title);
                    GameCardViewModel {
                        id: game.game_id,
                        title,
                        family,
                        cover_uri,
                        problem,
                        play_time: human_duration(duration_ms),
                        last_played: last_played_ms
                            .map(|value| human_relative(value, now_ms))
                            .unwrap_or_default(),
                        compat: compatibility_status,
                    }
                },
            )
            .collect();
        let selected_id = selected.as_ref().map(|game| game.game_id.clone());
        let (selected_play_time, selected_last_played, history) = match selected_id.as_deref() {
            Some(game_id) => {
                let stats = play_stats
                    .get(game_id)
                    .ok_or("ASTRA_EMU_LIBRARY_SNAPSHOT_STATS")?;
                let history = self
                    .library
                    .session_history(game_id)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(|session| PlaySessionViewModel {
                        started: local_time_label(session.start_unix_ms),
                        duration: human_duration(session.duration_ms),
                        ended_by: match session.ended_by {
                            Some(PlaySessionEndReason::Leave) => "返回游戏库".into(),
                            Some(PlaySessionEndReason::Shutdown) => "关闭程序".into(),
                            Some(_) => "异常结束".into(),
                            None => "进行中".into(),
                        },
                    })
                    .collect();
                (
                    human_duration(stats.total_duration_ms),
                    stats
                        .last_played_unix_ms
                        .map(|value| human_relative(value, now_ms))
                        .unwrap_or_default(),
                    history,
                )
            }
            None => (String::new(), String::new(), Vec::new()),
        };
        let profile = self.profile()?;
        let selected_releases = selected_id
            .as_deref()
            .and_then(|id| self.releases.get(id).cloned())
            .unwrap_or_default();
        let selected_compatibility = selected.as_ref().and_then(|game| {
            let linked = identities
                .get(&game.game_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            self.compatibility_for_identities(&game.game_id, linked)
        });
        let selected_compatibility_status = selected_compatibility
            .as_ref()
            .map(|entry| entry.status.as_str().to_string())
            .unwrap_or_default();
        let selected_compatibility_notes = selected_compatibility
            .as_ref()
            .and_then(|entry| entry.notes.clone())
            .unwrap_or_default();
        let selected_compatibility_updated = selected_compatibility
            .as_ref()
            .map(|entry| human_relative(entry.updated_at_unix_ms, now_ms))
            .unwrap_or_default();
        let selected_compatibility_vndb_id = selected_compatibility
            .as_ref()
            .map(|entry| format!("{} / {}", entry.vn_id, entry.release_id))
            .unwrap_or_default();
        let compatibility_sync_summary = match self.compatibility_database.as_ref() {
            Some(database) => {
                let fetched = self
                    .compatibility_fetched_at_unix_ms
                    .map(|value| human_relative(value, now_ms))
                    .unwrap_or_else(|| "本地缓存".into());
                format!(
                    "已收录 {} 条社区记录 · {}同步",
                    database.entries.len(),
                    fetched
                )
            }
            None => "尚未同步".into(),
        };
        let (engine_fields, family_fields) = self.family_fields()?;
        let selected_problem = selected
            .as_ref()
            .map(|game| self.game_problem(game, startup_busy))
            .unwrap_or_default();
        let mut notices = Vec::new();
        for (file, raw) in &self.plugin_errors {
            let text = describe_error(raw);
            notices.push(NoticeViewModel {
                id: format!("core-load:{file}"),
                severity: "error".into(),
                title: text.title,
                detail: text.detail,
                code: text.code,
                core: file.clone(),
            });
        }
        if self.registry.is_ready() && self.registry.descriptors().next().is_none() {
            notices.push(NoticeViewModel {
                id: "no-cores".into(),
                severity: "warning".into(),
                title: "没有可用的核心".into(),
                detail: "请在“核心”页安装核心，或使用包含 cores 目录的完整运行包。".into(),
                code: String::new(),
                core: String::new(),
            });
        }
        if !self.diagnostic.is_empty() {
            let text = describe_error(&self.diagnostic);
            notices.push(NoticeViewModel {
                id: format!("diagnostic:{}", &sha256_id(&self.diagnostic)[..12]),
                severity: self.diagnostic_severity.into(),
                title: text.title,
                detail: text.detail,
                code: text.code,
                core: String::new(),
            });
        }
        Ok(ManagerViewModel {
            library: LibraryViewModel {
                games: cards,
                query: self.search_query.clone(),
                sort: self.library_sort.clone(),
                compat_filter: self.compatibility_filter.clone(),
                cores_ready: self.cores_ready(),
                can_load_cores: self.core_refresh_enabled(),
                busy: startup_busy,
                status: self.startup_status(),
                selected_id: selected_id.clone().unwrap_or_default(),
            },
            selection: SelectionViewModel {
                id: selected_id.clone().unwrap_or_default(),
                title: selected
                    .as_ref()
                    .map(|game| game.display_title().into())
                    .unwrap_or_default(),
                family: selected
                    .as_ref()
                    .and_then(|game| game.family_id.as_deref())
                    .map(Self::engine_label)
                    .unwrap_or_default()
                    .into(),
                emulator: selected
                    .as_ref()
                    .and_then(|game| self.launch_candidate_for_game(game))
                    .map(|candidate| Self::emulator_label(&candidate.descriptor))
                    .unwrap_or_default(),
                launchable: selected
                    .as_ref()
                    .is_some_and(|game| self.launch_candidate_for_game(game).is_some()),
                problem: selected_problem,
                play_time: selected_play_time,
                last_played: selected_last_played,
                developer: metadata
                    .as_ref()
                    .map(|record| record.developers.join(", "))
                    .unwrap_or_default(),
                release_date: metadata
                    .as_ref()
                    .and_then(|record| record.release_date.clone())
                    .unwrap_or_default(),
                platforms: metadata
                    .as_ref()
                    .map(|record| record.platforms.join(", "))
                    .unwrap_or_default(),
                engine: metadata
                    .as_ref()
                    .and_then(|record| record.engine.clone())
                    .unwrap_or_default(),
                description: metadata
                    .as_ref()
                    .and_then(|record| record.description.clone())
                    .unwrap_or_default(),
                tags: metadata
                    .as_ref()
                    .map(|record| record.tags.join(", "))
                    .unwrap_or_default(),
                aliases: metadata
                    .as_ref()
                    .map(|record| record.alternate_titles.join(", "))
                    .unwrap_or_default(),
                compat: CompatViewModel {
                    status: selected_compatibility_status,
                    notes: selected_compatibility_notes,
                    updated: selected_compatibility_updated,
                    vndb_id: selected_compatibility_vndb_id,
                },
                releases: selected_releases
                    .into_iter()
                    .map(|(id, label)| ReleaseViewModel { id, label })
                    .collect(),
                history,
                matches: self
                    .match_candidates
                    .iter()
                    .filter(|(_, (game_id, _))| {
                        self.selected_case_id.as_deref() == Some(game_id.as_str())
                    })
                    .map(|(candidate_id, (_, record))| MatchViewModel {
                        id: candidate_id.clone(),
                        provider: record.provider.as_str().into(),
                        title: record.title.clone(),
                        aliases: record.alternate_titles.join(" / "),
                        release_date: record.release_date.clone().unwrap_or_default(),
                        developer: record.developers.join(" / "),
                    })
                    .collect(),
                metadata_busy: self.pending_metadata.values().any(|pending| {
                    pending.game_id.is_some() && pending.game_id == self.selected_case_id
                }),
                metadata_status: self
                    .selected_case_id
                    .as_ref()
                    .and_then(|id| self.metadata_status.get(id))
                    .cloned()
                    .or_else(|| {
                        metadata.as_ref().map(|record| {
                            format!("已关联 {}：{}", record.provider.as_str(), record.title)
                        })
                    })
                    .unwrap_or_default(),
                bangumi: BangumiViewModel {
                    status: self.bangumi_play_status.clone(),
                    rating: self.bangumi_rating,
                    note: self.bangumi_note.clone(),
                    summary: "同步需手动点击，不会自动上传。".into(),
                },
                engine_fields,
                family_fields,
                musica_key_available: self.musica_key_file_available(),
            },
            cores: self.cores_view()?,
            settings: SettingsViewModel {
                translation: TranslationViewModel {
                    kind: profile_kind(profile.clone()),
                    endpoint: profile
                        .as_ref()
                        .map(|profile| profile.endpoint.clone())
                        .unwrap_or_default(),
                    protocol: profile
                        .as_ref()
                        .map(|profile| match profile.protocol {
                            TranslationProtocol::Responses => "responses".into(),
                            TranslationProtocol::ChatCompletions => "chat_completions".into(),
                        })
                        .unwrap_or_else(|| "responses".into()),
                    model: profile
                        .as_ref()
                        .map(|profile| profile.model.clone())
                        .unwrap_or_default(),
                    language: profile
                        .as_ref()
                        .map(|profile| profile.target_language.clone())
                        .unwrap_or_else(|| "zh-CN".into()),
                    timeout_ms: profile
                        .as_ref()
                        .map(|profile| profile.timeout_ms.min(i32::MAX as u64) as i32)
                        .unwrap_or(15_000),
                },
                translation_consent: self.translation_consent,
                vndb_enabled: self.metadata_consent.get("vndb").copied().unwrap_or(false),
                bangumi_enabled: self
                    .metadata_consent
                    .get("bangumi")
                    .copied()
                    .unwrap_or(false),
                sensitive_covers: self.sensitive_covers,
                input: self.input_view(),
                filter_preset: self.filter_preset_id().into(),
                filter_fields: self.filter_fields()?,
                compat_source: DEFAULT_COMPATIBILITY_SOURCE_URL.into(),
                compat_summary: compatibility_sync_summary,
            },
            appearance: self.appearance_view(),
            notices,
            about: AboutViewModel {
                version: env!("CARGO_PKG_VERSION").into(),
                build: option_env!("ASTRA_EMU_MANAGER_RUSTC_FINGERPRINT")
                    .unwrap_or("development")
                    .into(),
            },
            session: SessionViewModel {
                actions: self.session_actions(),
                action_message: self.action_message.clone(),
                action_serial: self.action_serial,
            },
        })
    }

    fn game_problem(&self, game: &GameRecord, startup_busy: bool) -> String {
        let launchable = self.launch_candidate_for_game(game).is_some();
        let selection_required = self.probe_choices.contains_key(&game.game_id) && !launchable;
        if launchable {
            String::new()
        } else if startup_busy {
            "正在加载核心或校验游戏".into()
        } else if selection_required {
            "需要选择模拟器".into()
        } else {
            "尚未识别，请重新扫描".into()
        }
    }

    fn session_actions(&self) -> Vec<EngineActionViewModel> {
        let Some(active) = self
            .active
            .as_ref()
            .filter(|active| active.closing.is_none())
        else {
            return Vec::new();
        };
        let Some(plugin_id) = self
            .candidates
            .get(&active.game_id)
            .map(|candidate| candidate.descriptor.plugin_id.as_str())
        else {
            return Vec::new();
        };
        self.registry
            .core_metadata()
            .find(|meta| meta.descriptor.plugin_id == plugin_id)
            .map(|meta| {
                meta.menu
                    .iter()
                    .map(|item| EngineActionViewModel {
                        control: item.control as i32,
                        label: item.label.clone(),
                        icon: menu_icon_name(item.icon).into(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn menu_icon_name(icon: u32) -> &'static str {
    match icon {
        astra_emu_abi::MENU_ICON_QUICK_SAVE => "quick-save",
        astra_emu_abi::MENU_ICON_QUICK_LOAD => "quick-load",
        astra_emu_abi::MENU_ICON_SAVE => "save",
        astra_emu_abi::MENU_ICON_LOAD => "load",
        astra_emu_abi::MENU_ICON_AUTO => "auto",
        astra_emu_abi::MENU_ICON_SKIP => "skip",
        astra_emu_abi::MENU_ICON_BACKLOG => "backlog",
        astra_emu_abi::MENU_ICON_SETTINGS => "settings",
        _ => "generic",
    }
}
