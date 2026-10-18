use super::*;

impl AstraEmuManagerController {
    pub(super) fn launch_candidate_for_game(
        &self,
        game: &GameRecord,
    ) -> Option<&FamilyProbeCandidate> {
        let candidate = self.candidates.get(&game.game_id)?;
        if game.family_id.as_deref() != Some(candidate.report.family_id.as_str())
            || candidate.descriptor.family_id != candidate.report.family_id
            || self.registry.descriptor(&candidate.report.plugin_id) != Some(&candidate.descriptor)
        {
            return None;
        }
        Some(candidate)
    }

    pub(super) fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        self.family_options.clear();
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
        self.candidates.clear();
        self.probe_choices.clear();
        self.diagnostic.clear();
        let mut missing = 0;
        for path in &paths {
            if !path.is_dir() {
                self.clear_probe_state(&path_id(path))?;
                missing += 1;
            }
        }
        paths.retain(|path| path.is_dir());
        if !paths.is_empty() {
            self.scan_paths(&paths)?;
        }
        if missing > 0 {
            self.diagnostic = format!(
                "有 {missing} 个游戏目录当前不可用。资料库信息已保留，恢复目录后点击“扫描游戏”。"
            );
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
        if self.probe_choices.contains_key(case_id) && !self.candidates.contains_key(case_id) {
            return Err("ASTRA_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED".to_owned());
        }
        let candidate = self
            .launch_candidate_for_game(&game)
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_FAMILY_PROBE_REQUIRED".to_owned())?;
        let epoch = astra_emu_manager_core::model::allocate_session_epoch()
            .map_err(|error| error.to_string())?;
        let text = if candidate
            .descriptor
            .has_capability(astra_emu_manager_core::FamilyCapability::TextReplacement)
        {
            let profile = if self.translation_consent {
                Some(
                    self.profile()?
                        .ok_or("ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED")?,
                )
            } else {
                None
            };
            let extensions =
                astra_emu_manager_core::extensions::ExtensionStore::new(&self.data_dir).list()?;
            if profile.is_some() || extensions.iter().any(|extension| extension.enabled) {
                let secrets = std::sync::Arc::new(
                    ManagerSecretStore::open().map_err(|error| error.to_string())?,
                );
                Some(
                    TextReplacementBridge::new(epoch, profile, secrets, extensions)
                        .map_err(|error| error.to_string())?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let wake = self
            .host_wake
            .clone()
            .ok_or("ASTRA_EMU_HOST_WAKE_REQUIRED")?;
        let schema = self
            .registry
            .configuration(&candidate.descriptor.plugin_id)
            .map_err(|error| error.to_string())?;
        let configuration = self
            .library
            .family_configuration(&candidate.descriptor.plugin_id, case_id, &schema)
            .map_err(|error| error.to_string())?;
        self.mailbox
            .activate(epoch)
            .map_err(|error| error.to_string())?;
        self.registry.open(owner::OpenSession {
            epoch,
            game,
            candidate,
            configuration,
            window: self.window_state.ok_or("ASTRA_EMU_WINDOW_STATE_REQUIRED")?,
            text,
            wake,
            modal: None,
            window_service: None,
            cancellation: astra_emu_manager_core::NativeCancellation::new(epoch),
        })?;
        self.selected_case_id = Some(case_id.into());
        self.pending_events.clear();
        self.active = Some(owner::ActiveSessionView::opening(epoch, case_id.into()));
        self.diagnostic.clear();
        self.model()
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
