use super::*;

pub(super) fn install<C: ManagerController, R: AstraUnderlayRenderer>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
    gamepad: &std::rc::Rc<std::cell::RefCell<GameInputPump>>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
    runtime_schedule: &HostCallbackSlot,
) {
    let launch_weak = adapter.window().as_weak();
    let launch_controller = controller.clone();
    let launch_adapter = std::rc::Rc::downgrade(adapter);
    let launch_gamepad = gamepad.clone();
    let launch_runtime_schedule = runtime_schedule.clone();
    adapter.window().on_launch(move |case_id| {
        tracing::info!(event = "astra.emu.manager.launch.requested");
        let Some(window) = launch_weak.upgrade() else {
            return;
        };
        let result = window_events::current_window_state(window.window())
            .and_then(|state| launch_controller.borrow_mut().set_window_state(state))
            .and_then(|()| launch_controller.borrow_mut().launch(case_id.as_str()));
        match result {
            Ok(model) => {
                let mapping = launch_controller.borrow().input_mapping();
                if let Err(error) = launch_gamepad.borrow_mut().set_mapping(mapping) {
                    terminate_game(&launch_controller, &launch_adapter, &window, error);
                    return;
                }
                apply_model(&launch_adapter, &model);
                window.set_game_active(true);
                fire_host_callback(&launch_runtime_schedule);
            }
            Err(error) => {
                tracing::error!(event = "astra.emu.manager.launch.failed", diagnostic_code = %error);
                window.set_global_diagnostic(error.into());
            }
        }
    });
    let leave_weak = adapter.window().as_weak();
    let leave_controller = controller.clone();
    let leave_adapter = std::rc::Rc::downgrade(adapter);
    let leave_runtime_schedule = runtime_schedule.clone();
    adapter.window().on_leave_game(move || {
        let Some(window) = leave_weak.upgrade() else {
            return;
        };
        let result = leave_controller.borrow_mut().leave_game();
        window.set_game_active(false);
        match result {
            Ok(model) => {
                apply_model(&leave_adapter, &model);
                window.set_game_active(false);
                fire_host_callback(&leave_runtime_schedule);
            }
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let select_weak = adapter.window().as_weak();
    let select_controller = controller.clone();
    let select_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_select_case(move |selected| {
        let Some(window) = select_weak.upgrade() else {
            return;
        };
        match select_controller
            .borrow_mut()
            .select_case(selected.as_str())
        {
            Ok(model) => apply_model(&select_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let search_weak = adapter.window().as_weak();
    let search_controller = controller.clone();
    let search_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_search(move |query| {
        let Some(window) = search_weak.upgrade() else {
            return;
        };
        match search_controller.borrow_mut().search(query.as_str()) {
            Ok(model) => apply_model(&search_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let rescan_weak = adapter.window().as_weak();
    let rescan_controller = controller.clone();
    let rescan_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_rescan(move || {
        let Some(window) = rescan_weak.upgrade() else {
            return;
        };
        match rescan_controller.borrow_mut().rescan() {
            Ok(model) => apply_model(&rescan_adapter, &model),
            Err(error) => window.set_global_diagnostic(error.into()),
        }
    });
    let input_weak = adapter.window().as_weak();
    let input_controller = controller.clone();
    let input_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .on_game_input(move |control, pressed, value| {
            let result = input_controller
                .borrow_mut()
                .game_input(control.as_str(), pressed, value);
            if let Err(error) = result {
                if let Some(window) = input_weak.upgrade() {
                    terminate_game(&input_controller, &input_adapter, &window, error);
                }
            }
        });
    let save_translation_weak = adapter.window().as_weak();
    let save_translation_controller = controller.clone();
    let save_translation_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_save_translation_profile(
        move |endpoint_kind, endpoint, protocol, model, target_language, timeout_ms, secret| {
            let result = save_translation_controller
                .borrow_mut()
                .save_translation_profile(
                    endpoint_kind.as_str(),
                    endpoint.as_str(),
                    protocol.as_str(),
                    model.as_str(),
                    target_language.as_str(),
                    timeout_ms,
                    secret.as_str(),
                );
            if let Some(window) = save_translation_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&save_translation_adapter, &model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        },
    );
    let test_controller = controller.clone();
    let test_adapter = std::rc::Rc::downgrade(adapter);
    let test_weak = adapter.window().as_weak();
    adapter.window().on_test_translation_connection(move || {
        let result = test_controller.borrow_mut().test_translation_connection();
        if let Some(window) = test_weak.upgrade() {
            match result {
                Ok(model) => apply_model(&test_adapter, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let consent_weak = adapter.window().as_weak();
    let consent_controller = controller.clone();
    let consent_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().on_grant_translation_consent(move || {
        let result = consent_controller.borrow_mut().grant_translation_consent();
        if let Some(window) = consent_weak.upgrade() {
            match result {
                Ok(model) => apply_model(&consent_adapter, &model),
                Err(error) => window.set_global_diagnostic(error.into()),
            }
        }
    });
    let diagnostics_weak = adapter.window().as_weak();
    adapter.window().on_open_diagnostics(move || {
        if let Some(window) = diagnostics_weak.upgrade() {
            window.set_translation_overlay_active(false);
            window.set_filters_overlay_active(false);
            window.set_diagnostics_overlay_active(!window.get_diagnostics_overlay_active());
        }
    });
    let filters_weak = adapter.window().as_weak();
    adapter.window().on_open_filters(move || {
        if let Some(window) = filters_weak.upgrade() {
            window.set_translation_overlay_active(false);
            window.set_diagnostics_overlay_active(false);
            window.set_filters_overlay_active(!window.get_filters_overlay_active());
        }
    });
    {
        let c = controller.clone();
        let a = std::rc::Rc::downgrade(adapter);
        let w = adapter.window().as_weak();
        adapter.window().on_add_game_directory(move || {
            let Some(path) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            let result = c.borrow_mut().add_game_directory(&path);
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
        adapter.window().on_install_family_plugin(move || {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Family plugin", &[std::env::consts::DLL_EXTENSION])
                .pick_file()
            else {
                return;
            };
            let result = c.borrow_mut().install_family_plugin(&path);
            if let Some(window) = w.upgrade() {
                match result {
                    Ok(model) => apply_model(&a, &model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    }
    filters::install(adapter, controller, renderer);
}
