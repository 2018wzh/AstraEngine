use super::*;

impl AstraEmuManagerController {
    pub(super) fn open(mailbox: FrameMailbox) -> Result<Self, String> {
        let mut controller = Self::open_library(platform_data_dir()?, mailbox, parse_game_roots())?;
        controller.rescan()?;
        Ok(controller)
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

        let mut registry = FamilyProviderRegistry::new();
        registry
            .register_provider(astra_emu_fvp::FvpProvider::default())
            .map_err(|error| error.to_string())?;
        for installed in library
            .list_installed_plugins()
            .map_err(|e| e.to_string())?
        {
            registry
                .load_dynamic(&installed.location)
                .map_err(|e| e.to_string())?;
            let descriptor = registry
                .descriptor(&installed.plugin_id)
                .ok_or("ASTRA_EMU_PLUGIN_INSTALL_ID_MISMATCH")?;
            if descriptor.family_id != installed.family_id
                || descriptor.abi_fingerprint != installed.abi_fingerprint
                || descriptor.version != installed.version
                || descriptor.capabilities != installed.capabilities
                || descriptor.supported_formats != installed.supported_formats
            {
                return Err("ASTRA_EMU_PLUGIN_INSTALL_DESCRIPTOR_CHANGED".into());
            }
        }

        let input_mapping = library
            .load_input_mapping()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(default_vn_preset);
        let filter_settings = library
            .filter_settings()
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let (compatibility_database, compatibility_hash) = load_compatibility_cache(&data_dir)?;
        let metadata = MetadataRuntime::start()?;
        let appearance = library.appearance_settings().map_err(|e| e.to_string())?;
        let controller = Self {
            library,
            registry,
            candidates: BTreeMap::new(),
            probe_choices: BTreeMap::new(),
            selected_case_id: None,
            search_query: String::new(),
            diagnostic: String::new(),
            data_dir,
            game_roots,
            mailbox,
            active: None,
            audio_device: audio_executor::AudioDeviceKind::Native,
            active_play_session: None,
            pending_events: Vec::new(),
            window_state: None,
            host_wake: None,
            physical_keys: Vec::new(),
            library_sort: "title".into(),
            compatibility_filter: "all".into(),
            compatibility_database,
            compatibility_hash,
            compatibility_fetched_at_unix_ms: None,
            pinned_releases: BTreeMap::new(),
            filter_settings,
            input_mapping,
            input_config: InputConfigViewModel::default(),
            appearance,
            family_options: BTreeMap::new(),
            filter_options: BTreeMap::new(),
            metadata,
            metadata_sequence: 0,
            metadata_status: BTreeMap::new(),
            pending_metadata: BTreeMap::new(),
            match_candidates: BTreeMap::new(),
            metadata_consent: BTreeMap::new(),
            metadata_tokens: BTreeMap::new(),
            sensitive_covers: false,
            bangumi_play_status: "wish".into(),
            bangumi_rating: 0,
            bangumi_note: String::new(),
            releases: BTreeMap::new(),
            translation_consent: false,
            connection_test: None,
            held_controls: BTreeSet::new(),
            key_modifiers: KeyModifiers {
                shift: false,
                control: false,
                alt: false,
                super_key: false,
            },
            pointer_position: (0.0, 0.0),
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

    pub(super) fn probe_directory(&mut self, path: &Path) -> Result<FamilyProbeSelection, String> {
        let game_path = path
            .to_str()
            .ok_or_else(|| "ASTRA_EMU_GAME_PATH_UTF8".to_owned())?;
        let preferred_family_id = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|game| game.location == game_path)
            .and_then(|game| game.family_id);
        self.registry
            .probe(
                &ProbeRequest {
                    game_path: game_path.to_owned().into(),
                },
                None,
                preferred_family_id.as_deref(),
            )
            .map_err(|error| error.to_string())
    }

    pub(super) fn remember_probe_choice(
        &mut self,
        path: &Path,
        candidates: Vec<FamilyProbeCandidate>,
    ) -> Result<String, String> {
        let game_id = path_id(path);
        let location = path
            .to_str()
            .ok_or_else(|| "ASTRA_EMU_GAME_PATH_UTF8".to_owned())?
            .to_owned();
        self.library
            .add_game(&GameRecord {
                game_id: game_id.clone(),
                title: title_for_path(path),
                user_title: None,
                location,
                family_id: None,
                content_fingerprint: None,
                added_at_unix_ms: unix_time_ms()?,
            })
            .map_err(|error| error.to_string())?;
        self.probe_choices.insert(game_id.clone(), candidates);
        Ok(game_id)
    }

    pub(super) fn install_probe_choice(
        &mut self,
        game_id: &str,
        plugin_id: &str,
    ) -> Result<(), String> {
        let choices = self
            .probe_choices
            .get(game_id)
            .ok_or_else(|| "ASTRA_EMU_FAMILY_PROBE_NOT_AMBIGUOUS".to_owned())?;
        let candidate = choices
            .iter()
            .find(|candidate| candidate.report.plugin_id == plugin_id)
            .cloned()
            .ok_or_else(|| "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_INVALID".to_owned())?;
        self.library
            .set_game_family(game_id, Some(&candidate.report.family_id))
            .map_err(|error| error.to_string())?;
        self.candidates.insert(game_id.to_owned(), candidate);
        self.probe_choices.remove(game_id);
        Ok(())
    }

    pub(super) fn probe_selection_for_game(
        &self,
        game_id: &str,
    ) -> Option<&[FamilyProbeCandidate]> {
        self.probe_choices.get(game_id).map(Vec::as_slice)
    }

    pub(super) fn scan_paths(&mut self, paths: &[PathBuf]) -> Result<(), String> {
        for root in paths {
            for path in enumerate_directories(root)? {
                let selection = self.probe_directory(&path)?;
                let location = path
                    .to_str()
                    .ok_or_else(|| "ASTRA_EMU_GAME_PATH_UTF8".to_owned())?
                    .to_owned();
                let game_id = path_id(&path);
                match selection {
                    FamilyProbeSelection::Selected(candidate) => {
                        self.library
                            .add_game(&GameRecord {
                                game_id: game_id.clone(),
                                title: title_for_path(&path),
                                user_title: None,
                                location,
                                family_id: Some(candidate.report.family_id.clone()),
                                content_fingerprint: None,
                                added_at_unix_ms: unix_time_ms()?,
                            })
                            .map_err(|error| error.to_string())?;
                        self.probe_choices.remove(&game_id);
                        self.candidates.insert(game_id, *candidate);
                    }
                    FamilyProbeSelection::NoMatch => {}
                    FamilyProbeSelection::RequiresUserChoice(candidates) => {
                        self.remember_probe_choice(&path, candidates)?;
                        self.diagnostic =
                            "Multiple family providers matched a game; choose a provider in Family configuration.".into();
                    }
                }
            }
        }
        Ok(())
    }
}
