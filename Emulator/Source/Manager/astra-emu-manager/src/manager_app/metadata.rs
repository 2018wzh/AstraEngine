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

    pub(super) fn require_network(&self) -> Result<(), String> {
        if self.network_allowed {
            Ok(())
        } else {
            Err("ASTRA_EMU_NETWORK_PERMISSION_REQUIRED".into())
        }
    }

    pub(super) fn bangumi_token(&mut self) -> Result<Option<String>, String> {
        if !self.metadata_tokens_loaded {
            match ManagerSecretStore::open()
                .map_err(|error| error.to_string())?
                .resolve(BANGUMI_SECRET_REFERENCE)
            {
                Ok(token) => {
                    self.metadata_tokens
                        .insert(MetadataProviderId::Bangumi.as_str().into(), token);
                }
                Err(astra_emu_translation::TranslationError::SecretUnavailable) => {}
                Err(error) => return Err(error.to_string()),
            }
            self.metadata_tokens_loaded = true;
        }
        Ok(self
            .metadata_tokens
            .get(MetadataProviderId::Bangumi.as_str())
            .cloned())
    }

    /// Submit a request for one game and return its request id. A manual
    /// request takes the game out of the automatic linking queue.
    pub(super) fn queue_metadata(
        &mut self,
        game: &GameRecord,
        provider: MetadataProviderId,
        kind: MetadataCommandKind,
        prefix: &str,
    ) -> Result<String, String> {
        self.require_network()?;
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
        let access_token = match provider {
            MetadataProviderId::Bangumi => self.bangumi_token()?,
            _ => None,
        };
        self.metadata.submit(MetadataCommand {
            request_id: request_id.clone(),
            case_identity: game.game_id.clone(),
            provider,
            access_token,
            allow_sensitive_cover: self.sensitive_covers,
            kind,
        })?;
        self.pending_metadata.insert(
            request_id.clone(),
            PendingMetadata {
                game_id: Some(game.game_id.clone()),
                core: None,
            },
        );
        if prefix != metadata_auto_link::AUTO_LINK_PREFIX {
            self.auto_link_queue.retain(|id| *id != game.game_id);
        }
        Ok(request_id)
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
            if self.metadata_sync.request.as_deref() == Some(completion.request_id.as_str()) {
                self.metadata_sync.request = None;
            }
            if let Some(request) = self
                .auto_link
                .take_if(|request| request.request_id == completion.request_id)
            {
                self.finish_auto_link(request, completion.result)?;
                continue;
            }
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
                    self.diagnostic_severity = "error";
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
                self.store_match_candidates(game_id, provider, records);
                self.diagnostic.clear();
            }
            MetadataPayload::Fetch { record, cover } => {
                let game_id =
                    game_id.ok_or_else(|| "ASTRA_EMU_METADATA_REQUEST_UNKNOWN".to_owned())?;
                let linked_title = self.link_metadata(game_id, provider, *record, cover)?;
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
            MetadataPayload::Synced(outcome) => {
                if let Err(error) = self.apply_sync_outcome(*outcome) {
                    tracing::warn!(event = "astra.emu.metadata.sync_rejected", diagnostic_code = %error);
                    self.diagnostic = error;
                    self.diagnostic_severity = "error";
                }
            }
            MetadataPayload::CoreStaged { stem, version } => self.core_staged(&stem, &version),
        }
        Ok(())
    }

    pub(super) fn store_match_candidates(
        &mut self,
        game_id: &str,
        provider: MetadataProviderId,
        records: Vec<MetadataRecord>,
    ) {
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
    }

    /// Store a fetched record as the game's linked identity and snapshot,
    /// returning the linked title.
    pub(super) fn link_metadata(
        &mut self,
        game_id: &str,
        provider: MetadataProviderId,
        record: MetadataRecord,
        cover: Option<CoverAsset>,
    ) -> Result<String, String> {
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
        Ok(linked_title)
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
        Self::decode_metadata_snapshot(identity, snapshot.as_ref())
    }

    pub(super) fn metadata_from_snapshot(
        game_id: &str,
        identities: &[astra_emu_manager_core::ExternalIdentityRecord],
        snapshots: &BTreeMap<(String, String), astra_emu_manager_core::MetadataSnapshotRecord>,
    ) -> Result<Option<MetadataRecord>, String> {
        for provider in [MetadataProviderId::Vndb, MetadataProviderId::Bangumi] {
            let Some(identity) = identities
                .iter()
                .find(|entry| entry.provider == provider.as_str())
            else {
                continue;
            };
            let snapshot = snapshots.get(&(game_id.to_owned(), provider.as_str().to_owned()));
            if let Some(record) = Self::decode_metadata_snapshot(identity, snapshot)? {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    fn decode_metadata_snapshot(
        identity: &astra_emu_manager_core::ExternalIdentityRecord,
        snapshot: Option<&astra_emu_manager_core::MetadataSnapshotRecord>,
    ) -> Result<Option<MetadataRecord>, String> {
        snapshot
            .map(|snapshot| {
                if snapshot.remote_id != identity.remote_id {
                    return Err("ASTRA_EMU_METADATA_SNAPSHOT_IDENTITY_MISMATCH".into());
                }
                let mut record: MetadataRecord = serde_json::from_str(&snapshot.metadata_json)
                    .map_err(|_| "ASTRA_EMU_METADATA_SNAPSHOT_INVALID".to_owned())?;
                // Snapshots fetched before VNDB formatting was converted still
                // carry raw BBCode; conversion is idempotent for newer ones.
                if record.provider == MetadataProviderId::Vndb {
                    record.description = record
                        .description
                        .map(|description| plain_description(&description));
                }
                Ok(record)
            })
            .transpose()
    }
}
