use super::*;

pub(super) fn install<C: ManagerController, R: AstraUnderlayRenderer>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
    gamepad: &std::rc::Rc<std::cell::RefCell<GameInputPump>>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
    runtime_schedule: &HostCallbackSlot,
) {
    let retry_controller = controller.clone();
    let retry_window = adapter.window().as_weak();
    let retry_schedule = runtime_schedule.clone();
    adapter.window().global::<Dialogs>().on_retry(move || {
        let Some(window) = retry_window.upgrade() else {
            return;
        };
        match retry_controller.borrow_mut().retry_native() {
            Ok(()) => {
                let dialogs = window.global::<Dialogs>();
                dialogs.set_fatal_active(false);
                dialogs.set_fatal_retryable(false);
                fire_host_callback(&retry_schedule);
            }
            Err(error) => {
                let text = describe_error(&error);
                let dialogs = window.global::<Dialogs>();
                dialogs.set_fatal_title(text.title.into());
                dialogs.set_fatal_detail(text.detail.into());
                dialogs.set_fatal_code(text.code.into());
            }
        }
    });
    let launch_weak = adapter.window().as_weak();
    let launch_controller = controller.clone();
    let launch_adapter = std::rc::Rc::downgrade(adapter);
    let launch_gamepad = gamepad.clone();
    let launch_runtime_schedule = runtime_schedule.clone();
    adapter.window().global::<Library>().on_launch(move |case_id| {
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
                let session = window.global::<Session>();
                session.set_active(true);
                session.set_title("".into());
                let dialogs = window.global::<Dialogs>();
                dialogs.set_fatal_active(false);
                dialogs.set_fatal_retryable(false);
                fire_host_callback(&launch_runtime_schedule);
            }
            Err(error) => {
                tracing::error!(event = "astra.emu.manager.launch.failed", diagnostic_code = %error);
                report_error(&window, &error);
            }
        }
    });
    let leave_weak = adapter.window().as_weak();
    let leave_controller = controller.clone();
    let leave_adapter = std::rc::Rc::downgrade(adapter);
    let leave_runtime_schedule = runtime_schedule.clone();
    adapter.window().global::<Session>().on_leave(move || {
        let Some(window) = leave_weak.upgrade() else {
            return;
        };
        let result = leave_controller.borrow_mut().leave_game();
        let icon_result = window_commands::set_icon(window.window(), None);
        let result = match (result, icon_result) {
            (Ok(model), Ok(())) => Ok(model),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
        };
        let session = window.global::<Session>();
        session.set_active(false);
        session.set_fullscreen(false);
        let dialogs = window.global::<Dialogs>();
        dialogs.set_fatal_active(false);
        dialogs.set_fatal_retryable(false);
        window.window().set_fullscreen(false);
        match result {
            Ok(model) => {
                apply_model(&leave_adapter, &model);
                window.global::<Session>().set_active(false);
                fire_host_callback(&leave_runtime_schedule);
            }
            Err(error) => report_error(&window, &error),
        }
    });
    let select_weak = adapter.window().as_weak();
    let select_controller = controller.clone();
    let select_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Library>()
        .on_select(move |selected| {
            let Some(window) = select_weak.upgrade() else {
                return;
            };
            match select_controller
                .borrow_mut()
                .select_case(selected.as_str())
            {
                Ok(model) => apply_model(&select_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let search_weak = adapter.window().as_weak();
    let search_controller = controller.clone();
    let search_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Library>()
        .on_search(move |query| {
            let Some(window) = search_weak.upgrade() else {
                return;
            };
            match search_controller.borrow_mut().search(query.as_str()) {
                Ok(model) => apply_model(&search_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        });
    let rescan_weak = adapter.window().as_weak();
    let rescan_controller = controller.clone();
    let rescan_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().global::<Library>().on_rescan(move || {
        let Some(window) = rescan_weak.upgrade() else {
            return;
        };
        match rescan_controller.borrow_mut().rescan() {
            Ok(model) => apply_model(&rescan_adapter, &model),
            Err(error) => report_error(&window, &error),
        }
    });
    let input_weak = adapter.window().as_weak();
    let input_controller = controller.clone();
    let input_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Session>()
        .on_input(move |control, pressed, value| {
            let result = input_controller
                .borrow_mut()
                .game_input(control.as_str(), pressed, value);
            if let Err(error) = result {
                if let Some(window) = input_weak.upgrade() {
                    terminate_game(&input_controller, &input_adapter, &window, error);
                }
            }
        });
    let action_weak = adapter.window().as_weak();
    let action_controller = controller.clone();
    adapter
        .window()
        .global::<Session>()
        .on_engine_action(move |control| {
            let Some(window) = action_weak.upgrade() else {
                return;
            };
            if let Err(error) = action_controller.borrow_mut().engine_action(control as u32) {
                let session = window.global::<Session>();
                session.set_action_message(describe_error(&error).title.into());
                session.set_action_serial(session.get_action_serial().saturating_add(1));
            }
        });
    let fullscreen_weak = adapter.window().as_weak();
    adapter
        .window()
        .global::<Session>()
        .on_set_fullscreen(move |fullscreen| {
            if let Some(window) = fullscreen_weak.upgrade() {
                window.window().set_fullscreen(fullscreen);
                window.global::<Session>().set_fullscreen(fullscreen);
            }
        });
    let save_translation_weak = adapter.window().as_weak();
    let save_translation_controller = controller.clone();
    let save_translation_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Settings>()
        .on_save_translation(move |profile, secret| {
            let result = save_translation_controller
                .borrow_mut()
                .save_translation_profile(
                    profile.kind.as_str(),
                    profile.endpoint.as_str(),
                    profile.protocol.as_str(),
                    profile.model.as_str(),
                    profile.language.as_str(),
                    profile.timeout_ms,
                    secret.as_str(),
                );
            if let Some(window) = save_translation_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&save_translation_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let test_controller = controller.clone();
    let test_adapter = std::rc::Rc::downgrade(adapter);
    let test_weak = adapter.window().as_weak();
    adapter
        .window()
        .global::<Settings>()
        .on_test_translation(move || {
            let result = test_controller.borrow_mut().test_translation_connection();
            if let Some(window) = test_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&test_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let consent_weak = adapter.window().as_weak();
    let consent_controller = controller.clone();
    let consent_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Settings>()
        .on_grant_translation_consent(move || {
            let result = consent_controller.borrow_mut().grant_translation_consent();
            if let Some(window) = consent_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&consent_adapter, &model),
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
            .global::<Library>()
            .on_add_directory(move || {
                let Some(path) = rfd::FileDialog::new().pick_folder() else {
                    return;
                };
                let result = c.borrow_mut().add_game_directory(&path);
                if let Some(window) = w.upgrade() {
                    match result {
                        Ok(model) => apply_model(&a, &model),
                        Err(error) => report_error(&window, &error),
                    }
                }
            });
    }
    filters::install(adapter, controller, renderer);
}
