use super::*;

impl AstraEmuManagerController {
    pub(super) fn model(&self) -> Result<ManagerViewModel, String> {
        let now_ms = unix_time_ms()?;
        let query = self.search_query.trim().to_ascii_lowercase();
        let astra_emu_manager_core::LibrarySnapshot {
            games, play_stats, identities, metadata: metadata_snapshots,
        } = self.library.library_snapshot().map_err(|error| error.to_string())?;
        let mut selected = None;
        let mut metadata = None;
        let startup_busy = self.startup_busy();
        let mut rows = Vec::new();
        for mut game in games {
            let linked = identities.get(&game.game_id).map(Vec::as_slice).unwrap_or(&[]);
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
            let stats = play_stats.get(&game.game_id).ok_or("ASTRA_EMU_LIBRARY_SNAPSHOT_STATS")?;
            let cover_uri = if has_metadata {
                self.data_dir.join("covers").join(&game.game_id)
                    .to_str().ok_or("ASTRA_EMU_METADATA_COVER_PATH_UTF8")?.to_owned()
            } else {
                String::new()
            };
            let compatibility_status = self.compatibility_for_identities(&game.game_id, linked)
                .map(|entry| entry.status.as_str().to_owned()).unwrap_or_default();
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
            .map(|(mut game, duration_ms, last_played_ms, cover_uri, compatibility_status)| {
                let launchable = self.launch_candidate_for_game(&game).is_some();
                let selection_required = self.probe_choices.contains_key(&game.game_id) && !launchable;
                let title = game.user_title.take().unwrap_or(game.title);
                let family = game.family_id.as_deref().map(Self::engine_label)
                    .unwrap_or("未识别引擎").to_owned();
                GameCardViewModel {
                    case_id: game.game_id,
                    title,
                    family,
                    cover_uri,
                    diagnostic: if launchable {
                        String::new()
                    } else if startup_busy {
                        "正在加载核心或校验游戏".into()
                    } else if selection_required {
                        "provider selection required".into()
                    } else {
                        "family probe required".into()
                    },
                    play_time: human_duration(duration_ms),
                    last_played: last_played_ms.map(|value| human_relative(value, now_ms)).unwrap_or_default(),
                    compatibility_status,
                }
            })
            .collect();
        let selected_id = selected.as_ref().map(|game| game.game_id.clone());
        let (selected_play_time, selected_last_played, history) = match selected_id.as_deref() {
            Some(game_id) => {
                let stats = play_stats.get(game_id).ok_or("ASTRA_EMU_LIBRARY_SNAPSHOT_STATS")?;
                let history = self
                    .library
                    .session_history(game_id)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(|session| PlaySessionViewModel {
                        start_time: session.start_unix_ms.to_string(),
                        duration: human_duration(session.duration_ms),
                        ended_by: session
                            .ended_by
                            .map(|reason| format!("{reason:?}"))
                            .unwrap_or_else(|| "active".into()),
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
            let linked = identities.get(&game.game_id).map(Vec::as_slice).unwrap_or(&[]);
            self.compatibility_for_identities(&game.game_id, linked)
        });
        let selected_compatibility_status = selected_compatibility
            .as_ref()
            .map(|entry| compatibility_status_label(entry.status.as_str()))
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
                    .unwrap_or_else(|| "loaded".into());
                format!(
                    "Compat DB · {} entries · fetched {fetched}",
                    database.entries.len()
                )
            }
            None => "Not synchronized".into(),
        };
        let selected_cover_meta = metadata
            .as_ref()
            .and_then(|record| record.cover.as_ref())
            .map(|cover| {
                let dimensions = match (cover.width, cover.height) {
                    (Some(width), Some(height)) => format!("{width}x{height}"),
                    _ => "unknown-size".into(),
                };
                if metadata.as_ref().is_some_and(|record| record.sensitive) {
                    format!("{dimensions}; sensitive")
                } else {
                    dimensions
                }
            })
            .unwrap_or_default();
        let (common_engine_config_fields, family_config_fields) = self.family_fields()?;
        Ok(ManagerViewModel {
            games: cards,
            startup_busy,
            startup_status: self.startup_status(),
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
            match_reviews: self
                .match_candidates
                .iter()
                .filter(|(_, (game_id, _))| {
                    self.selected_case_id.as_deref() == Some(game_id.as_str())
                })
                .map(|(candidate_id, (game_id, record))| MatchReviewViewModel {
                    candidate_id: candidate_id.clone(),
                    case_id: game_id.clone(),
                    provider: record.provider.as_str().into(),
                    remote_id: record.remote_id.clone(),
                    title: record.title.clone(),
                    aliases: record.alternate_titles.join(" / "),
                    release_date: record.release_date.clone().unwrap_or_default(),
                    developer: record.developers.join(" / "),
                    evidence: "Provider search result".into(),
                    score_millis: 0,
                    diagnostic: String::new(),
                })
                .collect(),
            selected_case_id: selected_id.clone(),
            search_query: self.search_query.clone(),
            endpoint_identity: profile
                .as_ref()
                .map(TranslationProfile::provider_identity)
                .unwrap_or_default(),
            model_identity: profile
                .as_ref()
                .map(|profile| profile.model.clone())
                .unwrap_or_default(),
            global_diagnostic: self
                .plugin_errors
                .iter()
                .map(|(id, error)| {
                    format!("核心文件 {id} 未加载：{error}。修复运行包或用户数据目录的 cores 文件后重启 AstraEMU。")
                })
                .chain(
                    (self.registry.descriptors().next().is_none()).then(|| {
                        "没有可用核心。请使用含 cores 目录的完整 AstraEMU 运行包，然后重启客户端。".to_owned()
                    }),
                )
                .chain(
                    std::iter::once(self.diagnostic.clone()).filter(|message| !message.is_empty()),
                )
                .collect::<Vec<_>>()
                .join("\n"),
            translation_endpoint_kind: profile_kind(profile.clone()),
            translation_endpoint: profile
                .as_ref()
                .map(|profile| profile.endpoint.clone())
                .unwrap_or_default(),
            translation_protocol: profile
                .as_ref()
                .map(|profile| match profile.protocol {
                    TranslationProtocol::Responses => "responses".into(),
                    TranslationProtocol::ChatCompletions => "chat_completions".into(),
                })
                .unwrap_or_else(|| "responses".into()),
            translation_model: profile
                .as_ref()
                .map(|profile| profile.model.clone())
                .unwrap_or_default(),
            translation_target_language: profile
                .as_ref()
                .map(|profile| profile.target_language.clone())
                .unwrap_or_else(|| "zh-CN".into()),
            translation_timeout_ms: profile
                .as_ref()
                .map(|profile| profile.timeout_ms.min(i32::MAX as u64) as i32)
                .unwrap_or(15_000),
            translation_consent_present: self.translation_consent,
            filter_preset: self.filter_preset_id().into(),
            diagnostics_summary: self.diagnostic.clone(),
            vndb_consent: self.metadata_consent.get("vndb").copied().unwrap_or(false),
            bangumi_consent: self
                .metadata_consent
                .get("bangumi")
                .copied()
                .unwrap_or(false),
            sensitive_covers: self.sensitive_covers,
            bangumi_play_status: self.bangumi_play_status.clone(),
            bangumi_rating: self.bangumi_rating,
            bangumi_note: self.bangumi_note.clone(),
            bangumi_sync_summary: "Synchronization is explicit.".into(),
            selected_title: selected
                .as_ref()
                .map(|game| game.display_title().into())
                .unwrap_or_default(),
            selected_family: selected.as_ref()
                .and_then(|game| game.family_id.as_deref())
                .map(Self::engine_label).unwrap_or_default().into(),
            selected_emulator: selected.as_ref()
                .and_then(|game| self.launch_candidate_for_game(game))
                .map(|candidate| Self::emulator_label(&candidate.descriptor))
                .unwrap_or_default(),
            selected_launchable: selected.as_ref()
                .is_some_and(|game| self.launch_candidate_for_game(game).is_some()),
            selected_play_time,
            selected_last_played,
            play_history: history,
            library_sort: self.library_sort.clone(),
            compatibility_filter: self.compatibility_filter.clone(),
            compatibility_source_url: DEFAULT_COMPATIBILITY_SOURCE_URL.into(),
            compatibility_sync_summary,
            selected_compatibility_status,
            selected_compatibility_notes,
            selected_compatibility_updated,
            selected_compatibility_vndb_id,
            selected_releases,
            current_page: String::new(),
            input_config: self.input_view(),
            appearance: self.appearance_view(),
            common_engine_config_fields,
            family_config_fields,
            musica_key_file_available: self.musica_key_file_available(),
            filter_config_fields: self.filter_fields()?,
            version: env!("CARGO_PKG_VERSION").into(),
            build_identity: option_env!("ASTRA_EMU_MANAGER_RUSTC_FINGERPRINT")
                .unwrap_or("development")
                .into(),
            is_demo: false,
            selected_developer: metadata
                .as_ref()
                .map(|record| record.developers.join(", "))
                .unwrap_or_default(),
            selected_release_date: metadata
                .as_ref()
                .and_then(|record| record.release_date.clone())
                .unwrap_or_default(),
            selected_platforms: metadata
                .as_ref()
                .map(|record| record.platforms.join(", "))
                .unwrap_or_default(),
            selected_engine: metadata
                .as_ref()
                .and_then(|record| record.engine.clone())
                .unwrap_or_default(),
            selected_cover_meta,
            selected_description: metadata
                .as_ref()
                .and_then(|record| record.description.clone())
                .unwrap_or_default(),
            selected_tags: metadata
                .as_ref()
                .map(|record| record.tags.join(", "))
                .unwrap_or_default(),
            selected_aliases: metadata
                .as_ref()
                .map(|record| record.alternate_titles.join(", "))
                .unwrap_or_default(),
        })
    }
}
