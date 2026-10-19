use super::*;

pub(super) fn install<C: ManagerController>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
) {
    let c = controller.clone();
    let a = std::rc::Rc::downgrade(adapter);
    let w = adapter.window().as_weak();
    adapter
        .window()
        .global::<App>()
        .on_appearance_changed(move |appearance| {
            let settings = astra_emu_manager_core::AppearanceSettings {
                theme_mode: appearance.theme_mode.to_string(),
                density: appearance.density.to_string(),
                cover_size: appearance.cover_size.to_string(),
                view_mode: appearance.view_mode.to_string(),
                reduce_motion: appearance.reduce_motion,
                cover_badges: appearance.cover_badges,
            };
            let result = c.borrow_mut().set_appearance(settings);
            if let Some(window) = w.upgrade() {
                match result {
                    Ok(model) => apply_model(&a, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    {
        let c = controller.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        adapter
            .window()
            .global::<Selection>()
            .on_config_changed(move |key, value| {
                let result = {
                    let mut controller = c.borrow_mut();
                    controller
                        .family_config_changed(key.as_str(), value.as_str())
                        .and_then(|()| {
                            if matches!(
                                key.as_str(),
                                "manager.emulator_id" | "manager.config_scope"
                            ) {
                                controller.model().map(Some)
                            } else {
                                Ok(None)
                            }
                        })
                };
                if let Some(window) = w.upgrade() {
                    match result {
                        Ok(Some(model)) => apply_model(&a, &model),
                        Ok(None) => {}
                        Err(error) => report_error(&window, &error),
                    }
                }
            });
    }
    {
        let c = controller.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        adapter
            .window()
            .global::<Selection>()
            .on_save_config(move || {
                let result = c.borrow_mut().save_family_config();
                if let Some(window) = w.upgrade() {
                    match result {
                        Ok(model) => apply_model(&a, &model),
                        Err(error) => report_error(&window, &error),
                    }
                }
            });
    }
    {
        let c = controller.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        adapter
            .window()
            .global::<Selection>()
            .on_reset_config(move || {
                let result = c.borrow_mut().reset_family_config();
                if let Some(window) = w.upgrade() {
                    match result {
                        Ok(model) => apply_model(&a, &model),
                        Err(error) => report_error(&window, &error),
                    }
                }
            });
    }
    {
        let c = controller.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        adapter
            .window()
            .global::<Selection>()
            .on_pick_musica_key(move || {
                let result = c.borrow_mut().select_musica_key_file();
                if let Some(window) = w.upgrade() {
                    match result {
                        Ok(model) => apply_model(&a, &model),
                        Err(error) => report_error(&window, &error),
                    }
                }
            });
    }
    let sort_weak = adapter.window().as_weak();
    let sort_controller = controller.clone();
    let sort_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Library>()
        .on_set_sort(move |mode| {
            let Some(window) = sort_weak.upgrade() else {
                return;
            };
            match sort_controller.borrow_mut().set_library_sort(mode.as_str()) {
                Ok(model) => apply_model(&sort_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let compat_filter_weak = adapter.window().as_weak();
    let compat_filter_controller = controller.clone();
    let compat_filter_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Library>()
        .on_set_compat_filter(move |filter| {
            let Some(window) = compat_filter_weak.upgrade() else {
                return;
            };
            match compat_filter_controller
                .borrow_mut()
                .set_compatibility_filter(filter.as_str())
            {
                Ok(model) => apply_model(&compat_filter_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let compat_refresh_weak = adapter.window().as_weak();
    let compat_refresh_controller = controller.clone();
    let compat_refresh_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Selection>()
        .on_refresh_compat(move || {
            let Some(window) = compat_refresh_weak.upgrade() else {
                return;
            };
            match compat_refresh_controller
                .borrow_mut()
                .refresh_compatibility()
            {
                Ok(model) => apply_model(&compat_refresh_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let release_fetch_weak = adapter.window().as_weak();
    let release_fetch_controller = controller.clone();
    let release_fetch_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Selection>()
        .on_fetch_releases(move || {
            let Some(window) = release_fetch_weak.upgrade() else {
                return;
            };
            match release_fetch_controller.borrow_mut().fetch_releases() {
                Ok(model) => apply_model(&release_fetch_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let release_pin_weak = adapter.window().as_weak();
    let release_pin_controller = controller.clone();
    let release_pin_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Selection>()
        .on_pin_release(move |release_id| {
            let Some(window) = release_pin_weak.upgrade() else {
                return;
            };
            match release_pin_controller
                .borrow_mut()
                .pin_release(release_id.as_str())
            {
                Ok(model) => apply_model(&release_pin_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let input_config_weak = adapter.window().as_weak();
    let input_config_controller = controller.clone();
    adapter.window().global::<Settings>().on_save_input(
        move |gamepad_enabled, gamepad_deadzone| {
            let result = input_config_controller
                .borrow_mut()
                .save_input_config(gamepad_enabled, gamepad_deadzone.as_str());
            match result {
                Ok(()) => {}
                Err(error) => {
                    if let Some(window) = input_config_weak.upgrade() {
                        report_error(&window, &error);
                    }
                }
            }
        },
    );
    let binding_weak = adapter.window().as_weak();
    let binding_controller = controller.clone();
    adapter
        .window()
        .global::<Settings>()
        .on_bind(move |button_id, key_name| {
            let result = binding_controller
                .borrow_mut()
                .set_gamepad_binding(button_id.as_str(), key_name.as_str());
            match result {
                Ok(()) => {}
                Err(error) => {
                    if let Some(window) = binding_weak.upgrade() {
                        report_error(&window, &error);
                    }
                }
            }
        });
    let reset_mapping_weak = adapter.window().as_weak();
    let reset_mapping_controller = controller.clone();
    let reset_mapping_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Settings>()
        .on_reset_bindings(move || {
            let result = {
                let mut controller = reset_mapping_controller.borrow_mut();
                controller
                    .reset_gamepad_mapping()
                    .and_then(|()| controller.model())
            };
            if let Some(window) = reset_mapping_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&reset_mapping_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let save_per_game_weak = adapter.window().as_weak();
    let save_per_game_controller = controller.clone();
    let save_per_game_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Selection>()
        .on_save_game_input(move |gamepad_enabled, deadzone| {
            let Some(window) = save_per_game_weak.upgrade() else {
                return;
            };
            match save_per_game_controller
                .borrow_mut()
                .save_per_game_input_mapping(gamepad_enabled, deadzone.as_str())
            {
                Ok(model) => apply_model(&save_per_game_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let clear_per_game_weak = adapter.window().as_weak();
    let clear_per_game_controller = controller.clone();
    let clear_per_game_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Selection>()
        .on_clear_game_input(move || {
            let Some(window) = clear_per_game_weak.upgrade() else {
                return;
            };
            match clear_per_game_controller
                .borrow_mut()
                .clear_per_game_input_mapping()
            {
                Ok(model) => apply_model(&clear_per_game_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
}
