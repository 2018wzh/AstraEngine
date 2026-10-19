use super::*;
use astra_emu_manager_core::model::{ConfigEntry, ConfigField, ConfigKind, ConfigValue};

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
        if self
            .family_options
            .get("manager.emulator_id")
            .is_some_and(|draft| draft != &plugin)
        {
            return Err("ASTRA_EMU_EMULATOR_SELECTION_REQUIRED".into());
        }
        let schema = self
            .registry
            .configuration(&plugin)
            .map_err(|error| error.to_string())?;
        Ok((plugin, schema))
    }

    pub(super) fn save_typed_family_config(&mut self) -> Result<(), String> {
        let (plugin, schema) = self.config_schema()?;
        let scope = self.config_scope().to_owned();
        save_config_draft(
            &mut self.library,
            &plugin,
            &scope,
            &schema,
            &self.family_options,
        )?;
        self.family_options
            .retain(|key, _| key == "manager.config_scope");
        Ok(())
    }

    pub(super) fn append_typed_family_fields(
        &self,
        fields: &mut Vec<ConfigFieldViewModel>,
    ) -> Result<(), String> {
        let (plugin, schema) = self.config_schema()?;
        let entries = self
            .library
            .family_configuration(&plugin, self.config_scope(), &schema)
            .map_err(|error| error.to_string())?;
        fields.push(ConfigFieldViewModel {
            key: "manager.config_scope".into(),
            label: "配置范围".into(),
            description: "core 为该模拟器默认值，game 为当前游戏覆盖；不同模拟器的配置相互独立，下次启动生效。".into(),
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
        });
        append_config_fields(&schema, &entries, &self.family_options, fields);
        Ok(())
    }
}

fn append_config_fields(
    schema: &[ConfigField],
    entries: &[ConfigEntry],
    drafts: &BTreeMap<String, String>,
    fields: &mut Vec<ConfigFieldViewModel>,
) {
    for (field, entry) in schema.iter().zip(entries) {
        let key = format!("config.{}", field.id);
        let unit = if field.unit.is_empty() {
            String::new()
        } else {
            format!(" {}", field.unit)
        };
        let (kind, choices, constraint) = match &field.kind {
            ConfigKind::Bool => ("bool", Vec::new(), String::new()),
            ConfigKind::Integer { min, max } => {
                ("integer", Vec::new(), format!("{min} … {max}{unit}"))
            }
            ConfigKind::Number { min, max } => {
                ("float", Vec::new(), format!("{min} … {max}{unit}"))
            }
            ConfigKind::String { .. } => ("string", Vec::new(), String::new()),
            ConfigKind::Enum { choices } => (
                "enum",
                (0..choices.len())
                    .map(|index| choice_label(field, index))
                    .collect(),
                String::new(),
            ),
        };
        let description = match (field.description.is_empty(), constraint.is_empty()) {
            (_, true) => field.description.clone(),
            (true, false) => format!("范围：{constraint}"),
            (false, false) => format!("{}（范围：{constraint}）", field.description),
        };
        fields.push(ConfigFieldViewModel {
            value: drafts
                .get(&key)
                .cloned()
                .unwrap_or_else(|| display_config_value(field, &entry.value)),
            key,
            label: field.label.clone(),
            description,
            kind: kind.into(),
            enum_values: choices,
            required: true,
            min: 0,
            max: 0,
        });
    }
}

fn choice_label(field: &ConfigField, index: usize) -> String {
    match &field.kind {
        ConfigKind::Enum { choices } => field
            .choice_labels
            .get(index)
            .or_else(|| choices.get(index))
            .cloned()
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn save_config_draft(
    library: &mut Library,
    plugin: &str,
    scope: &str,
    schema: &[ConfigField],
    drafts: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut entries = library
        .family_configuration(plugin, scope, schema)
        .map_err(|error| error.to_string())?;
    let changes = parse_config_entries(
        schema,
        drafts
            .iter()
            .filter_map(|(key, value)| key.strip_prefix("config.").map(|id| (id, value.as_str()))),
    )?;
    for change in changes {
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == change.id)
            .ok_or("ASTRA_EMU_FAMILY_CONFIG: unknown field")?;
        entry.value = change.value;
    }
    library
        .save_family_configuration(plugin, scope, schema, &entries)
        .map_err(|error| error.to_string())
}

pub(super) fn parse_config_entries<'a>(
    schema: &[ConfigField],
    values: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<ConfigEntry>, String> {
    let mut entries = Vec::new();
    for (id, raw) in values {
        let field = schema
            .iter()
            .find(|field| field.id == id)
            .ok_or_else(|| format!("ASTRA_EMU_FAMILY_CONFIG: {id}: unknown field"))?;
        entries.push(ConfigEntry {
            id: field.id.clone(),
            value: parse_config_value(field, raw)?,
        });
    }
    Ok(entries)
}

fn display_config_value(field: &ConfigField, value: &ConfigValue) -> String {
    match (value, &field.kind) {
        (ConfigValue::Bool(value), _) => value.to_string(),
        (ConfigValue::Integer(value), _) => value.to_string(),
        (ConfigValue::Number(value), _) => value.to_string(),
        (ConfigValue::Enum(value), ConfigKind::Enum { choices }) => choices
            .iter()
            .position(|choice| choice == value)
            .map_or_else(|| value.clone(), |index| choice_label(field, index)),
        (ConfigValue::String(value) | ConfigValue::Enum(value), _) => value.clone(),
    }
}

fn enum_choice(field: &ConfigField, choices: &[String], raw: &str) -> String {
    if choices.iter().any(|choice| choice == raw) {
        return raw.into();
    }
    field
        .choice_labels
        .iter()
        .position(|label| label == raw)
        .and_then(|index| choices.get(index))
        .map_or_else(|| raw.into(), Clone::clone)
}

fn parse_config_value(field: &ConfigField, raw: &str) -> Result<ConfigValue, String> {
    let invalid = || format!("ASTRA_EMU_FAMILY_CONFIG: {}: invalid value", field.id);
    let value = match field.kind {
        ConfigKind::Bool => ConfigValue::Bool(raw.parse().map_err(|_| invalid())?),
        ConfigKind::Integer { .. } => ConfigValue::Integer(raw.parse().map_err(|_| invalid())?),
        ConfigKind::Number { .. } => ConfigValue::Number(raw.parse().map_err(|_| invalid())?),
        ConfigKind::String { .. } => ConfigValue::String(raw.into()),
        ConfigKind::Enum { ref choices } => ConfigValue::Enum(enum_choice(field, choices, raw)),
    };
    if !field.kind.accepts(&value) {
        return Err(invalid());
    }
    Ok(value)
}

#[cfg(test)]
#[path = "family_config_tests.rs"]
mod tests;
