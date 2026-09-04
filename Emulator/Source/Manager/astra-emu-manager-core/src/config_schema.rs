//! Generic config schema for Family / Extension / Filter.
//! Manager-owned registry that drives generic Slint forms without hard-coding
//! per-family UI. ABI v9/v1 remain stable: schema lives in Manager, not in
//! the descriptor wire. Future Family ABI v10 may move schema to plugin-
//! provided hook `astra.emu.config.schema.v1`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFieldKind {
    String,
    Secret,
    Integer,
    Bool,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFieldDescriptor {
    /// Stable key, e.g. "fvp.nls" or "translate.endpoint".
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: ConfigFieldKind,
    /// Stringified default, e.g. "shift_jis" or "30000".
    pub default_value: Option<String>,
    pub required: bool,
    /// For Enum kind.
    #[serde(default)]
    pub enum_values: Vec<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// For Secret/String length bounds.
    pub max_len: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigSchema {
    pub schema_id: String,
    /// Family or Extension provider id this schema belongs to, e.g. "fvp" or "translate.openai-compatible".
    pub owner_id: String,
    pub title: String,
    pub fields: Vec<ConfigFieldDescriptor>,
}

impl ConfigSchema {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_id.is_empty() || self.owner_id.is_empty() {
            return Err("schema_id/owner_id must be non-empty".into());
        }
        if self.fields.is_empty() {
            return Err(format!(
                "schema {} must have at least one field",
                self.schema_id
            ));
        }
        let mut keys = std::collections::BTreeSet::new();
        for field in &self.fields {
            if field.key.is_empty() || field.key.len() > 128 {
                return Err(format!("field key {:?} invalid", field.key));
            }
            if !keys.insert(&field.key) {
                return Err(format!("duplicate field key {}", field.key));
            }
            if field.label.is_empty() || field.description.is_empty() {
                return Err(format!(
                    "field {} label/description must be non-empty",
                    field.key
                ));
            }
            match field.kind {
                ConfigFieldKind::Enum => {
                    if field.enum_values.is_empty() {
                        return Err(format!("enum field {} must have values", field.key));
                    }
                    if let Some(default) = &field.default_value {
                        if !field.enum_values.contains(default) {
                            return Err(format!(
                                "enum field {} default {:?} not in values",
                                field.key, default
                            ));
                        }
                    }
                }
                ConfigFieldKind::Integer => {
                    if let (Some(min), Some(max)) = (field.min, field.max) {
                        if min > max {
                            return Err(format!("field {} min > max", field.key));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Validate a concrete options map against this schema.
    pub fn validate_options(&self, options: &BTreeMap<String, String>) -> Result<(), String> {
        self.validate()?;
        for field in &self.fields {
            let value = options.get(&field.key);
            if field.required && value.is_none() {
                // Allow missing if default exists, otherwise error.
                if field.default_value.is_none() {
                    return Err(format!("required field {} is missing", field.key));
                }
                continue;
            }
            let Some(value) = value else { continue };
            match field.kind {
                ConfigFieldKind::String | ConfigFieldKind::Secret => {
                    if let Some(max_len) = field.max_len {
                        if value.len() > max_len {
                            return Err(format!("field {} exceeds max_len {}", field.key, max_len));
                        }
                    }
                    if value.is_empty() && field.required {
                        return Err(format!("field {} must be non-empty", field.key));
                    }
                }
                ConfigFieldKind::Integer => {
                    let parsed: i64 = value
                        .parse()
                        .map_err(|_| format!("field {} must be integer", field.key))?;
                    if let Some(min) = field.min {
                        if parsed < min {
                            return Err(format!("field {} < min {}", field.key, min));
                        }
                    }
                    if let Some(max) = field.max {
                        if parsed > max {
                            return Err(format!("field {} > max {}", field.key, max));
                        }
                    }
                }
                ConfigFieldKind::Bool => {
                    if value != "true" && value != "false" {
                        return Err(format!("field {} must be true/false", field.key));
                    }
                }
                ConfigFieldKind::Enum => {
                    if !field.enum_values.contains(value) {
                        return Err(format!(
                            "field {} value {:?} not in {:?}",
                            field.key, value, field.enum_values
                        ));
                    }
                }
            }
        }
        // Reject unknown keys.
        for key in options.keys() {
            if !self.fields.iter().any(|f| &f.key == key) {
                return Err(format!("unknown field {}", key));
            }
        }
        Ok(())
    }

    /// Merge options with defaults for UI display.
    pub fn with_defaults(&self, options: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut merged = options.clone();
        for field in &self.fields {
            if !merged.contains_key(&field.key) {
                if let Some(default) = &field.default_value {
                    merged.insert(field.key.clone(), default.clone());
                }
            }
        }
        merged
    }
}

// ---------------------------------------------------------------------------
// Built-in registry — Manager-owned, not ABI-transported.
// ---------------------------------------------------------------------------

pub fn family_config_schema(family_id: &str) -> Option<ConfigSchema> {
    match family_id {
        "fvp" => Some(ConfigSchema {
            schema_id: "astra.emu.family.fvp.config.v1".into(),
            owner_id: "fvp".into(),
            title: "FVP Family Options".into(),
            fields: vec![
                ConfigFieldDescriptor {
                    key: "fvp.nls".into(),
                    label: "Text encoding (NLS)".into(),
                    description:
                        "Source text encoding for FVP HCB scripts. Wrong value causes garbled text."
                            .into(),
                    kind: ConfigFieldKind::Enum,
                    default_value: Some("shift_jis".into()),
                    required: true,
                    enum_values: vec!["shift_jis".into(), "gbk".into(), "utf8".into()],
                    min: None,
                    max: None,
                    max_len: None,
                },
                ConfigFieldDescriptor {
                    key: "fvp.pack_paths".into(),
                    label: "Pack paths (JSON)".into(),
                    description: "JSON array of pack path overrides. Empty uses auto-probe.".into(),
                    kind: ConfigFieldKind::String,
                    default_value: Some("[]".into()),
                    required: false,
                    enum_values: vec![],
                    min: None,
                    max: None,
                    max_len: Some(4096),
                },
            ],
        }),
        "minori" => Some(ConfigSchema {
            schema_id: "astra.emu.family.minori.config.v1".into(),
            owner_id: "minori".into(),
            title: "Minori Family Options".into(),
            fields: vec![ConfigFieldDescriptor {
                key: "minori.locale".into(),
                label: "Locale".into(),
                description: "Runtime locale for Minori PAZ scripts.".into(),
                kind: ConfigFieldKind::Enum,
                default_value: Some("ja-JP".into()),
                required: false,
                enum_values: vec!["ja-JP".into(), "zh-CN".into(), "en-US".into()],
                min: None,
                max: None,
                max_len: None,
            }],
        }),
        _ => None,
    }
}

pub fn extension_config_schema(extension_id: &str) -> Option<ConfigSchema> {
    match extension_id {
        "astra.emu.translation.openai-compatible" | "translate.openai-compatible" => {
            Some(ConfigSchema {
                schema_id: "astra.emu.extension.translate.config.v1".into(),
                owner_id: extension_id.into(),
                title: "Translation Provider".into(),
                fields: vec![
                    ConfigFieldDescriptor {
                        key: "translate.endpoint_kind".into(),
                        label: "Endpoint kind".into(),
                        description: "Provider preset.".into(),
                        kind: ConfigFieldKind::Enum,
                        default_value: Some("ecnu".into()),
                        required: true,
                        enum_values: vec!["ecnu".into(), "openai".into(), "third_party".into()],
                        min: None,
                        max: None,
                        max_len: None,
                    },
                    ConfigFieldDescriptor {
                        key: "translate.endpoint".into(),
                        label: "HTTPS API base URL".into(),
                        description: "Full HTTPS URL, never switched on failure.".into(),
                        kind: ConfigFieldKind::String,
                        default_value: None,
                        required: true,
                        enum_values: vec![],
                        min: None,
                        max: None,
                        max_len: Some(1024),
                    },
                    ConfigFieldDescriptor {
                        key: "translate.protocol".into(),
                        label: "Protocol".into(),
                        description: "responses or chat_completions.".into(),
                        kind: ConfigFieldKind::Enum,
                        default_value: Some("responses".into()),
                        required: true,
                        enum_values: vec!["responses".into(), "chat_completions".into()],
                        min: None,
                        max: None,
                        max_len: None,
                    },
                    ConfigFieldDescriptor {
                        key: "translate.model".into(),
                        label: "Model".into(),
                        description: "No implicit default.".into(),
                        kind: ConfigFieldKind::String,
                        default_value: None,
                        required: true,
                        enum_values: vec![],
                        min: None,
                        max: None,
                        max_len: Some(256),
                    },
                    ConfigFieldDescriptor {
                        key: "translate.target_language".into(),
                        label: "Target language".into(),
                        description: "e.g. zh-CN".into(),
                        kind: ConfigFieldKind::String,
                        default_value: Some("zh-CN".into()),
                        required: true,
                        enum_values: vec![],
                        min: None,
                        max: None,
                        max_len: Some(32),
                    },
                    ConfigFieldDescriptor {
                        key: "translate.context_sentences".into(),
                        label: "Context sentences".into(),
                        description: "0..32".into(),
                        kind: ConfigFieldKind::Integer,
                        default_value: Some("10".into()),
                        required: false,
                        enum_values: vec![],
                        min: Some(0),
                        max: Some(32),
                        max_len: None,
                    },
                    ConfigFieldDescriptor {
                        key: "translate.timeout_ms".into(),
                        label: "Timeout (ms)".into(),
                        description: "1000..120000".into(),
                        kind: ConfigFieldKind::Integer,
                        default_value: Some("30000".into()),
                        required: false,
                        enum_values: vec![],
                        min: Some(1000),
                        max: Some(120000),
                        max_len: None,
                    },
                    ConfigFieldDescriptor {
                        key: "translate.secret".into(),
                        label: "API credential".into(),
                        description: "Stored in platform keyring, never logged.".into(),
                        kind: ConfigFieldKind::Secret,
                        default_value: None,
                        required: false,
                        enum_values: vec![],
                        min: None,
                        max: None,
                        max_len: Some(1024),
                    },
                ],
            })
        }
        _ => None,
    }
}

pub fn filter_config_schema() -> ConfigSchema {
    ConfigSchema {
        schema_id: "astra.emu.filter.config.v1".into(),
        owner_id: "filter".into(),
        title: "Filter Preset".into(),
        fields: vec![ConfigFieldDescriptor {
            key: "filter.preset".into(),
            label: "Final-frame filter".into(),
            description: "FVP has no per-layer metadata, only final-frame presets.".into(),
            kind: ConfigFieldKind::Enum,
            default_value: Some("none".into()),
            required: true,
            enum_values: vec![
                "none".into(),
                "grayscale".into(),
                "crt-soft".into(),
                "warm".into(),
            ],
            min: None,
            max: None,
            max_len: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn fvp_schema_validates() {
        let schema = family_config_schema("fvp").unwrap();
        schema.validate().unwrap();
        let mut opts = BTreeMap::new();
        opts.insert("fvp.nls".into(), "shift_jis".into());
        opts.insert("fvp.pack_paths".into(), "[]".into());
        schema.validate_options(&opts).unwrap();
        let mut bad = opts.clone();
        bad.insert("fvp.nls".into(), "invalid".into());
        assert!(schema.validate_options(&bad).is_err());
    }

    #[test]
    fn translate_schema_validates() {
        let schema = extension_config_schema("astra.emu.translation.openai-compatible").unwrap();
        schema.validate().unwrap();
        let mut opts = BTreeMap::new();
        opts.insert("translate.endpoint_kind".into(), "ecnu".into());
        opts.insert("translate.endpoint".into(), "https://example.com/v1".into());
        opts.insert("translate.protocol".into(), "responses".into());
        opts.insert("translate.model".into(), "gpt-4o".into());
        opts.insert("translate.target_language".into(), "zh-CN".into());
        schema.validate_options(&opts).unwrap();
    }

    #[test]
    fn filter_schema_validates() {
        let schema = filter_config_schema();
        schema.validate().unwrap();
        let mut opts = BTreeMap::new();
        opts.insert("filter.preset".into(), "grayscale".into());
        schema.validate_options(&opts).unwrap();
        opts.insert("filter.preset".into(), "unknown".into());
        assert!(schema.validate_options(&opts).is_err());
    }
}
