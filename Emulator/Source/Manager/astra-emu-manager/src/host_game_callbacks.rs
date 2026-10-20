use super::*;

pub(super) fn install<C: ManagerController, R: AstraUnderlayRenderer>(
    wiring: &Wiring<C>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
    runtime_schedule: &HostCallbackSlot,
) {
    let Some(window) = wiring.window.upgrade() else {
        return;
    };
    let w = wiring.clone();
    let retry_schedule = runtime_schedule.clone();
    window.global::<Dialogs>().on_retry(move || {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = w.controller.borrow_mut().retry_native();
        let dialogs = window.global::<Dialogs>();
        match result {
            Ok(()) => {
                dialogs.set_fatal_active(false);
                dialogs.set_fatal_retryable(false);
                fire_host_callback(&retry_schedule);
            }
            Err(error) => {
                let text = describe_error(&error);
                dialogs.set_fatal_title(text.title.into());
                dialogs.set_fatal_detail(text.detail.into());
                dialogs.set_fatal_code(text.code.into());
            }
        }
    });
    let w = wiring.clone();
    let launch_schedule = runtime_schedule.clone();
    window.global::<Library>().on_launch(move |game_id| {
        tracing::info!(event = "astra.emu.manager.launch.requested");
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = window_events::current_window_state(window.window())
            .and_then(|state| w.controller.borrow_mut().set_window_state(state))
            .and_then(|()| w.controller.borrow_mut().launch(game_id.as_str()));
        match result {
            Ok(model) => {
                let mapping = w.controller.borrow().input_mapping();
                if let Err(error) = w.gamepad.borrow_mut().set_mapping(mapping) {
                    terminate_game(&w.controller, &w.adapter, &window, error);
                    return;
                }
                apply_model(&w.adapter, &model);
                let session = window.global::<Session>();
                session.set_active(true);
                session.set_title("".into());
                let dialogs = window.global::<Dialogs>();
                dialogs.set_fatal_active(false);
                dialogs.set_fatal_retryable(false);
                fire_host_callback(&launch_schedule);
            }
            Err(error) => {
                tracing::error!(event = "astra.emu.manager.launch.failed", diagnostic_code = %error);
                report_error(&window, &error);
            }
        }
    });
    let w = wiring.clone();
    let leave_schedule = runtime_schedule.clone();
    window.global::<Session>().on_leave(move || {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = w.controller.borrow_mut().leave_game();
        finish_leave(&window, &w.adapter, result);
        fire_host_callback(&leave_schedule);
    });
    let w = wiring.clone();
    window
        .global::<Session>()
        .on_input(move |control, pressed, value| {
            let result = w
                .controller
                .borrow_mut()
                .game_input(control.as_str(), pressed, value);
            if let (Err(error), Some(window)) = (result, w.window.upgrade()) {
                report_input_error(&window, &error);
            }
        });
    let w = wiring.clone();
    window.global::<Session>().on_engine_action(move |control| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let Ok(control) = u32::try_from(control) else {
            return report_error(&window, "ASTRA_EMU_ENGINE_ACTION_UNAVAILABLE");
        };
        if let Err(error) = w.controller.borrow_mut().engine_action(control) {
            let session = window.global::<Session>();
            session.set_action_message(describe_error(&error).title.into());
            session.set_action_serial(session.get_action_serial().saturating_add(1));
        }
    });
    let w = wiring.clone();
    window
        .global::<Session>()
        .on_set_fullscreen(move |fullscreen| {
            if let Some(window) = w.window.upgrade() {
                window.window().set_fullscreen(fullscreen);
                window.global::<Session>().set_fullscreen(fullscreen);
            }
        });
    filters::install(wiring, renderer);
}
