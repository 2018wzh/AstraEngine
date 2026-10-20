use super::*;

pub(super) fn install<C: ManagerController>(wiring: &Wiring<C>) {
    let Some(window) = wiring.window.upgrade() else {
        return;
    };
    install_library(wiring, &window);
    install_selection(wiring, &window);
    install_game_settings(wiring, &window);
    let w = wiring.clone();
    window.global::<Logs>().on_load(move |core| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        match w.controller.borrow().log_text(core.as_str()) {
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
}

fn install_library<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
) {
    let library = window.global::<Library>();
    let w = wiring.clone();
    library.on_prepare(move || w.run(|c| c.prepare_library()));
    let w = wiring.clone();
    library.on_cancel_prepare(move || w.run(|c| c.cancel_prepare()));
    let w = wiring.clone();
    library.on_add_directory(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("选择游戏目录")
            .pick_folder()
        {
            w.run(|c| c.add_game_directory(&path));
        }
    });
    let w = wiring.clone();
    library.on_search(move |query| w.run(|c| c.search(query.as_str())));
    let w = wiring.clone();
    library.on_set_sort(move |mode| w.run(|c| c.set_library_sort(mode.as_str())));
    let w = wiring.clone();
    library.on_set_compat_filter(move |filter| {
        w.run(|c| c.set_compatibility_filter(filter.as_str()))
    });
    let w = wiring.clone();
    library.on_select(move |game_id| w.run(|c| c.select_case(game_id.as_str())));
    let w = wiring.clone();
    library.on_open_game_settings(move |game_id| w.run(|c| c.select_case(game_id.as_str())));
    let w = wiring.clone();
    library.on_open_folder(move |game_id| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = w
            .controller
            .borrow()
            .game_location(game_id.as_str())
            .and_then(|location| open_path(&location));
        if let Err(error) = result {
            report_error(&window, &error);
        }
    });
    let w = wiring.clone();
    library.on_remove(move |game_id| w.run(|c| c.remove_from_library(game_id.as_str())));
}

fn install_selection<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
) {
    let selection = window.global::<Selection>();
    let w = wiring.clone();
    selection.on_choose_emulator(move |plugin_id| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let game_id = window.global::<Selection>().get_game().id;
        w.run(|c| c.set_game_emulator(game_id.as_str(), plugin_id.as_str()));
    });
    let w = wiring.clone();
    selection.on_pin_release(move |release_id| w.run(|c| c.pin_release(release_id.as_str())));
    let w = wiring.clone();
    selection.on_fetch_releases(move || w.run(|c| c.fetch_releases()));
    let w = wiring.clone();
    selection.on_sync_bangumi(move |progress| {
        w.run(|c| {
            c.update_bangumi_play_status(
                progress.status.as_str(),
                progress.rating,
                progress.note.as_str(),
            )
        })
    });
    let w = wiring.clone();
    selection.on_search_metadata(move |provider, query| {
        w.run(|c| c.search_metadata(provider.as_str(), query.as_str()))
    });
    let w = wiring.clone();
    selection.on_accept_match(move |candidate| w.run(|c| c.accept_match(candidate.as_str())));
    let w = wiring.clone();
    selection.on_refresh_metadata(move |provider| w.run(|c| c.refresh_metadata(provider.as_str())));
    let w = wiring.clone();
    selection.on_unlink_metadata(move |provider| w.run(|c| c.unlink_identity(provider.as_str())));
}

fn game_id(window: &slint::Weak<astra_emu_manager_ui_slint::ManagerWindow>) -> Option<slint::SharedString> {
    window
        .upgrade()
        .map(|window| window.global::<GameSettings>().get_game_id())
}

fn install_game_settings<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
) {
    let settings = window.global::<GameSettings>();
    let w = wiring.clone();
    settings.on_close(move || w.run(|c| c.cancel_key_capture()));
    let w = wiring.clone();
    settings.on_set_emulator(move |plugin_id| {
        if let Some(game) = game_id(&w.window) {
            w.run(|c| c.set_game_emulator(game.as_str(), plugin_id.as_str()));
        }
    });
    let w = wiring.clone();
    settings.on_set_render(move |mode, width, height| {
        let Some(game) = game_id(&w.window) else {
            return;
        };
        w.run_field(FieldScope::GameRender, "", |c| {
            let preference = render_choice(mode.as_str(), width, height)?;
            c.set_render_preference(game.as_str(), preference)
        });
    });
    let w = wiring.clone();
    settings.on_field_changed(move |key, value| {
        if let Some(game) = game_id(&w.window) {
            w.run_field(FieldScope::Game, key.as_str(), |c| {
                c.set_game_config(game.as_str(), key.as_str(), value.as_str())
            });
        }
    });
    let w = wiring.clone();
    settings.on_reset_field(move |key| {
        if let Some(game) = game_id(&w.window) {
            w.run_field(FieldScope::Game, key.as_str(), |c| {
                c.reset_game_config(game.as_str(), key.as_str())
            });
        }
    });
    let w = wiring.clone();
    settings.on_pick_path(move |key| {
        if let Some(game) = game_id(&w.window) {
            w.run_field(FieldScope::Game, key.as_str(), |c| {
                c.pick_config_path(game.as_str(), key.as_str())
            });
        }
    });
    let w = wiring.clone();
    settings.on_apply_recommended(move || {
        if let Some(game) = game_id(&w.window) {
            w.run(|c| c.apply_recommended_config(game.as_str()));
        }
    });
    let w = wiring.clone();
    settings.on_set_input_override(move |enabled| {
        if let Some(game) = game_id(&w.window) {
            w.run_mapping(|c| c.set_game_input_override(game.as_str(), enabled));
        }
    });
    let w = wiring.clone();
    settings.on_patch_pick_script(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("选择可信的本地 Luau 补丁脚本")
            .add_filter("Luau 脚本", &["luau"])
            .pick_file()
        {
            w.run(|c| c.set_patch_script(&path));
        }
    });
    let w = wiring.clone();
    settings.on_patch_select_recipe(move |id| w.run(|c| c.select_patch_recipe(id.as_str())));
    let w = wiring.clone();
    settings.on_patch_pick_directory(move |kind| {
        let picked = if kind == "output" {
            // A folder picker only returns existing directories; the save dialog names
            // a new one, which the conversion creates.
            rfd::FileDialog::new()
                .set_title("选择尚不存在的新输出目录名称")
                .set_file_name("localized-output")
                .save_file()
        } else {
            rfd::FileDialog::new()
                .set_title("选择补丁输入目录")
                .pick_folder()
        };
        if let Some(path) = picked {
            w.run(|c| c.set_patch_directory(kind.as_str(), &path));
        }
    });
    let w = wiring.clone();
    settings.on_patch_start(move || w.run(|c| c.start_patch()));
    let w = wiring.clone();
    settings.on_patch_cancel(move || w.run(|c| c.cancel_file_patch()));
    let w = wiring.clone();
    settings.on_patch_open_output(move || {
        let result = w.controller.borrow_mut().open_patch_output();
        let opened = result.is_ok();
        w.show(result);
        if let (true, Some(window)) = (opened, w.window.upgrade()) {
            window.global::<App>().set_page("library".into());
            window.global::<Library>().set_detail_open(true);
        }
    });
}

/// `RenderPicker` choice plus custom size; negative sizes are invalid.
pub(super) fn render_choice(
    mode: &str,
    width: i32,
    height: i32,
) -> Result<Option<astra_emu_manager_core::RenderPreference>, String> {
    let size = |value: i32| {
        u32::try_from(value).map_err(|_| "ASTRA_EMU_RENDER_PREFERENCE_INVALID".to_owned())
    };
    astra_emu_manager_core::RenderPreference::from_choice(mode, size(width)?, size(height)?)
}
