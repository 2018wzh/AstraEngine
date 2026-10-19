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
    controller.refresh_cores().unwrap();
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

#[test]
fn typed_game_configuration_round_trips_after_controller_reopen() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut controller = configuration_controller(root.path());
        controller
            .family_config_changed("config.launch_mode", "title")
            .unwrap();
        controller.save_family_config().unwrap();

        let schema = controller
            .registry
            .configuration("config-test-plugin")
            .unwrap();
        let entries = controller
            .library
            .family_configuration("config-test-plugin", "game", &schema)
            .unwrap();
        assert_eq!(entries[0].value, ConfigValue::Enum("title".into()));
    }

    let controller = configuration_controller(root.path());
    let model = controller.model().unwrap();
    let fields = model.selection.family_fields;
    let launch_mode = fields
        .iter()
        .find(|field| field.key == "config.launch_mode")
        .unwrap();
    assert_eq!(launch_mode.label, "Launch mode");
    assert_eq!(launch_mode.value, "标题菜单启动");
    assert_eq!(launch_mode.enum_values, ["直接进入脚本", "标题菜单启动"]);
}

fn logical_schema() -> Vec<ConfigField> {
    let extent = |id: &str, default| ConfigField {
        id: id.into(),
        label: id.into(),
        group: "高级".into(),
        kind: ConfigKind::Integer { min: 1, max: 16384 },
        default: ConfigValue::Integer(default),
        description: "逻辑舞台尺寸".into(),
        choice_labels: Vec::new(),
        flags: Default::default(),
        step: Some(1.0),
        unit: "px".into(),
    };
    vec![extent("logical_width", 1280), extent("logical_height", 720)]
}

#[test]
fn drafts_save_with_native_scope_precedence() {
    let root = tempfile::tempdir().unwrap();
    let schema = logical_schema();
    let plugin = "astra.emu.musica";
    {
        let mut controller = configuration_controller(root.path());
        save_config_draft(
            &mut controller.library,
            plugin,
            "",
            &schema,
            &BTreeMap::from([
                ("config.logical_width".into(), "1024".into()),
                ("config.logical_height".into(), "640".into()),
            ]),
        )
        .unwrap();
        let inherited = controller
            .library
            .family_configuration(plugin, "game", &schema)
            .unwrap();
        let mut fields = Vec::new();
        append_config_fields(&schema, &inherited, &BTreeMap::new(), &mut fields);
        assert_eq!(
            fields
                .iter()
                .map(|field| field.value.as_str())
                .collect::<Vec<_>>(),
            ["1024", "640"]
        );
        assert_eq!(fields[0].description, "逻辑舞台尺寸（范围：1 … 16384 px）");
        let drafts = BTreeMap::from([(fields[0].key.clone(), "1280".into())]);
        save_config_draft(&mut controller.library, plugin, "game", &schema, &drafts).unwrap();
    }
    let mut controller = configuration_controller(root.path());
    let startup = controller
        .library
        .family_configuration(plugin, "game", &schema)
        .unwrap();
    assert_eq!(
        startup.iter().map(|entry| &entry.value).collect::<Vec<_>>(),
        [&ConfigValue::Integer(1280), &ConfigValue::Integer(640)]
    );
    for raw in ["0", "16385"] {
        assert!(save_config_draft(
            &mut controller.library,
            plugin,
            "game",
            &schema,
            &BTreeMap::from([
                ("config.logical_width".into(), "800".into()),
                ("config.logical_height".into(), raw.into()),
            ]),
        )
        .is_err());
        assert_eq!(
            controller
                .library
                .family_configuration(plugin, "game", &schema)
                .unwrap(),
            startup
        );
    }
    controller
        .library
        .save_family_configuration(plugin, "game", &schema, &[])
        .unwrap();
    assert_eq!(
        controller
            .library
            .family_configuration(plugin, "game", &schema)
            .unwrap(),
        vec![
            ConfigEntry {
                id: "logical_width".into(),
                value: ConfigValue::Integer(1024),
            },
            ConfigEntry {
                id: "logical_height".into(),
                value: ConfigValue::Integer(640),
            },
        ]
    );
}

#[test]
fn staged_emulator_hides_previous_fields_across_scope_changes_and_blocks_old_writes() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configuration_controller(root.path());
    controller
        .family_config_changed("manager.config_scope", "core")
        .unwrap();
    controller
        .family_config_changed("config.launch_mode", "title")
        .unwrap();
    controller.save_family_config().unwrap();
    let mut other = configuration_candidate();
    other.report.plugin_id = "other-emulator".into();
    other.descriptor.plugin_id = "other-emulator".into();
    controller
        .probe_choices
        .get_mut("game")
        .unwrap()
        .push(other);
    controller
        .family_config_changed("manager.emulator_id", "other-emulator")
        .unwrap();
    for scope in ["game", "core"] {
        controller
            .family_config_changed("manager.config_scope", scope)
            .unwrap();
        let model = controller.model().unwrap();
        assert!(model
            .selection
            .family_fields
            .iter()
            .all(|field| field.key == "manager.emulator_id"));
        assert_eq!(model.selection.family_fields[0].value, "other-emulator");
        assert!(controller
            .family_config_changed("config.launch_mode", "direct")
            .is_err());
        assert!(controller.reset_family_config().is_err());
    }
    controller
        .family_config_changed("manager.emulator_id", "config-test-plugin")
        .unwrap();
    let model = controller.model().unwrap();
    assert_eq!(
        model
            .selection
            .family_fields
            .iter()
            .find(|field| field.key == "config.launch_mode")
            .unwrap()
            .value,
        "标题菜单启动"
    );
}
