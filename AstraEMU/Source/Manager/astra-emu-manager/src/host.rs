use astra_emu_manager_ui_slint::{ManagerViewModel, SlintManagerAdapter};
use slint::ComponentHandle;
use thiserror::Error;

use crate::gamepad::GameInputPump;

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
    fn rescan(&mut self) -> Result<ManagerViewModel, String>;
    fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String>;
    fn leave_game(&mut self) -> Result<ManagerViewModel, String>;
    fn poll_platform(&mut self) -> Result<Option<ManagerViewModel>, String> {
        Ok(None)
    }
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
    let platform_timer = slint::Timer::default();
    let platform_weak = adapter.window().as_weak();
    let platform_controller = controller.clone();
    let platform_adapter = adapter.clone();
    platform_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(250),
        move || {
            let Some(window) = platform_weak.upgrade() else {
                return;
            };
            match platform_controller.borrow_mut().poll_platform() {
                Ok(Some(model)) => platform_adapter.apply(&model),
                Ok(None) => {}
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        },
    );
    let gamepad_timer = slint::Timer::default();
    let gamepad_weak = adapter.window().as_weak();
    let gamepad_controller = controller.clone();
    let mut gamepad = GameInputPump::new();
    gamepad_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(8),
        move || {
            let Some(window) = gamepad_weak.upgrade() else {
                return;
            };
            let input_blocked = !window.get_game_active()
                || window.get_translation_overlay_active()
                || window.get_diagnostics_overlay_active()
                || window.get_patches_overlay_active()
                || window.get_filters_overlay_active();
            let events = match gamepad.poll() {
                Ok(events) => events,
                Err(error) => {
                    window.set_global_diagnostic(error.into());
                    return;
                }
            };
            for event in events {
                if input_blocked {
                    continue;
                }
                if let Err(error) = gamepad_controller.borrow_mut().game_input(
                    event.control,
                    event.pressed,
                    event.value,
                ) {
                    window.set_global_diagnostic(error.into());
                    break;
                }
            }
        },
    );
    let launch_weak = adapter.window().as_weak();
    let launch_controller = controller.clone();
    let launch_adapter = adapter.clone();
    adapter.window().on_launch(move |case_id| {
        let Some(window) = launch_weak.upgrade() else {
            return;
        };
        match launch_controller.borrow_mut().launch(case_id.as_str()) {
            Ok(model) => {
                launch_adapter.apply(&model);
                window.set_game_active(true);
            }
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let leave_weak = adapter.window().as_weak();
    let leave_controller = controller.clone();
    let leave_adapter = adapter.clone();
    adapter.window().on_leave_game(move || {
        let Some(window) = leave_weak.upgrade() else {
            return;
        };
        match leave_controller.borrow_mut().leave_game() {
            Ok(model) => {
                leave_adapter.apply(&model);
                window.set_game_active(false);
            }
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
            window.set_about_active(false);
            window.set_settings_active(true);
        }
    });
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
