use super::*;
use owner::PrepareProgress;
use probe_worker::{ScanKind, ScanReport, ScanRequest};
use std::sync::{atomic::AtomicBool, Arc};

pub(super) enum StartupState {
    Waiting,
    Loading,
    Scanning(ScanKind),
    Ready,
    Failed(String),
}

/// Scan started automatically once core loading finishes.
pub(super) enum PendingScan {
    Full,
    Import(PathBuf),
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

    pub(super) fn cores_started(&self) -> bool {
        !matches!(self.startup_state, StartupState::Waiting)
    }

    pub(super) fn startup_status(&self) -> String {
        match &self.startup_state {
            StartupState::Waiting => "点击“准备”加载模拟器核心并扫描资料库。".into(),
            StartupState::Loading => match self.prepare_progress {
                Some(PrepareProgress::Cores { loaded, total }) => {
                    format!("正在加载模拟器核心（{loaded}/{total}）…")
                }
                _ => "正在加载模拟器核心…".into(),
            },
            StartupState::Scanning(kind) => {
                let action = if *kind == ScanKind::Import {
                    "正在导入游戏目录"
                } else {
                    "正在扫描资料库"
                };
                match self.prepare_progress {
                    Some(PrepareProgress::Scan { directories, games }) => {
                        format!("{action}：已检查 {directories} 个目录，识别 {games} 部作品…")
                    }
                    _ => format!("{action}…"),
                }
            }
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
            StartupState::Ready => "资料库已准备好。更换核心文件后需重启应用。".into(),
        }
    }

    pub(super) fn prepare_view(&self) -> (&'static str, &'static str, Option<f32>, bool) {
        let progress = match (&self.startup_state, self.prepare_progress) {
            (StartupState::Loading, Some(PrepareProgress::Cores { loaded, total }))
                if total > 0 =>
            {
                Some(loaded as f32 / total as f32)
            }
            _ => None,
        };
        match self.startup_state {
            StartupState::Waiting => ("unprepared", "准备", None, false),
            StartupState::Loading => ("loading-cores", "准备", progress, false),
            StartupState::Scanning(_) => ("scanning", "刷新资料库", None, true),
            StartupState::Ready => ("ready", "刷新资料库", None, false),
            StartupState::Failed(_) => ("failed", "准备", None, false),
        }
    }

    /// Load cores on first use, then scan every configured root.
    pub(super) fn prepare_library(&mut self) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        match self.startup_state {
            StartupState::Waiting => {
                self.refresh_cores()?;
                self.prepare_scan_pending = Some(PendingScan::Full);
            }
            StartupState::Ready => self.begin_scan(ScanKind::Full, self.scan_roots()?)?,
            StartupState::Loading | StartupState::Scanning(_) => {
                return Err("ASTRA_EMU_PREPARE_BUSY".into());
            }
            StartupState::Failed(_) => return Err("ASTRA_EMU_PREPARE_RESTART_REQUIRED".into()),
        }
        self.model()
    }

    pub(super) fn cancel_prepare(&mut self) -> Result<ManagerViewModel, String> {
        if !matches!(self.startup_state, StartupState::Scanning(_)) {
            return Err("ASTRA_EMU_PREPARE_NOT_CANCELLABLE".into());
        }
        if let Some(cancel) = &self.scan_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        self.model()
    }

    pub(super) fn scan_roots(&self) -> Result<Vec<PathBuf>, String> {
        let mut roots: BTreeSet<PathBuf> = self.game_roots.iter().cloned().collect();
        roots.extend(
            self.library
                .library_roots()
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|root| PathBuf::from(root.path)),
        );
        Ok(roots.into_iter().collect())
    }

    fn refresh_cores(&mut self) -> Result<(), String> {
        if self.cores_started() {
            return Err("ASTRA_EMU_OWNER_ALREADY_STARTED".into());
        }
        self.startup_state = StartupState::Loading;
        self.prepare_progress = None;
        if let Err(error) = self.registry.start() {
            self.background_failed(error.clone());
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn begin_scan(&mut self, kind: ScanKind, paths: Vec<PathBuf>) -> Result<(), String> {
        if self.patch_job.is_some() {
            return Err("ASTRA_EMU_PATCH_INVALID_REQUEST: 文件补丁转换期间不能扫描游戏".into());
        }
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
        let excluded = self
            .library
            .excluded_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| PathBuf::from(game.location))
            .collect();
        let cancel = Arc::new(AtomicBool::new(false));
        self.registry.scan(ScanRequest {
            kind,
            paths,
            games,
            excluded,
            cancel: cancel.clone(),
        })?;
        self.scan_cancel = Some(cancel);
        self.prepare_progress = None;
        self.startup_state = StartupState::Scanning(kind);
        self.diagnostic.clear();
        Ok(())
    }

    pub(super) fn poll_background(&mut self) -> Result<bool, String> {
        let mut changed = false;
        let mut loaded = false;
        while let Some(event) = self.registry.take_background() {
            changed = true;
            match event {
                owner::BackgroundEvent::Ready(Ok((_, errors))) => {
                    self.plugin_errors = errors;
                    if matches!(self.startup_state, StartupState::Loading) {
                        self.startup_state = StartupState::Ready;
                        self.prepare_progress = None;
                        loaded = true;
                    }
                }
                owner::BackgroundEvent::Ready(Err(error)) => self.background_failed(error),
                owner::BackgroundEvent::Progress(progress) => {
                    self.prepare_progress = Some(progress);
                }
                owner::BackgroundEvent::Scanned(kind, result) => {
                    self.scan_cancel = None;
                    self.prepare_progress = None;
                    match result {
                        Ok(report) => {
                            self.startup_state = StartupState::Ready;
                            match self.apply_scan_report(kind, report) {
                                Ok(added) => {
                                    if kind == ScanKind::Full {
                                        self.show_snackbar(
                                            if added > 0 {
                                                format!("发现 {added} 部新作品")
                                            } else {
                                                "资料库已是最新".into()
                                            },
                                            None,
                                        );
                                    }
                                    if let Err(error) = self.enqueue_auto_link() {
                                        self.diagnostic = error;
                                        self.diagnostic_severity = "error";
                                    }
                                }
                                Err(error) => {
                                    self.background_failed(error.clone());
                                    return Err(error);
                                }
                            }
                        }
                        Err(error) if error == "ASTRA_EMU_SCAN_CANCELLED" => {
                            self.startup_state = StartupState::Ready;
                            if self.patch_import.take().is_some() {
                                self.patch_import_failed(error);
                            }
                            self.show_snackbar("已取消扫描", None);
                        }
                        Err(error) => self.background_failed(error),
                    }
                }
                owner::BackgroundEvent::Exited(error) => {
                    if !matches!(self.startup_state, StartupState::Failed(_)) {
                        self.background_failed(error);
                    }
                }
            }
        }
        if loaded {
            if let Some(pending) = self.prepare_scan_pending.take().filter(|_| self.cores_ready()) {
                let scan = match pending {
                    PendingScan::Full => self
                        .scan_roots()
                        .and_then(|roots| self.begin_scan(ScanKind::Full, roots)),
                    PendingScan::Import(path) => self.begin_scan(ScanKind::Import, vec![path]),
                };
                if let Err(error) = scan {
                    self.diagnostic = error;
                    self.diagnostic_severity = "error";
                }
            }
        }
        Ok(changed)
    }

    fn background_failed(&mut self, error: String) {
        self.diagnostic.clone_from(&error);
        self.diagnostic_severity = "error";
        self.prepare_scan_pending = None;
        self.scan_cancel = None;
        self.prepare_progress = None;
        if self.patch_import.take().is_some() {
            self.patch_import_failed(error.clone());
        }
        self.startup_state = StartupState::Failed(error);
    }

    /// Returns how many games the scan added to the library.
    fn apply_scan_report(&mut self, kind: ScanKind, mut report: ScanReport) -> Result<usize, String> {
        let automatic = if kind == ScanKind::Import {
            self.patch_import.take()
        } else {
            None
        };
        let result = (|| {
            if let Some(expected) = &automatic {
                let output = fs::canonicalize(&expected.output)
                    .map_err(|_| "ASTRA_EMU_PATCH_PATH: completed output is no longer available")?;
                // Never apply an automatic result to a nested/other game, nor
                // let unrelated missing library paths mutate the source cache.
                report.games.retain(|game| game.path == output);
                if report.games.len() != 1 {
                    return Err(
                        "ASTRA_EMU_FAMILY_PROBE_REQUIRED: completed output root was not recognized"
                            .into(),
                    );
                }
                report.missing = 0;
            }
            self.apply_scan_report_inner(kind, report, automatic.as_ref())
        })();
        if automatic.is_some() {
            match result {
                Ok(_) => self.patch_import_succeeded(),
                Err(error) => self.patch_import_failed(error),
            }
            Ok(0)
        } else {
            result
        }
    }

    fn apply_scan_report_inner(
        &mut self,
        kind: ScanKind,
        report: ScanReport,
        automatic: Option<&file_patch::PendingImport>,
    ) -> Result<usize, String> {
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
        let mut added = 0;
        for result in report.games {
            let mut ids = result.game_ids;
            let profile = if kind == ScanKind::Import {
                result.patch_profile.transpose()?
            } else {
                None
            };
            if let Some(expected) = automatic {
                let profile = profile
                    .as_ref()
                    .ok_or("ASTRA_EMU_PATCH_INTEGRITY: completed output receipt is missing")?;
                if expected.profile.plugin_id != profile.plugin_id
                    || expected.profile.configuration != profile.configuration
                {
                    return Err(
                        "ASTRA_EMU_PATCH_INTEGRITY: output receipt changed before import".into(),
                    );
                }
            }
            if profile.is_some() && ids.len() > 1 {
                return Err("ASTRA_EMU_LIBRARY_INVALID_LOCATION: patch output has ambiguous library aliases".into());
            }
            if result.candidates.is_empty() {
                if profile.is_some() {
                    return Err("ASTRA_EMU_EMULATOR_SELECTION_INVALID: patch output has no supported emulator".into());
                }
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
                added += 1;
            }
            let mut choices = result.candidates;
            for id in ids {
                if automatic.is_some_and(|expected| expected.source_game_id == id) {
                    return Err(
                        "ASTRA_EMU_PATCH_INTEGRITY: output import would modify source game".into(),
                    );
                }
                if let Some(profile) = &profile {
                    self.apply_import_profile(&id, std::mem::take(&mut choices), profile)?;
                } else {
                    self.set_probe_candidates(&id, choices.clone())?;
                }
                imported.insert(id);
            }
        }
        if kind == ScanKind::Import && automatic.is_none() {
            if let Some(id) = imported.first() {
                self.selected_case_id = Some(id.clone());
            }
            if imported.is_empty() {
                self.diagnostic = "所选目录中未发现当前核心支持的游戏。请选择原生游戏目录，或检查运行包包含的核心。".into();
                self.diagnostic_severity = "warning";
            }
        }
        if report.missing > 0 {
            self.diagnostic = format!(
                "有 {} 个游戏目录当前不可用。资料库信息已保留，恢复目录后点击“刷新资料库”。",
                report.missing
            );
            self.diagnostic_severity = "warning";
        }
        if self.selected_case_id.is_none() && automatic.is_none() {
            self.selected_case_id = self.first_visible_game()?;
        }
        Ok(added)
    }
}
