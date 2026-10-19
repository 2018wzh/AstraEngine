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
    macro_rules! settings_callback {
        ($callback:ident, |$controller:ident $(, $arg:ident)*| $call:expr) => {{
            let weak = adapter.window().as_weak();
            let callback_controller = controller.clone();
            let callback_adapter = std::rc::Rc::downgrade(adapter);
            adapter.window().global::<Settings>().$callback(move |$($arg),*| {
                let result = {
                    let mut $controller = callback_controller.borrow_mut();
                    $call
                };
                if let Some(window) = weak.upgrade() {
                    match result {
                        Ok(model) => apply_model(&callback_adapter, &model),
                        Err(error) => report_error(&window, &error),
                    }
                }
            });
        }};
    }
    settings_callback!(on_set_network_allowed, |c, enabled| c
        .set_network_allowed(enabled));
    settings_callback!(on_set_sensitive_covers, |c, enabled| c
        .set_sensitive_covers(enabled));
    settings_callback!(on_save_bangumi_token, |c, token| c
        .save_bangumi_token(token.as_str()));
    settings_callback!(on_clear_bangumi_token, |c| c.clear_bangumi_token());
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
    let catalog_weak = adapter.window().as_weak();
    adapter
        .window()
        .global::<FilePatch>()
        .on_load_catalog(move || {
            let Some(window) = catalog_weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<_, String> {
                let executable = std::env::current_exe()
                    .map_err(|error| format!("ASTRA_EMU_PATCH_PATH: {error}"))?;
                let directory = executable
                    .parent()
                    .ok_or("ASTRA_EMU_PATCH_PATH: executable has no parent")?
                    .join("patches");
                let catalog =
                    astra_emu_patch::list_recipes(&directory).map_err(|error| error.to_string())?;
                let mut titles = Vec::<slint::SharedString>::with_capacity(catalog.len());
                let mut scripts = Vec::<slint::SharedString>::with_capacity(catalog.len());
                for recipe in catalog {
                    let path = directory.join(recipe.script);
                    let path = path
                        .to_str()
                        .ok_or("ASTRA_EMU_PATCH_PATH: script path must be Unicode")?;
                    titles.push(recipe.title.into());
                    scripts.push(path.into());
                }
                Ok((titles, scripts))
            })();
            match result {
                Ok((titles, scripts)) => {
                    let patch = window.global::<FilePatch>();
                    if patch.get_script().is_empty() && !scripts.is_empty() {
                        patch.set_script(scripts[0].clone());
                        patch.set_catalog_index(0);
                    }
                    patch.set_recipe_titles(slint::ModelRc::new(slint::VecModel::from(titles)));
                    patch.set_recipe_scripts(slint::ModelRc::new(slint::VecModel::from(scripts)));
                }
                Err(error) => report_error(&window, &error),
            }
        });
    let script_weak = adapter.window().as_weak();
    adapter
        .window()
        .global::<FilePatch>()
        .on_pick_script(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title("选择可信的本地 Luau 补丁脚本")
                .add_filter("Luau 脚本", &["luau"])
                .pick_file()
            else {
                return;
            };
            if let Some(window) = script_weak.upgrade() {
                if let Some(path) = path.to_str() {
                    let patch = window.global::<FilePatch>();
                    patch.set_script(path.into());
                    patch.set_catalog_index(-1);
                } else {
                    report_error(&window, "ASTRA_EMU_PATCH_PATH: script path must be Unicode");
                }
            }
        });
    let patch_weak = adapter.window().as_weak();
    adapter
        .window()
        .global::<FilePatch>()
        .on_pick_patch_directory(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title("选择补丁输入目录（取消则保留原选择）")
                .pick_folder()
            else {
                return;
            };
            if let Some(window) = patch_weak.upgrade() {
                if let Some(path) = path.to_str() {
                    window
                        .global::<FilePatch>()
                        .set_patch_directory(path.into());
                } else {
                    report_error(&window, "ASTRA_EMU_PATCH_PATH: patch path must be Unicode");
                }
            }
        });
    let output_weak = adapter.window().as_weak();
    adapter
        .window()
        .global::<FilePatch>()
        .on_pick_output_directory(move || {
            // A folder picker only returns existing directories. Save-file selection
            // chooses a new directory name without creating it; apply creates the tree.
            let Some(path) = rfd::FileDialog::new()
                .set_title("选择尚不存在的新输出目录名称")
                .set_file_name("localized-output")
                .save_file()
            else {
                return;
            };
            if let Some(window) = output_weak.upgrade() {
                if let Some(path) = path.to_str() {
                    window
                        .global::<FilePatch>()
                        .set_output_directory(path.into());
                } else {
                    report_error(&window, "ASTRA_EMU_PATCH_PATH: output path must be Unicode");
                }
            }
        });
    let patch_start_weak = adapter.window().as_weak();
    let patch_start_controller = controller.clone();
    let patch_start_adapter = std::rc::Rc::downgrade(adapter);
    adapter
        .window()
        .global::<FilePatch>()
        .on_start(move |script, patch, output| {
            let result = patch_start_controller.borrow_mut().start_file_patch(
                std::path::Path::new(script.as_str()),
                std::path::Path::new(patch.as_str()),
                std::path::Path::new(output.as_str()),
            );
            if let Some(window) = patch_start_weak.upgrade() {
                match result {
                    Ok(model) => apply_model(&patch_start_adapter, &model),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let patch_cancel_weak = adapter.window().as_weak();
    let patch_cancel_controller = controller.clone();
    let patch_cancel_adapter = std::rc::Rc::downgrade(adapter);
    adapter.window().global::<FilePatch>().on_cancel(move || {
        let result = patch_cancel_controller.borrow_mut().cancel_file_patch();
        if let Some(window) = patch_cancel_weak.upgrade() {
            match result {
                Ok(model) => apply_model(&patch_cancel_adapter, &model),
                Err(error) => report_error(&window, &error),
            }
        }
    });
    // Worker completions and gamepad edges only wake the Slint event loop. The
    // actual drain is performed here on the UI thread immediately before the
    // underlay render, so no controller/UI object crosses a worker boundary.
}
