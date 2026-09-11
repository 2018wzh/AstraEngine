use super::*;

pub(super) fn install<C: ManagerController>(
    adapter: &std::rc::Rc<SlintManagerAdapter>,
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
) {
    macro_rules! metadata_callback {
        ($callback:ident, $method:ident, |$($arg:ident),*|) => {{
            let weak = adapter.window().as_weak();
            let callback_controller = controller.clone();
            let callback_adapter = std::rc::Rc::downgrade(adapter);
            adapter.window().$callback(move |$($arg),*| {
                let result = callback_controller.borrow_mut().$method($($arg.as_str()),*);
                if let Some(window) = weak.upgrade() {
                    match result {
                        Ok(model) => apply_model(&callback_adapter, &model),
                        Err(error) => window.set_global_diagnostic(error.into()),
                    }
                }
            });
        }};
    }
    metadata_callback!(on_search_metadata, search_metadata, |provider, query|);
    metadata_callback!(on_refresh_metadata, refresh_metadata, |provider|);
    metadata_callback!(on_accept_match, accept_match, |candidate_id|);
    metadata_callback!(on_unlink_identity, unlink_identity, |provider|);
    let consent_weak = adapter.window().as_weak();
    let metadata_consent_controller = controller.clone();
    let metadata_consent_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .on_set_metadata_consent(move |provider, enabled, secret| {
            let result = metadata_consent_controller
                .borrow_mut()
                .set_metadata_consent(provider.as_str(), enabled, secret.as_str());
            if let Some(window) = consent_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&metadata_consent_adapter, &model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    let cover_weak = adapter.window().as_weak();
    let cover_controller = controller.clone();
    let cover_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .on_set_sensitive_cover_policy(move |provider, enabled| {
            let result = cover_controller
                .borrow_mut()
                .set_sensitive_cover_policy(provider.as_str(), enabled);
            if let Some(window) = cover_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&cover_adapter, &model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    let play_weak = adapter.window().as_weak();
    let play_controller = controller.clone();
    let play_adapter = std::rc::Rc::downgrade(adapter);
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
                    Ok(model) => apply_model(&play_adapter, &model),
                    Err(error) => window.set_global_diagnostic(error.into()),
                }
            }
        });
    // Worker completions and gamepad edges only wake the Slint event loop. The
    // actual drain is performed here on the UI thread immediately before the
    // underlay render, so no controller/UI object crosses a worker boundary.
}
