use super::*;

impl AstraEmuManagerController {
    pub(super) fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        if case_id.is_empty() {
            self.selected_case_id = None;
        } else {
            self.game(case_id)?;
            self.load_game_input_mapping(case_id)?;
            self.selected_case_id = Some(case_id.into());
        }
        self.model()
    }

    pub(super) fn search(&mut self, query: &str) -> Result<ManagerViewModel, String> {
        if query.chars().count() > 256 {
            return Err("ASTRA_EMU_SEARCH_QUERY_BOUNDS".into());
        }
        self.search_query = query.into();
        self.model()
    }

    pub(super) fn rescan(&mut self) -> Result<ManagerViewModel, String> {
        let mut paths = self.game_roots.clone();
        paths.extend(
            self.library
                .list_games()
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|game| PathBuf::from(game.location)),
        );
        paths.sort();
        paths.dedup();
        self.diagnostic.clear();
        if !paths.is_empty() {
            self.scan_paths(&paths)?;
        }
        if self.selected_case_id.is_none() {
            self.selected_case_id = self
                .library
                .list_games()
                .map_err(|error| error.to_string())?
                .first()
                .map(|game| game.game_id.clone());
        }
        self.model()
    }

    pub(super) fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        let result = self.try_launch(case_id);
        if let Err(error) = &result {
            self.diagnostic.clone_from(error);
        }
        result
    }

    fn try_launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        tracing::info!(event = "astra.emu.manager.launch.opening");
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        let game = self.game(case_id)?;
        self.load_game_input_mapping(case_id)?;
        let candidate = self
            .candidates
            .get(case_id)
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_FAMILY_PROBE_REQUIRED".to_owned())?;
        let text = if self.translation_consent
            && candidate
                .descriptor
                .has_capability(astra_emu_manager_core::FamilyCapability::TextReplacement)
        {
            let profile = self
                .profile()?
                .ok_or("ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED")?;
            let secrets =
                std::sync::Arc::new(ManagerSecretStore::open().map_err(|error| error.to_string())?);
            Some(TextReplacementBridge::new(profile, secrets).map_err(|error| error.to_string())?)
        } else {
            None
        };
        let wake = self
            .host_wake
            .clone()
            .ok_or("ASTRA_EMU_HOST_WAKE_REQUIRED")?;
        let audio = candidate
            .descriptor
            .has_capability(astra_emu_manager_core::FamilyCapability::PcmAudio)
            .then(|| audio_executor::HostAudioExecutor::new(self.audio_device, Some(wake)));
        let active = ActiveFamilySession::open(
            &game,
            &candidate,
            &mut self.registry,
            self.mailbox.clone(),
            self.window_state.ok_or("ASTRA_EMU_WINDOW_STATE_REQUIRED")?,
            text,
            audio,
        )?;
        match self.library.start_play_session(case_id, unix_time_ms()?) {
            Ok(session_id) => {
                self.selected_case_id = Some(case_id.into());
                self.pending_events.clear();
                self.active = Some(active);
                self.active_play_session = Some(session_id);
                self.diagnostic.clear();
                match self.model() {
                    Ok(model) => Ok(model),
                    Err(error) => match self.close_active(PlaySessionEndReason::Crash) {
                        Ok(()) => Err(error),
                        Err(cleanup) => Err(format!("{error}; {cleanup}")),
                    },
                }
            }
            Err(error) => {
                let mut active = active;
                match active.close() {
                    Ok(()) => Err(error.to_string()),
                    Err(cleanup) => Err(format!("{error}; {cleanup}")),
                }
            }
        }
    }

    pub(super) fn leave_game(&mut self) -> Result<ManagerViewModel, String> {
        self.close_active(PlaySessionEndReason::Leave)?;
        self.pending_events.clear();
        self.model()
    }

    pub(super) fn set_library_sort(&mut self, mode: &str) -> Result<ManagerViewModel, String> {
        if !matches!(mode, "title" | "recent" | "play_time") {
            return Err("ASTRA_EMU_LIBRARY_SORT_INVALID".into());
        }
        self.library_sort = mode.into();
        self.model()
    }

    pub(super) fn set_compatibility_filter(
        &mut self,
        filter: &str,
    ) -> Result<ManagerViewModel, String> {
        if !matches!(
            filter,
            "all" | "unknown" | "perfect" | "completable" | "flawed" | "boot_only" | "unplayable"
        ) {
            return Err("ASTRA_EMU_COMPATIBILITY_FILTER_INVALID".into());
        }
        self.compatibility_filter = filter.into();
        self.model()
    }
}
