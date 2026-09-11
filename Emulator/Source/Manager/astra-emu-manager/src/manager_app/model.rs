use super::*;

impl AstraEmuManagerController {
    pub(super) fn model(&self) -> Result<ManagerViewModel, String> {
        let now_ms = unix_time_ms()?;
        let query = self.search_query.trim().to_ascii_lowercase();
        let all_games = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?;
        let mut rows = Vec::new();
        for mut game in all_games {
            if let Some(metadata) = self.metadata_for_game(&game.game_id)? {
                game.title = metadata.title;
            }
            if !query.is_empty()
                && !game.display_title().to_ascii_lowercase().contains(&query)
                && !game.game_id.to_ascii_lowercase().contains(&query)
            {
                continue;
            }
            let stats = self
                .library
                .play_stats(&game.game_id)
                .map_err(|error| error.to_string())?;
            let cover_uri = self.cover_uri(&game.game_id)?;
            let compatibility_status = self.compatibility_status_for_game(&game)?;
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
        let cards = rows
            .iter()
            .map(
                |(game, duration_ms, last_played_ms, cover_uri, compatibility_status)| {
                    GameCardViewModel {
                        case_id: game.game_id.clone(),
                        title: game.display_title().into(),
                        family: if self.probe_choices.contains_key(&game.game_id) {
                            "selection required".into()
                        } else {
                            game.family_id.clone().unwrap_or_else(|| "unknown".into())
                        },
                        cover_uri: cover_uri.clone(),
                        diagnostic: String::new(),
                        play_time: human_duration(*duration_ms),
                        last_played: last_played_ms
                            .map(|value| human_relative(value, now_ms))
                            .unwrap_or_default(),
                        compatibility_status: compatibility_status.clone(),
                    }
                },
            )
            .collect();

        let selected = self
            .selected_case_id
            .as_deref()
            .map(|id| {
                let mut game = self.game(id)?;
                if let Some(metadata) = self.metadata_for_game(id)? {
                    game.title = metadata.title;
                }
                Ok::<_, String>(game)
            })
            .transpose()?;
        let selected_id = selected.as_ref().map(|game| game.game_id.clone());
        let (selected_play_time, selected_last_played, history) = match selected_id.as_deref() {
            Some(game_id) => {
                let stats = self
                    .library
                    .play_stats(game_id)
                    .map_err(|error| error.to_string())?;
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
        let metadata = selected_id
            .as_deref()
            .map(|id| self.metadata_for_game(id))
            .transpose()?
            .flatten();
        let profile = self.profile()?;
        let selected_releases = selected_id
            .as_deref()
            .and_then(|id| self.releases.get(id).cloned())
            .unwrap_or_default();
        let selected_compatibility = selected
            .as_ref()
            .map(|game| self.compatibility_for_game(game))
            .transpose()?
            .flatten();
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
        Ok(ManagerViewModel {
            games: cards,
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
            global_diagnostic: self.diagnostic.clone(),
            audio_device: self.audio_device.as_str().into(),
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
            selected_family: selected
                .as_ref()
                .and_then(|game| game.family_id.clone())
                .unwrap_or_default(),
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
            family_config_fields: self.family_fields(),
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
