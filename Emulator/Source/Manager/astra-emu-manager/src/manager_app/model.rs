use super::*;
use astra_emu_manager_ui_slint::{
    describe_error, AboutViewModel, BangumiViewModel, CompatViewModel, EngineActionViewModel,
    ExcludedViewModel, GameCardViewModel, LibraryViewModel, MatchViewModel, NoticeViewModel,
    PlaySessionViewModel, ReleaseViewModel, SelectionViewModel, SessionViewModel,
    SettingsViewModel,
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
            let compatibility_status = self.compatibility_status(&game.game_id, linked);
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
                        .map(|id| self.engine_label(id))
                        .unwrap_or_else(|| "未识别引擎".to_owned());
                    let problem = self.game_problem(&game);
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
        let selected_compatibility = selected
            .as_ref()
            .map(|game| {
                let linked = identities
                    .get(&game.game_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                self.compat_view(&game.game_id, linked, now_ms)
            })
            .unwrap_or_default();
        let recommended = match selected_id.as_deref() {
            Some(game_id) => self.recommended_preset(game_id)?.map(|preset| preset.summary.clone()),
            None => None,
        };
        let recipes = self.recipe_views(selected_id.as_deref())?;
        let (emulator_choices, emulator_id, family_fields, render, game_mapping) =
            match selected_id.as_deref() {
                Some(game_id) => {
                    let override_mapping = self
                        .library
                        .game_settings(game_id)
                        .map_err(|error| error.to_string())?
                        .and_then(|settings| settings.input_mapping);
                    (
                        self.emulator_choices(game_id),
                        self.candidates
                            .get(game_id)
                            .map(|candidate| candidate.report.plugin_id.clone())
                            .unwrap_or_default(),
                        self.game_config_fields(game_id)?,
                        self.game_render_view(game_id)?,
                        override_mapping,
                    )
                }
                None => Default::default(),
            };
        let mut cores = self.cores_view()?;
        cores.defaults = self.emulator_defaults_view()?;
        let selected_problem = selected
            .as_ref()
            .map(|game| self.game_problem(game))
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
        if !self.patch_state.error.is_empty() {
            let text = describe_error(&self.patch_state.error);
            notices.push(NoticeViewModel {
                id: format!("file-patch:{}", self.patch_sequence),
                severity: "error".into(),
                title: text.title,
                detail: text.detail,
                code: text.code,
                core: String::new(),
            });
        }
        let (prepare_state, prepare_label, prepare_progress, can_cancel_prepare) =
            self.prepare_view();
        let startup_status = self.startup_status();
        Ok(ManagerViewModel {
            library: LibraryViewModel {
                games: cards,
                query: self.search_query.clone(),
                sort: self.library_sort.clone(),
                compat_filter: self.compatibility_filter.clone(),
                cores_ready: self.cores_ready(),
                busy: startup_busy || self.patch_job.is_some(),
                status: if self.patch_job.is_some() {
                    "正在应用文件补丁；可继续浏览或在详情中取消。".into()
                } else {
                    startup_status.clone()
                },
                prepare_state: prepare_state.into(),
                prepare_label: prepare_label.into(),
                prepare_detail: startup_status,
                prepare_progress,
                can_cancel_prepare,
                selected_id: selected_id.clone().unwrap_or_default(),
                roots: self
                    .library
                    .library_roots()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(|root| root.path)
                    .collect(),
                excluded: self
                    .library
                    .excluded_games()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(|game| ExcludedViewModel {
                        id: game.game_id,
                        title: game.title,
                        location: game.location,
                    })
                    .collect(),
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
                    .map(|id| self.engine_label(id))
                    .unwrap_or_default(),
                emulator: selected
                    .as_ref()
                    .and_then(|game| self.launch_candidate_for_game(game))
                    .map(|candidate| Self::emulator_label(&candidate.descriptor))
                    .unwrap_or_default(),
                launchable: selected.as_ref().is_some_and(|game| {
                    !self.patch_busy()
                        && !startup_busy
                        && !astra_emu_patch::is_staging_path(Path::new(&game.location))
                        && self.launch_candidate_for_game(game).is_some()
                }),
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
                compat: selected_compatibility,
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
                emulator_choices,
                emulator_id,
                family_fields,
                render,
                input_override: game_mapping.is_some(),
                game_bindings: game_mapping
                    .as_ref()
                    .map(view_fields::binding_views)
                    .unwrap_or_default(),
                recommended_available: recommended.is_some(),
                recommended_summary: recommended.unwrap_or_default(),
                recipes,
            },
            network: self.network_view(now_ms),
            cores,
            settings: SettingsViewModel {
                translation: translation::profile_form(profile.as_ref()),
                translation_enabled: self.translation_enabled,
                translation_status: self.translation_status.clone(),
                translation_models: self.translation_models.clone(),
                network_allowed: self.network_allowed,
                sensitive_covers: self.sensitive_covers,
                input: self.input_view(),
                filter_preset: self.filter_preset_id().into(),
                filter_fields: self.filter_fields()?,
                compat_source: DEFAULT_METADATA_BASE_URL.into(),
                compat_summary: self.compatibility_summary(),
            },
            appearance: self.appearance_view(),
            notices,
            snackbar: self.snackbar.view(),
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
            patch: self.patch_view(),
        })
    }

    fn game_problem(&self, game: &GameRecord) -> String {
        if astra_emu_patch::is_staging_path(Path::new(&game.location)) {
            return "这是尚未发布的临时目录，不能启动".into();
        }
        if self.patch_busy() {
            return "文件补丁转换或导入尚未结束，暂不能启动游戏".into();
        }
        if self.launch_candidate_for_game(game).is_some() {
            return String::new();
        }
        match self.startup_state {
            background::StartupState::Waiting => "需要先准备资料库".into(),
            background::StartupState::Loading | background::StartupState::Scanning(_) => {
                "正在准备资料库".into()
            }
            background::StartupState::Failed(_) => "准备失败，请查看核心诊断".into(),
            background::StartupState::Ready if self.probe_choices.contains_key(&game.game_id) => {
                "需要选择模拟器".into()
            }
            background::StartupState::Ready => "未识别到可用的模拟器".into(),
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
