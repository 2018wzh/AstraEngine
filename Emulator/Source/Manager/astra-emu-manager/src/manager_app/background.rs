use super::*;
use probe_worker::{ScanKind, ScanReport, ScanRequest};

pub(super) enum StartupState {
    Waiting,
    Loading,
    Scanning(ScanKind),
    Ready,
    Failed(String),
}

impl AstraEmuManagerController {
    pub(super) fn startup_busy(&self) -> bool {
        matches!(
            self.startup_state,
            StartupState::Loading | StartupState::Scanning(_)
        )
    }

    pub(super) fn cores_ready(&self) -> bool {
        self.registry.is_ready() && self.registry.descriptors().next().is_some()
    }

    pub(super) fn core_refresh_enabled(&self) -> bool {
        matches!(self.startup_state, StartupState::Waiting)
    }

    pub(super) fn startup_status(&self) -> String {
        match &self.startup_state {
            StartupState::Waiting => "点击“刷新核心”加载模拟器核心，再扫描或添加游戏。".into(),
            StartupState::Loading => "正在加载模拟器核心…".into(),
            StartupState::Scanning(ScanKind::Import) => "正在导入游戏目录…".into(),
            StartupState::Scanning(_) => "正在扫描游戏目录…".into(),
            StartupState::Failed(error) if self.registry.is_ready() => {
                format!("游戏扫描失败：{error}")
            }
            StartupState::Failed(error) => {
                format!("核心加载或后台运行失败：{error}。请修复后重启应用。")
            }
            StartupState::Ready if self.registry.descriptors().next().is_none() => {
                "未加载模拟器核心，请检查核心安装和加载错误后重启应用。".into()
            }
            StartupState::Ready if !self.plugin_errors.is_empty() => format!(
                "有 {} 个核心加载失败，请查看核心诊断；修复或更换核心后需重启应用。",
                self.plugin_errors.len()
            ),
            StartupState::Ready => {
                "核心已加载，可扫描或添加游戏。更换核心文件后需重启应用。".into()
            }
        }
    }

    pub(super) fn refresh_cores(&mut self) -> Result<ManagerViewModel, String> {
        if !self.core_refresh_enabled() {
            return Err("ASTRA_EMU_OWNER_ALREADY_STARTED".into());
        }
        self.startup_state = StartupState::Loading;
        if let Err(error) = self.registry.start() {
            self.background_failed(error.clone());
            return Err(error);
        }
        self.model()
    }

    pub(super) fn begin_scan(&mut self, kind: ScanKind, paths: Vec<PathBuf>) -> Result<(), String> {
        if self.startup_busy() {
            return Err("ASTRA_EMU_FAMILY_INITIALIZING".into());
        }
        if !self.cores_ready() {
            return Err("ASTRA_EMU_CORES_NOT_READY".into());
        }
        let games = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?;
        self.registry.scan(ScanRequest { kind, paths, games })?;
        self.startup_state = StartupState::Scanning(kind);
        self.diagnostic.clear();
        Ok(())
    }

    pub(super) fn poll_background(&mut self) -> Result<bool, String> {
        let mut changed = false;
        while let Some(event) = self.registry.take_background() {
            changed = true;
            match event {
                owner::BackgroundEvent::Ready(Ok((_, errors))) => {
                    self.plugin_errors = errors;
                    if matches!(self.startup_state, StartupState::Loading) {
                        self.startup_state = StartupState::Ready;
                    }
                }
                owner::BackgroundEvent::Ready(Err(error)) => self.background_failed(error),
                owner::BackgroundEvent::Scanned(kind, result) => match result {
                    Ok(report) => {
                        self.startup_state = StartupState::Ready;
                        if let Err(error) = self.apply_scan_report(kind, report) {
                            self.background_failed(error.clone());
                            return Err(error);
                        }
                    }
                    Err(error) => self.background_failed(error),
                },
                owner::BackgroundEvent::Exited(error) => {
                    if !matches!(self.startup_state, StartupState::Failed(_)) {
                        self.background_failed(error);
                    }
                }
            }
        }
        Ok(changed)
    }

    fn background_failed(&mut self, error: String) {
        self.diagnostic.clone_from(&error);
        self.startup_state = StartupState::Failed(error);
    }

    fn apply_scan_report(&mut self, kind: ScanKind, report: ScanReport) -> Result<(), String> {
        tracing::info!(
            event = "astra.emu.manager.probe_refresh",
            games = report.games.len(),
            missing = report.missing,
            cache_hits = report.cache_hits
        );
        if let Some(error) = report.cache_error {
            tracing::warn!(event = "astra.emu.manager.probe_cache_failed", diagnostic_code = %error);
        }
        let mut imported = BTreeSet::new();
        for result in report.games {
            let mut ids = result.game_ids;
            if result.candidates.is_empty() {
                for id in ids {
                    self.clear_probe_state(&id)?;
                }
                continue;
            }
            if ids.is_empty() {
                let location = result.path.to_str().ok_or("ASTRA_EMU_GAME_PATH_UTF8")?;
                let game = GameRecord {
                    game_id: path_id(&result.path),
                    title: title_for_path(&result.path),
                    user_title: None,
                    location: location.into(),
                    family_id: None,
                    content_fingerprint: None,
                    added_at_unix_ms: unix_time_ms()?,
                };
                self.library
                    .add_game(&game)
                    .map_err(|error| error.to_string())?;
                ids.push(game.game_id);
            }
            for id in ids {
                self.set_probe_candidates(&id, result.candidates.clone())?;
                imported.insert(id);
            }
        }
        if kind == ScanKind::Import {
            if let Some(id) = imported.first() {
                self.family_options.clear();
                self.load_game_input_mapping(id)?;
                self.selected_case_id = Some(id.clone());
            }
            if imported.is_empty() {
                self.diagnostic = "所选目录中未发现当前核心支持的游戏。请选择原生游戏目录，或检查运行包包含的核心。".into();
            }
        }
        if report.missing > 0 {
            self.diagnostic = format!(
                "有 {} 个游戏目录当前不可用。资料库信息已保留，恢复目录后点击“扫描游戏”。",
                report.missing
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
        Ok(())
    }
}
