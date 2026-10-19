use super::*;

pub(super) fn install<C: ManagerController>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
) {
    let core_weak = adapter.window().as_weak();
    let core_controller = controller.clone();
    let core_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().global::<Cores>().on_load(move || {
        let result = core_controller.borrow_mut().refresh_cores();
        if let Some(window) = core_weak.upgrade() {
            match result {
                Ok(model) => apply_model(&core_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        }
    });
    let install_weak = adapter.window().as_weak();
    let install_controller = controller.clone();
    let install_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().global::<Cores>().on_install(move || {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("核心文件", &[std::env::consts::DLL_EXTENSION])
            .pick_file()
        else {
            return;
        };
        let result = install_controller.borrow_mut().install_core(&path);
        if let Some(window) = install_weak.upgrade() {
            match result {
                Ok(model) => apply_model(&install_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        }
    });
    let uninstall_weak = adapter.window().as_weak();
    let uninstall_controller = controller.clone();
    let uninstall_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Cores>()
        .on_uninstall(move |file| {
            let result = uninstall_controller
                .borrow_mut()
                .uninstall_core(file.as_str());
            if let Some(window) = uninstall_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&uninstall_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let logs_weak = adapter.window().as_weak();
    let logs_controller = controller.clone();
    adapter.window().global::<Logs>().on_load(move |core| {
        let Some(window) = logs_weak.upgrade() else {
            return;
        };
        match logs_controller.borrow().log_text(core.as_str()) {
            Ok((text, count)) => {
                let logs = window.global::<Logs>();
                logs.set_text(if count == 0 {
                    "暂无相关日志".into()
                } else {
                    text.into()
                });
                logs.set_count(i32::try_from(count).unwrap_or(i32::MAX));
            }
            Err(error) => report_error(&window, &error),
        }
    });
    macro_rules! metadata_callback {
        ($callback:ident, $method:ident, |$($arg:ident),*|) => {{
            let weak = adapter.window().as_weak();
            let callback_controller = controller.clone();
            let callback_adapter = std::rc::Rc::downgrade(adapter);
            adapter.window().global::<Selection>().$callback(move |$($arg),*| {
                let result = callback_controller.borrow_mut().$method($($arg.as_str()),*);
                if let Some(window) = weak.upgrade() {
                    match result {
                        Ok(model) => apply_model(&callback_adapter, &model),
                        Err(error) => report_error(&window, &error),
                    }
                }
            });
        }};
    }
    metadata_callback!(on_search_metadata, search_metadata, |provider, query|);
    metadata_callback!(on_refresh_metadata, refresh_metadata, |provider|);
    metadata_callback!(on_accept_match, accept_match, |candidate_id|);
    metadata_callback!(on_unlink_metadata, unlink_identity, |provider|);
    let consent_weak = adapter.window().as_weak();
    let metadata_consent_controller = controller.clone();
    let metadata_consent_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Settings>()
        .on_set_metadata_consent(move |provider, enabled, secret| {
            let result = metadata_consent_controller
                .borrow_mut()
                .set_metadata_consent(provider.as_str(), enabled, secret.as_str());
            if let Some(window) = consent_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&metadata_consent_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let cover_weak = adapter.window().as_weak();
    let cover_controller = controller.clone();
    let cover_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Settings>()
        .on_set_sensitive_covers(move |provider, enabled| {
            let result = cover_controller
                .borrow_mut()
                .set_sensitive_cover_policy(provider.as_str(), enabled);
            if let Some(window) = cover_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&cover_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let play_weak = adapter.window().as_weak();
    let play_controller = controller.clone();
    let play_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<Selection>()
        .on_sync_bangumi(move |progress| {
            let result = play_controller.borrow_mut().update_bangumi_play_status(
                progress.status.as_str(),
                progress.rating,
                progress.note.as_str(),
            );
            if let Some(window) = play_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&play_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    // Worker completions and gamepad edges only wake the Slint event loop. The
    // actual drain is performed here on the UI thread immediately before the
    // underlay render, so no controller/UI object crosses a worker boundary.
}
