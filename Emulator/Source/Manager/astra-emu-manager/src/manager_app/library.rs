use super::*;

impl AstraEmuManagerController {
    pub(super) fn add_game_directory(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        self.begin_scan(probe_worker::ScanKind::Import, vec![path.to_owned()])?;
        self.model()
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
        let appearance = library.appearance_settings().map_err(|e| e.to_string())?;
        let games = library.list_games().map_err(|error| error.to_string())?;
        let selected_case_id = games.first().map(|game| game.game_id.clone());
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
        let controller = Self {
            library,
            registry,
            candidates: BTreeMap::new(),
            startup_state: background::StartupState::Waiting,
            probe_choices: BTreeMap::new(),
            selected_case_id,
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
        };
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
