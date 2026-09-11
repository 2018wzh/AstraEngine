use astra_emu_manager_core::InputMapping;
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

#[path = "host_filters.rs"]
mod filters;
#[path = "host_game_callbacks.rs"]
mod game_callbacks;
#[path = "host_library_callbacks.rs"]
mod library_callbacks;
#[path = "host_settings_callbacks.rs"]
mod settings_callbacks;
#[path = "host_events.rs"]
mod window_events;

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

pub trait AstraUnderlayRenderer: 'static {
    fn configure_filter(
        &mut self,
        _config: &crate::effects::FilterConfiguration,
        _source: Option<&str>,
    ) -> Result<(), String> {
        Err("ASTRA_EMU_FILTER_RENDERER_NOT_CONFIGURED".into())
    }
    fn setup(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String>;
    fn stage_texture(&self) -> Option<wgpu::Texture> {
        None
    }
    fn take_stage_texture_update(&mut self) -> Option<(wgpu::Texture, u32, u32)> {
        None
    }
    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String>;
    fn teardown(&mut self);
}

pub trait ManagerController: 'static {
    fn set_window_state(&mut self, state: astra_emu_family_api::WindowState) -> Result<(), String>;
    fn physical_event(&mut self, event: astra_emu_family_api::FamilyEvent) -> Result<(), String>;
    fn is_game_active(&self) -> bool;
    fn model(&self) -> Result<ManagerViewModel, String>;
    fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String>;
    fn search(&mut self, query: &str) -> Result<ManagerViewModel, String>;
    #[allow(clippy::too_many_arguments)]
    fn save_translation_profile(
        &mut self,
        endpoint_kind: &str,
        endpoint: &str,
        protocol: &str,
        model: &str,
        target_language: &str,

        timeout_ms: i32,

        secret: &str,
    ) -> Result<ManagerViewModel, String>;
    fn grant_translation_consent(&mut self) -> Result<ManagerViewModel, String>;
    fn test_translation_connection(&mut self) -> Result<ManagerViewModel, String>;
    fn add_game_directory(&mut self, path: &std::path::Path) -> Result<ManagerViewModel, String>;
    fn install_family_plugin(&mut self, path: &std::path::Path)
        -> Result<ManagerViewModel, String>;
    fn filter_settings(&self) -> astra_emu_manager_core::FilterSettings;
    fn pending_filter_settings(&self) -> Result<astra_emu_manager_core::FilterSettings, String>;
    fn commit_filter_settings(
        &mut self,
        settings: astra_emu_manager_core::FilterSettings,
    ) -> Result<ManagerViewModel, String>;
    fn game_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String>;
    /// Clear physical keys and modifiers when focus leaves the game surface
    /// so a later focus cannot inherit a stuck input state.
    fn release_inputs(&mut self) -> Result<(), String>;
    fn rescan(&mut self) -> Result<ManagerViewModel, String>;
    fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String>;
    fn leave_game(&mut self) -> Result<ManagerViewModel, String>;
    fn search_metadata(&mut self, provider: &str, query: &str) -> Result<ManagerViewModel, String>;
    fn refresh_metadata(&mut self, _provider: &str) -> Result<ManagerViewModel, String>;
    fn accept_match(&mut self, _candidate_id: &str) -> Result<ManagerViewModel, String>;
    fn unlink_identity(&mut self, _provider: &str) -> Result<ManagerViewModel, String>;
    fn set_metadata_consent(
        &mut self,
        _provider: &str,
        _enabled: bool,
        _secret: &str,
    ) -> Result<ManagerViewModel, String>;
    fn set_sensitive_cover_policy(
        &mut self,
        _provider: &str,
        _enabled: bool,
    ) -> Result<ManagerViewModel, String>;
    fn update_bangumi_play_status(
        &mut self,
        _status: &str,
        _rating: i32,
        _note: &str,
    ) -> Result<ManagerViewModel, String>;
    fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String>;
    fn set_host_wake(&mut self, _wake: HostWake);
    /// Absolute deadline for the next fixed runtime tick.  The host schedules
    /// a single wake at this deadline; rendering never advances the runtime.
    fn runtime_deadline(&self) -> Option<Instant>;
    fn advance_runtime(&mut self) -> Result<Option<ManagerViewModel>, String>;
    /// Update the platform theme used by automatic appearance mode.
    fn set_system_theme(&mut self, dark: bool);
    fn set_theme(&mut self, _dark: bool) -> Result<(), String>;
    fn set_grid_columns(&mut self, _columns: i32) -> Result<(), String>;
    fn set_theme_mode(&mut self, _mode: &str) -> Result<(), String>;
    fn set_accent(&mut self, _accent: &str) -> Result<(), String>;
    fn set_density(&mut self, _density: &str) -> Result<(), String>;
    fn set_view_mode(&mut self, _view_mode: &str) -> Result<(), String>;
    fn set_audio_device(&mut self, device: &str) -> Result<(), String>;
    fn family_config_changed(&mut self, _key: &str, _value: &str) -> Result<(), String>;
    fn save_family_config(&mut self) -> Result<ManagerViewModel, String>;
    fn reset_family_config(&mut self) -> Result<ManagerViewModel, String>;
    fn filter_config_changed(&mut self, _key: &str, _value: &str) -> Result<(), String>;
    fn reset_filter_config(&mut self) -> Result<ManagerViewModel, String>;
    /// Library sort mode: "title" | "recent" | "play_time".
    fn set_library_sort(&mut self, _mode: &str) -> Result<ManagerViewModel, String>;
    /// Compatibility filter: "all" | "perfect" | "completable" | "flawed" |
    /// "boot_only" | "unplayable" | "unknown".
    fn set_compatibility_filter(&mut self, _filter: &str) -> Result<ManagerViewModel, String>;
    /// Queue a compatibility database refresh.
    fn refresh_compatibility(&mut self) -> Result<ManagerViewModel, String>;
    /// Fetch the VNDB releases (rIDs) of the selected work so its local
    /// installation can be pinned to a specific game version.
    fn fetch_releases(&mut self) -> Result<ManagerViewModel, String>;
    /// Pin the selected installation to a specific VNDB release (rID).
    fn pin_release(&mut self, _release_id: &str) -> Result<ManagerViewModel, String>;
    fn save_input_config(
        &mut self,

        _gamepad_enabled: bool,
        _gamepad_deadzone: &str,
    ) -> Result<(), String>;
    /// The active device-to-key mapping, used to (re)configure the gamepad pump.
    fn input_mapping(&self) -> InputMapping;
    /// Rebind a single gamepad input to a new key name.
    fn set_gamepad_binding(&mut self, _button_id: &str, _key_name: &str) -> Result<(), String>;
    /// Reset the gamepad button mapping to the general-purpose VN preset.
    fn reset_gamepad_mapping(&mut self) -> Result<(), String>;
    /// Save the current global input mapping as a per-game override for the
    /// selected work.
    fn save_per_game_input_mapping(
        &mut self,
        gamepad_enabled: bool,
        deadzone: &str,
    ) -> Result<ManagerViewModel, String>;
    /// Clear the per-game input mapping override for the selected work.
    fn clear_per_game_input_mapping(&mut self) -> Result<ManagerViewModel, String>;
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
    {
        let mut settings = slint::wgpu_29::WGPUSettings::default();
        settings.power_preference = wgpu::PowerPreference::HighPerformance;
        settings.device_memory_hints = wgpu::MemoryHints::Performance;
        // The final-frame effects use compute and storage textures; Slint's UI-only
        // WebGL2 defaults expose neither capability.
        settings.device_required_limits = wgpu::Limits::default();
        settings.device_required_features |= crate::effects::FilterEngine::required_features();
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .require_wgpu_29(slint::wgpu_29::WGPUConfiguration::Automatic(settings))
            .select()?;
    }
    let adapter = std::rc::Rc::new(SlintManagerAdapter::new()?);
    adapter.apply(&controller.model().map_err(HostError::Renderer)?);
    adapter.window().set_game_active(game_active);
    let controller = std::rc::Rc::new(std::cell::RefCell::new(controller));
    let renderer = std::rc::Rc::new(std::cell::RefCell::new(renderer));
    let fatal_error = std::rc::Rc::new(std::cell::RefCell::new(None));
    let fatal_error_callback = fatal_error.clone();
    let window_weak = adapter.window().as_weak();
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
    window_events::install(&adapter, controller.clone());
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
    *runtime_schedule.borrow_mut() = Some(Box::new(move || {
        let timer = schedule_timer.clone();
        let slot = runtime_schedule_slot.clone();
        let weak = runtime_weak.clone();
        let controller = runtime_controller.clone();
        let adapter = runtime_adapter.clone();
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
                window.set_game_active(controller.borrow().is_game_active());
                window.window().request_redraw();
            }
            if let Some(slot) = slot.upgrade() {
                fire_host_callback(&slot);
            }
        });
    }));
    fire_host_callback(&runtime_schedule);
    game_callbacks::install(
        &adapter,
        &controller,
        &gamepad,
        &renderer,
        &runtime_schedule,
    );
    settings_callbacks::install(&adapter, &controller);
    library_callbacks::install(&adapter, &controller);
    let event_controller = controller.clone();
    let event_adapter = std::rc::Rc::downgrade(&adapter);
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
                if let Some(window) = event_window.upgrade() {
                    if window.get_theme_mode() == "system" {
                        let dark = window_events::system_theme(window.window())?;
                        event_controller.borrow_mut().set_system_theme(dark);
                        window.set_theme_dark(dark);
                    }
                }
                filters::configure(&mut *renderer_callback.borrow_mut(), &event_controller.borrow().filter_settings())?;
                if let Some(texture) = renderer_callback.borrow().stage_texture() {
                    let image = slint::Image::try_from(texture).map_err(|_| "WGPU stage texture import failed".to_string())?;
                    window_weak.upgrade().ok_or_else(|| "Manager window disappeared during renderer setup".to_string())?.set_stage_frame(image);
                }
                Ok(())
            }
            slint::RenderingState::BeforeRendering => {
                if let Some(window) = event_window.upgrade() {
                    let input_blocked = !window.get_game_active()
                        || window.get_translation_overlay_active()
                        || window.get_diagnostics_overlay_active()
                        || window.get_filters_overlay_active();
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
                                if !input_blocked && window.get_game_active() {
                                    for event in events {
                                        let input = event_controller.borrow_mut().game_input(
                                            &event.control,
                                            event.pressed,
                                            event.value,
                                        );
                                        if let Err(error) = input {
                                            terminate_game(&event_controller, &event_adapter, &window, error);
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                if window.get_game_active() { terminate_game(&event_controller, &event_adapter, &window, error); }
                                else { window.set_global_diagnostic(error.into()); }
                            }
                        }
                    }
                }
                let mut renderer = renderer_callback.borrow_mut();
                renderer.render(context)?;
                if let Some((texture, width, height)) = renderer.take_stage_texture_update() {
                    let aspect = texture.width() as f32 / texture.height() as f32;
                    let image = slint::Image::try_from(texture).map_err(|_| "WGPU stage texture import failed".to_string())?;
                    let window = window_weak.upgrade().ok_or_else(|| "Manager window disappeared during texture update".to_string())?;
                    window.set_stage_frame(image);
                    window.set_stage_output_aspect(aspect);
                    window.set_stage_native_width(width as f32);
                    window.set_stage_native_height(height as f32);
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
        }
    }).map_err(|error| HostError::Renderer(error.to_string()))?;
    let run_result = adapter.window().run();
    runtime_timer.stop();
    runtime_schedule.borrow_mut().take();
    let shutdown_result = controller.borrow_mut().leave_game();
    run_result?;
    shutdown_result.map_err(HostError::Renderer)?;
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

fn apply_model(adapter: &std::rc::Weak<SlintManagerAdapter>, model: &ManagerViewModel) {
    if let Some(adapter) = adapter.upgrade() {
        adapter.apply(model);
    }
}

fn terminate_game<C: ManagerController>(
    controller: &std::cell::RefCell<C>,
    adapter: &std::rc::Weak<SlintManagerAdapter>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    error: String,
) {
    let cleanup = controller.borrow_mut().leave_game();
    window.set_game_active(false);
    let message = match cleanup {
        Ok(model) => {
            apply_model(adapter, &model);
            error
        }
        Err(cleanup) => format!("{error}; {cleanup}"),
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
    tracing::error!(event = "astra.emu.host.game_terminated", diagnostic_code);
    window.set_global_diagnostic(message.clone().into());
    window.set_fatal_error_message(message.into());
    window.set_fatal_error_active(true);
}
