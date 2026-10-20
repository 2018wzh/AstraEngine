use super::*;
use astra_emu_manager_ui_slint::PatchViewModel;
use astra_emu_patch::{
    OutputProfile, PatchError, PatchErrorKind, PatchProgress, PatchRequest, PatchResult,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, Receiver, TryRecvError},
    Arc, Mutex,
};
use std::thread::JoinHandle;

pub(super) struct PatchJob {
    pub id: u64,
    pub source_game_id: String,
    pub cancelled: Arc<AtomicBool>,
    pub progress: Arc<Mutex<Option<PatchProgress>>>,
    pub completion: Receiver<Result<PatchResult, PatchError>>,
    pub thread: Option<JoinHandle<()>>,
}

impl PatchJob {
    fn start(
        id: u64,
        source_game_id: String,
        request: PatchRequest,
        wake: HostWake,
    ) -> Result<Self, String> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(None));
        let (sender, completion) = sync_channel(1);
        let signal = cancelled.clone();
        let slot = progress.clone();
        let progress_wake = wake.clone();
        let thread = std::thread::Builder::new()
            .name("astra-file-patch".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    astra_emu_patch::apply(request, signal, move |value| {
                        let first = slot
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .replace(value)
                            .is_none();
                        if first {
                            progress_wake();
                        }
                    })
                }))
                .unwrap_or_else(|_| {
                    Err(PatchError::new(
                        PatchErrorKind::Script,
                        "conversion worker panicked",
                    ))
                });
                let _ = sender.send(result);
                wake();
            })
            .map_err(|cause| {
                format!("ASTRA_EMU_PATCH_IO: cannot start conversion worker: {cause}")
            })?;
        Ok(Self {
            id,
            source_game_id,
            cancelled,
            progress,
            completion,
            thread: Some(thread),
        })
    }

    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(super) fn join(&mut self) -> Result<(), String> {
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            Err("ASTRA_EMU_PATCH_SCRIPT: conversion worker could not be joined".into())
        } else {
            Ok(())
        }
    }
}

impl Drop for PatchJob {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
            let _ = self.join();
        }
    }
}

pub(super) struct PendingImport {
    pub output: PathBuf,
    pub source_game_id: String,
    pub profile: OutputProfile,
}

impl AstraEmuManagerController {
    pub(super) fn patch_busy(&self) -> bool {
        self.patch_job.is_some() || self.patch_import.is_some()
    }

    /// `step`: 0 not started, 4 running or failed, 5 imported.
    pub(super) fn patch_view(&self) -> PatchViewModel {
        let mut view = self.patch_state.clone();
        view.running = self.patch_job.is_some();
        if view.running || !view.error.is_empty() {
            view.step = 4;
        }
        view.can_start = !self.patch_busy()
            && self.active.is_none()
            && !self.startup_busy()
            && self.cores_ready()
            && self.selected_case_id.is_some();
        if view.phase.is_empty() && !view.can_start {
            view.phase = if self.active.is_some() {
                "结束游戏后才能应用文件补丁".into()
            } else if self.startup_busy() {
                "请等待资料库准备完成".into()
            } else if !self.cores_ready() {
                "请先准备资料库".into()
            } else {
                "请先选择来源游戏".into()
            };
        }
        view
    }

    /// Choose a recipe from the metadata patch catalog by id.
    pub(super) fn select_patch_recipe(&mut self, id: &str) -> Result<ManagerViewModel, String> {
        self.recipe_script(id)?;
        self.set_patch_selection(|state| state.script = id.into())
    }

    /// Choose a trusted local Luau script.
    pub(super) fn set_patch_script(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        let path = path
            .to_str()
            .ok_or("ASTRA_EMU_PATCH_PATH: script path must be Unicode")?
            .to_owned();
        self.set_patch_selection(|state| state.script = path)
    }

    /// `kind` is `patch` (input directory; empty uses the game directory) or `output`.
    pub(super) fn set_patch_directory(
        &mut self,
        kind: &str,
        path: &Path,
    ) -> Result<ManagerViewModel, String> {
        let path = path
            .to_str()
            .ok_or("ASTRA_EMU_PATCH_PATH: directory path must be Unicode")?
            .to_owned();
        match kind {
            "patch" => self.set_patch_selection(|state| state.patch_directory = path),
            "output" => self.set_patch_selection(|state| state.output_directory = path),
            _ => Err("ASTRA_EMU_PATCH_INVALID_REQUEST: unknown directory kind".into()),
        }
    }

    fn set_patch_selection(
        &mut self,
        change: impl FnOnce(&mut PatchViewModel),
    ) -> Result<ManagerViewModel, String> {
        if self.patch_busy() {
            return Err("ASTRA_EMU_PATCH_INVALID_REQUEST: 已有文件补丁任务或导入正在进行".into());
        }
        change(&mut self.patch_state);
        self.patch_state.step = 0;
        self.model()
    }

    fn recipe_script(&self, id: &str) -> Result<PathBuf, String> {
        let catalog = self
            .metadata_catalog
            .as_ref()
            .ok_or("ASTRA_EMU_PATCH_RECIPE_UNKNOWN")?;
        catalog
            .recipes
            .iter()
            .find(|recipe| recipe.id == id)
            .map(|recipe| catalog.patch_directory().join(&recipe.script))
            .ok_or_else(|| "ASTRA_EMU_PATCH_RECIPE_UNKNOWN".into())
    }

    /// Start the selected recipe or local script on the selected game.
    pub(super) fn start_patch(&mut self) -> Result<ManagerViewModel, String> {
        let state = &self.patch_state;
        let script = if state.script.is_empty() {
            PathBuf::new()
        } else {
            self.recipe_script(&state.script)
                .unwrap_or_else(|_| PathBuf::from(&state.script))
        };
        let patch = PathBuf::from(&state.patch_directory);
        let output = PathBuf::from(&state.output_directory);
        self.start_file_patch(&script, &patch, &output)
    }

    /// Select the game imported from the finished conversion.
    pub(super) fn open_patch_output(&mut self) -> Result<ManagerViewModel, String> {
        let output = fs::canonicalize(&self.patch_state.output)
            .map_err(|_| "ASTRA_EMU_PATCH_PATH: completed output is no longer available")?;
        let location = output.to_str().ok_or("ASTRA_EMU_GAME_PATH_UTF8")?;
        let game = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|game| game.location == location)
            .ok_or("ASTRA_EMU_GAME_NOT_FOUND")?;
        self.select_case(&game.game_id)
    }

    pub(super) fn start_file_patch(
        &mut self,
        script: &Path,
        patch: &Path,
        output: &Path,
    ) -> Result<ManagerViewModel, String> {
        if self.patch_busy() {
            return Err("ASTRA_EMU_PATCH_INVALID_REQUEST: 已有文件补丁任务或导入正在进行".into());
        }
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        if self.startup_busy() {
            return Err("ASTRA_EMU_FAMILY_INITIALIZING".into());
        }
        if !self.cores_ready() {
            return Err("ASTRA_EMU_CORES_NOT_READY".into());
        }
        if script.as_os_str().is_empty() || output.as_os_str().is_empty() {
            return Err("ASTRA_EMU_PATCH_INVALID_REQUEST: 请选择脚本和新的输出目录".into());
        }
        let game = self.selected_game()?;
        let source = PathBuf::from(&game.location);
        let request = PatchRequest {
            script: script.to_owned(),
            patch: if patch.as_os_str().is_empty() {
                source.clone()
            } else {
                patch.to_owned()
            },
            source,
            output: output.to_owned(),
        };
        let id = self
            .patch_sequence
            .checked_add(1)
            .ok_or("ASTRA_EMU_PATCH_BOUNDS: conversion identity exhausted")?;
        let wake = self
            .host_wake
            .clone()
            .ok_or("ASTRA_EMU_HOST_WAKE_REQUIRED")?;
        let job = PatchJob::start(id, game.game_id, request, wake)?;
        self.patch_sequence = id;
        self.patch_job = Some(job);
        self.patch_state = PatchViewModel {
            phase: "准备文件补丁任务…".into(),
            output: output.to_string_lossy().into_owned(),
            script: std::mem::take(&mut self.patch_state.script),
            patch_directory: std::mem::take(&mut self.patch_state.patch_directory),
            output_directory: std::mem::take(&mut self.patch_state.output_directory),
            ..Default::default()
        };
        self.model()
    }

    pub(super) fn cancel_file_patch(&mut self) -> Result<ManagerViewModel, String> {
        if let Some(job) = &self.patch_job {
            job.cancel();
            self.patch_state.phase = "正在取消，等待文件句柄和工作线程回收…".into();
        }
        self.model()
    }

    pub(super) fn poll_file_patch(&mut self) -> Result<bool, String> {
        let Some(job) = self.patch_job.as_ref() else {
            return Ok(false);
        };
        let mut changed = false;
        if let Some(progress) = job
            .progress
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        {
            self.patch_state.phase = if job.cancelled.load(Ordering::Acquire) {
                "正在取消…".into()
            } else {
                progress.phase
            };
            self.patch_state.completed = progress.completed;
            self.patch_state.total = progress.total;
            changed = true;
        }
        let result = match job.completion.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(changed),
            Err(TryRecvError::Disconnected) => Err(PatchError::new(
                PatchErrorKind::Script,
                "conversion worker disconnected",
            )),
        };
        let mut job = self
            .patch_job
            .take()
            .expect("completed job retained until join");
        let joined = job.join();
        if job.id != self.patch_sequence {
            return Err("ASTRA_EMU_PATCH_INTEGRITY: stale conversion completion".into());
        }
        match result {
            Ok(result) => {
                self.patch_state.output = result.output.to_string_lossy().into_owned();
                self.patch_state.completed = 1;
                self.patch_state.total = Some(1);
                if let Err(error) = joined {
                    self.patch_import_failed(error);
                    return Ok(true);
                }
                self.patch_state.phase = "文件转换完成，正在导入独立目录…".into();
                self.patch_import = Some(PendingImport {
                    output: result.output.clone(),
                    source_game_id: std::mem::take(&mut job.source_game_id),
                    profile: result.profile,
                });
                if let Err(error) =
                    self.begin_scan(probe_worker::ScanKind::Import, vec![result.output])
                {
                    self.patch_import.take();
                    self.patch_import_failed(error);
                }
            }
            Err(error) => {
                if error.kind() == PatchErrorKind::Cancelled && joined.is_ok() {
                    self.patch_state.phase = "已取消，未发布输出目录".into();
                    self.patch_state.error.clear();
                } else {
                    self.patch_state.phase = "文件补丁处理失败".into();
                    self.patch_state.error = joined.err().unwrap_or_else(|| error.to_string());
                }
            }
        }
        Ok(true)
    }

    pub(super) fn patch_import_failed(&mut self, error: String) {
        self.patch_state.phase = "文件转换完成，导入失败".into();
        self.patch_state.error = format!("文件转换完成，导入失败：{error}");
    }

    pub(super) fn patch_import_succeeded(&mut self) {
        self.patch_state.phase = "文件转换完成，已加入独立库条目".into();
        self.patch_state.error.clear();
        self.patch_state.step = 5;
    }

    pub(super) fn apply_import_profile(
        &mut self,
        game_id: &str,
        choices: Vec<FamilyProbeCandidate>,
        profile: &OutputProfile,
    ) -> Result<(), String> {
        let selected = choices.iter().find(|candidate| candidate.report.plugin_id == profile.plugin_id)
            .cloned().ok_or("ASTRA_EMU_EMULATOR_SELECTION_INVALID: patch output is not recognized by its requested emulator")?;
        if selected.descriptor.family_id != selected.report.family_id
            || self.registry.descriptor(&profile.plugin_id) != Some(&selected.descriptor)
        {
            return Err(
                "ASTRA_EMU_EMULATOR_SELECTION_INVALID: patch output descriptor mismatch".into(),
            );
        }
        let schema = self
            .registry
            .configuration(&profile.plugin_id)
            .map_err(|error| error.to_string())?;
        let values = family_config::parse_config_entries(
            &schema,
            profile
                .configuration
                .iter()
                .map(|(id, value)| (id.as_str(), value.as_str())),
        )?;
        self.library
            .apply_patch_profile(
                game_id,
                &selected.report.family_id,
                &profile.plugin_id,
                &schema,
                &values,
            )
            .map_err(|error| error.to_string())?;
        // No candidate cache mutation precedes the complete SQLite transaction.
        self.probe_choices.insert(game_id.to_owned(), choices);
        self.candidates.insert(game_id.to_owned(), selected);
        Ok(())
    }
}

#[cfg(test)]
#[path = "file_patch_tests.rs"]
mod tests;
