use super::*;

fn matches_game_directory(location: &str, path: &Path) -> bool {
    Path::new(location) == path
        || fs::canonicalize(location).is_ok_and(|directory| directory == path)
}

impl AstraEmuManagerController {
    pub(super) fn add_game_directory(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        let imported = self.scan_paths(&[path.to_owned()])?;
        if let Some(game_id) = imported.first() {
            self.family_options.clear();
            self.load_game_input_mapping(game_id)?;
            self.selected_case_id = Some(game_id.clone());
        }
        self.diagnostic = if imported.is_empty() {
            "所选目录中未发现当前核心支持的游戏。请选择原生游戏目录，或检查运行包包含的核心。"
                .into()
        } else {
            String::new()
        };
        self.model()
    }

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
        let cores_dir = data_dir.join("cores");
        fs::create_dir_all(&cores_dir).map_err(|_| "ASTRA_EMU_CORES_DIRECTORY_CREATE")?;
        let mut library =
            Library::open(data_dir.join("library.sqlite3")).map_err(|error| error.to_string())?;
        library
            .settle_abandoned_sessions(unix_time_ms()?)
            .map_err(|error| error.to_string())?;

        #[cfg(not(target_os = "android"))]
        let directories = core_directories(&cores_dir)?;
        #[cfg(target_os = "android")]
        let directories = Vec::new();
        let (registry, plugin_errors) = owner::FamilyOwner::start(directories, mailbox.clone())?;

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
            plugin_errors,
            data_dir,
            _extensions: crate::text_service::ExtensionRuntime::new().map_err(|error| error.to_string())?,
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
            .find(|game| matches_game_directory(&game.location, path))
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
        let game_id = self.upsert_scanned_game(path, None)?;
        self.clear_probe_state(&game_id)?;
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

    pub(super) fn clear_probe_state(&mut self, game_id: &str) -> Result<(), String> {
        self.candidates.remove(game_id);
        self.probe_choices.remove(game_id);
        if self.selected_case_id.as_deref() == Some(game_id) {
            self.family_options.remove("family.plugin_id");
        }
        if self
            .library
            .game(game_id)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            self.library
                .set_game_family(game_id, None)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn upsert_scanned_game(
        &mut self,
        path: &Path,
        family_id: Option<&str>,
    ) -> Result<String, String> {
        let location = path
            .to_str()
            .ok_or_else(|| "ASTRA_EMU_GAME_PATH_UTF8".to_owned())?
            .to_owned();
        let mut game = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|game| matches_game_directory(&game.location, path))
            .max_by_key(|game| game.location == location)
            .unwrap_or(GameRecord {
                game_id: path_id(path),
                title: title_for_path(path),
                user_title: None,
                location: location.clone(),
                family_id: None,
                content_fingerprint: None,
                added_at_unix_ms: unix_time_ms()?,
            });
        let game_id = game.game_id.clone();
        game.title = title_for_path(path);
        game.family_id = family_id.map(str::to_owned);
        self.library
            .add_game(&game)
            .map_err(|error| error.to_string())?;
        if self.selected_case_id.as_deref() == Some(game_id.as_str()) {
            self.family_options.remove("family.plugin_id");
        }
        Ok(game_id)
    }

    pub(super) fn scan_paths(&mut self, paths: &[PathBuf]) -> Result<Vec<String>, String> {
        let mut imported = BTreeSet::new();
        let known_games = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?;
        let mut roots = paths
            .iter()
            .map(|root| {
                fs::canonicalize(root).map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_INVALID".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        roots.sort();
        roots.dedup();
        let mut visited = BTreeSet::new();
        let mut game_directories = Vec::<PathBuf>::new();
        for root in roots {
            if !root.is_dir() {
                return Err("ASTRA_EMU_GAME_DIRECTORY_NOT_DIRECTORY".into());
            }
            let mut queue = vec![(root, 0_usize)];
            while let Some((path, depth)) = queue.pop() {
                if game_directories.iter().any(|game| path.starts_with(game))
                    || !visited.insert(path.clone())
                {
                    continue;
                }
                if visited.len() > MAX_SCAN_DIRECTORIES {
                    return Err("ASTRA_EMU_GAME_DIRECTORY_BOUNDS".into());
                }
                let selection = self.probe_directory(&path)?;
                match selection {
                    FamilyProbeSelection::Selected(candidate) => {
                        let game_id =
                            self.upsert_scanned_game(&path, Some(&candidate.report.family_id))?;
                        self.probe_choices.remove(&game_id);
                        imported.insert(game_id.clone());
                        for game in known_games
                            .iter()
                            .filter(|game| matches_game_directory(&game.location, &path))
                        {
                            self.library
                                .set_game_family(&game.game_id, Some(&candidate.report.family_id))
                                .map_err(|error| error.to_string())?;
                            self.probe_choices.remove(&game.game_id);
                            self.candidates
                                .insert(game.game_id.clone(), (*candidate).clone());
                        }
                        self.candidates.insert(game_id, *candidate);
                        game_directories.push(path);
                    }
                    FamilyProbeSelection::NoMatch => {
                        for game in known_games
                            .iter()
                            .filter(|game| matches_game_directory(&game.location, &path))
                        {
                            self.clear_probe_state(&game.game_id)?;
                        }
                        if depth < MAX_SCAN_DEPTH {
                            for entry in fs::read_dir(&path)
                                .map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION".to_owned())?
                            {
                                let entry = entry.map_err(|_| {
                                    "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION".to_owned()
                                })?;
                                let kind = entry.file_type().map_err(|_| {
                                    "ASTRA_EMU_GAME_DIRECTORY_ENUMERATION".to_owned()
                                })?;
                                if kind.is_dir() && !kind.is_symlink() {
                                    queue.push((entry.path(), depth + 1));
                                }
                            }
                        }
                    }
                    FamilyProbeSelection::RequiresUserChoice(candidates) => {
                        imported.insert(self.remember_probe_choice(&path, candidates.clone())?);
                        for game in known_games
                            .iter()
                            .filter(|game| matches_game_directory(&game.location, &path))
                        {
                            self.clear_probe_state(&game.game_id)?;
                            self.probe_choices
                                .insert(game.game_id.clone(), candidates.clone());
                        }
                        game_directories.push(path);
                        self.diagnostic =
                            "Multiple family providers matched a game; choose a provider in Family configuration.".into();
                    }
                }
            }
        }
        Ok(imported.into_iter().collect())
    }
}
