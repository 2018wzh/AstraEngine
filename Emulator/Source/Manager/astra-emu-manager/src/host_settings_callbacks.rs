use super::*;

pub(super) fn install<C: ManagerController>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
    gamepad: &std::rc::Rc<std::cell::RefCell<GameInputPump>>,
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
                let Some(window) = w.upgrade() else {
                    return;
                };
                let game_id = window.global::<Selection>().get_game().id;
                let result = if key == "manager.emulator_id" {
                    c.borrow_mut()
                        .set_game_emulator(game_id.as_str(), value.as_str())
                } else {
                    c.borrow_mut()
                        .set_game_config(game_id.as_str(), key.as_str(), value.as_str())
                };
                match result {
                    Ok(model) => apply_model(&a, &model),
                    Err(error) => report_error(&window, &error),
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
            match compat_refresh_controller.borrow_mut().sync_metadata() {
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
    let mapping_change = {
        let c = controller.clone();
        let g = gamepad.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        move |change: &dyn Fn(&mut C) -> Result<ManagerViewModel, String>| {
            let result = {
                let mut controller = c.borrow_mut();
                change(&mut controller).and_then(|model| {
                    g.borrow_mut().set_mapping(controller.input_mapping())?;
                    Ok(model)
                })
            };
            if let Some(window) = w.upgrade() {
                match result {
                    Ok(model) => apply_model(&a, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        }
    };
    let settings = adapter.window().global::<Settings>();
    let save_input = mapping_change.clone();
    settings.on_save_input(move |enabled, deadzone| {
        save_input(&|c| {
            c.set_gamepad_enabled(enabled)?;
            c.set_gamepad_deadzone(deadzone.as_str())
        });
    });
    let bind = mapping_change.clone();
    settings.on_bind(move |button, key| {
        bind(&|c| c.set_gamepad_binding(button.as_str(), key.as_str()));
    });
    let reset_bindings = mapping_change.clone();
    settings.on_reset_bindings(move || reset_bindings(&|c| c.reset_gamepad_bindings()));
    let selection = adapter.window().global::<Selection>();
    let game_override = |enabled: bool| {
        let change = mapping_change.clone();
        let w = adapter.window().as_weak();
        move || {
            let Some(window) = w.upgrade() else {
                return;
            };
            let game_id = window.global::<Selection>().get_game().id;
            change(&|c| c.set_game_input_override(game_id.as_str(), enabled));
        }
    };
    let save_game_input = game_override(true);
    selection.on_save_game_input(move |_, _| save_game_input());
    selection.on_clear_game_input(game_override(false));
}
