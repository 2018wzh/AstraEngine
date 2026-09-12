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
}
