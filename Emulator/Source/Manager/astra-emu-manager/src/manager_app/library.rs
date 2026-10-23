use super::*;

impl AstraEmuManagerController {
    /// Persist the directory as a library root, then import it (loading cores first if needed).
    pub(super) fn add_game_directory(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        self.ensure_owner_idle()?;
        if self.startup_busy() {
            return Err("ASTRA_EMU_PREPARE_BUSY".into());
        }
        if let background::StartupState::Failed(_) = self.startup_state {
            return Err("ASTRA_EMU_PREPARE_RESTART_REQUIRED".into());
        }
        if self.patch_job.is_some() {
            return Err("ASTRA_EMU_PATCH_INVALID_REQUEST: 文件补丁转换期间不能扫描游戏".into());
        }
        let root = fs::canonicalize(path)
            .ok()
            .filter(|root| root.is_dir())
            .ok_or("ASTRA_EMU_GAME_DIRECTORY_INVALID")?;
        if astra_emu_patch::is_staging_path(&root) {
            return Err(
                "ASTRA_EMU_PATCH_INVALID_REQUEST: private staging cannot be imported".into(),
            );
        }
        self.library
            .add_library_root(root.to_str().ok_or("ASTRA_EMU_GAME_PATH_UTF8")?)
            .map_err(|error| error.to_string())?;
        if self.cores_started() {
            self.begin_scan(probe_worker::ScanKind::Import, vec![root])?;
            self.model()
        } else {
            let model = self.prepare_library()?;
            self.prepare_scan_pending = Some(background::PendingScan::Import(root));
            Ok(model)
        }
    }

    /// Persist a library root without loading cores or scanning.
    pub(super) fn add_library_root(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        let root = fs::canonicalize(path)
            .ok()
            .filter(|root| root.is_dir())
            .ok_or("ASTRA_EMU_GAME_DIRECTORY_INVALID")?;
        self.library
            .add_library_root(root.to_str().ok_or("ASTRA_EMU_GAME_PATH_UTF8")?)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    /// Stop scanning a root; its games stay in the library.
    pub(super) fn remove_library_root(&mut self, path: &str) -> Result<ManagerViewModel, String> {
        self.library
            .remove_library_root(path)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn onboarding_completed(&self) -> Result<bool, String> {
        Ok(self
            .library
            .app_state::<bool>(ONBOARDING_COMPLETED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false))
    }

    pub(super) fn finish_onboarding(&mut self) -> Result<ManagerViewModel, String> {
        self.library
            .set_app_state(ONBOARDING_COMPLETED_KEY, &true)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn game_location(&self, game_id: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from(self.game(game_id)?.location))
    }

    pub(super) fn remove_from_library(&mut self, game_id: &str) -> Result<ManagerViewModel, String> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.game_id == game_id)
        {
            self.ensure_owner_idle()?;
        }
        let game = self.game(game_id)?;
        self.library
            .exclude_game(game_id)
            .map_err(|error| error.to_string())?;
        self.auto_link_queue.retain(|id| id != game_id);
        if self.selected_case_id.as_deref() == Some(game_id) {
            self.selected_case_id = None;
        }
        self.last_excluded = Some(game_id.to_owned());
        self.show_snackbar(
            format!("已从资料库移除《{}》", game.display_title()),
            Some(snackbar::SnackbarAction::Undo),
        );
        self.model()
    }

    pub(super) fn undo_remove(&mut self) -> Result<ManagerViewModel, String> {
        let game_id = self
            .last_excluded
            .clone()
            .ok_or("ASTRA_EMU_UNDO_UNAVAILABLE")?;
        self.restore_game(&game_id)
    }

    pub(super) fn restore_game(&mut self, game_id: &str) -> Result<ManagerViewModel, String> {
        if !self
            .library
            .restore_game(game_id)
            .map_err(|error| error.to_string())?
        {
            return Err("ASTRA_EMU_GAME_NOT_FOUND".into());
        }
        if self.last_excluded.as_deref() == Some(game_id) {
            self.last_excluded = None;
        }
        self.model()
    }

    pub(super) fn first_visible_game(&self) -> Result<Option<String>, String> {
        let excluded: BTreeSet<String> = self
            .library
            .excluded_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| game.game_id)
            .collect();
        Ok(self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| game.game_id)
            .find(|id| !excluded.contains(id)))
    }

    pub(super) fn open(mailbox: FrameMailbox) -> Result<Self, String> {
        Self::open_library(platform_data_dir()?, mailbox, parse_game_roots())
    }

    pub(super) fn open_library(
        data_dir: PathBuf,
        mailbox: FrameMailbox,
        game_roots: Vec<PathBuf>,
    ) -> Result<Self, String> {
        fs::create_dir_all(&data_dir).map_err(|_| "ASTRA_EMU_DATA_DIRECTORY_CREATE")?;
        let mut library =
            Library::open(data_dir.join("library.sqlite3")).map_err(|error| error.to_string())?;
        library
            .settle_abandoned_sessions(unix_time_ms()?)
            .map_err(|error| error.to_string())?;

        let registry = owner::FamilyOwner::new(data_dir.clone(), mailbox.clone());

        let input_mapping = library
            .load_input_mapping()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(default_vn_preset);
        let filter_settings = library
            .filter_settings()
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let metadata_catalog = metadata_cache::select_catalog(
            &data_dir,
            metadata_cache::bundled_directory().as_deref(),
        );
        let metadata_sync_record = library
            .app_state::<metadata_sync::SyncRecord>(metadata_sync::METADATA_SYNC_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let metadata = MetadataRuntime::start()?;
        let auto_link_attempted = library
            .app_state::<BTreeSet<String>>(metadata_auto_link::AUTO_LINK_ATTEMPTED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let appearance = library.appearance_settings().map_err(|e| e.to_string())?;
        let games = library.list_games().map_err(|error| error.to_string())?;
        let mut pinned_releases = BTreeMap::new();
        for game in &games {
            if let Some(release) = library
                .game_release(&game.game_id)
                .map_err(|error| error.to_string())?
            {
                pinned_releases.insert(game.game_id.clone(), release);
            }
        }
        let network_allowed = library
            .app_state::<bool>(NETWORK_ALLOWED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let translation_enabled = library
            .app_state::<bool>(TRANSLATION_ENABLED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let sensitive_covers = library
            .app_state::<bool>(SENSITIVE_COVERS_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let mut controller = Self {
            library,
            registry,
            candidates: BTreeMap::new(),
            startup_state: background::StartupState::Waiting,
            probe_choices: BTreeMap::new(),
            selected_case_id: None,
            search_query: String::new(),
            diagnostic: String::new(),
            diagnostic_severity: "info",
            plugin_errors: BTreeMap::new(),
            data_dir,
            extensions: None,
            game_roots,
            mailbox,
            active: None,
            active_play_session: None,
            pending_events: Vec::new(),
            window_state: None,
            host_wake: None,
            physical_keys: Vec::new(),
            library_sort: "title".into(),
            compatibility_filter: "all".into(),
            metadata_catalog,
            metadata_sync: metadata_sync::SyncState {
                record: metadata_sync_record,
                request: None,
            },
            pinned_releases,
            filter_settings,
            input_mapping,
            appearance,
            session_mapping: None,
            defaults_plugin: None,
            metadata,
            metadata_sequence: 0,
            metadata_status: BTreeMap::new(),
            pending_metadata: BTreeMap::new(),
            match_candidates: BTreeMap::new(),
            auto_link_queue: VecDeque::new(),
            auto_link: None,
            auto_link_attempted,
            network_allowed,
            metadata_tokens: BTreeMap::new(),
            metadata_tokens_loaded: false,
            sensitive_covers,
            bangumi_play_status: "wish".into(),
            bangumi_rating: 0,
            bangumi_note: String::new(),
            releases: BTreeMap::new(),
            translation_enabled,
            translation_models: Vec::new(),
            translation_status: String::new(),
            connection_test: None,
            held_controls: BTreeSet::new(),
            key_modifiers: KeyModifiers {
                shift: false,
                control: false,
                alt: false,
                super_key: false,
            },
            pointer_position: (0.0, 0.0),
            action_message: String::new(),
            action_serial: 0,
            patch_job: None,
            patch_import: None,
            patch_state: Default::default(),
            patch_sequence: 0,
            snackbar: snackbar::Snackbar::default(),
            prepare_scan_pending: None,
            prepare_progress: None,
            scan_cancel: None,
            last_excluded: None,
            key_capture: None,
        };
        controller.selected_case_id = controller.first_visible_game()?;
        Ok(controller)
    }

    pub(super) fn game(&self, game_id: &str) -> Result<GameRecord, String> {
        self.library
            .game(game_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "ASTRA_EMU_GAME_NOT_FOUND".into())
    }

    pub(super) fn selected_game(&self) -> Result<GameRecord, String> {
        self.selected_case_id
            .as_deref()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())
            .and_then(|id| self.game(id))
    }

    pub(super) fn set_probe_candidates(
        &mut self,
        game_id: &str,
        choices: Vec<FamilyProbeCandidate>,
    ) -> Result<(), String> {
        self.clear_probe_state(game_id)?;
        let saved = self
            .library
            .game_settings(game_id)
            .map_err(|error| error.to_string())?
            .and_then(|settings| settings.emulator_id);
        let selected = match saved {
            Some(id) => choices.iter().find(|choice| choice.report.plugin_id == id),
            None if choices.len() == 1 => choices.first(),
            None => None,
        }
        .map(|choice| choice.report.plugin_id.clone());
        if selected.is_none() {
            if let Some(first) = choices.first().filter(|first| {
                choices
                    .iter()
                    .all(|choice| choice.report.family_id == first.report.family_id)
            }) {
                self.library
                    .set_game_family(game_id, Some(&first.report.family_id))
                    .map_err(|error| error.to_string())?;
            }
        }
        self.probe_choices.insert(game_id.to_owned(), choices);
        if let Some(plugin_id) = selected {
            self.install_probe_choice(game_id, &plugin_id)?;
        }
        Ok(())
    }

    pub(super) fn install_probe_choice(
        &mut self,
        game_id: &str,
        plugin_id: &str,
    ) -> Result<(), String> {
        let candidate = self
            .probe_choices
            .get(game_id)
            .and_then(|choices| {
                choices
                    .iter()
                    .find(|choice| choice.report.plugin_id == plugin_id)
            })
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_EMULATOR_SELECTION_INVALID".to_owned())?;
        self.library
            .select_game_emulator(
                game_id,
                &candidate.report.family_id,
                &candidate.report.plugin_id,
            )
            .map_err(|error| error.to_string())?;
        self.candidates.insert(game_id.to_owned(), candidate);
        Ok(())
    }

    pub(super) fn probe_selection_for_game(
        &self,
        game_id: &str,
    ) -> Option<&[FamilyProbeCandidate]> {
        self.probe_choices.get(game_id).map(Vec::as_slice)
    }

    pub(super) fn clear_probe_state(&mut self, game_id: &str) -> Result<(), String> {
        self.candidates.remove(game_id);
        self.probe_choices.remove(game_id);
        Ok(())
    }
}
