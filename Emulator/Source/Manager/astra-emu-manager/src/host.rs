use astra_emu_manager_core::{default_vn_preset, InputMapping};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use astra_emu_manager_ui_slint::{ManagerViewModel, SlintManagerAdapter};
use slint::ComponentHandle;
use thiserror::Error;

use crate::gamepad::GameInputPump;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranslationOverlayView {
    pub source: String,
    pub translated: String,
    pub status: String,
    pub endpoint: String,
    pub model: String,
    pub sent_scope: String,
}

pub trait AstraUnderlayRenderer: 'static {
    fn setup(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String>;
    fn stage_texture(&self) -> Option<wgpu::Texture> {
        None
    }
    fn take_stage_texture_update(&mut self) -> Option<(wgpu::Texture, u32, u32)> {
        None
    }
    fn translation_overlay(&self) -> Option<TranslationOverlayView> {
        None
    }
    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String>;
    fn teardown(&mut self);
}

pub trait ManagerController: 'static {
    fn model(&self) -> Result<ManagerViewModel, String>;
    fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String>;
    fn search(&mut self, query: &str) -> Result<ManagerViewModel, String>;
    fn configure_nls(&mut self, nls: &str) -> Result<ManagerViewModel, String>;
    #[allow(clippy::too_many_arguments)]
    fn save_translation_profile(
        &mut self,
        endpoint_kind: &str,
        endpoint: &str,
        protocol: &str,
        model: &str,
        target_language: &str,
        context_sentences: i32,
        body_limit_bytes: i32,
        timeout_ms: i32,
        background: &str,
        glossary: &str,
        secret: &str,
    ) -> Result<ManagerViewModel, String>;
    fn grant_translation_consent(&mut self) -> Result<ManagerViewModel, String>;
    fn set_translation_cache(&mut self, enabled: bool) -> Result<ManagerViewModel, String>;
    fn set_filter_preset(&mut self, preset_id: &str) -> Result<ManagerViewModel, String>;
    fn set_patch_mode(&mut self, mode: &str) -> Result<ManagerViewModel, String>;
    fn reset_translation(&mut self) -> Result<(), String>;
    fn game_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String>;
    fn save_game(&mut self) -> Result<ManagerViewModel, String>;
    fn restore_game(&mut self) -> Result<ManagerViewModel, String>;
    fn rescan(&mut self) -> Result<ManagerViewModel, String>;
    fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String>;
    fn leave_game(&mut self) -> Result<ManagerViewModel, String>;
    fn refresh_metadata(&mut self, _provider: &str) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn accept_match(&mut self, _candidate_id: &str) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn reject_match(&mut self, _candidate_id: &str) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn unlink_identity(&mut self, _provider: &str) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn link_external_id(
        &mut self,
        _provider: &str,
        _remote_id: &str,
    ) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn set_metadata_consent(
        &mut self,
        _provider: &str,
        _enabled: bool,
        _secret: &str,
    ) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn set_sensitive_cover_policy(
        &mut self,
        _provider: &str,
        _enabled: bool,
    ) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn update_bangumi_play_status(
        &mut self,
        _status: &str,
        _rating: i32,
        _note: &str,
    ) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_METADATA_NOT_CONFIGURED".into())
    }
    fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String> {
        Ok(None)
    }
    fn set_host_wake(&mut self, _wake: HostWake) {}
    /// Absolute deadline for the next fixed runtime tick.  The host schedules
    /// a single wake at this deadline; rendering never advances the runtime.
    fn runtime_deadline(&self) -> Option<Instant> {
        None
    }
    fn advance_runtime(&mut self) -> Result<Option<ManagerViewModel>, String> {
        Ok(None)
    }
    // ===== UI redesign callbacks (default implementations keep existing
    // controllers source-compatible until they opt in) =====
    /// Sidebar / bottom navigation. Pure UI state; the host applies the page
    /// switch directly and forwards the event here for optional persistence.
    fn navigate(&mut self, _page: &str) -> Result<(), String> {
        Ok(())
    }
    fn set_theme(&mut self, _dark: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_grid_columns(&mut self, _columns: i32) -> Result<(), String> {
        Ok(())
    }
    /// Library sort mode: "title" | "recent" | "play_time". Default keeps the
    /// existing ordering and simply re-renders.
    fn set_library_sort(&mut self, _mode: &str) -> Result<ManagerViewModel, String> {
        self.model()
    }
    /// Compatibility filter: "all" | "perfect" | "completable" | "flawed" |
    /// "boot_only" | "unplayable" | "unknown". Default re-renders unchanged.
    fn set_compatibility_filter(&mut self, _filter: &str) -> Result<ManagerViewModel, String> {
        self.model()
    }
    /// Queue a compatibility database refresh. Default: not configured.
    fn refresh_compatibility(&mut self) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_COMPATIBILITY_NOT_CONFIGURED".into())
    }
    fn save_input_config(
        &mut self,
        _confirm_key: &str,
        _cancel_key: &str,
        _touch_sensitivity: f32,
        _gamepad_enabled: bool,
        _gamepad_deadzone: &str,
    ) -> Result<(), String> {
        Ok(())
    }
    /// The active device-to-key mapping, used to (re)configure the gamepad pump.
    fn input_mapping(&self) -> InputMapping {
        default_vn_preset()
    }
    /// Rebind a single gamepad input to a new key name.
    fn set_gamepad_binding(&mut self, _button_id: &str, _key_name: &str) -> Result<(), String> {
        Ok(())
    }
    /// Reset the gamepad button mapping to the general-purpose VN preset.
    fn reset_gamepad_mapping(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// Save the current global input mapping as a per-game override for the
    /// selected work.
    fn save_per_game_input_mapping(&mut self) -> Result<ManagerViewModel, String> {
        self.model()
    }
    /// Clear the per-game input mapping override for the selected work.
    fn clear_per_game_input_mapping(&mut self) -> Result<ManagerViewModel, String> {
        self.model()
    }
    /// VFS browser: select a file (preview) or enter a directory. An empty
    /// path keeps the current directory and only refreshes the view.
    fn vfs_browse(&mut self, _path: &str) -> Result<ManagerViewModel, String> {
        self.model()
    }
    fn vfs_toggle_expand(&mut self, _path: &str) -> Result<ManagerViewModel, String> {
        self.model()
    }
    fn vfs_navigate_up(&mut self) -> Result<ManagerViewModel, String> {
        self.model()
    }
    fn vfs_refresh(&mut self) -> Result<ManagerViewModel, String> {
        self.model()
    }
    fn export_vfs_file(&mut self, _path: &str) -> Result<ManagerViewModel, String> {
        Err("ASTRA_EMU_VFS_EXPORT_NOT_CONFIGURED".into())
    }
    fn copy_vfs_path(&mut self, _path: &str) -> Result<(), String> {
        Ok(())
    }
}

fn fire_host_callback(slot: &HostCallbackSlot) {
    let Some(mut callback) = slot.borrow_mut().take() else {
        return;
    };
    callback();
    *slot.borrow_mut() = Some(callback);
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
) -> Result<(), HostError> {
    run_manager_with_initial_state(controller, renderer, false)
}

pub fn run_manager_with_initial_state<C: ManagerController, R: AstraUnderlayRenderer>(
    controller: C,
    renderer: R,
    game_active: bool,
) -> Result<(), HostError> {
    #[cfg(not(target_os = "android"))]
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .require_wgpu_29(slint::wgpu_29::WGPUConfiguration::default())
        .select()?;
    let adapter = std::rc::Rc::new(SlintManagerAdapter::new()?);
    adapter.apply(&controller.model().map_err(HostError::Renderer)?);
    adapter.window().set_game_active(game_active);
    let controller = std::rc::Rc::new(std::cell::RefCell::new(controller));
    let renderer = std::rc::Rc::new(std::cell::RefCell::new(renderer));
    let fatal_error = std::rc::Rc::new(std::cell::RefCell::new(None));
    let fatal_error_callback = fatal_error.clone();
    let window_weak = adapter.window().as_weak();
    let gamepad = std::rc::Rc::new(std::cell::RefCell::new(GameInputPump::new(
        controller.borrow().input_mapping(),
    )));
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
    gamepad.borrow_mut().set_wake(host_wake);
    let runtime_timer = std::rc::Rc::new(slint::Timer::default());
    let runtime_schedule: HostCallbackSlot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let runtime_schedule_slot = runtime_schedule.clone();
    let runtime_weak = adapter.window().as_weak();
    let runtime_controller = controller.clone();
    let runtime_adapter = adapter.clone();
    *runtime_schedule.borrow_mut() = Some(Box::new(move || {
        let timer = runtime_timer.clone();
        let slot = runtime_schedule_slot.clone();
        let weak = runtime_weak.clone();
        let controller = runtime_controller.clone();
        let adapter = runtime_adapter.clone();
        let Some(deadline) = controller.borrow().runtime_deadline() else {
            return;
        };
        let delay = deadline.saturating_duration_since(Instant::now());
        timer.start(slint::TimerMode::SingleShot, delay, move || {
            if let Some(window) = weak.upgrade() {
                match controller.borrow_mut().advance_runtime() {
                    Ok(Some(model)) => adapter.apply(&model),
                    Ok(None) => {}
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
            fire_host_callback(&slot);
        });
    }));
    fire_host_callback(&runtime_schedule);
    let launch_weak = adapter.window().as_weak();
    let launch_controller = controller.clone();
    let launch_adapter = adapter.clone();
    let launch_gamepad = gamepad.clone();
    let launch_runtime_schedule = runtime_schedule.clone();
    adapter.window().on_launch(move |case_id| {
        let Some(window) = launch_weak.upgrade() else {
            return;
        };
        match launch_controller.borrow_mut().launch(case_id.as_str()) {
            Ok(model) => {
                let mapping = launch_controller.borrow().input_mapping();
                launch_gamepad.borrow_mut().set_mapping(mapping);
                launch_adapter.apply(&model);
                window.set_game_active(true);
                fire_host_callback(&launch_runtime_schedule);
            }
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let leave_weak = adapter.window().as_weak();
    let leave_controller = controller.clone();
    let leave_adapter = adapter.clone();
    let leave_gamepad = gamepad.clone();
    let leave_runtime_schedule = runtime_schedule.clone();
    adapter.window().on_leave_game(move || {
        let Some(window) = leave_weak.upgrade() else {
            return;
        };
        match leave_controller.borrow_mut().leave_game() {
            Ok(model) => {
                let mapping = leave_controller.borrow().input_mapping();
                leave_gamepad.borrow_mut().set_mapping(mapping);
                leave_adapter.apply(&model);
                window.set_game_active(false);
                fire_host_callback(&leave_runtime_schedule);
            }
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let save_weak = adapter.window().as_weak();
    let save_controller = controller.clone();
    let save_adapter = adapter.clone();
    adapter.window().on_save_game(move || {
        let Some(window) = save_weak.upgrade() else {
            return;
        };
        match save_controller.borrow_mut().save_game() {
            Ok(model) => save_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let restore_weak = adapter.window().as_weak();
    let restore_controller = controller.clone();
    let restore_adapter = adapter.clone();
    adapter.window().on_restore_game(move || {
        let Some(window) = restore_weak.upgrade() else {
            return;
        };
        match restore_controller.borrow_mut().restore_game() {
            Ok(model) => restore_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let select_weak = adapter.window().as_weak();
    let select_controller = controller.clone();
    let select_adapter = adapter.clone();
    adapter.window().on_select_case(move |selected| {
        let Some(window) = select_weak.upgrade() else {
            return;
        };
        if selected.is_empty() {
            return;
        }
        match select_controller
            .borrow_mut()
            .select_case(selected.as_str())
        {
            Ok(model) => select_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let search_weak = adapter.window().as_weak();
    let search_controller = controller.clone();
    let search_adapter = adapter.clone();
    adapter.window().on_search(move |query| {
        let Some(window) = search_weak.upgrade() else {
            return;
        };
        match search_controller.borrow_mut().search(query.as_str()) {
            Ok(model) => search_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let rescan_weak = adapter.window().as_weak();
    let rescan_controller = controller.clone();
    let rescan_adapter = adapter.clone();
    adapter.window().on_rescan(move || {
        let Some(window) = rescan_weak.upgrade() else {
            return;
        };
        match rescan_controller.borrow_mut().rescan() {
            Ok(model) => rescan_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let configure_weak = adapter.window().as_weak();
    let configure_controller = controller.clone();
    let configure_adapter = adapter.clone();
    adapter.window().on_configure_nls(move |nls| {
        let Some(window) = configure_weak.upgrade() else {
            return;
        };
        match configure_controller
            .borrow_mut()
            .configure_nls(nls.as_str())
        {
            Ok(model) => configure_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let input_weak = adapter.window().as_weak();
    let input_controller = controller.clone();
    adapter
        .window()
        .on_game_input(move |control, pressed, value| {
            if let Err(error) =
                input_controller
                    .borrow_mut()
                    .game_input(control.as_str(), pressed, value)
            {
                if let Some(window) = input_weak.upgrade() {
                    window.set_global_diagnostic(error.into());
                }
            }
        });
    let save_translation_weak = adapter.window().as_weak();
    let save_translation_controller = controller.clone();
    let save_translation_adapter = adapter.clone();
    adapter.window().on_save_translation_profile(
        move |endpoint_kind,
              endpoint,
              protocol,
              model,
              target_language,
              context_sentences,
              body_limit_bytes,
              timeout_ms,
              background,
              glossary,
              secret| {
            let result = save_translation_controller
                .borrow_mut()
                .save_translation_profile(
                    endpoint_kind.as_str(),
                    endpoint.as_str(),
                    protocol.as_str(),
                    model.as_str(),
                    target_language.as_str(),
                    context_sentences,
                    body_limit_bytes,
                    timeout_ms,
                    background.as_str(),
                    glossary.as_str(),
                    secret.as_str(),
                );
            if let Some(window) = save_translation_weak.upgrade() {
                match result {
                    Ok(model) => save_translation_adapter.apply(&model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        },
    );
    let consent_weak = adapter.window().as_weak();
    let consent_controller = controller.clone();
    let consent_adapter = adapter.clone();
    adapter.window().on_grant_translation_consent(move || {
        let result = consent_controller.borrow_mut().grant_translation_consent();
        if let Some(window) = consent_weak.upgrade() {
            match result {
                Ok(model) => consent_adapter.apply(&model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let cache_weak = adapter.window().as_weak();
    let cache_controller = controller.clone();
    let cache_adapter = adapter.clone();
    adapter.window().on_set_translation_cache(move |enabled| {
        let result = cache_controller.borrow_mut().set_translation_cache(enabled);
        if let Some(window) = cache_weak.upgrade() {
            match result {
                Ok(model) => cache_adapter.apply(&model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let translation_weak = adapter.window().as_weak();
    adapter.window().on_open_translation(move || {
        if let Some(window) = translation_weak.upgrade() {
            window.set_diagnostics_overlay_active(false);
            window.set_patches_overlay_active(false);
            window.set_filters_overlay_active(false);
            window.set_translation_overlay_active(!window.get_translation_overlay_active());
        }
    });
    let diagnostics_weak = adapter.window().as_weak();
    adapter.window().on_open_diagnostics(move || {
        if let Some(window) = diagnostics_weak.upgrade() {
            window.set_translation_overlay_active(false);
            window.set_patches_overlay_active(false);
            window.set_filters_overlay_active(false);
            window.set_diagnostics_overlay_active(!window.get_diagnostics_overlay_active());
        }
    });
    let patches_weak = adapter.window().as_weak();
    adapter.window().on_open_patches(move || {
        if let Some(window) = patches_weak.upgrade() {
            window.set_translation_overlay_active(false);
            window.set_diagnostics_overlay_active(false);
            window.set_filters_overlay_active(false);
            window.set_patches_overlay_active(!window.get_patches_overlay_active());
        }
    });
    let patch_mode_weak = adapter.window().as_weak();
    let patch_mode_controller = controller.clone();
    let patch_mode_adapter = adapter.clone();
    adapter.window().on_set_patch_mode(move |mode| {
        let result = patch_mode_controller
            .borrow_mut()
            .set_patch_mode(mode.as_str());
        if let Some(window) = patch_mode_weak.upgrade() {
            match result {
                Ok(model) => patch_mode_adapter.apply(&model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let filters_weak = adapter.window().as_weak();
    adapter.window().on_open_filters(move || {
        if let Some(window) = filters_weak.upgrade() {
            window.set_translation_overlay_active(false);
            window.set_diagnostics_overlay_active(false);
            window.set_patches_overlay_active(false);
            window.set_filters_overlay_active(!window.get_filters_overlay_active());
        }
    });
    let filter_weak = adapter.window().as_weak();
    let filter_controller = controller.clone();
    let filter_adapter = adapter.clone();
    adapter.window().on_set_filter_preset(move |preset| {
        let result = filter_controller
            .borrow_mut()
            .set_filter_preset(preset.as_str());
        if let Some(window) = filter_weak.upgrade() {
            match result {
                Ok(model) => filter_adapter.apply(&model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let reset_translation_weak = adapter.window().as_weak();
    let reset_translation_controller = controller.clone();
    adapter.window().on_reset_translation(move || {
        if let Err(error) = reset_translation_controller
            .borrow_mut()
            .reset_translation()
        {
            if let Some(window) = reset_translation_weak.upgrade() {
                window.set_global_diagnostic(error.into());
            }
        }
    });
    let settings_weak = adapter.window().as_weak();
    adapter.window().on_open_settings(move || {
        if let Some(window) = settings_weak.upgrade() {
            window.set_current_page("settings".into());
            window.set_about_active(false);
            window.set_settings_active(true);
        }
    });
    // ===== Navigation / theme / appearance =====
    let navigate_weak = adapter.window().as_weak();
    let navigate_controller = controller.clone();
    adapter.window().on_navigate(move |page| {
        if let Some(window) = navigate_weak.upgrade() {
            window.set_current_page(page.clone());
        }
        let _ = navigate_controller.borrow_mut().navigate(page.as_str());
    });
    let theme_weak = adapter.window().as_weak();
    let theme_controller = controller.clone();
    adapter.window().on_toggle_theme(move || {
        if let Some(window) = theme_weak.upgrade() {
            let dark = !window.get_theme_dark();
            window.set_theme_dark(dark);
            let _ = theme_controller.borrow_mut().set_theme(dark);
        }
    });
    let grid_weak = adapter.window().as_weak();
    let grid_controller = controller.clone();
    adapter.window().on_set_grid_columns(move |columns| {
        if let Some(window) = grid_weak.upgrade() {
            let _ = grid_controller.borrow_mut().set_grid_columns(columns);
            window.set_grid_columns(columns);
        }
    });
    let sort_weak = adapter.window().as_weak();
    let sort_controller = controller.clone();
    let sort_adapter = adapter.clone();
    adapter.window().on_set_library_sort(move |mode| {
        let Some(window) = sort_weak.upgrade() else {
            return;
        };
        match sort_controller.borrow_mut().set_library_sort(mode.as_str()) {
            Ok(model) => sort_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let compat_filter_weak = adapter.window().as_weak();
    let compat_filter_controller = controller.clone();
    let compat_filter_adapter = adapter.clone();
    adapter.window().on_set_compatibility_filter(move |filter| {
        let Some(window) = compat_filter_weak.upgrade() else {
            return;
        };
        match compat_filter_controller
            .borrow_mut()
            .set_compatibility_filter(filter.as_str())
        {
            Ok(model) => compat_filter_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let compat_refresh_weak = adapter.window().as_weak();
    let compat_refresh_controller = controller.clone();
    let compat_refresh_adapter = adapter.clone();
    adapter.window().on_refresh_compatibility(move || {
        let Some(window) = compat_refresh_weak.upgrade() else {
            return;
        };
        match compat_refresh_controller
            .borrow_mut()
            .refresh_compatibility()
        {
            Ok(model) => compat_refresh_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let input_config_weak = adapter.window().as_weak();
    let input_config_controller = controller.clone();
    let input_config_gamepad = gamepad.clone();
    adapter.window().on_save_input_config(
        move |confirm_key, cancel_key, touch_sensitivity, gamepad_enabled, gamepad_deadzone| {
            let result = input_config_controller.borrow_mut().save_input_config(
                confirm_key.as_str(),
                cancel_key.as_str(),
                touch_sensitivity,
                gamepad_enabled,
                gamepad_deadzone.as_str(),
            );
            match result {
                Ok(()) => {
                    let mapping = input_config_controller.borrow().input_mapping();
                    input_config_gamepad.borrow_mut().set_mapping(mapping);
                }
                Err(error) => {
                    if let Some(window) = input_config_weak.upgrade() {
                        window.set_global_diagnostic(error.into());
                    }
                }
            }
        },
    );
    let binding_weak = adapter.window().as_weak();
    let binding_controller = controller.clone();
    let binding_gamepad = gamepad.clone();
    adapter
        .window()
        .on_set_gamepad_binding(move |button_id, key_name| {
            let result = binding_controller
                .borrow_mut()
                .set_gamepad_binding(button_id.as_str(), key_name.as_str());
            match result {
                Ok(()) => {
                    let mapping = binding_controller.borrow().input_mapping();
                    binding_gamepad.borrow_mut().set_mapping(mapping);
                }
                Err(error) => {
                    if let Some(window) = binding_weak.upgrade() {
                        window.set_global_diagnostic(error.into());
                    }
                }
            }
        });
    let reset_mapping_weak = adapter.window().as_weak();
    let reset_mapping_controller = controller.clone();
    let reset_mapping_gamepad = gamepad.clone();
    adapter.window().on_reset_gamepad_mapping(move || {
        let result = reset_mapping_controller
            .borrow_mut()
            .reset_gamepad_mapping();
        match result {
            Ok(()) => {
                let mapping = reset_mapping_controller.borrow().input_mapping();
                reset_mapping_gamepad.borrow_mut().set_mapping(mapping);
            }
            Err(error) => {
                if let Some(window) = reset_mapping_weak.upgrade() {
                    window.set_global_diagnostic(error.into());
                }
            }
        }
    });
    let save_per_game_weak = adapter.window().as_weak();
    let save_per_game_controller = controller.clone();
    let save_per_game_adapter = adapter.clone();
    adapter.window().on_save_per_game_input_mapping(move || {
        let Some(window) = save_per_game_weak.upgrade() else {
            return;
        };
        match save_per_game_controller
            .borrow_mut()
            .save_per_game_input_mapping()
        {
            Ok(model) => save_per_game_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let clear_per_game_weak = adapter.window().as_weak();
    let clear_per_game_controller = controller.clone();
    let clear_per_game_adapter = adapter.clone();
    adapter.window().on_clear_per_game_input_mapping(move || {
        let Some(window) = clear_per_game_weak.upgrade() else {
            return;
        };
        match clear_per_game_controller
            .borrow_mut()
            .clear_per_game_input_mapping()
        {
            Ok(model) => clear_per_game_adapter.apply(&model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    // ===== VFS browser =====
    let open_vfs_weak = adapter.window().as_weak();
    let open_vfs_controller = controller.clone();
    let open_vfs_adapter = adapter.clone();
    adapter.window().on_open_vfs(move || {
        if let Some(window) = open_vfs_weak.upgrade() {
            window.set_current_page("vfs".into());
        }
        match open_vfs_controller.borrow_mut().vfs_browse("") {
            Ok(model) => open_vfs_adapter.apply(&model),
            Err(error) => {
                if let Some(window) = open_vfs_weak.upgrade() {
                    window.set_global_diagnostic(error.into());
                }
            }
        }
    });
    macro_rules! vfs_model_callback {
        ($on:ident, $method:ident) => {{
            let weak = adapter.window().as_weak();
            let callback_controller = controller.clone();
            let callback_adapter = adapter.clone();
            adapter.window().$on(move |path| {
                let result = callback_controller.borrow_mut().$method(path.as_str());
                if let Some(window) = weak.upgrade() {
                    match result {
                        Ok(model) => callback_adapter.apply(&model),
                        Err(error) => window.set_global_diagnostic(error.into()),
                    }
                }
            });
        }};
    }
    vfs_model_callback!(on_vfs_select_entry, vfs_browse);
    vfs_model_callback!(on_vfs_toggle_expand, vfs_toggle_expand);
    vfs_model_callback!(on_vfs_export_file, export_vfs_file);
    let vfs_up_weak = adapter.window().as_weak();
    let vfs_up_controller = controller.clone();
    let vfs_up_adapter = adapter.clone();
    adapter.window().on_vfs_navigate_up(move || {
        let result = vfs_up_controller.borrow_mut().vfs_navigate_up();
        if let Some(window) = vfs_up_weak.upgrade() {
            match result {
                Ok(model) => vfs_up_adapter.apply(&model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let vfs_refresh_weak = adapter.window().as_weak();
    let vfs_refresh_controller = controller.clone();
    let vfs_refresh_adapter = adapter.clone();
    adapter.window().on_vfs_refresh(move || {
        let result = vfs_refresh_controller.borrow_mut().vfs_refresh();
        if let Some(window) = vfs_refresh_weak.upgrade() {
            match result {
                Ok(model) => vfs_refresh_adapter.apply(&model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let copy_path_controller = controller.clone();
    adapter.window().on_vfs_copy_path(move |path| {
        let _ = copy_path_controller
            .borrow_mut()
            .copy_vfs_path(path.as_str());
    });
    macro_rules! metadata_callback {
        ($callback:ident, $method:ident, |$($arg:ident),*|) => {{
            let weak = adapter.window().as_weak();
            let callback_controller = controller.clone();
            let callback_adapter = adapter.clone();
            adapter.window().$callback(move |$($arg),*| {
                let result = callback_controller.borrow_mut().$method($($arg.as_str()),*);
                if let Some(window) = weak.upgrade() {
                    match result {
                        Ok(model) => callback_adapter.apply(&model),
                        Err(error) => window.set_global_diagnostic(error.into()),
                    }
                }
            });
        }};
    }
    metadata_callback!(on_refresh_metadata, refresh_metadata, |provider|);
    metadata_callback!(on_accept_match, accept_match, |candidate_id|);
    metadata_callback!(on_reject_match, reject_match, |candidate_id|);
    metadata_callback!(on_unlink_identity, unlink_identity, |provider|);
    metadata_callback!(on_link_external_id, link_external_id, |provider, remote_id|);
    let consent_weak = adapter.window().as_weak();
    let metadata_consent_controller = controller.clone();
    let metadata_consent_adapter = adapter.clone();
    adapter
        .window()
        .on_set_metadata_consent(move |provider, enabled, secret| {
            let result = metadata_consent_controller
                .borrow_mut()
                .set_metadata_consent(provider.as_str(), enabled, secret.as_str());
            if let Some(window) = consent_weak.upgrade() {
                match result {
                    Ok(model) => metadata_consent_adapter.apply(&model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    let cover_weak = adapter.window().as_weak();
    let cover_controller = controller.clone();
    let cover_adapter = adapter.clone();
    adapter
        .window()
        .on_set_sensitive_cover_policy(move |provider, enabled| {
            let result = cover_controller
                .borrow_mut()
                .set_sensitive_cover_policy(provider.as_str(), enabled);
            if let Some(window) = cover_weak.upgrade() {
                match result {
                    Ok(model) => cover_adapter.apply(&model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    let play_weak = adapter.window().as_weak();
    let play_controller = controller.clone();
    let play_adapter = adapter.clone();
    adapter
        .window()
        .on_update_bangumi_play_status(move |status, rating, note| {
            let result = play_controller.borrow_mut().update_bangumi_play_status(
                status.as_str(),
                rating,
                note.as_str(),
            );
            if let Some(window) = play_weak.upgrade() {
                match result {
                    Ok(model) => play_adapter.apply(&model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    // Worker completions and gamepad edges only wake the Slint event loop. The
    // actual drain is performed here on the UI thread immediately before the
    // underlay render, so no controller/UI object crosses a worker boundary.
    let event_controller = controller.clone();
    let event_adapter = adapter.clone();
    let event_gamepad = gamepad.clone();
    let event_window = adapter.window().as_weak();
    let event_pending = async_events_pending.clone();
    let renderer_callback = renderer.clone();
    adapter.window().window().set_rendering_notifier(move |state, api| {
        let slint::GraphicsAPI::WGPU29 { device, queue, .. } = api else {
            record_fatal(&fatal_error_callback, "rendering notifier did not provide WGPU 29".into());
            let _ = slint::quit_event_loop();
            return;
        };
        let context = WgpuFrameContext { device, queue };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match state {
            slint::RenderingState::RenderingSetup => {
                renderer_callback.borrow_mut().setup(context)?;
                if let Some(texture) = renderer_callback.borrow().stage_texture() {
                    let image = slint::Image::try_from(texture).map_err(|_| "WGPU stage texture import failed".to_string())?;
                    window_weak.upgrade().ok_or_else(|| "Manager window disappeared during renderer setup".to_string())?.set_stage_frame(image);
                }
                Ok(())
            }
            slint::RenderingState::BeforeRendering => {
                if let Some(window) = event_window.upgrade() {
                    if event_pending.swap(false, Ordering::Acquire) {
                        match event_controller.borrow_mut().poll_platform() {
                            Ok(Some(model)) => event_adapter.apply(&model),
                            Ok(None) => {}
                            Err(error) => window.set_global_diagnostic(error.into()),
                        }
                        let input_blocked = !window.get_game_active()
                            || window.get_translation_overlay_active()
                            || window.get_diagnostics_overlay_active()
                            || window.get_patches_overlay_active()
                            || window.get_filters_overlay_active();
                        match event_gamepad.borrow_mut().poll() {
                            Ok(events) => {
                                if !input_blocked {
                                    for event in events {
                                        if let Err(error) = event_controller.borrow_mut().game_input(
                                            &event.control,
                                            event.pressed,
                                            event.value,
                                        ) {
                                            window.set_global_diagnostic(error.into());
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(error) => window.set_global_diagnostic(error.into()),
                        }
                    }
                }
                let mut renderer = renderer_callback.borrow_mut();
                renderer.render(context)?;
                if let Some((texture, width, height)) = renderer.take_stage_texture_update() {
                    let image = slint::Image::try_from(texture).map_err(|_| "WGPU stage texture import failed".to_string())?;
                    let window = window_weak.upgrade().ok_or_else(|| "Manager window disappeared during texture update".to_string())?;
                    window.set_stage_frame(image);
                    window.set_stage_native_width(width as f32);
                    window.set_stage_native_height(height as f32);
                }
                if let Some(overlay) = renderer.translation_overlay() {
                    if let Some(window) = window_weak.upgrade() {
                        window.set_translation_source(overlay.source.into());
                        window.set_translation_output(overlay.translated.into());
                        window.set_translation_status(overlay.status.into());
                        window.set_translation_endpoint(overlay.endpoint.into());
                        window.set_translation_model(overlay.model.into());
                        window.set_translation_scope(overlay.sent_scope.into());
                    }
                }
                Ok(())
            },
            slint::RenderingState::RenderingTeardown => { renderer_callback.borrow_mut().teardown(); Ok(()) }
            _ => Ok(()),
        })).unwrap_or_else(|_| Err("underlay renderer panicked".into()));
        if let Err(error) = result {
            tracing::error!(event = "astra.emu.host.renderer_failed", diagnostic_code = "ASTRA_EMU_HOST_RENDERER", message = %error);
            record_fatal(&fatal_error_callback, error);
            let _ = slint::quit_event_loop();
        } else if matches!(state, slint::RenderingState::BeforeRendering) {
            if let Some(window) = window_weak.upgrade() {
                if window.get_game_active() {
                    window.window().request_redraw();
                }
            }
        }
    }).map_err(|error| HostError::Renderer(error.to_string()))?;
    adapter.window().run()?;
    if let Some(error) = fatal_error.borrow_mut().take() {
        return Err(HostError::Renderer(error));
    }
    Ok(())
}

fn record_fatal(slot: &std::cell::RefCell<Option<String>>, error: String) {
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(error);
    }
}
