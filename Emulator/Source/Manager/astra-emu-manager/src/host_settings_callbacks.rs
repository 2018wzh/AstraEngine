use super::*;

pub(super) fn install<C: ManagerController>(wiring: &Wiring<C>, restart: &std::rc::Rc<dyn Fn()>) {
    let Some(window) = wiring.window.upgrade() else {
        return;
    };
    install_app(wiring, &window, restart);
    install_settings(wiring, &window);
    install_cores(wiring, &window, restart);
    install_onboarding(wiring, &window);
}

fn install_app<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    restart: &std::rc::Rc<dyn Fn()>,
) {
    let app = window.global::<App>();
    let w = wiring.clone();
    app.on_appearance_changed(move |appearance| {
        let settings = astra_emu_manager_core::AppearanceSettings {
            theme_mode: appearance.theme_mode.to_string(),
            density: appearance.density.to_string(),
            cover_size: appearance.cover_size.to_string(),
            view_mode: appearance.view_mode.to_string(),
            reduce_motion: appearance.reduce_motion,
            cover_badges: appearance.cover_badges,
        };
        w.run(|c| c.set_appearance(settings));
    });
    let w = wiring.clone();
    app.on_open_path(move |key| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let path = match key.as_str() {
            "data" => Ok(w.controller.borrow().data_dir().to_owned()),
            "logs" => Ok(w.controller.borrow().data_dir().join("diagnostics")),
            "third-party-notices" => std::env::current_exe()
                .map_err(|error| format!("ASTRA_EMU_OPEN_PATH_FAILED: {error}"))
                .and_then(|exe| {
                    exe.parent()
                        .map(|dir| dir.join("THIRD_PARTY_NOTICES.md"))
                        .ok_or_else(|| "ASTRA_EMU_OPEN_PATH_FAILED".to_owned())
                }),
            _ => Err("ASTRA_EMU_OPEN_PATH_FAILED: unknown location".to_owned()),
        };
        if let Err(error) = path.and_then(|path| open_path(&path)) {
            report_error(&window, &error);
        }
    });
    let restart_app = restart.clone();
    app.on_restart(move || restart_app());

    let network = window.global::<Network>();
    let w = wiring.clone();
    network.on_set_allowed(move |allowed| w.run(|c| c.set_network_allowed(allowed)));
    let w = wiring.clone();
    network.on_sync_now(move || w.run(|c| c.sync_metadata()));

    let w = wiring.clone();
    let restart_app = restart.clone();
    window
        .global::<Notices>()
        .on_snackbar_action_clicked(move || {
            let result = w.controller.borrow_mut().snackbar_action();
            match result {
                Ok(SnackbarOutcome::Model(model)) => w.show(Ok(*model)),
                Ok(SnackbarOutcome::Restart) => restart_app(),
                Err(error) => w.show(Err(error)),
            }
        });
}

fn translation_form(
    form: &astra_emu_manager_ui_slint::TranslationForm,
) -> astra_emu_manager_ui_slint::TranslationFormViewModel {
    astra_emu_manager_ui_slint::TranslationFormViewModel {
        provider: form.provider.to_string(),
        base_url: form.base_url.to_string(),
        openai_api: form.openai_api.to_string(),
        model: form.model.to_string(),
        language: form.language.to_string(),
        timeout_s: form.timeout_s,
        custom_prompt: form.custom_prompt.to_string(),
        context_segments: form.context_segments,
        context_chars: form.context_chars,
    }
}

fn install_settings<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
) {
    let settings = window.global::<Settings>();
    let w = wiring.clone();
    settings.on_save_translation(move |form, secret| {
        w.run(|c| c.save_translation_profile(&translation_form(&form), secret.as_str()))
    });
    let w = wiring.clone();
    settings.on_test_translation(move |form, secret| {
        w.run(|c| c.test_translation_connection(&translation_form(&form), secret.as_str()))
    });
    let w = wiring.clone();
    settings.on_set_translation_enabled(move |enabled| w.run(|c| c.set_translation_enabled(enabled)));
    let w = wiring.clone();
    settings.on_set_sensitive_covers(move |enabled| w.run(|c| c.set_sensitive_covers(enabled)));
    let w = wiring.clone();
    settings.on_save_bangumi_token(move |token| w.run(|c| c.save_bangumi_token(token.as_str())));
    let w = wiring.clone();
    settings.on_clear_bangumi_token(move || w.run(|c| c.clear_bangumi_token()));
    let w = wiring.clone();
    settings.on_set_gamepad_enabled(move |enabled| w.run_mapping(|c| c.set_gamepad_enabled(enabled)));
    let w = wiring.clone();
    settings.on_set_deadzone(move |deadzone| {
        w.run_mapping(|c| c.set_gamepad_deadzone(deadzone.as_str()))
    });
    let w = wiring.clone();
    settings.on_reset_bindings(move || w.run_mapping(|c| c.reset_gamepad_bindings()));
    let w = wiring.clone();
    settings.on_add_root(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("添加库目录")
            .pick_folder()
        {
            w.run(|c| c.add_game_directory(&path));
        }
    });
    let w = wiring.clone();
    settings.on_remove_root(move |path| w.run(|c| c.remove_library_root(path.as_str())));
    let w = wiring.clone();
    settings.on_restore_game(move |game_id| w.run(|c| c.restore_game(game_id.as_str())));
}

fn install_cores<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    restart: &std::rc::Rc<dyn Fn()>,
) {
    let cores = window.global::<Cores>();
    let w = wiring.clone();
    cores.on_install_local(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("从本地安装核心")
            .add_filter("核心包或核心文件", &["zip", std::env::consts::DLL_EXTENSION])
            .pick_file()
        {
            w.run(|c| c.install_core(&path));
        }
    });
    let w = wiring.clone();
    cores.on_install_online(move |plugin_id| w.run(|c| c.install_online_core(plugin_id.as_str())));
    let w = wiring.clone();
    cores.on_uninstall(move |core_id| w.run(|c| c.uninstall_core(core_id.as_str())));
    let restart_app = restart.clone();
    cores.on_restart(move || restart_app());
    let w = wiring.clone();
    cores.on_select_defaults(move |plugin_id| {
        w.run(|c| c.select_emulator_defaults(plugin_id.as_str()))
    });
    let default_plugin = |w: &Wiring<C>| {
        w.window
            .upgrade()
            .map(|window| window.global::<Cores>().get_default_plugin())
    };
    let w = wiring.clone();
    cores.on_default_field_changed(move |key, value| {
        if let Some(plugin) = default_plugin(&w) {
            w.run_field(FieldScope::EmulatorDefault, key.as_str(), |c| {
                c.set_emulator_default(plugin.as_str(), key.as_str(), value.as_str())
            });
        }
    });
    let w = wiring.clone();
    cores.on_reset_default_field(move |key| {
        if let Some(plugin) = default_plugin(&w) {
            w.run_field(FieldScope::EmulatorDefault, key.as_str(), |c| {
                c.reset_emulator_default(plugin.as_str(), key.as_str())
            });
        }
    });
    let w = wiring.clone();
    cores.on_set_default_render(move |mode, width, height| {
        let Some(plugin) = default_plugin(&w) else {
            return;
        };
        w.run_field(FieldScope::DefaultRender, "", |c| {
            let preference = library_callbacks::render_choice(mode.as_str(), width, height)?
                .unwrap_or(astra_emu_manager_core::RenderPreference::Default);
            c.set_emulator_render_default(plugin.as_str(), preference)
        });
    });
}

fn install_onboarding<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &astra_emu_manager_ui_slint::ManagerWindow,
) {
    let onboarding = window.global::<Onboarding>();
    let w = wiring.clone();
    onboarding.on_add_root(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("添加库目录")
            .pick_folder()
        {
            w.run(|c| c.add_library_root(&path));
        }
    });
    let w = wiring.clone();
    onboarding.on_remove_root(move |path| w.run(|c| c.remove_library_root(path.as_str())));
    let w = wiring.clone();
    onboarding.on_finish(move || w.run(|c| c.finish_onboarding()));
}
