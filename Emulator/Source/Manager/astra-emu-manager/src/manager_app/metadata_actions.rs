use super::*;

impl AstraEmuManagerController {
    pub(super) fn search_metadata(
        &mut self,
        provider: &str,
        query: &str,
    ) -> Result<ManagerViewModel, String> {
        let provider = Self::metadata_provider(provider)?;
        let game = self.selected_game()?;
        self.queue_metadata(
            &game,
            provider,
            MetadataCommandKind::Search(MetadataSearchQuery {
                title: query.trim().into(),
                aliases: Vec::new(),
                developer: None,
                release_date: None,
                limit: 10,
            }),
            "search",
        )?;
        self.match_candidates
            .retain(|_, (game_id, _)| game_id != &game.game_id);
        self.metadata_status
            .insert(game.game_id, "正在搜索…".into());
        self.diagnostic.clear();
        self.model()
    }

    pub(super) fn refresh_metadata(&mut self, provider: &str) -> Result<ManagerViewModel, String> {
        let game = self.selected_game()?;
        let provider = Self::metadata_provider(provider)?;
        let record = self
            .metadata_record(&game.game_id, provider)?
            .ok_or_else(|| "ASTRA_EMU_METADATA_NOT_LINKED".to_owned())?;
        self.link_external_id(provider.as_str(), &record.remote_id)
    }

    pub(super) fn accept_match(&mut self, candidate_id: &str) -> Result<ManagerViewModel, String> {
        let (game_id, record) = self
            .match_candidates
            .get(candidate_id)
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_METADATA_MATCH_NOT_PENDING".to_owned())?;
        if self.selected_case_id.as_deref() != Some(game_id.as_str()) {
            return Err("ASTRA_EMU_METADATA_MATCH_GAME_MISMATCH".into());
        }
        let game = self.game(&game_id)?;
        self.queue_metadata(
            &game,
            record.provider,
            MetadataCommandKind::Fetch(record.remote_id),
            "manual",
        )?;
        self.model()
    }

    pub(super) fn unlink_identity(&mut self, provider: &str) -> Result<ManagerViewModel, String> {
        let game = self.selected_game()?;
        let provider = Self::metadata_provider(provider)?;
        if self
            .pending_metadata
            .values()
            .any(|pending| pending.game_id.as_deref() == Some(game.game_id.as_str()))
        {
            return Err("ASTRA_EMU_METADATA_REQUEST_PENDING".into());
        }
        self.library
            .remove_external_identity(&game.game_id, provider.as_str())
            .map_err(|error| error.to_string())?;
        self.metadata_status
            .insert(game.game_id, "已解除关联".into());
        self.model()
    }

    pub(super) fn link_external_id(
        &mut self,
        provider: &str,
        remote_id: &str,
    ) -> Result<ManagerViewModel, String> {
        let provider = Self::metadata_provider(provider)?;
        let remote_id = remote_id.trim();
        if remote_id.is_empty() || remote_id.len() > 256 || remote_id.contains('\0') {
            return Err("ASTRA_EMU_METADATA_REMOTE_ID_INVALID".into());
        }
        let game = self.selected_game()?;
        self.queue_metadata(
            &game,
            provider,
            MetadataCommandKind::Fetch(remote_id.into()),
            "manual",
        )?;
        self.model()
    }

    pub(super) fn set_metadata_consent(
        &mut self,
        provider: &str,
        enabled: bool,
        secret: &str,
    ) -> Result<ManagerViewModel, String> {
        let provider = Self::metadata_provider(provider)?;
        if provider == MetadataProviderId::Bangumi && !secret.is_empty() {
            ManagerSecretStore::open()
                .map_err(|error| error.to_string())?
                .store("metadata.bangumi", secret)
                .map_err(|error| error.to_string())?;
            self.metadata_tokens
                .insert(provider.as_str().into(), secret.into());
        }
        self.metadata_consent
            .insert(provider.as_str().into(), enabled);
        self.model()
    }

    pub(super) fn set_sensitive_cover_policy(
        &mut self,
        provider: &str,
        enabled: bool,
    ) -> Result<ManagerViewModel, String> {
        let _ = Self::metadata_provider(provider)?;
        self.sensitive_covers = enabled;
        self.model()
    }

    pub(super) fn update_bangumi_play_status(
        &mut self,
        status: &str,
        rating: i32,
        note: &str,
    ) -> Result<ManagerViewModel, String> {
        let status_value = match status {
            "wish" => BangumiPlayStatus::Wish,
            "doing" => BangumiPlayStatus::Doing,
            "collect" => BangumiPlayStatus::Collect,
            "on_hold" => BangumiPlayStatus::OnHold,
            "dropped" => BangumiPlayStatus::Dropped,
            _ => return Err("ASTRA_EMU_BANGUMI_PLAY_STATUS_INVALID".into()),
        };
        let rating = if rating == 0 {
            None
        } else {
            Some(u8::try_from(rating).map_err(|_| "ASTRA_EMU_BANGUMI_RATING_INVALID")?)
        };
        let game = self.selected_game()?;
        let identity = self
            .library
            .external_identities(&game.game_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|identity| identity.provider == "bangumi")
            .ok_or_else(|| "ASTRA_EMU_BANGUMI_IDENTITY_REQUIRED".to_owned())?;
        let subject_id = identity
            .remote_id
            .parse::<u32>()
            .map_err(|_| "ASTRA_EMU_BANGUMI_REMOTE_ID_INVALID".to_owned())?;
        let update = BangumiPlayUpdate {
            subject_id,
            status: status_value,
            rating,
            note: (!note.trim().is_empty()).then(|| note.trim().into()),
            private: true,
        };
        update.validate().map_err(|error| error.to_string())?;
        self.queue_metadata(
            &game,
            MetadataProviderId::Bangumi,
            MetadataCommandKind::SyncBangumi(update),
            "play",
        )?;
        self.bangumi_play_status = status.into();
        self.bangumi_rating = i32::from(rating.unwrap_or_default());
        self.bangumi_note = note.into();
        self.model()
    }

    pub(super) fn refresh_compatibility(&mut self) -> Result<ManagerViewModel, String> {
        self.metadata_allowed(MetadataProviderId::Vndb)?;
        let request_id = self.next_metadata_id("compatibility")?;
        self.metadata.submit(MetadataCommand {
            request_id: request_id.clone(),
            case_identity: String::new(),
            provider: MetadataProviderId::Vndb,
            access_token: None,
            allow_sensitive_cover: false,
            kind: MetadataCommandKind::RefreshCompatibility {
                source_url: DEFAULT_COMPATIBILITY_SOURCE_URL.into(),
                cached_hash: self.compatibility_hash.clone(),
            },
        })?;
        self.pending_metadata
            .insert(request_id, PendingMetadata { game_id: None });
        self.diagnostic = "Compatibility refresh queued".into();
        self.model()
    }

    pub(super) fn fetch_releases(&mut self) -> Result<ManagerViewModel, String> {
        let game = self.selected_game()?;
        let identity = self
            .library
            .external_identities(&game.game_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|identity| identity.provider == "vndb")
            .ok_or_else(|| "ASTRA_EMU_VNDB_IDENTITY_REQUIRED".to_owned())?;
        self.queue_metadata(
            &game,
            MetadataProviderId::Vndb,
            MetadataCommandKind::FetchReleases(identity.remote_id),
            "releases",
        )?;
        self.model()
    }

    pub(super) fn pin_release(&mut self, release_id: &str) -> Result<ManagerViewModel, String> {
        let game = self.selected_game()?;
        if !self
            .releases
            .get(&game.game_id)
            .is_some_and(|values| values.iter().any(|(id, _)| id == release_id))
        {
            return Err("ASTRA_EMU_VNDB_RELEASE_INVALID".into());
        }
        self.pinned_releases.insert(game.game_id, release_id.into());
        self.diagnostic.clear();
        self.model()
    }
}
