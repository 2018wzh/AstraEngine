use super::*;

pub(super) fn install<C: ManagerController>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
) {
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
    adapter.window().on_navigate(move |page| {
        if let Some(window) = navigate_weak.upgrade() {
            window.set_current_page(page);
        }
    });
    let c = controller.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter.window().on_toggle_theme(move || {
        if let Some(window) = w.upgrade() {
            let result = {
                let mut c = c.borrow_mut();
                c.set_theme(!window.get_theme_dark())
                    .and_then(|()| c.model())
            };
            match result {
                Ok(model) => apply_model(&a, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let c = controller.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter.window().on_set_grid_columns(move |columns| {
        if let Some(window) = w.upgrade() {
            let result = {
                let mut c = c.borrow_mut();
                c.set_grid_columns(columns).and_then(|()| c.model())
            };
            match result {
                Ok(model) => apply_model(&a, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let c = controller.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter.window().on_set_theme_mode(move |mode| {
        if let Some(window) = w.upgrade() {
            let result = (|| {
                let system_dark = if mode == "system" {
                    Some(window_events::system_theme(window.window())?)
                } else {
                    None
                };
                let mut c = c.borrow_mut();
                c.set_theme_mode(mode.as_str())?;
                if let Some(dark) = system_dark {
                    c.set_system_theme(dark);
                }
                c.model()
            })();
            match result {
                Ok(model) => apply_model(&a, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    macro_rules! appearance_callback {
        ($callback:ident, $method:ident) => {{
            let c = controller.clone();
            let a = std::rc::Rc::downgrade(adapter);
            let w = adapter.window().as_weak();
            adapter.window().$callback(move |value| {
                let result = {
                    let mut c = c.borrow_mut();
                    c.$method(value.as_str()).and_then(|()| c.model())
                };
                if let Some(window) = w.upgrade() {
                    match result {
                        Ok(model) => apply_model(&a, &model),
                        Err(error) => window.set_global_diagnostic(error.into()),
                    }
                }
            });
        }};
    }
    appearance_callback!(on_set_accent, set_accent);
    appearance_callback!(on_set_density, set_density);
    appearance_callback!(on_set_view_mode, set_view_mode);
    appearance_callback!(on_set_audio_device, set_audio_device);
    // Generic config handlers
    {
        let c = controller.clone();
        let w = adapter.window().as_weak();
        adapter
            .window()
            .on_family_config_changed(move |key, value| {
                if let Err(error) = c
                    .borrow_mut()
                    .family_config_changed(key.as_str(), value.as_str())
                {
                    if let Some(window) = w.upgrade() {
                        window.set_global_diagnostic(error.into());
                    }
                }
            });
    }
    {
        let c = controller.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        adapter.window().on_save_family_config(move || {
            let result = c.borrow_mut().save_family_config();
            if let Some(window) = w.upgrade() {
                match result {
                    Ok(model) => apply_model(&a, &model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    }
    {
        let c = controller.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        adapter.window().on_reset_family_config(move || {
            let result = c.borrow_mut().reset_family_config();
            if let Some(window) = w.upgrade() {
                match result {
                    Ok(model) => apply_model(&a, &model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    }
    let sort_weak = adapter.window().as_weak();
    let sort_controller = controller.clone();
    let sort_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_set_library_sort(move |mode| {
        let Some(window) = sort_weak.upgrade() else {
            return;
        };
        match sort_controller.borrow_mut().set_library_sort(mode.as_str()) {
            Ok(model) => apply_model(&sort_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let compat_filter_weak = adapter.window().as_weak();
    let compat_filter_controller = controller.clone();
    let compat_filter_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_set_compatibility_filter(move |filter| {
        let Some(window) = compat_filter_weak.upgrade() else {
            return;
        };
        match compat_filter_controller
            .borrow_mut()
            .set_compatibility_filter(filter.as_str())
        {
            Ok(model) => apply_model(&compat_filter_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let compat_refresh_weak = adapter.window().as_weak();
    let compat_refresh_controller = controller.clone();
    let compat_refresh_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_refresh_compatibility(move || {
        let Some(window) = compat_refresh_weak.upgrade() else {
            return;
        };
        match compat_refresh_controller
            .borrow_mut()
            .refresh_compatibility()
        {
            Ok(model) => apply_model(&compat_refresh_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let release_fetch_weak = adapter.window().as_weak();
    let release_fetch_controller = controller.clone();
    let release_fetch_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_fetch_releases(move || {
        let Some(window) = release_fetch_weak.upgrade() else {
            return;
        };
        match release_fetch_controller.borrow_mut().fetch_releases() {
            Ok(model) => apply_model(&release_fetch_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let release_pin_weak = adapter.window().as_weak();
    let release_pin_controller = controller.clone();
    let release_pin_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_pin_release(move |release_id| {
        let Some(window) = release_pin_weak.upgrade() else {
            return;
        };
        match release_pin_controller
            .borrow_mut()
            .pin_release(release_id.as_str())
        {
            Ok(model) => apply_model(&release_pin_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let input_config_weak = adapter.window().as_weak();
    let input_config_controller = controller.clone();
    adapter
        .window()
        .on_save_input_config(move |gamepad_enabled, gamepad_deadzone| {
            let result = input_config_controller
                .borrow_mut()
                .save_input_config(gamepad_enabled, gamepad_deadzone.as_str());
            match result {
                Ok(()) => {}
                Err(error) => {
                    if let Some(window) = input_config_weak.upgrade() {
                        window.set_global_diagnostic(error.into());
                    }
                }
            }
        });
    let binding_weak = adapter.window().as_weak();
    let binding_controller = controller.clone();
    adapter
        .window()
        .on_set_gamepad_binding(move |button_id, key_name| {
            let result = binding_controller
                .borrow_mut()
                .set_gamepad_binding(button_id.as_str(), key_name.as_str());
            match result {
                Ok(()) => {}
                Err(error) => {
                    if let Some(window) = binding_weak.upgrade() {
                        window.set_global_diagnostic(error.into());
                    }
                }
            }
        });
    let reset_mapping_weak = adapter.window().as_weak();
    let reset_mapping_controller = controller.clone();
    let reset_mapping_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_reset_gamepad_mapping(move || {
        let result = {
            let mut controller = reset_mapping_controller.borrow_mut();
            controller
                .reset_gamepad_mapping()
                .and_then(|()| controller.model())
        };
        if let Some(window) = reset_mapping_weak.upgrade() {
            match result {
                Ok(model) => apply_model(&reset_mapping_adapter, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let save_per_game_weak = adapter.window().as_weak();
    let save_per_game_controller = controller.clone();
    let save_per_game_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .on_save_per_game_input_mapping(move |gamepad_enabled, deadzone| {
            let Some(window) = save_per_game_weak.upgrade() else {
                return;
            };
            match save_per_game_controller
                .borrow_mut()
                .save_per_game_input_mapping(gamepad_enabled, deadzone.as_str())
            {
                Ok(model) => apply_model(&save_per_game_adapter, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        });
    let clear_per_game_weak = adapter.window().as_weak();
    let clear_per_game_controller = controller.clone();
    let clear_per_game_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_clear_per_game_input_mapping(move || {
        let Some(window) = clear_per_game_weak.upgrade() else {
            return;
        };
        match clear_per_game_controller
            .borrow_mut()
            .clear_per_game_input_mapping()
        {
            Ok(model) => apply_model(&clear_per_game_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    // ===== VFS browser =====
}
