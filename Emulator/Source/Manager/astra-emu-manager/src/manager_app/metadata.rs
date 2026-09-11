use super::*;

impl AstraEmuManagerController {
    pub(super) fn next_metadata_id(&mut self, prefix: &str) -> Result<String, String> {
        self.metadata_sequence = self
            .metadata_sequence
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_METADATA_REQUEST_OVERFLOW".to_owned())?;
        Ok(format!("metadata-{prefix}-{}", self.metadata_sequence))
    }

    pub(super) fn metadata_provider(value: &str) -> Result<MetadataProviderId, String> {
        match value {
            "vndb" => Ok(MetadataProviderId::Vndb),
            "bangumi" => Ok(MetadataProviderId::Bangumi),
            _ => Err("ASTRA_EMU_METADATA_PROVIDER_INVALID".into()),
        }
    }

    pub(super) fn metadata_allowed(&self, provider: MetadataProviderId) -> Result<(), String> {
        self.metadata_consent
            .get(provider.as_str())
            .copied()
            .filter(|allowed| *allowed)
            .map(|_| ())
            .ok_or_else(|| "ASTRA_EMU_METADATA_CONSENT_REQUIRED".into())
    }

    pub(super) fn queue_metadata(
        &mut self,
        game: &GameRecord,
        provider: MetadataProviderId,
        kind: MetadataCommandKind,
        prefix: &str,
    ) -> Result<(), String> {
        self.metadata_allowed(provider)?;
        if self
            .pending_metadata
            .values()
            .any(|pending| pending.game_id.as_deref() == Some(game.game_id.as_str()))
        {
            return Err("ASTRA_EMU_METADATA_REQUEST_PENDING".into());
        }
        if let MetadataCommandKind::Search(query) = &kind {
            query.validate().map_err(|error| error.to_string())?;
        }
        let request_id = self.next_metadata_id(prefix)?;
        self.metadata.submit(MetadataCommand {
            request_id: request_id.clone(),
            case_identity: game.game_id.clone(),
            provider,
            access_token: self.metadata_tokens.get(provider.as_str()).cloned(),
            allow_sensitive_cover: self.sensitive_covers,
            kind,
        })?;
        self.pending_metadata.insert(
            request_id,
            PendingMetadata {
                game_id: Some(game.game_id.clone()),
            },
        );
        Ok(())
    }

    pub(super) fn poll_metadata(&mut self) -> Result<bool, String> {
        let mut changed = false;
        while let Some(completion) = self.metadata.try_recv()? {
            changed = true;
            let pending = self
                .pending_metadata
                .remove(&completion.request_id)
                .ok_or_else(|| "ASTRA_EMU_METADATA_RESPONSE_UNKNOWN".to_owned())?;
            if completion.case_identity != pending.game_id.as_deref().unwrap_or_default() {
                return Err("ASTRA_EMU_METADATA_RESPONSE_IDENTITY_MISMATCH".into());
            }
            let game_id = pending.game_id;
            match completion.result {
                Ok(payload) => {
                    self.apply_metadata_payload(game_id.as_deref(), completion.provider, payload)?
                }
                Err(error) => {
                    if let Some(game_id) = game_id {
                        self.metadata_status
                            .insert(game_id, "请求失败，请检查错误信息后重试".into());
                    }
                    self.diagnostic = error;
                }
            }
        }
        Ok(changed)
    }

    pub(super) fn apply_metadata_payload(
        &mut self,
        game_id: Option<&str>,
        provider: MetadataProviderId,
        payload: MetadataPayload,
    ) -> Result<(), String> {
        match payload {
            MetadataPayload::Search(records) => {
                let game_id =
                    game_id.ok_or_else(|| "ASTRA_EMU_METADATA_REQUEST_UNKNOWN".to_owned())?;
                self.metadata_status.insert(
                    game_id.into(),
                    if records.is_empty() {
                        "没有找到作品，请尝试原名、译名或其他关键词".into()
                    } else {
                        format!("找到 {} 个作品，请选择关联", records.len())
                    },
                );
                for (index, record) in records.into_iter().enumerate() {
                    let candidate_id = format!(
                        "match-{}-{index}",
                        sha256_id(&format!(
                            "{game_id}:{}:{}",
                            provider.as_str(),
                            record.remote_id
                        ))
                    );
                    self.match_candidates
                        .insert(candidate_id, (game_id.to_owned(), record));
                }
                self.diagnostic.clear();
            }
            MetadataPayload::Fetch { record, cover } => {
                let game_id =
                    game_id.ok_or_else(|| "ASTRA_EMU_METADATA_REQUEST_UNKNOWN".to_owned())?;
                let record = *record;
                let linked_title = record.title.clone();
                if let Some(cover) = cover.as_ref() {
                    store_cover(&self.data_dir, game_id, cover)?;
                }
                let encoded = serde_json::to_string(&record)
                    .map_err(|_| "ASTRA_EMU_METADATA_SERIALIZATION".to_owned())?;
                let now = unix_time_ms()?;
                self.library
                    .set_external_identity(&astra_emu_manager_core::ExternalIdentityRecord {
                        game_id: game_id.into(),
                        provider: provider.as_str().into(),
                        remote_id: record.remote_id.clone(),
                        provenance: "metadata-provider".into(),
                        linked_at_unix_ms: now,
                    })
                    .map_err(|error| error.to_string())?;
                self.library
                    .set_metadata_snapshot(&astra_emu_manager_core::MetadataSnapshotRecord {
                        game_id: game_id.into(),
                        provider: provider.as_str().into(),
                        remote_id: record.remote_id,
                        metadata_json: encoded,
                        fetched_at_unix_ms: now,
                        state: astra_emu_manager_core::MetadataSnapshotState::Fresh,
                    })
                    .map_err(|error| error.to_string())?;
                self.match_candidates.retain(|_, (id, _)| id != game_id);
                self.metadata_status
                    .insert(game_id.into(), format!("已关联：{linked_title}"));
                self.diagnostic.clear();
            }
            MetadataPayload::Releases(releases) => {
                let game_id =
                    game_id.ok_or_else(|| "ASTRA_EMU_METADATA_REQUEST_UNKNOWN".to_owned())?;
                self.releases.insert(
                    game_id.into(),
                    releases
                        .into_iter()
                        .map(|release| (release.release_id, release.title.unwrap_or_default()))
                        .collect(),
                );
            }
            MetadataPayload::BangumiPlaySynced => self.diagnostic.clear(),
            MetadataPayload::Compatibility(fetch) => self.apply_compatibility_payload(fetch)?,
        }
        Ok(())
    }

    pub(super) fn apply_compatibility_payload(
        &mut self,
        fetch: CompatibilityFetch,
    ) -> Result<(), String> {
        match fetch {
            CompatibilityFetch::NotModified => {
                self.compatibility_fetched_at_unix_ms = Some(unix_time_ms()?);
                self.diagnostic.clear();
            }
            CompatibilityFetch::Updated {
                database,
                response_hash,
            } => {
                let hash = save_compatibility_cache(&self.data_dir, &database)?;
                if hash != response_hash {
                    return Err("ASTRA_EMU_COMPATIBILITY_CACHE_HASH_MISMATCH".into());
                }
                self.compatibility_database = Some(database);
                self.compatibility_hash = Some(response_hash);
                self.compatibility_fetched_at_unix_ms = Some(unix_time_ms()?);
                self.diagnostic.clear();
            }
        }
        Ok(())
    }

    pub(super) fn metadata_record(
        &self,
        game_id: &str,
        provider: MetadataProviderId,
    ) -> Result<Option<MetadataRecord>, String> {
        let identities = self
            .library
            .external_identities(game_id)
            .map_err(|error| error.to_string())?;
        let Some(identity) = identities
            .iter()
            .find(|identity| identity.provider == provider.as_str())
        else {
            return Ok(None);
        };
        let snapshot = self
            .library
            .metadata_snapshot(game_id, provider.as_str())
            .map_err(|error| error.to_string())?;
        snapshot
            .map(|snapshot| {
                if snapshot.remote_id != identity.remote_id {
                    return Err("ASTRA_EMU_METADATA_SNAPSHOT_IDENTITY_MISMATCH".into());
                }
                serde_json::from_str(&snapshot.metadata_json)
                    .map_err(|_| "ASTRA_EMU_METADATA_SNAPSHOT_INVALID".to_owned())
            })
            .transpose()
    }

    pub(super) fn metadata_for_game(
        &self,
        game_id: &str,
    ) -> Result<Option<MetadataRecord>, String> {
        if let Some(record) = self.metadata_record(game_id, MetadataProviderId::Vndb)? {
            return Ok(Some(record));
        }
        self.metadata_record(game_id, MetadataProviderId::Bangumi)
    }

    pub(super) fn cover_uri(&self, game_id: &str) -> Result<String, String> {
        if self.metadata_for_game(game_id)?.is_none() {
            return Ok(String::new());
        }
        let directory = self.data_dir.join("covers");
        for extension in ["png", "jpg", "webp", "gif", "bmp"] {
            let path = directory.join(format!("{game_id}.{extension}"));
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => {
                    return path
                        .to_str()
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| "ASTRA_EMU_METADATA_COVER_PATH_UTF8".to_owned());
                }
                Ok(_) => return Err("ASTRA_EMU_METADATA_COVER_NOT_FILE".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("ASTRA_EMU_METADATA_COVER_STAT".into()),
            }
        }
        Ok(String::new())
    }

    pub(super) fn compatibility_for_game(
        &self,
        game: &GameRecord,
    ) -> Result<Option<CompatibilityEntry>, String> {
        let Some(database) = self.compatibility_database.as_ref() else {
            return Ok(None);
        };
        let identities = self
            .library
            .external_identities(&game.game_id)
            .map_err(|error| error.to_string())?;
        let Some(vndb_id) = identities
            .iter()
            .find(|identity| identity.provider == MetadataProviderId::Vndb.as_str())
            .map(|identity| identity.remote_id.as_str())
        else {
            return Ok(None);
        };
        let pinned = self.pinned_releases.get(&game.game_id);
        let mut matches = database
            .entries
            .iter()
            .filter(|entry| entry.vn_id == vndb_id)
            .filter(|entry| pinned.is_none_or(|release| entry.release_id == *release));
        let first = matches.next().cloned();
        if pinned.is_none() && matches.next().is_some() {
            return Ok(None);
        }
        Ok(first)
    }

    pub(super) fn compatibility_status_for_game(
        &self,
        game: &GameRecord,
    ) -> Result<String, String> {
        Ok(self
            .compatibility_for_game(game)?
            .map(|entry| entry.status.as_str().to_owned())
            .unwrap_or_default())
    }
}
