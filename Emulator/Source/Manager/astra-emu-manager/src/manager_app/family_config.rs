use super::*;
use astra_emu_manager_core::model::{ConfigEntry, ConfigField, ConfigKind, ConfigValue};

const MUSICA_PLUGIN: &str = "astra.emu.musica";

impl AstraEmuManagerController {
    pub(super) fn ensure_no_session(&self) -> Result<(), String> {
        if self.session_running() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        Ok(())
    }

    fn game_plugin(&self, game_id: &str) -> Result<String, String> {
        self.game(game_id)?;
        self.candidates
            .get(game_id)
            .map(|candidate| candidate.descriptor.plugin_id.clone())
            .ok_or_else(|| "ASTRA_EMU_EMULATOR_SELECTION_REQUIRED".into())
    }

    fn schema_field(&self, plugin_id: &str, field_id: &str) -> Result<(Vec<ConfigField>, usize), String> {
        let schema = self.registry.configuration(plugin_id)?;
        let index = schema
            .iter()
            .position(|field| field.id == field_id)
            .ok_or_else(|| format!("ASTRA_EMU_FAMILY_CONFIG: {field_id}: unknown field"))?;
        Ok((schema, index))
    }

    fn store_config(
        &mut self,
        plugin_id: &str,
        game_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<(), String> {
        let (schema, index) = self.schema_field(plugin_id, field_id)?;
        let entry = ConfigEntry {
            id: field_id.into(),
            value: parse_config_value(&schema[index], raw)?,
        };
        self.library
            .set_family_override(plugin_id, game_id, &schema, entry)
            .map_err(|error| error.to_string())
    }

    pub(super) fn set_game_emulator(
        &mut self,
        game_id: &str,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        self.install_probe_choice(game_id, plugin_id)?;
        self.model()
    }

    pub(super) fn set_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let plugin = self.game_plugin(game_id)?;
        self.store_config(&plugin, game_id, field_id, raw)?;
        self.model()
    }

    pub(super) fn reset_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let plugin = self.game_plugin(game_id)?;
        self.library
            .remove_family_override(&plugin, game_id, field_id)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn set_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        self.store_config(plugin_id, "", field_id, raw)?;
        self.model()
    }

    pub(super) fn reset_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        self.registry.configuration(plugin_id)?;
        self.library
            .remove_family_override(plugin_id, "", field_id)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn select_emulator_defaults(
        &mut self,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        if plugin_id.is_empty() {
            self.defaults_plugin = None;
        } else {
            self.registry.configuration(plugin_id)?;
            self.defaults_plugin = Some(plugin_id.into());
        }
        self.model()
    }

    pub(super) fn pick_config_path(
        &mut self,
        game_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let plugin = self.game_plugin(game_id)?;
        let (schema, index) = self.schema_field(&plugin, field_id)?;
        let field = &schema[index];
        let mut dialog = rfd::FileDialog::new().set_title(field.label.as_str());
        let picked = if field.flags.directory_path {
            dialog.pick_folder()
        } else if field.flags.file_path {
            if plugin == MUSICA_PLUGIN && field.id == "profile_file" {
                dialog = dialog.add_filter("Musica key.toml", &["toml"]);
            }
            dialog.pick_file()
        } else {
            return Err(format!("ASTRA_EMU_FAMILY_CONFIG: {field_id}: not a path"));
        };
        let Some(path) = picked else {
            return self.model();
        };
        self.store_config_path(&plugin, game_id, field, &path)?;
        self.model()
    }

    fn store_config_path(
        &mut self,
        plugin: &str,
        game_id: &str,
        field: &ConfigField,
        path: &Path,
    ) -> Result<(), String> {
        if plugin == MUSICA_PLUGIN && field.id == "profile_file" {
            astra_emu_musica_keys::load_musica_key_file(path).map_err(|e| e.to_string())?;
        }
        let game = self.game(game_id)?;
        let raw = config_path_value(path, Path::new(&game.location), field.flags.game_relative)?;
        self.store_config(plugin, game_id, &field.id, &raw)
    }

    pub(super) fn game_config_fields(
        &self,
        game_id: &str,
    ) -> Result<Vec<ConfigFieldViewModel>, String> {
        let Some(plugin) = self
            .candidates
            .get(game_id)
            .map(|candidate| candidate.descriptor.plugin_id.as_str())
        else {
            return Ok(Vec::new());
        };
        self.scope_fields(plugin, game_id)
    }

    pub(super) fn emulator_default_fields(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<ConfigFieldViewModel>, String> {
        self.scope_fields(plugin_id, "")
    }

    fn scope_fields(
        &self,
        plugin_id: &str,
        game_id: &str,
    ) -> Result<Vec<ConfigFieldViewModel>, String> {
        let Ok(schema) = self.registry.configuration(plugin_id) else {
            return Ok(Vec::new());
        };
        let entries = self
            .library
            .family_configuration(plugin_id, game_id, &schema)
            .map_err(|error| error.to_string())?;
        let overrides = self
            .library
            .family_overrides(plugin_id, game_id)
            .map_err(|error| error.to_string())?;
        Ok(config_field_views(&schema, &entries, &overrides))
    }
}

/// Canonical path, made relative to the game directory when the field allows it
/// and the path lies inside that directory.
fn config_path_value(path: &Path, game: &Path, game_relative: bool) -> Result<String, String> {
    let path = fs::canonicalize(path).map_err(|_| "ASTRA_EMU_CONFIG_PATH_INVALID")?;
    let game = fs::canonicalize(game).map_err(|_| "ASTRA_EMU_GAME_DIRECTORY_INVALID")?;
    let stored = match path.strip_prefix(&game) {
        Ok(relative) if game_relative && !relative.as_os_str().is_empty() => relative,
        _ => path.as_path(),
    };
    stored
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "ASTRA_EMU_CONFIG_PATH_UTF8".into())
}

pub(super) fn config_field_views(
    schema: &[ConfigField],
    entries: &[ConfigEntry],
    overrides: &[ConfigEntry],
) -> Vec<ConfigFieldViewModel> {
    schema
        .iter()
        .zip(entries)
        .map(|(field, entry)| {
            let (kind, min, max, option_values) = match &field.kind {
                ConfigKind::Bool => ("bool", 0.0, 0.0, Vec::new()),
                ConfigKind::Integer { min, max } => ("integer", *min as f64, *max as f64, Vec::new()),
                ConfigKind::Number { min, max } => ("float", *min, *max, Vec::new()),
                ConfigKind::String { .. } if field.flags.file_path => ("file", 0.0, 0.0, Vec::new()),
                ConfigKind::String { .. } if field.flags.directory_path => {
                    ("directory", 0.0, 0.0, Vec::new())
                }
                ConfigKind::String { .. } => ("string", 0.0, 0.0, Vec::new()),
                ConfigKind::Enum { choices } => ("enum", 0.0, 0.0, choices.clone()),
            };
            ConfigFieldViewModel {
                key: field.id.clone(),
                label: field.label.clone(),
                description: field.description.clone(),
                group: field.group.clone(),
                kind: kind.into(),
                value: raw_config_value(&entry.value),
                display_value: display_config_value(field, &entry.value),
                options: (0..option_values.len())
                    .map(|index| choice_label(field, index))
                    .collect(),
                option_values,
                min,
                max,
                step: field.step.unwrap_or(0.0),
                unit: field.unit.clone(),
                advanced: field.flags.advanced,
                error: String::new(),
                overridden: overrides.iter().any(|stored| stored.id == field.id),
            }
        })
        .collect()
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

fn raw_config_value(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Bool(value) => value.to_string(),
        ConfigValue::Integer(value) => value.to_string(),
        ConfigValue::Number(value) => value.to_string(),
        ConfigValue::String(value) | ConfigValue::Enum(value) => value.clone(),
    }
}

fn display_config_value(field: &ConfigField, value: &ConfigValue) -> String {
    match (value, &field.kind) {
        (ConfigValue::Enum(value), ConfigKind::Enum { choices }) => choices
            .iter()
            .position(|choice| choice == value)
            .map_or_else(|| value.clone(), |index| choice_label(field, index)),
        _ => raw_config_value(value),
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

pub(super) fn parse_config_entries<'a>(
    schema: &[ConfigField],
    values: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<ConfigEntry>, String> {
    values
        .into_iter()
        .map(|(id, raw)| {
            let field = schema
                .iter()
                .find(|field| field.id == id)
                .ok_or_else(|| format!("ASTRA_EMU_FAMILY_CONFIG: {id}: unknown field"))?;
            Ok(ConfigEntry {
                id: field.id.clone(),
                value: parse_config_value(field, raw)?,
            })
        })
        .collect()
}

pub(super) fn parse_config_value(field: &ConfigField, raw: &str) -> Result<ConfigValue, String> {
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
