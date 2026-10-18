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
        common: &mut Vec<GenericConfigFieldViewModel>,
        specific: &mut Vec<GenericConfigFieldViewModel>,
    ) -> Result<(), String> {
        let (plugin, schema) = self.config_schema()?;
        let entries = self
            .library
            .family_configuration(&plugin, self.config_scope(), &schema)
            .map_err(|error| error.to_string())?;
        let descriptor = &self
            .selected_case_id
            .as_ref()
            .and_then(|id| self.candidates.get(id))
            .ok_or("ASTRA_EMU_FAMILY_PROBE_REQUIRED")?
            .descriptor;
        specific.push(GenericConfigFieldViewModel {
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
        append_config_fields(
            descriptor,
            &schema,
            &entries,
            &self.family_options,
            common,
            specific,
        );
        Ok(())
    }
}

fn append_config_fields(
    descriptor: &FamilyPluginDescriptor,
    schema: &[ConfigField],
    entries: &[ConfigEntry],
    drafts: &BTreeMap<String, String>,
    common: &mut Vec<GenericConfigFieldViewModel>,
    specific: &mut Vec<GenericConfigFieldViewModel>,
) {
    let render_fields = native_render_fields(descriptor, schema);
    for (field, entry) in schema.iter().zip(entries) {
        let key = format!("config.{}", field.id);
        let common_label = render_fields
            .iter()
            .find(|(id, _)| *id == field.id)
            .map(|(_, label)| *label);
        let (kind, choices, constraint) = match &field.kind {
            ConfigKind::Bool => ("bool", Vec::new(), String::new()),
            ConfigKind::Integer { min, max } => ("integer", Vec::new(), format!("{min} … {max}")),
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
        let view = GenericConfigFieldViewModel {
            value: drafts
                .get(&key)
                .cloned()
                .unwrap_or_else(|| display_config_value(&entry.value)),
            key,
            label: common_label.unwrap_or(&field.label).into(),
            description: if common_label.is_some() {
                let semantics = if field.id == "render_scale" {
                    "核心原生渲染倍率，保持逻辑舞台宽高比"
                } else {
                    "核心原生渲染像素尺寸，不改变逻辑舞台"
                };
                format!("{semantics}；滤镜输入，{constraint}；下次启动生效。")
            } else {
                format!("{} {constraint}", field.group)
            },
            kind: kind.into(),
            enum_values: choices,
            required: true,
            min: 0,
            max: 0,
        };
        if common_label.is_some() {
            common.push(view);
        } else {
            specific.push(view);
        }
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
    for (field, entry) in schema.iter().zip(&mut entries) {
        if let Some(raw) = drafts.get(&format!("config.{}", field.id)) {
            entry.value = parse_config_value(field, raw)?;
        }
    }
    library
        .save_family_configuration(plugin, scope, schema, &entries)
        .map_err(|error| error.to_string())
}

// Only known native descriptor conventions expose R. A width/height pair is
// indivisible: an incomplete or changed schema stays simulator-specific.
fn native_render_fields(
    descriptor: &FamilyPluginDescriptor,
    schema: &[ConfigField],
) -> &'static [(&'static str, &'static str)] {
    let (expected, scaled): (&[(&str, &str)], bool) =
        match (descriptor.family_id.as_str(), descriptor.plugin_id.as_str()) {
            ("fvp", "astra.emu.fvp") => (&[("render_scale", "滤镜输入 · 核心原生渲染倍率")], true),
            ("musica", "astra.emu.musica") => (
                &[
                    ("render_width", "滤镜输入 · 核心原生渲染宽度"),
                    ("render_height", "滤镜输入 · 核心原生渲染高度"),
                ],
                false,
            ),
            ("softpal", "sena-native-koikake") => (
                &[
                    ("raster_width", "滤镜输入 · 核心原生渲染宽度"),
                    ("raster_height", "滤镜输入 · 核心原生渲染高度"),
                ],
                false,
            ),
            _ => return &[],
        };
    if expected.iter().all(|(id, _)| {
        let mut matches = schema.iter().filter(|field| field.id == *id);
        matches.next().is_some_and(|field| {
            let native_range = match (&field.kind, scaled) {
                (ConfigKind::Number { min, max }, true) => {
                    min.is_finite() && max.is_finite() && *min > 0.0 && max >= min
                }
                (ConfigKind::Integer { min, max }, false) => {
                    *min >= 1 && max >= min && *max <= i64::from(u32::MAX)
                }
                _ => false,
            };
            native_range && field.kind.accepts(&field.default)
        }) && matches.next().is_none()
    }) {
        expected
    } else {
        &[]
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
#[path = "family_config_tests.rs"]
mod tests;
