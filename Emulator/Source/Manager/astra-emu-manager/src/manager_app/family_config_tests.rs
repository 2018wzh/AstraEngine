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
        choices: vec!["shift_jis".into()],
    };
    assert!(parse_config_value(&field, "utf8").is_err());
    assert_eq!(display_config_value(&ConfigValue::Bool(false)), "false");
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
    assert!(model.common_engine_config_fields.is_empty());
    let fields = model.family_config_fields;
    let launch_mode = fields
        .iter()
        .find(|field| field.key == "config.launch_mode")
        .unwrap();
    assert_eq!(launch_mode.value, "title");
}

fn musica_schema() -> (FamilyPluginDescriptor, Vec<ConfigField>) {
    let mut descriptor = configuration_candidate().descriptor;
    descriptor.family_id = "musica".into();
    descriptor.plugin_id = "astra.emu.musica".into();
    let extent = |id: &str, default| ConfigField {
        id: id.into(),
        label: id.into(),
        group: "Presentation".into(),
        kind: ConfigKind::Integer { min: 1, max: 16384 },
        default: ConfigValue::Integer(default),
    };
    (
        descriptor,
        vec![
            extent("render_width", 1280),
            extent("render_height", 720),
            extent("logical_width", 1280),
            extent("logical_height", 720),
        ],
    )
}

#[test]
fn native_render_capability_requires_a_recognized_complete_typed_schema() {
    let (mut descriptor, mut schema) = musica_schema();
    let common = native_render_fields(&descriptor, &schema);
    assert_eq!(
        common.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        ["render_width", "render_height"]
    );
    assert!(native_render_fields(&descriptor, &schema[1..]).is_empty());
    schema[1].kind = ConfigKind::Number {
        min: 1.0,
        max: 16384.0,
    };
    schema[1].default = ConfigValue::Number(720.0);
    assert!(native_render_fields(&descriptor, &schema).is_empty());
    schema[1].kind = ConfigKind::Integer { min: 1, max: 16384 };
    schema[1].default = ConfigValue::Integer(0);
    assert!(native_render_fields(&descriptor, &schema).is_empty());
    schema[1].default = ConfigValue::Integer(720);
    schema.push(schema[0].clone());
    assert!(native_render_fields(&descriptor, &schema).is_empty());
    schema.pop();
    schema[0].kind = ConfigKind::Integer { min: 64, max: 8192 };
    schema[1].kind = ConfigKind::Integer { min: 64, max: 8192 };
    assert_eq!(native_render_fields(&descriptor, &schema).len(), 2);
    assert!(parse_config_value(&schema[0], "8193").is_err());
    descriptor.plugin_id = "unknown.musica".into();
    assert!(native_render_fields(&descriptor, &schema).is_empty());
    descriptor.plugin_id = "astra.emu.musica".into();
    descriptor.family_id = "unknown".into();
    assert!(native_render_fields(&descriptor, &schema).is_empty());

    descriptor.family_id = "fvp".into();
    descriptor.plugin_id = "astra.emu.fvp".into();
    let scale = ConfigField {
        id: "render_scale".into(),
        label: "Render scale".into(),
        group: "Presentation".into(),
        kind: ConfigKind::Number {
            min: 0.25,
            max: 4.0,
        },
        default: ConfigValue::Number(1.0),
    };
    assert_eq!(
        native_render_fields(&descriptor, std::slice::from_ref(&scale)).len(),
        1
    );
    assert_eq!(
        parse_config_value(&scale, "1.5").unwrap(),
        ConfigValue::Number(1.5)
    );
    for raw in ["0.24", "4.01", "NaN"] {
        assert!(parse_config_value(&scale, raw).is_err());
    }

    descriptor.family_id = "softpal".into();
    descriptor.plugin_id = "sena-native-koikake".into();
    schema.truncate(2);
    for (field, id) in schema.iter_mut().zip(["raster_width", "raster_height"]) {
        field.id = id.into();
        field.kind = ConfigKind::Integer {
            min: 320,
            max: 3840,
        };
        for raw in ["319", "3841"] {
            assert!(parse_config_value(field, raw).is_err());
        }
    }
    assert_eq!(native_render_fields(&descriptor, &schema).len(), 2);
    assert!(native_render_fields(&descriptor, &schema[..1]).is_empty());
}

#[test]
fn common_and_specific_drafts_save_together_with_native_scope_precedence() {
    let root = tempfile::tempdir().unwrap();
    let (descriptor, schema) = musica_schema();
    let plugin = descriptor.plugin_id.as_str();
    {
        let mut controller = configuration_controller(root.path());
        save_config_draft(
            &mut controller.library,
            plugin,
            "",
            &schema,
            &BTreeMap::from([
                ("config.render_width".into(), "1920".into()),
                ("config.render_height".into(), "1080".into()),
                ("config.logical_width".into(), "1024".into()),
                ("config.logical_height".into(), "640".into()),
            ]),
        )
        .unwrap();
        let inherited = controller
            .library
            .family_configuration(plugin, "game", &schema)
            .unwrap();
        let (mut common, mut specific) = (Vec::new(), Vec::new());
        append_config_fields(
            &descriptor,
            &schema,
            &inherited,
            &BTreeMap::new(),
            &mut common,
            &mut specific,
        );
        let mut drafts: BTreeMap<_, _> = common
            .into_iter()
            .zip(["2560", "1440"])
            .map(|(field, value)| (field.key, value.into()))
            .collect();
        let logical_width = specific
            .iter()
            .find(|field| field.key == "config.logical_width")
            .unwrap();
        drafts.insert(logical_width.key.clone(), "1280".into());
        save_config_draft(&mut controller.library, plugin, "game", &schema, &drafts).unwrap();
    }
    let mut controller = configuration_controller(root.path());
    let startup = controller
        .library
        .family_configuration(plugin, "game", &schema)
        .unwrap();
    let (mut common, mut specific) = (Vec::new(), Vec::new());
    append_config_fields(
        &descriptor,
        &schema,
        &startup,
        &BTreeMap::new(),
        &mut common,
        &mut specific,
    );
    assert_eq!(
        common
            .iter()
            .map(|field| field.value.as_str())
            .collect::<Vec<_>>(),
        ["2560", "1440"]
    );
    assert!(
        specific
            .iter()
            .all(|field| field.key.starts_with("config.logical_"))
    );
    assert_eq!(
        startup.iter().map(|entry| &entry.value).collect::<Vec<_>>(),
        [
            &ConfigValue::Integer(2560),
            &ConfigValue::Integer(1440),
            &ConfigValue::Integer(1280),
            &ConfigValue::Integer(640),
        ]
    );
    for raw in ["0", "16385"] {
        assert!(
            save_config_draft(
                &mut controller.library,
                plugin,
                "game",
                &schema,
                &BTreeMap::from([
                    ("config.render_width".into(), "3840".into()),
                    ("config.render_height".into(), raw.into()),
                    ("config.logical_width".into(), "800".into()),
                ]),
            )
            .is_err()
        );
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
                id: "render_width".into(),
                value: ConfigValue::Integer(1920),
            },
            ConfigEntry {
                id: "render_height".into(),
                value: ConfigValue::Integer(1080),
            },
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
        assert!(model.common_engine_config_fields.is_empty());
        assert!(
            model
                .family_config_fields
                .iter()
                .all(|field| field.key == "manager.emulator_id")
        );
        assert_eq!(model.family_config_fields[0].value, "other-emulator");
        assert!(
            controller
                .family_config_changed("config.launch_mode", "direct")
                .is_err()
        );
        assert!(controller.reset_family_config().is_err());
    }
    controller
        .family_config_changed("manager.emulator_id", "config-test-plugin")
        .unwrap();
    let model = controller.model().unwrap();
    assert_eq!(
        model
            .family_config_fields
            .iter()
            .find(|field| field.key == "config.launch_mode")
            .unwrap()
            .value,
        "title"
    );
}
