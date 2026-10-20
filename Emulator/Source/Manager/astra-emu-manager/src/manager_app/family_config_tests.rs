use super::*;

fn configuration_provider() -> astra_emu_manager_core::LoadedFamilyPlugin {
    let mut fixture =
        crate::test_native::Fixture::new("config-test-plugin", "config-test-family", "config.test");
    fixture.config = true;
    crate::test_native::install(0, fixture)
}

fn configuration_candidate() -> FamilyProbeCandidate {
    FamilyProbeCandidate {
        report: astra_emu_manager_core::FamilyProbeReport {
            plugin_id: "config-test-plugin".into(),
            family_id: "config-test-family".into(),
            game_id: "game".into(),
            format: "config.test".into(),
            confidence_permyriad: 10_000,
        },
        descriptor: FamilyPluginDescriptor {
            family_id: "config-test-family".into(),
            plugin_id: "config-test-plugin".into(),
            abi_fingerprint: astra_emu_manager_core::INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            version: "1.0.0".into(),
            capabilities: vec![astra_emu_manager_core::FamilyCapability::CpuFrame],
            supported_formats: vec!["config.test".into()],
            display_name: "config-test-plugin".into(),
            family_name: "config-test-family".into(),
            render: None,
        },
    }
}

fn configuration_controller(root: &std::path::Path) -> AstraEmuManagerController {
    let mut controller = AstraEmuManagerController::open_library(
        root.to_path_buf(),
        crate::stage_renderer::FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    controller.prepare_library().unwrap();
    crate::manager_app::probe_state_tests::settle_background(&mut controller);
    controller
        .registry
        .register_provider(configuration_provider)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while controller
        .registry
        .descriptor("config-test-plugin")
        .is_none()
    {
        controller.poll_platform().unwrap();
        assert!(
            Instant::now() < deadline,
            "provider metadata did not arrive"
        );
        std::thread::yield_now();
    }
    controller
        .library
        .add_game(&GameRecord {
            game_id: "game".into(),
            title: "Configuration test".into(),
            user_title: None,
            location: "fixtures/config".into(),
            family_id: Some("config-test-family".into()),
            content_fingerprint: None,
            added_at_unix_ms: 1,
        })
        .unwrap();
    controller.selected_case_id = Some("game".into());
    let candidate = configuration_candidate();
    controller
        .candidates
        .insert("game".into(), candidate.clone());
    controller
        .probe_choices
        .insert("game".into(), vec![candidate]);
    controller
}

#[test]
fn form_parses_full_width_numeric_values_and_rejects_non_finite_or_wrong_enum() {
    let mut field = ConfigField {
        id: "count".into(),
        label: "Count".into(),
        group: "Test".into(),
        kind: ConfigKind::Integer {
            min: i64::MIN,
            max: i64::MAX,
        },
        default: ConfigValue::Integer(0),
        description: String::new(),
        choice_labels: Vec::new(),
        flags: Default::default(),
        step: None,
        unit: String::new(),
    };
    assert_eq!(
        parse_config_value(&field, &i64::MAX.to_string()).unwrap(),
        ConfigValue::Integer(i64::MAX)
    );
    assert!(parse_config_value(&field, "9223372036854775808").is_err());
    field.kind = ConfigKind::Number {
        min: -1.0,
        max: 1.0,
    };
    assert_eq!(
        parse_config_value(&field, "-0.25").unwrap(),
        ConfigValue::Number(-0.25)
    );
    assert!(parse_config_value(&field, "NaN").is_err());
    field.kind = ConfigKind::Enum {
        choices: vec!["shift_jis".into(), "gbk".into()],
    };
    assert!(parse_config_value(&field, "utf8").is_err());
    field.choice_labels = vec!["Shift-JIS（日文原版）".into(), "GBK（简体中文）".into()];
    assert_eq!(
        parse_config_value(&field, "GBK（简体中文）").unwrap(),
        ConfigValue::Enum("gbk".into())
    );
    assert_eq!(
        parse_config_value(&field, "gbk").unwrap(),
        ConfigValue::Enum("gbk".into())
    );
    assert_eq!(
        display_config_value(&field, &ConfigValue::Enum("shift_jis".into())),
        "Shift-JIS（日文原版）"
    );
    assert_eq!(
        display_config_value(&field, &ConfigValue::Bool(false)),
        "false"
    );
}

const PLUGIN: &str = "config-test-plugin";

fn overrides(controller: &AstraEmuManagerController, game_id: &str) -> Vec<ConfigEntry> {
    controller.library.family_overrides(PLUGIN, game_id).unwrap()
}

fn field<'a>(fields: &'a [ConfigFieldViewModel], key: &str) -> &'a ConfigFieldViewModel {
    fields.iter().find(|field| field.key == key).unwrap()
}

#[test]
fn game_config_stores_only_the_changed_override_and_reset_removes_it() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    let model = controller.set_game_config("game", "launch_mode", "title").unwrap();
    assert_eq!(
        overrides(&controller, "game"),
        [ConfigEntry {
            id: "launch_mode".into(),
            value: ConfigValue::Enum("title".into()),
        }]
    );
    assert!(overrides(&controller, "").is_empty());
    let launch_mode = field(&model.game_settings.fields, "launch_mode");
    assert!(launch_mode.overridden);
    assert_eq!(launch_mode.value, "title");
    assert_eq!(launch_mode.display_value, "标题菜单启动");
    assert_eq!(launch_mode.option_values, ["direct", "title"]);
    assert_eq!(launch_mode.options, ["直接进入脚本", "标题菜单启动"]);
    assert_eq!(launch_mode.kind, "enum");

    let model = controller.reset_game_config("game", "launch_mode").unwrap();
    assert!(overrides(&controller, "game").is_empty());
    let launch_mode = field(&model.game_settings.fields, "launch_mode");
    assert!(!launch_mode.overridden);
    assert_eq!(launch_mode.value, "direct");
}

#[test]
fn invalid_or_unknown_values_leave_storage_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    controller.set_game_config("game", "launch_mode", "title").unwrap();
    let before = overrides(&controller, "game");
    assert_eq!(
        controller
            .set_game_config("game", "launch_mode", "menu")
            .unwrap_err(),
        "ASTRA_EMU_FAMILY_CONFIG: launch_mode: invalid value"
    );
    assert!(controller.set_game_config("game", "missing", "1").is_err());
    assert_eq!(overrides(&controller, "game"), before);
}

#[test]
fn emulator_defaults_are_inherited_by_games_without_marking_them_overridden() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    controller.select_emulator_defaults(PLUGIN).unwrap();
    let model = controller
        .set_emulator_default(PLUGIN, "launch_mode", "标题菜单启动")
        .unwrap();
    assert!(field(&model.cores.default_fields, "launch_mode").overridden);
    assert_eq!(model.cores.default_plugin, PLUGIN);
    let inherited = field(&model.game_settings.fields, "launch_mode");
    assert_eq!(inherited.value, "title");
    assert!(!inherited.overridden);
    assert!(overrides(&controller, "game").is_empty());

    controller.reset_emulator_default(PLUGIN, "launch_mode").unwrap();
    assert!(overrides(&controller, "").is_empty());
}

#[test]
fn configuration_changes_are_rejected_while_a_game_runs() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    controller.active = Some(owner::ActiveSessionView::opening(1, "game".into()));
    let results = [
        controller.set_game_config("game", "launch_mode", "title"),
        controller.reset_game_config("game", "launch_mode"),
        controller.set_emulator_default(PLUGIN, "launch_mode", "title"),
        controller.set_game_emulator("game", PLUGIN),
        controller.set_render_preference("game", None),
        controller.set_gamepad_binding("south", "enter"),
        controller.set_game_gamepad_binding("game", "south", "enter"),
        controller.set_game_input_override("game", true),
        controller.reset_gamepad_bindings(),
    ];
    controller.active = None;
    for result in results {
        assert_eq!(result.unwrap_err(), "ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE");
    }
    assert!(overrides(&controller, "game").is_empty());
}

#[test]
fn custom_render_outside_the_emulator_range_is_rejected_without_storing() {
    use astra_emu_manager_core::{RenderPreference, RenderSupport};
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    controller.candidates.get_mut("game").unwrap().descriptor.render = Some(RenderSupport {
        min: (320, 180),
        max: (3840, 2160),
        default: None,
        preserve_aspect: false,
    });
    controller
        .set_render_preference("game", Some(RenderPreference::Preset { height: 1440 }))
        .unwrap();
    assert_eq!(
        controller
            .set_render_preference(
                "game",
                Some(RenderPreference::Custom {
                    width: 5000,
                    height: 5000,
                }),
            )
            .unwrap_err(),
        "ASTRA_EMU_RENDER_EXTENT_UNSUPPORTED"
    );
    let stored = controller.library.game_settings("game").unwrap().unwrap();
    assert_eq!(stored.render, Some(RenderPreference::Preset { height: 1440 }));
    let model = controller
        .set_render_preference(
            "game",
            Some(RenderPreference::Custom {
                width: 2560,
                height: 1440,
            }),
        )
        .unwrap();
    assert!(model.game_settings.render.supported);
    assert_eq!(model.game_settings.render.mode, "custom");
    assert_eq!(model.game_settings.render.min, "320×180");
    assert_eq!(model.game_settings.render.max, "3840×2160");
    controller.set_render_preference("game", None).unwrap();
    assert_eq!(
        controller.library.game_settings("game").unwrap().unwrap().render,
        None
    );
}

#[test]
fn emulator_render_default_is_validated_and_followed_by_games() {
    use astra_emu_manager_core::RenderPreference;
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    assert_eq!(
        controller
            .set_emulator_render_default(
                PLUGIN,
                RenderPreference::Custom {
                    width: 1280,
                    height: 720,
                },
            )
            .unwrap_err(),
        "ASTRA_EMU_RENDER_EXTENT_UNSUPPORTED"
    );
    controller
        .set_emulator_render_default(PLUGIN, RenderPreference::Adaptive)
        .unwrap();
    let settings = astra_emu_manager_core::GameSettings::default();
    assert_eq!(
        controller
            .effective_render_preference(PLUGIN, &settings)
            .unwrap(),
        RenderPreference::Adaptive
    );
    let pinned = astra_emu_manager_core::GameSettings {
        render: Some(RenderPreference::Default),
        ..settings
    };
    assert_eq!(
        controller.effective_render_preference(PLUGIN, &pinned).unwrap(),
        RenderPreference::Default
    );
}

#[test]
fn game_relative_paths_are_stored_relative_only_inside_the_game_directory() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    std::fs::create_dir_all(game.join("data")).unwrap();
    std::fs::write(game.join("data").join("key.toml"), b"").unwrap();
    std::fs::write(root.path().join("outside.toml"), b"").unwrap();
    let inside = game.join("data").join("key.toml");
    assert_eq!(
        config_path_value(&inside, &game, true).unwrap(),
        Path::new("data").join("key.toml").to_str().unwrap()
    );
    let absolute = config_path_value(&inside, &game, false).unwrap();
    assert!(Path::new(&absolute).is_absolute());
    let outside = config_path_value(&root.path().join("outside.toml"), &game, true).unwrap();
    assert!(Path::new(&outside).is_absolute());
}

#[test]
fn gamepad_changes_persist_and_game_overrides_copy_the_global_mapping() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut controller = configuration_controller(root.path());
        controller.set_gamepad_binding("south", "space").unwrap();
        controller.set_gamepad_deadzone("high").unwrap();
        assert!(controller.set_gamepad_binding("south", "not-a-key").is_err());
        assert!(controller.set_gamepad_deadzone("huge").is_err());
        let model = controller.set_game_input_override("game", true).unwrap();
        assert!(model.game_settings.input_override);
        let model = controller
            .set_game_gamepad_binding("game", "south", "enter")
            .unwrap();
        let south = |bindings: &[astra_emu_manager_ui_slint::GamepadBindingViewModel]| {
            bindings
                .iter()
                .find(|binding| binding.id == "south")
                .unwrap()
                .key
                .clone()
        };
        assert_eq!(south(&model.game_settings.bindings), "enter");
        assert_eq!(south(&model.settings.bindings), "space");
    }
    let mut controller = configuration_controller(root.path());
    let global = controller.input_mapping();
    assert_eq!(global.deadzone, GamepadDeadzone::High);
    assert_eq!(global.gamepad[&GamepadInput::South], "space");
    let game = controller.game_input_mapping("game").unwrap();
    assert_eq!(game.gamepad[&GamepadInput::South], "enter");
    controller.reset_gamepad_bindings().unwrap();
    assert_eq!(controller.input_mapping().gamepad, default_vn_preset().gamepad);
    controller.set_game_input_override("game", false).unwrap();
    assert_eq!(
        controller.game_input_mapping("game").unwrap(),
        controller.input_mapping()
    );
}

#[test]
fn filter_changes_roll_back_when_the_renderer_rejects_them() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    let mut applied = Vec::new();
    controller
        .set_filter_preset("sharpen", &mut |settings| {
            applied.push(settings.clone());
            Ok(())
        })
        .unwrap();
    let saved = controller.library.filter_settings().unwrap().unwrap();
    assert_eq!(saved, controller.filter_settings);
    let mut calls = 0;
    let error = controller
        .set_filter_parameter("filter.strength", "0.8", &mut |settings| {
            calls += 1;
            if settings.configuration.strength == 0.8 {
                Err("ASTRA_EMU_FILTER_COMPILE: rejected".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error, "ASTRA_EMU_FILTER_COMPILE: rejected");
    assert_eq!(calls, 2);
    assert_eq!(controller.filter_settings, saved);
    assert_eq!(controller.library.filter_settings().unwrap().unwrap(), saved);
    assert!(controller
        .set_filter_parameter("filter.scale", "9", &mut |_| Ok(()))
        .is_err());
    controller
        .set_filter_preset("anime4k", &mut |_| Ok(()))
        .unwrap();
    assert_eq!(controller.filter_settings.configuration.scale, 2.0);
    controller.reset_filter_parameters(&mut |_| Ok(())).unwrap();
    assert_eq!(
        controller.library.filter_settings().unwrap().unwrap().configuration.strength,
        astra_emu_manager_core::FilterConfiguration::default().strength
    );
    assert_eq!(applied.len(), 1);
}

#[test]
fn typed_game_configuration_round_trips_after_controller_reopen() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut controller = configuration_controller(root.path());
        controller.set_game_config("game", "launch_mode", "title").unwrap();
    }
    let controller = configuration_controller(root.path());
    let model = controller.model().unwrap();
    let launch_mode = field(&model.game_settings.fields, "launch_mode");
    assert_eq!(launch_mode.label, "Launch mode");
    assert_eq!(launch_mode.display_value, "标题菜单启动");
    assert!(launch_mode.overridden);
}

fn install_preset(controller: &mut AstraEmuManagerController, configuration: &str) {
    let directory = controller.data_dir.join("preset-catalog");
    let preset = format!(
        r#"{{"vn_id":"v17","release_id":"r1","emulator":"{PLUGIN}","summary":"推荐","configuration":{configuration}}}"#
    );
    let tree = crate::metadata_cache::tests::files(
        crate::metadata_cache::tests::compatibility_json(""),
        &preset,
    );
    let _ = fs::remove_dir_all(&directory);
    crate::metadata_cache::tests::write_tree(&directory, 1, &tree);
    controller.metadata_catalog = Some(
        crate::metadata_cache::MetadataCatalog::load(
            &directory,
            crate::metadata_cache::MetadataSource::Bundled,
        )
        .unwrap(),
    );
    controller
        .library
        .set_external_identity(&astra_emu_manager_core::ExternalIdentityRecord {
            game_id: "game".into(),
            provider: "vndb".into(),
            remote_id: "v17".into(),
            provenance: "test".into(),
            linked_at_unix_ms: 1,
        })
        .unwrap();
}

#[test]
fn recommended_configuration_is_written_as_game_overrides() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    install_preset(&mut controller, r#"{"launch_mode":"title"}"#);
    let model = controller.apply_recommended_config("game").unwrap();
    assert!(model.game_settings.recommended_available);
    assert_eq!(model.game_settings.recommended_summary, "推荐");
    assert_eq!(
        overrides(&controller, "game"),
        [ConfigEntry {
            id: "launch_mode".into(),
            value: ConfigValue::Enum("title".into()),
        }]
    );
}

#[test]
fn recommended_configuration_with_unknown_field_restores_overrides() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    install_preset(
        &mut controller,
        r#"{"launch_mode":"direct","zz_unknown_field":"1"}"#,
    );
    assert_eq!(
        controller.apply_recommended_config("game").unwrap_err(),
        "ASTRA_EMU_PRESET_INVALID"
    );
    assert!(overrides(&controller, "game").is_empty());

    controller
        .set_game_config("game", "launch_mode", "title")
        .unwrap();
    let before = overrides(&controller, "game");
    install_preset(&mut controller, r#"{"launch_mode":"direct","zz_unknown_field":"1"}"#);
    assert_eq!(
        controller.apply_recommended_config("game").unwrap_err(),
        "ASTRA_EMU_PRESET_INVALID"
    );
    assert_eq!(overrides(&controller, "game"), before);
}
