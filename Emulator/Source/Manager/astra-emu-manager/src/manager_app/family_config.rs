use super::*;
use astra_emu_family_api::{ConfigField, ConfigKind, ConfigValue};

impl AstraEmuManagerController {
    pub(super) fn config_scope(&self) -> &str {
        if self
            .family_options
            .get("manager.config_scope")
            .is_some_and(|scope| scope == "core")
        {
            ""
        } else {
            self.selected_case_id.as_deref().unwrap_or_default()
        }
    }

    pub(super) fn config_schema(&self) -> Result<(String, Vec<ConfigField>), String> {
        let selected = self
            .selected_case_id
            .as_ref()
            .and_then(|id| self.candidates.get(id))
            .ok_or("ASTRA_EMU_FAMILY_PROBE_REQUIRED")?;
        let plugin = selected.descriptor.plugin_id.clone();
        let schema = self
            .registry
            .configuration(&plugin)
            .map_err(|error| error.to_string())?;
        Ok((plugin, schema.into_vec()))
    }

    pub(super) fn save_typed_family_config(&mut self) -> Result<(), String> {
        let (plugin, schema) = self.config_schema()?;
        let scope = self.config_scope().to_owned();
        let mut entries = self
            .library
            .family_configuration(&plugin, &scope, &schema)
            .map_err(|error| error.to_string())?;
        for entry in &mut entries {
            if let Some(raw) = self.family_options.get(&format!("config.{}", entry.id)) {
                let field = schema
                    .iter()
                    .find(|field| field.id == entry.id)
                    .ok_or("ASTRA_EMU_FAMILY_CONFIG")?;
                entry.value = parse_config_value(field, raw)?;
            }
        }
        self.library
            .save_family_configuration(&plugin, &scope, &schema, &entries)
            .map_err(|error| error.to_string())?;
        self.family_options
            .retain(|key, _| key == "manager.config_scope");
        Ok(())
    }

    pub(super) fn typed_family_fields(&self) -> Result<Vec<GenericConfigFieldViewModel>, String> {
        let (plugin, schema) = self.config_schema()?;
        let entries = self
            .library
            .family_configuration(&plugin, self.config_scope(), &schema)
            .map_err(|error| error.to_string())?;
        let mut fields = vec![GenericConfigFieldViewModel {
            key: "manager.config_scope".into(),
            label: "配置范围".into(),
            description: "core 为该核心默认值，game 为当前游戏覆盖；下次启动生效。".into(),
            kind: "enum".into(),
            value: if self.config_scope().is_empty() {
                "core"
            } else {
                "game"
            }
            .into(),
            enum_values: vec!["game".into(), "core".into()],
            required: true,
            min: 0,
            max: 0,
        }];
        for (field, entry) in schema.iter().zip(entries) {
            let key = format!("config.{}", field.id);
            let (kind, choices, constraint) = match &field.kind {
                ConfigKind::Bool => ("bool", Vec::new(), String::new()),
                ConfigKind::Integer { min, max } => {
                    ("integer", Vec::new(), format!("{min} … {max}"))
                }
                ConfigKind::Number { min, max } => ("float", Vec::new(), format!("{min} … {max}")),
                ConfigKind::String { max_bytes } => (
                    "string",
                    Vec::new(),
                    format!("最多 {max_bytes} UTF-8 bytes"),
                ),
                ConfigKind::Enum { choices } => (
                    "enum",
                    choices.iter().map(ToString::to_string).collect(),
                    String::new(),
                ),
            };
            fields.push(GenericConfigFieldViewModel {
                value: self
                    .family_options
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| display_config_value(&entry.value)),
                key,
                label: field.label.to_string(),
                description: format!("{} {constraint}", field.group),
                kind: kind.into(),
                enum_values: choices,
                required: true,
                min: 0,
                max: 0,
            });
        }
        Ok(fields)
    }
}

fn display_config_value(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Bool(value) => value.to_string(),
        ConfigValue::Integer(value) => value.to_string(),
        ConfigValue::Number(value) => value.to_string(),
        ConfigValue::String(value) | ConfigValue::Enum(value) => value.to_string(),
    }
}

fn parse_config_value(field: &ConfigField, raw: &str) -> Result<ConfigValue, String> {
    let invalid = || format!("ASTRA_EMU_FAMILY_CONFIG: {}: invalid value", field.id);
    let value = match field.kind {
        ConfigKind::Bool => ConfigValue::Bool(raw.parse().map_err(|_| invalid())?),
        ConfigKind::Integer { .. } => ConfigValue::Integer(raw.parse().map_err(|_| invalid())?),
        ConfigKind::Number { .. } => ConfigValue::Number(raw.parse().map_err(|_| invalid())?),
        ConfigKind::String { .. } => ConfigValue::String(raw.into()),
        ConfigKind::Enum { .. } => ConfigValue::Enum(raw.into()),
    };
    if !field.kind.accepts(&value) {
        return Err(invalid());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConfigProvider;

    impl astra_emu_family_api::FamilyProvider for ConfigProvider {
        fn descriptor(&self) -> astra_emu_family_api::FamilyResult<astra_emu_family_api::FamilyDescriptor> {
            Ok(astra_emu_family_api::FamilyDescriptor {
                family_id: "config-test-family".into(),
                plugin_id: "config-test-plugin".into(),
                abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
                version: "1.0.0".into(),
                capabilities: vec![astra_emu_family_api::FamilyCapability::CpuFrame].into(),
                supported_formats: vec!["config.test".into()].into(),
                configuration: vec![ConfigField {
                    id: "launch_mode".into(),
                    label: "Launch mode".into(),
                    group: "Startup".into(),
                    kind: ConfigKind::Enum {
                        choices: vec!["direct".into(), "title".into()].into(),
                    },
                    default: ConfigValue::Enum("direct".into()),
                }]
                .into(),
            })
        }

        fn probe(
            &self,
            _request: astra_emu_family_api::ProbeRequest,
        ) -> astra_emu_family_api::FamilyResult<Option<astra_emu_family_api::ProbeReport>> {
            Ok(None)
        }

        fn open(
            &mut self,
            _request: astra_emu_family_api::OpenRequest,
        ) -> astra_emu_family_api::FamilyResult<astra_emu_family_api::FamilyOpen> {
            unreachable!("configuration persistence test does not open a family session")
        }
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
                abi_fingerprint: astra_emu_manager_core::INDEPENDENT_FAMILY_ABI_FINGERPRINT
                    .into(),
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
        controller
            .registry
            .register_provider(ConfigProvider)
            .unwrap();
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
        controller
            .candidates
            .insert("game".into(), configuration_candidate());
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
            choices: vec!["shift_jis".into()].into(),
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
        let fields = controller.typed_family_fields().unwrap();
        let launch_mode = fields
            .iter()
            .find(|field| field.key == "config.launch_mode")
            .unwrap();
        assert_eq!(launch_mode.value, "title");
    }
}
