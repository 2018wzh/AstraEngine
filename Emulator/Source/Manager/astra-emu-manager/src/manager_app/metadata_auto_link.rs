//! Automatic VNDB linking: after a scan or when network access is granted,
//! unlinked games are searched by title one at a time and linked only when
//! exactly one result has the same normalized title.

use super::*;

/// Game ids whose automatic search has run. Pruned to library games on every
/// write, so the set stays bounded by the library and removed games drop out.
pub(super) const AUTO_LINK_ATTEMPTED_KEY: &str = "metadata.auto_link_attempted";
pub(super) const AUTO_LINK_PREFIX: &str = "auto";
const AUTO_LINK_RESULTS: u8 = 10;

pub(super) enum AutoLinkStage {
    Search { query: String },
    Fetch,
}

pub(super) struct AutoLinkRequest {
    pub(super) request_id: String,
    pub(super) game_id: String,
    pub(super) stage: AutoLinkStage,
}

impl AstraEmuManagerController {
    /// Queue every unlinked, not yet attempted game and start the batch.
    /// Does nothing while network access is not allowed.
    pub(super) fn enqueue_auto_link(&mut self) -> Result<(), String> {
        if !self.network_allowed {
            return Ok(());
        }
        let snapshot = self
            .library
            .library_snapshot()
            .map_err(|error| error.to_string())?;
        for game in snapshot.games {
            let linked = snapshot
                .identities
                .get(&game.game_id)
                .is_some_and(|identities| {
                    identities
                        .iter()
                        .any(|identity| identity.provider == MetadataProviderId::Vndb.as_str())
                });
            if linked
                || game.display_title().trim().is_empty()
                || self.auto_link_attempted.contains(&game.game_id)
                || self.auto_link_queue.contains(&game.game_id)
                || self
                    .auto_link
                    .as_ref()
                    .is_some_and(|request| request.game_id == game.game_id)
            {
                continue;
            }
            self.auto_link_queue.push_back(game.game_id);
        }
        self.pump_auto_link()
    }

    /// Submit the next queued search unless an automatic request is running.
    pub(super) fn pump_auto_link(&mut self) -> Result<(), String> {
        if self.auto_link.is_some() {
            return Ok(());
        }
        if !self.network_allowed {
            self.auto_link_queue.clear();
            return Ok(());
        }
        while let Some(game_id) = self.auto_link_queue.pop_front() {
            let Some(game) = self
                .library
                .game(&game_id)
                .map_err(|error| error.to_string())?
            else {
                continue;
            };
            let linked = self
                .library
                .external_identities(&game_id)
                .map_err(|error| error.to_string())?
                .iter()
                .any(|identity| identity.provider == MetadataProviderId::Vndb.as_str());
            let busy = self
                .pending_metadata
                .values()
                .any(|pending| pending.game_id.as_deref() == Some(game_id.as_str()));
            let query = MetadataSearchQuery {
                title: game.display_title().trim().into(),
                aliases: Vec::new(),
                developer: None,
                release_date: None,
                limit: AUTO_LINK_RESULTS,
            };
            if linked
                || busy
                || self.auto_link_attempted.contains(&game_id)
                || query.validate().is_err()
            {
                continue;
            }
            let title = query.title.clone();
            match self.queue_metadata(
                &game,
                MetadataProviderId::Vndb,
                MetadataCommandKind::Search(query),
                AUTO_LINK_PREFIX,
            ) {
                Ok(request_id) => {
                    self.metadata_status
                        .insert(game_id.clone(), "正在 VNDB 自动匹配…".into());
                    self.auto_link = Some(AutoLinkRequest {
                        request_id,
                        game_id,
                        stage: AutoLinkStage::Search { query: title },
                    });
                    return Ok(());
                }
                Err(error) => {
                    self.stop_auto_link(error);
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Apply a completed automatic request and continue with the next game.
    pub(super) fn finish_auto_link(
        &mut self,
        request: AutoLinkRequest,
        result: Result<MetadataPayload, String>,
    ) -> Result<(), String> {
        let AutoLinkRequest { game_id, stage, .. } = request;
        match (stage, result) {
            (_, Err(error)) => {
                self.metadata_status
                    .insert(game_id, "请求失败，请检查错误信息后重试".into());
                self.stop_auto_link(error);
                return Ok(());
            }
            (AutoLinkStage::Search { query }, Ok(MetadataPayload::Search(records))) => {
                let matched = unique_exact_match(&query, &records)
                    .map(|index| records[index].remote_id.clone());
                let count = records.len();
                self.match_candidates
                    .retain(|_, (candidate_game, _)| *candidate_game != game_id);
                self.store_match_candidates(&game_id, MetadataProviderId::Vndb, records);
                if let Some(remote_id) = matched {
                    if !self.network_allowed {
                        self.metadata_status
                            .insert(game_id, format!("找到 {count} 个作品，请选择关联"));
                        self.auto_link_queue.clear();
                        return Ok(());
                    }
                    let game = self.game(&game_id)?;
                    match self.queue_metadata(
                        &game,
                        MetadataProviderId::Vndb,
                        MetadataCommandKind::Fetch(remote_id),
                        AUTO_LINK_PREFIX,
                    ) {
                        Ok(request_id) => {
                            self.metadata_status
                                .insert(game_id.clone(), "正在自动关联…".into());
                            self.auto_link = Some(AutoLinkRequest {
                                request_id,
                                game_id,
                                stage: AutoLinkStage::Fetch,
                            });
                        }
                        Err(error) => self.stop_auto_link(error),
                    }
                    return Ok(());
                }
                self.metadata_status.insert(
                    game_id.clone(),
                    if count == 0 {
                        "未在 VNDB 找到匹配作品".into()
                    } else {
                        format!("未能自动关联，找到 {count} 个候选，请手动选择")
                    },
                );
                self.mark_auto_link_attempted(&game_id)?;
            }
            (AutoLinkStage::Fetch, Ok(MetadataPayload::Fetch { record, cover })) => {
                let title =
                    self.link_metadata(&game_id, MetadataProviderId::Vndb, *record, cover)?;
                self.metadata_status
                    .insert(game_id.clone(), format!("已自动关联：{title}"));
                self.mark_auto_link_attempted(&game_id)?;
            }
            (_, Ok(_)) => return Err("ASTRA_EMU_METADATA_RESPONSE_UNEXPECTED".into()),
        }
        self.pump_auto_link()
    }

    /// End the batch after a failed request: one diagnostic, no attempt
    /// marks, and the remaining games are retried at the next trigger.
    fn stop_auto_link(&mut self, error: String) {
        tracing::warn!(
            event = "astra.emu.metadata.auto_link_stopped",
            remaining = self.auto_link_queue.len(),
            diagnostic_code = %error
        );
        self.auto_link_queue.clear();
        self.diagnostic = error;
        self.diagnostic_severity = "error";
    }

    pub(super) fn mark_auto_link_attempted(&mut self, game_id: &str) -> Result<(), String> {
        let present = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| game.game_id)
            .collect::<BTreeSet<_>>();
        self.auto_link_attempted.insert(game_id.to_owned());
        self.auto_link_attempted.retain(|id| present.contains(id));
        self.library
            .set_app_state(AUTO_LINK_ATTEMPTED_KEY, &self.auto_link_attempted)
            .map_err(|error| error.to_string())
    }
}
