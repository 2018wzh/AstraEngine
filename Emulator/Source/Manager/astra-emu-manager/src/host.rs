use astra_emu_manager_core::InputMapping;
use astra_media_core::Viewport2D;
use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

pub(super) use astra_emu_manager_ui_slint::{
    describe_error, report_error, App, Cores, Dialogs, Filters, GameSettings, Library, Logs,
    ManagerViewModel, Network, Notices, Onboarding, Selection, Session, Settings,
    SlintManagerAdapter,
};
use slint::ComponentHandle;
use thiserror::Error;

use crate::gamepad::GameInputPump;
use wiring::{FieldScope, Wiring};

#[path = "host_filters.rs"]
mod filters;
#[path = "host_game_callbacks.rs"]
mod game_callbacks;
#[path = "host_library_callbacks.rs"]
mod library_callbacks;
#[path = "host_modals.rs"]
mod modals;
#[path = "host_pointer.rs"]
mod pointer;
#[path = "host_settings_callbacks.rs"]
mod settings_callbacks;
#[path = "host_window_commands.rs"]
mod window_commands;
#[path = "host_events.rs"]
mod window_events;
#[path = "host_wiring.rs"]
mod wiring;

type HostCallback = Box<dyn FnMut()>;
type HostCallbackSlot = std::rc::Rc<std::cell::RefCell<Option<HostCallback>>>;

/// Thread-safe edge-triggered wake used by workers to notify the Slint host.
/// The callback only schedules work back onto the UI thread; it never mutates
/// controller or renderer state from a worker.
pub type HostWake = Arc<dyn Fn() + Send + Sync + 'static>;

pub struct WgpuFrameContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
}

/// A rendered stage plus the content rectangle in the texture shown by the
/// Manager. The rectangle is in output-texture pixels, so it remains valid
/// when a final-frame filter changes the texture dimensions.
pub struct StageTextureUpdate {
    pub texture: wgpu::Texture,
    pub logical_width: u32,
    pub logical_height: u32,
    pub raster_width: u32,
    pub raster_height: u32,
    pub content: Viewport2D,
}

pub trait AstraUnderlayRenderer: 'static {
    fn configure_filter(
        &mut self,
        _config: &crate::effects::FilterConfiguration,
        _source: Option<&str>,
    ) -> Result<(), String> {
        Err("ASTRA_EMU_FILTER_RENDERER_NOT_CONFIGURED".into())
    }
    fn setup(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String>;
    fn take_stage_texture_update(&mut self) -> Result<Option<StageTextureUpdate>, String> {
        Ok(None)
    }
    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String>;
    fn teardown(&mut self) -> Result<(), String>;
}

pub trait ManagerController: 'static {
    fn game_fullscreen(&self) -> bool {
        false
    }
    fn take_game_icon(&mut self) -> Option<astra_emu_manager_core::model::FamilyWindowIcon> {
        None
    }
    fn take_game_pointer(&mut self) -> Option<astra_emu_manager_core::model::FamilyPointerRequest> {
        None
    }
    fn game_title(&self) -> Option<&str> {
        None
    }
    fn take_modal_update(&mut self) -> Option<crate::modal_service::ModalUiUpdate> {
        None
    }
    fn respond_modal(
        &mut self,
        _id: crate::modal_service::ModalId,
        _response: astra_emu_manager_core::model::ModalResponse,
    ) -> Result<(), String> {
        Err("ASTRA_EMU_MANAGER_MODAL_SERVICE_UNAVAILABLE".into())
    }
    fn take_window_request(&mut self) -> Option<crate::window_service::WindowPresentation> {
        None
    }
    fn respond_window(
        &mut self,
        _request: &crate::window_service::WindowPresentation,
        _result: Result<crate::window_service::WindowOutcome, String>,
    ) -> Result<(), String> {
        Err("ASTRA_EMU_WINDOW_SERVICE_UNAVAILABLE".into())
    }
    fn native_failure(&self) -> Option<(&str, bool)> {
        None
    }
    fn retry_native(&mut self) -> Result<(), String> {
        Err("ASTRA_EMU_CORE_RETRY_UNAVAILABLE".into())
    }
    fn set_window_state(
        &mut self,
        state: astra_emu_manager_core::model::WindowState,
    ) -> Result<(), String>;
    fn physical_event(
        &mut self,
        event: astra_emu_manager_core::model::FamilyEvent,
    ) -> Result<(), String>;
    fn is_game_active(&self) -> bool;
    /// Load cores on first use (never at startup), then scan every library root.
    /// `ASTRA_EMU_PREPARE_BUSY` while loading/scanning, `ASTRA_EMU_PREPARE_RESTART_REQUIRED` after failure.
    fn prepare_library(&mut self) -> Result<ManagerViewModel, String>;
    /// Cancel a running scan; core loading cannot be cancelled.
    fn cancel_prepare(&mut self) -> Result<ManagerViewModel, String>;
    /// Hide a game from the library and later scans; the snackbar offers undo.
    fn remove_from_library(&mut self, game_id: &str) -> Result<ManagerViewModel, String>;
    fn restore_game(&mut self, game_id: &str) -> Result<ManagerViewModel, String>;
    /// Run the current snackbar action (undo removal or request a restart).
    fn snackbar_action(&mut self) -> Result<SnackbarOutcome, String>;
    /// Start capturing the next keyboard key for `button`; `game_id` targets a per-game mapping.
    fn begin_key_capture(
        &mut self,
        button: &str,
        game_id: Option<&str>,
    ) -> Result<ManagerViewModel, String>;
    fn cancel_key_capture(&mut self) -> Result<ManagerViewModel, String>;
    fn key_capture(&self) -> Option<KeyCaptureState<'_>>;
    /// Bind the captured key; keys without a mapping name keep the capture
    /// waiting and set the hint to “这个键不能用于映射”.
    fn capture_key(
        &mut self,
        code: astra_emu_manager_core::model::KeyCode,
    ) -> Result<ManagerViewModel, String>;
    /// Whether the persisted network permission is granted.
    fn network_allowed(&self) -> bool;
    /// Persist a library root without loading cores or scanning.
    fn add_library_root(&mut self, path: &std::path::Path) -> Result<ManagerViewModel, String>;
    fn remove_library_root(&mut self, path: &str) -> Result<ManagerViewModel, String>;
    fn onboarding_completed(&self) -> Result<bool, String>;
    fn finish_onboarding(&mut self) -> Result<ManagerViewModel, String>;
    fn game_location(&self, game_id: &str) -> Result<std::path::PathBuf, String>;
    fn data_dir(&self) -> &std::path::Path;
    fn model(&self) -> Result<ManagerViewModel, String>;
    fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String>;
    fn search(&mut self, query: &str) -> Result<ManagerViewModel, String>;
    fn save_translation_profile(
        &mut self,
        form: &astra_emu_manager_ui_slint::TranslationFormViewModel,
        secret: &str,
    ) -> Result<ManagerViewModel, String>;
    /// Test the form values; an empty secret falls back to the stored one.
    fn test_translation_connection(
        &mut self,
        form: &astra_emu_manager_ui_slint::TranslationFormViewModel,
        secret: &str,
    ) -> Result<ManagerViewModel, String>;
    fn set_translation_enabled(&mut self, enabled: bool) -> Result<ManagerViewModel, String>;
    /// One persisted switch for VNDB, Bangumi, metadata sync, core downloads and translation.
    fn set_network_allowed(&mut self, enabled: bool) -> Result<ManagerViewModel, String>;
    fn set_sensitive_covers(&mut self, enabled: bool) -> Result<ManagerViewModel, String>;
    fn save_bangumi_token(&mut self, token: &str) -> Result<ManagerViewModel, String>;
    fn clear_bangumi_token(&mut self) -> Result<ManagerViewModel, String>;
    fn add_game_directory(&mut self, path: &std::path::Path) -> Result<ManagerViewModel, String>;
    /// Patch recipe id from the metadata catalog.
    fn select_patch_recipe(&mut self, id: &str) -> Result<ManagerViewModel, String>;
    fn set_patch_script(&mut self, path: &std::path::Path) -> Result<ManagerViewModel, String>;
    /// `kind` is `patch` or `output`.
    fn set_patch_directory(
        &mut self,
        kind: &str,
        path: &std::path::Path,
    ) -> Result<ManagerViewModel, String>;
    /// Convert the selected game with the chosen script and directories.
    fn start_patch(&mut self) -> Result<ManagerViewModel, String>;
    fn cancel_file_patch(&mut self) -> Result<ManagerViewModel, String>;
    /// Select the game imported from the finished conversion.
    fn open_patch_output(&mut self) -> Result<ManagerViewModel, String>;
    fn filter_settings(&self) -> astra_emu_manager_core::FilterSettings;
    /// Filter changes are applied through `apply` (the renderer), then saved;
    /// on failure the previous settings are re-applied and stay persisted.
    fn set_filter_preset(
        &mut self,
        id: &str,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String>;
    fn set_filter_parameter(
        &mut self,
        key: &str,
        value: &str,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String>;
    fn reset_filter_parameters(
        &mut self,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String>;
    /// Load an external HLSL effect (at most 1 MiB).
    fn load_filter_source(
        &mut self,
        path: &std::path::Path,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String>;
    fn game_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String>;
    /// Clear physical keys and modifiers when focus leaves the game surface
    /// so a later focus cannot inherit a stuck input state.
    fn release_inputs(&mut self) -> Result<(), String>;
    fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String>;
    fn leave_game(&mut self) -> Result<ManagerViewModel, String>;
    fn search_metadata(&mut self, provider: &str, query: &str) -> Result<ManagerViewModel, String>;
    fn refresh_metadata(&mut self, _provider: &str) -> Result<ManagerViewModel, String>;
    fn accept_match(&mut self, _candidate_id: &str) -> Result<ManagerViewModel, String>;
    fn unlink_identity(&mut self, _provider: &str) -> Result<ManagerViewModel, String>;
    fn update_bangumi_play_status(
        &mut self,
        _status: &str,
        _rating: i32,
        _note: &str,
    ) -> Result<ManagerViewModel, String>;
    fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String>;
    fn set_host_wake(&mut self, _wake: HostWake);
    /// Next UI input/metadata service deadline. Native core clocks are independent.
    fn runtime_deadline(&self) -> Option<Instant>;
    /// Drain owner results and enqueue input; never execute a native core on UI.
    fn advance_runtime(&mut self) -> Result<Option<ManagerViewModel>, String>;
    fn set_appearance(
        &mut self,
        appearance: astra_emu_manager_core::AppearanceSettings,
    ) -> Result<ManagerViewModel, String>;
    /// Stage a local `.zip` core package or single core library; applied on restart.
    fn install_core(&mut self, source: &std::path::Path) -> Result<ManagerViewModel, String>;
    /// Download the newest catalog release of a core; needs network permission.
    fn install_online_core(&mut self, plugin_id: &str) -> Result<ManagerViewModel, String>;
    /// `core_id` is a `Cores.items` id (plugin id, or directory stem for failed cores).
    fn uninstall_core(&mut self, core_id: &str) -> Result<ManagerViewModel, String>;
    fn log_text(&self, core: &str) -> Result<(String, usize), String>;
    fn engine_action(&mut self, control: u32) -> Result<(), String>;
    /// Every configuration setter below persists immediately and fails with
    /// `ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE` while a game runs.
    fn set_game_emulator(
        &mut self,
        game_id: &str,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String>;
    /// `raw` is an option value or its label; invalid values leave storage unchanged.
    fn set_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String>;
    fn reset_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String>;
    fn set_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String>;
    fn reset_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String>;
    /// Choose the emulator whose defaults the cores page edits; empty clears it.
    fn select_emulator_defaults(&mut self, plugin_id: &str) -> Result<ManagerViewModel, String>;
    /// Open a file or folder dialog per the field's path flags and store the pick.
    fn pick_config_path(
        &mut self,
        game_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String>;
    /// `None` follows the emulator default.
    fn set_render_preference(
        &mut self,
        game_id: &str,
        preference: Option<astra_emu_manager_core::RenderPreference>,
    ) -> Result<ManagerViewModel, String>;
    fn set_emulator_render_default(
        &mut self,
        plugin_id: &str,
        preference: astra_emu_manager_core::RenderPreference,
    ) -> Result<ManagerViewModel, String>;
    /// Library sort mode: "title" | "recent" | "play_time".
    fn set_library_sort(&mut self, _mode: &str) -> Result<ManagerViewModel, String>;
    /// Compatibility filter: "all" | "perfect" | "completable" | "flawed" |
    /// "boot_only" | "unplayable" | "unknown".
    fn set_compatibility_filter(&mut self, _filter: &str) -> Result<ManagerViewModel, String>;
    /// Queue an AstraEMU-metadata synchronization.
    fn sync_metadata(&mut self) -> Result<ManagerViewModel, String>;
    /// Write the selected release's recommended configuration as game overrides.
    fn apply_recommended_config(&mut self, game_id: &str) -> Result<ManagerViewModel, String>;
    /// Fetch the VNDB releases (rIDs) of the selected work so its local
    /// installation can be pinned to a specific game version.
    fn fetch_releases(&mut self) -> Result<ManagerViewModel, String>;
    /// Pin the selected installation to a specific VNDB release (rID).
    fn pin_release(&mut self, _release_id: &str) -> Result<ManagerViewModel, String>;
    /// The pump mapping: the running game's mapping, otherwise the global one.
    /// Push it to `GameInputPump::set_mapping` after every mapping change.
    fn input_mapping(&self) -> InputMapping;
    fn set_gamepad_enabled(&mut self, enabled: bool) -> Result<ManagerViewModel, String>;
    fn set_gamepad_deadzone(&mut self, deadzone: &str) -> Result<ManagerViewModel, String>;
    /// Bind a global gamepad input to a key name; an empty key removes the binding.
    fn set_gamepad_binding(&mut self, button: &str, key: &str) -> Result<ManagerViewModel, String>;
    /// Restore the recommended VN button bindings.
    fn reset_gamepad_bindings(&mut self) -> Result<ManagerViewModel, String>;
    /// Enabling copies the global mapping to the game; disabling removes the override.
    fn set_game_input_override(
        &mut self,
        game_id: &str,
        enabled: bool,
    ) -> Result<ManagerViewModel, String>;
    fn set_game_gamepad_binding(
        &mut self,
        game_id: &str,
        button: &str,
        key: &str,
    ) -> Result<ManagerViewModel, String>;
}

pub enum SnackbarOutcome {
    Model(Box<ManagerViewModel>),
    Restart,
}

/// The controller's pending key capture.
pub struct KeyCaptureState<'a> {
    pub button: &'a str,
    pub game_id: Option<&'a str>,
    /// Non-empty after an unmappable key; the capture keeps waiting.
    pub hint: &'a str,
}

/// What the host does with a window close request.
#[derive(Debug)]
pub enum CloseDecision {
    /// A game was running and has been left; the Manager window stays open.
    KeepWindow(Result<Box<ManagerViewModel>, String>),
    HideWindow,
}

/// Closing the window while a game runs only leaves the game.
pub fn close_requested<C: ManagerController>(controller: &mut C) -> CloseDecision {
    if controller.is_game_active() {
        CloseDecision::KeepWindow(controller.leave_game().map(Box::new))
    } else {
        CloseDecision::HideWindow
    }
}

/// Open a file or directory in the platform file manager without a shell.
pub fn open_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|error| format!("ASTRA_EMU_OPEN_PATH_FAILED: {error}"))
}
pub type FilterApply<'a> =
    &'a mut dyn FnMut(&astra_emu_manager_core::FilterSettings) -> Result<(), String>;

fn fire_host_callback(slot: &HostCallbackSlot) {
    let Some(mut callback) = slot.borrow_mut().take() else {
        return;
    };
    callback();
    *slot.borrow_mut() = Some(callback);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostExit {
    Quit,
    Restart,
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error("ASTRA_EMU_HOST_BACKEND: {0}")]
    Backend(#[from] slint::PlatformError),
    #[error("ASTRA_EMU_HOST_RENDERER: {0}")]
    Renderer(String),
}

pub fn run_manager<C: ManagerController, R: AstraUnderlayRenderer>(
    controller: C,
    renderer: R,
) -> Result<HostExit, HostError> {
    run_manager_with_initial_state(controller, renderer, false)
}

pub fn run_manager_with_initial_state<C: ManagerController, R: AstraUnderlayRenderer>(
    controller: C,
    renderer: R,
    game_active: bool,
) -> Result<HostExit, HostError> {
    crate::startup::mark("ui_backend_begin");
    #[cfg(not(target_os = "android"))]
    {
        let mut settings = slint::wgpu_30::WGPUSettings::default();
        settings.power_preference = wgpu::PowerPreference::HighPerformance;
        settings.device_memory_hints = wgpu::MemoryHints::Performance;
        // The final-frame effects use compute and storage textures; Slint's UI-only
        // WebGL2 defaults expose neither capability.
        settings.device_required_limits = wgpu::Limits::default();
        settings.device_required_features |= crate::effects::FilterEngine::required_features();
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .require_wgpu_30(slint::wgpu_30::WGPUConfiguration::Automatic(settings))
            .select()?;
    }
    crate::startup::mark("ui_backend_selected");
    let adapter = std::rc::Rc::new(SlintManagerAdapter::new()?);
    crate::startup::mark("window_object_created");
    adapter.apply(&controller.model().map_err(HostError::Renderer)?);
    if !controller
        .onboarding_completed()
        .map_err(HostError::Renderer)?
    {
        adapter
            .window()
            .global::<App>()
            .set_page("onboarding".into());
    }
    crate::startup::mark("initial_model_applied");
    adapter.set_session_active(game_active);
    let controller = std::rc::Rc::new(std::cell::RefCell::new(controller));
    let renderer = std::rc::Rc::new(std::cell::RefCell::new(renderer));
    let fatal_error = std::rc::Rc::new(std::cell::RefCell::new(None));
    let fatal_error_callback = fatal_error.clone();
    let window_weak = adapter.window().as_weak();
    let host_exit = std::rc::Rc::new(Cell::new(HostExit::Quit));
    let gamepad = std::rc::Rc::new(std::cell::RefCell::new(
        GameInputPump::new(controller.borrow().input_mapping()).map_err(HostError::Renderer)?,
    ));
    // Worker completions are edge-triggered.  The render callback only drains
    // the bounded completion queues when a worker has signalled this flag;
    // ordinary Slint repaints must not turn into a hidden polling loop.
    let async_events_pending = Arc::new(AtomicBool::new(true));
    let async_events_for_wake = async_events_pending.clone();
    let wake_window = adapter.window().as_weak();
    let host_wake: HostWake = Arc::new(move || {
        async_events_for_wake.store(true, Ordering::Release);
        let weak = wake_window.clone();
        if let Err(error) = slint::invoke_from_event_loop(move || {
            if let Some(window) = weak.upgrade() {
                window.window().request_redraw();
            }
        }) {
            tracing::debug!(
                event = "astra.emu.host.wake_rejected",
                diagnostic_code = "ASTRA_EMU_HOST_WAKE_REJECTED",
                error = %error
            );
        }
    });
    controller.borrow_mut().set_host_wake(host_wake.clone());
    let pointer = pointer::HostPointer::install(&adapter);
    window_events::install(
        &adapter,
        controller.clone(),
        pointer.clone(),
        gamepad.clone(),
    );
    gamepad
        .borrow_mut()
        .set_wake(host_wake)
        .map_err(HostError::Renderer)?;
    let runtime_timer = std::rc::Rc::new(slint::Timer::default());
    let runtime_schedule: HostCallbackSlot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let runtime_schedule_slot = std::rc::Rc::downgrade(&runtime_schedule);
    let runtime_weak = adapter.window().as_weak();
    let runtime_controller = controller.clone();
    let runtime_adapter = std::rc::Rc::downgrade(&adapter);
    let schedule_timer = runtime_timer.clone();
    let runtime_pointer = pointer.clone();
    *runtime_schedule.borrow_mut() = Some(Box::new(move || {
        let timer = schedule_timer.clone();
        let slot = runtime_schedule_slot.clone();
        let weak = runtime_weak.clone();
        let controller = runtime_controller.clone();
        let adapter = runtime_adapter.clone();
        let pointer = runtime_pointer.clone();
        let Some(deadline) = controller.borrow().runtime_deadline() else {
            timer.stop();
            return;
        };
        let delay = deadline.saturating_duration_since(Instant::now());
        timer.start(slint::TimerMode::SingleShot, delay, move || {
            if let Some(window) = weak.upgrade() {
                let result = controller.borrow_mut().advance_runtime();
                match result {
                    Ok(Some(model)) => apply_model(&adapter, &model),
                    Ok(None) => {}
                    Err(error) => {
                        terminate_game(&controller, &adapter, &window, error);
                    }
                }
                let session = window.global::<Session>();
                session.set_active(controller.borrow().is_game_active());
                let title =
                    slint::SharedString::from(controller.borrow().game_title().unwrap_or(""));
                if session.get_title() != title {
                    session.set_title(title);
                }
                let icon = controller.borrow_mut().take_game_icon();
                if icon.is_some() || !controller.borrow().is_game_active() {
                    if let Err(error) = window_commands::set_icon(window.window(), icon) {
                        terminate_game(&controller, &adapter, &window, error);
                    }
                }
                let pointer_request = controller.borrow_mut().take_game_pointer();
                if let Some(request) = pointer_request {
                    if let Err(error) = pointer::HostPointer::apply(&pointer, &window, request) {
                        terminate_game(&controller, &adapter, &window, error);
                    }
                }
                let request = controller.borrow_mut().take_window_request();
                if let Some(request) = request {
                    let result = window_commands::execute(&window, &pointer, &request.command);
                    if let Err(error) = controller.borrow_mut().respond_window(&request, result) {
                        report_error(&window, &error);
                    }
                }
                if let Some((failure, can_retry)) = controller.borrow().native_failure() {
                    apply_fatal(&window, failure, can_retry);
                }
                window.window().request_redraw();
            }
            if let Some(slot) = slot.upgrade() {
                fire_host_callback(&slot);
            }
        });
    }));
    fire_host_callback(&runtime_schedule);
    let restart: std::rc::Rc<dyn Fn()> = {
        let host_exit = host_exit.clone();
        std::rc::Rc::new(move || {
            host_exit.set(HostExit::Restart);
            let _ = slint::quit_event_loop();
        })
    };
    let wiring = Wiring::new(&adapter, &controller, &gamepad);
    game_callbacks::install(&wiring, &renderer, &runtime_schedule);
    settings_callbacks::install(&wiring, &restart);
    library_callbacks::install(&wiring);
    {
        let close_controller = controller.clone();
        let close_adapter = std::rc::Rc::downgrade(&adapter);
        let close_window = adapter.window().as_weak();
        let close_schedule = runtime_schedule.clone();
        adapter.window().window().on_close_requested(move || {
            let decision = close_requested(&mut *close_controller.borrow_mut());
            match decision {
                CloseDecision::KeepWindow(result) => {
                    if let Some(window) = close_window.upgrade() {
                        finish_leave(&window, &close_adapter, result.map(|model| *model));
                    }
                    fire_host_callback(&close_schedule);
                    slint::CloseRequestResponse::KeepWindowShown
                }
                CloseDecision::HideWindow => slint::CloseRequestResponse::HideWindow,
            }
        });
    }
    let modal_ui = modals::install(&adapter, &controller);
    let event_controller = controller.clone();
    let event_adapter = std::rc::Rc::downgrade(&adapter);
    let event_gamepad = gamepad.clone();
    let event_window = adapter.window().as_weak();
    let event_pending = async_events_pending.clone();
    let renderer_callback = renderer.clone();
    let initial_window_pending = std::cell::Cell::new(true);
    let first_ui_frame_pending = std::cell::Cell::new(true);
    let pad_pressed = std::cell::RefCell::new(Vec::<String>::new());
    adapter.window().window().set_rendering_notifier(move |state, api| {
        let slint::GraphicsAPI::WGPU30 { device, queue, .. } = api else {
            record_fatal(&fatal_error_callback, "rendering notifier did not provide WGPU 30".into());
            let _ = slint::quit_event_loop();
            return;
        };
        let context = WgpuFrameContext { device, queue };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match state {
            slint::RenderingState::RenderingSetup => {
                crate::startup::mark("ui_gpu_ready");
                renderer_callback.borrow_mut().setup(context)?;
                if let Some(window) = event_window.upgrade() {
                    if initial_window_pending.replace(false) {
                        // Winit creates its native window when the event loop starts.
                        // Defer fitting until the first render finishes applying size.
                        let weak = window.as_weak();
                        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                            if let Some(window) = weak.upgrade() {
                                window_commands::fit_initial_window(window.window());
                            }
                        });
                    }
                    if let Ok(dark) = window_events::system_theme(window.window()) {
                        window.global::<App>().set_system_dark(dark);
                    }
                }
                filters::configure(&mut *renderer_callback.borrow_mut(), &event_controller.borrow().filter_settings())?;
                if let Some(update) = renderer_callback.borrow_mut().take_stage_texture_update()? {
                    let window = window_weak
                        .upgrade()
                        .ok_or_else(|| "Manager window disappeared during renderer setup".to_string())?;
                    apply_stage_texture_update(&window, update)?;
                }
                Ok(())
            }
            slint::RenderingState::BeforeRendering => {
                if let Some(window) = event_window.upgrade() {
                    modals::service(&window, &event_controller, &modal_ui);
                    let input_blocked = !session_input_open(&window);
                    if input_blocked {
                        let release = event_controller.borrow_mut().release_inputs();
                        if let Err(error) = release {
                            terminate_game(&event_controller, &event_adapter, &window, error);
                        }
                    }
                    if event_pending.swap(false, Ordering::Acquire) {
                        let poll_result = event_controller.borrow_mut().poll_platform();
                        match poll_result {
                            Ok(Some(model)) => apply_model(&event_adapter, &model),
                            Ok(None) => {}
                            Err(error) => terminate_game(&event_controller, &event_adapter, &window, error),
                        }
                        match event_gamepad.borrow_mut().poll() {
                            Ok(events) => {
                                let session = window.global::<Session>();
                                if !session.get_active() && update_pad_state(&mut pad_pressed.borrow_mut(), &events) {
                                    if let Some(adapter) = event_adapter.upgrade() {
                                        adapter.set_pad_state(&pad_pressed.borrow());
                                    }
                                }
                                if session.get_active() && session.get_menu_open() {
                                    let keys: Vec<_> = events
                                        .iter()
                                        .filter_map(|event| {
                                            menu_key(event.button?).map(|key| (key, event.pressed))
                                        })
                                        .collect();
                                    if !keys.is_empty() {
                                        // Dispatching from inside the rendering notifier would re-enter Slint.
                                        let weak = window.as_weak();
                                        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                                            if let Some(window) = weak.upgrade() {
                                                dispatch_menu_keys(&window, &keys);
                                            }
                                        });
                                    }
                                } else if !input_blocked && session.get_active() {
                                    for event in events.into_iter().filter(|event| !event.control.is_empty()) {
                                        let input = event_controller.borrow_mut().game_input(
                                            &event.control,
                                            event.pressed,
                                            event.value,
                                        );
                                        if let Err(error) = input {
                                            report_input_error(&window, &error);
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                if window.global::<Session>().get_active() { terminate_game(&event_controller, &event_adapter, &window, error); }
                                else { report_error(&window, &error); }
                            }
                        }
                    }
                }
                let mut renderer = renderer_callback.borrow_mut();
                renderer.render(context)?;
                if let Some(update) = renderer.take_stage_texture_update()? {
                    let window = window_weak.upgrade().ok_or_else(|| "Manager window disappeared during texture update".to_string())?;
                    apply_stage_texture_update(&window, update)?;
                }
                Ok(())
            },
            slint::RenderingState::AfterRendering => {
                if first_ui_frame_pending.replace(false) {
                    crate::startup::mark("first_ui_frame_rendered");
                    let weak = event_window.clone();
                    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                        if let Some(window) = weak.upgrade() {
                            window.global::<Library>().set_cover_loading(true);
                        }
                    });
                    if event_controller.borrow().network_allowed() {
                        let controller = event_controller.clone();
                        let adapter = event_adapter.clone();
                        let weak = event_window.clone();
                        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                            let result = controller.borrow_mut().sync_metadata();
                            match result {
                                Ok(model) => apply_model(&adapter, &model),
                                Err(error) => {
                                    if let Some(window) = weak.upgrade() {
                                        report_error(&window, &error);
                                    }
                                }
                            }
                        });
                    }
                }
                Ok(())
            },
            slint::RenderingState::RenderingTeardown => renderer_callback.borrow_mut().teardown(),
            _ => Ok(()),
        })).unwrap_or_else(|_| Err("underlay renderer panicked".into()));
        if let Err(error) = result {
            tracing::error!(event = "astra.emu.host.renderer_failed", diagnostic_code = "ASTRA_EMU_HOST_RENDERER", message = %error);
            record_fatal(&fatal_error_callback, error);
            let _ = slint::quit_event_loop();
        }
    }).map_err(|error| HostError::Renderer(error.to_string()))?;
    crate::startup::mark("window_show_requested");
    adapter.window().show()?;
    crate::startup::mark("window_show_returned");
    let run_result = adapter.window().run();
    runtime_timer.stop();
    runtime_schedule.borrow_mut().take();
    let shutdown_result = controller.borrow_mut().leave_game();
    // A notifier can be torn down only when the window is dropped. Drain the
    // graphics owner explicitly before choosing the application's exit result.
    let graphics_result = renderer.borrow_mut().teardown();
    if let Err(error) = &graphics_result {
        tracing::error!(event = "astra.emu.host.graphics_cleanup_failed", message = %error);
    }
    run_result?;
    shutdown_result.map_err(HostError::Renderer)?;
    graphics_result.map_err(HostError::Renderer)?;
    if let Some(error) = fatal_error.borrow_mut().take() {
        return Err(HostError::Renderer(error));
    }
    Ok(host_exit.get())
}

fn apply_stage_texture_update(
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    update: StageTextureUpdate,
) -> Result<(), String> {
    let image = slint::Image::try_from(update.texture)
        .map_err(|_| "WGPU stage texture import failed".to_string())?;
    let session = window.global::<Session>();
    session.set_frame(image);
    session.set_output_aspect(update.raster_width as f32 / update.raster_height as f32);
    session.set_native_width(update.logical_width as f32);
    session.set_native_height(update.logical_height as f32);
    session.set_raster_width(update.raster_width as f32);
    session.set_raster_height(update.raster_height as f32);
    session.set_content_x(update.content.x as f32);
    session.set_content_y(update.content.y as f32);
    session.set_content_width(update.content.width as f32);
    session.set_content_height(update.content.height as f32);
    Ok(())
}

fn record_fatal(slot: &std::cell::RefCell<Option<String>>, error: String) {
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(error);
    }
}

fn apply_model(adapter: &std::rc::Weak<SlintManagerAdapter>, model: &ManagerViewModel) {
    if let Some(adapter) = adapter.upgrade() {
        adapter.apply(model);
    }
}

/// Track held gamepad buttons by binding id; true when the set changed.
fn update_pad_state(pressed: &mut Vec<String>, events: &[crate::gamepad::GameInput]) -> bool {
    let mut changed = false;
    for event in events {
        let Some(button) = event.button else {
            continue;
        };
        let id = button.as_str();
        let held = pressed.iter().position(|value| value == id);
        match (event.pressed, held) {
            (true, None) => pressed.push(id.to_owned()),
            (false, Some(index)) => {
                pressed.remove(index);
            }
            _ => continue,
        }
        changed = true;
    }
    changed
}

fn session_input_open(window: &astra_emu_manager_ui_slint::ManagerWindow) -> bool {
    let session = window.global::<Session>();
    let dialogs = window.global::<Dialogs>();
    session.get_active()
        && !session.get_menu_open()
        && !dialogs.get_fatal_active()
        && !dialogs.get_modal_active()
}

/// Drop a stale frame and its geometry so the next session never shows it.
fn clear_session_frame(session: &Session<'_>) {
    session.set_frame(slint::Image::default());
    session.set_output_aspect(0.0);
    session.set_native_width(0.0);
    session.set_native_height(0.0);
    session.set_raster_width(0.0);
    session.set_raster_height(0.0);
    session.set_content_x(0.0);
    session.set_content_y(0.0);
    session.set_content_width(0.0);
    session.set_content_height(0.0);
}

/// Shared UI side of leaving a game: Session.leave, window close, and terminate.
fn finish_leave(
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    adapter: &std::rc::Weak<SlintManagerAdapter>,
    result: Result<ManagerViewModel, String>,
) {
    let icon_result = window_commands::set_icon(window.window(), None);
    let session = window.global::<Session>();
    session.set_active(false);
    session.set_fullscreen(false);
    session.set_menu_open(false);
    clear_session_frame(&session);
    let dialogs = window.global::<Dialogs>();
    dialogs.set_fatal_active(false);
    dialogs.set_fatal_retryable(false);
    window.window().set_fullscreen(false);
    match (result, icon_result) {
        (Ok(model), Ok(())) => apply_model(adapter, &model),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => report_error(window, &error),
        (Err(error), Err(cleanup)) => report_error(window, &format!("{error}; {cleanup}")),
    }
}

/// Input never ends a session: a full queue drops the event, other errors are reported.
fn report_input_error(window: &astra_emu_manager_ui_slint::ManagerWindow, error: &str) {
    if is_input_queue_full(error) {
        tracing::warn!(
            event = "astra.emu.host.input_dropped",
            diagnostic_code = "ASTRA_EMU_INPUT_QUEUE_FULL"
        );
    } else {
        report_error(window, error);
    }
}

fn is_input_queue_full(error: &str) -> bool {
    error.starts_with("ASTRA_EMU_INPUT_QUEUE_FULL")
}

/// Session-menu navigation key for a gamepad button.
fn menu_key(button: astra_emu_manager_core::GamepadInput) -> Option<slint::platform::Key> {
    use astra_emu_manager_core::GamepadInput as Pad;
    use slint::platform::Key;
    Some(match button {
        Pad::DpadUp => Key::UpArrow,
        Pad::DpadDown => Key::DownArrow,
        Pad::DpadLeft => Key::LeftArrow,
        Pad::DpadRight => Key::RightArrow,
        Pad::South => Key::Return,
        Pad::East => Key::Escape,
        _ => return None,
    })
}

fn dispatch_menu_keys(
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    keys: &[(slint::platform::Key, bool)],
) {
    for &(key, pressed) in keys {
        let text = slint::SharedString::from(key);
        window.window().dispatch_event(if pressed {
            slint::platform::WindowEvent::KeyPressed { text }
        } else {
            slint::platform::WindowEvent::KeyReleased { text }
        });
    }
}

fn apply_fatal(window: &astra_emu_manager_ui_slint::ManagerWindow, message: &str, retryable: bool) {
    let text = describe_error(message);
    let dialogs = window.global::<Dialogs>();
    dialogs.set_fatal_title(text.title.into());
    dialogs.set_fatal_detail(text.detail.into());
    dialogs.set_fatal_code(text.code.into());
    dialogs.set_fatal_retryable(retryable);
    dialogs.set_fatal_active(true);
}

fn terminate_game<C: ManagerController>(
    controller: &std::cell::RefCell<C>,
    adapter: &std::rc::Weak<SlintManagerAdapter>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    error: String,
) {
    let cleanup = controller.borrow_mut().leave_game();
    let icon_cleanup = window_commands::set_icon(window.window(), None);
    let session = window.global::<Session>();
    session.set_active(false);
    session.set_fullscreen(false);
    session.set_menu_open(false);
    clear_session_frame(&session);
    window.window().set_fullscreen(false);
    let message = match cleanup {
        Ok(model) => {
            apply_model(adapter, &model);
            error
        }
        Err(cleanup) => format!("{error}; {cleanup}"),
    };
    let message = match icon_cleanup {
        Ok(()) => message,
        Err(cleanup) => format!("{message}; {cleanup}"),
    };
    let diagnostic_code = message
        .split(':')
        .next()
        .filter(|code| {
            code.starts_with("ASTRA_")
                && code.len() <= 128
                && code
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
        .unwrap_or("ASTRA_EMU_SESSION_FAILED");
    tracing::error!(event = "astra.emu.host.game_terminated", diagnostic_code, message = %message);
    report_error(window, &message);
    apply_fatal(window, &message, false);
}
