//! Schema-driven Manager settings used by the desktop form.
//!
//! Family-specific runtime options belong to the Family implementation. The
//! Manager only owns global settings for the host and the translation service.

use std::collections::{BTreeMap, BTreeSet};

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
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: ConfigFieldKind,
    pub default_value: Option<String>,
    pub required: bool,
    #[serde(default)]
    pub enum_values: Vec<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub max_len: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigSchema {
    pub schema_id: String,
    pub owner_id: String,
    pub title: String,
    pub fields: Vec<ConfigFieldDescriptor>,
}

impl ConfigSchema {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_id.is_empty() || self.owner_id.is_empty() || self.title.is_empty() {
            return Err("schema identity and title must be non-empty".into());
        }
        if self.fields.is_empty() {
            return Err(format!("schema {} has no fields", self.schema_id));
        }
        let mut keys = BTreeSet::new();
        for field in &self.fields {
            if field.key.is_empty() || field.key.len() > 128 || !keys.insert(&field.key) {
                return Err(format!(
                    "field key {:?} is invalid or duplicated",
                    field.key
                ));
            }
            if field.label.is_empty() || field.description.is_empty() {
                return Err(format!("field {} needs a label and description", field.key));
            }
            if let (Some(min), Some(max)) = (field.min, field.max) {
                if min > max {
                    return Err(format!("field {} has min > max", field.key));
                }
            }
            if matches!(field.kind, ConfigFieldKind::Enum) {
                if field.enum_values.is_empty()
                    || field.enum_values.iter().any(String::is_empty)
                    || duplicate_values(&field.enum_values)
                {
                    return Err(format!("enum field {} has invalid values", field.key));
                }
                if let Some(default) = &field.default_value {
                    if !field.enum_values.contains(default) {
                        return Err(format!("enum field {} default is not a value", field.key));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn validate_options(&self, options: &BTreeMap<String, String>) -> Result<(), String> {
        self.validate()?;
        for field in &self.fields {
            let Some(value) = options.get(&field.key) else {
                if field.required && field.default_value.is_none() {
                    return Err(format!("required field {} is missing", field.key));
                }
                continue;
            };
            if value.is_empty() && field.required {
                return Err(format!("required field {} is empty", field.key));
            }
            if let Some(max_len) = field.max_len {
                if value.chars().count() > max_len {
                    return Err(format!("field {} exceeds its length bound", field.key));
                }
            }
            match field.kind {
                ConfigFieldKind::Integer => {
                    let parsed = value
                        .parse::<i64>()
                        .map_err(|_| format!("field {} must be an integer", field.key))?;
                    if field.min.is_some_and(|min| parsed < min)
                        || field.max.is_some_and(|max| parsed > max)
                    {
                        return Err(format!("field {} is outside its range", field.key));
                    }
                }
                ConfigFieldKind::Bool => {
                    if !matches!(value.as_str(), "true" | "false") {
                        return Err(format!("field {} must be true or false", field.key));
                    }
                }
                ConfigFieldKind::Enum => {
                    if !field.enum_values.contains(value) {
                        return Err(format!("field {} has an unsupported value", field.key));
                    }
                }
                ConfigFieldKind::String | ConfigFieldKind::Secret => {}
            }
        }
        for key in options.keys() {
            if !self.fields.iter().any(|field| &field.key == key) {
                return Err(format!("unknown field {}", key));
            }
        }
        Ok(())
    }

    pub fn with_defaults(&self, options: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut result = options.clone();
        for field in &self.fields {
            if let Some(default) = &field.default_value {
                result
                    .entry(field.key.clone())
                    .or_insert_with(|| default.clone());
            }
        }
        result
    }
}

fn duplicate_values(values: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}

/// The only configuration form owned by the Manager. The credential field is
/// an opaque keyring reference; the credential value never enters this map.
pub fn translation_config_schema() -> ConfigSchema {
    ConfigSchema {
        schema_id: "astra.emu.translation.config.v2".into(),
        owner_id: "astra-emu-translation-openai-compatible".into(),
        title: "Translation service".into(),
        fields: vec![
            ConfigFieldDescriptor {
                key: "translation.endpoint".into(),
                label: "API base URL".into(),
                description: "HTTPS OpenAI-compatible API base URL".into(),
                kind: ConfigFieldKind::String,
                default_value: None,
                required: true,
                enum_values: vec![],
                min: None,
                max: None,
                max_len: Some(1_024),
            },
            ConfigFieldDescriptor {
                key: "translation.protocol".into(),
                label: "Protocol".into(),
                description: "Responses or Chat Completions".into(),
                kind: ConfigFieldKind::Enum,
                default_value: Some("responses".into()),
                required: true,
                enum_values: vec!["responses".into(), "chat_completions".into()],
                min: None,
                max: None,
                max_len: None,
            },
            ConfigFieldDescriptor {
                key: "translation.model".into(),
                label: "Model".into(),
                description: "Model identifier sent to the configured endpoint".into(),
                kind: ConfigFieldKind::String,
                default_value: None,
                required: true,
                enum_values: vec![],
                min: None,
                max: None,
                max_len: Some(256),
            },
            ConfigFieldDescriptor {
                key: "translation.target_language".into(),
                label: "Target language".into(),
                description: "Language tag used in the translation instruction".into(),
                kind: ConfigFieldKind::String,
                default_value: Some("zh-CN".into()),
                required: true,
                enum_values: vec![],
                min: None,
                max: None,
                max_len: Some(32),
            },
            ConfigFieldDescriptor {
                key: "translation.timeout_ms".into(),
                label: "Request timeout (ms)".into(),
                description: "Per-request timeout; default is 15000 ms".into(),
                kind: ConfigFieldKind::Integer,
                default_value: Some("15000".into()),
                required: true,
                enum_values: vec![],
                min: Some(1_000),
                max: Some(120_000),
                max_len: None,
            },
            ConfigFieldDescriptor {
                key: "translation.secret_reference".into(),
                label: "Credential reference".into(),
                description: "Opaque platform-keyring reference".into(),
                kind: ConfigFieldKind::Secret,
                default_value: Some("translation.default".into()),
                required: true,
                enum_values: vec![],
                min: None,
                max: None,
                max_len: Some(128),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_schema_accepts_bounded_options() {
        let schema = translation_config_schema();
        schema.validate().unwrap();
        let mut options = BTreeMap::new();
        options.insert(
            "translation.endpoint".into(),
            "https://example.com/v1".into(),
        );
        options.insert("translation.protocol".into(), "responses".into());
        options.insert("translation.model".into(), "translator".into());
        options.insert("translation.timeout_ms".into(), "15000".into());
        schema
            .validate_options(&schema.with_defaults(&options))
            .unwrap();
    }

    #[test]
    fn unknown_and_out_of_range_options_are_rejected() {
        let schema = translation_config_schema();
        let mut options = BTreeMap::new();
        options.insert("translation.endpoint".into(), "https://example.com".into());
        options.insert("translation.timeout_ms".into(), "0".into());
        assert!(schema.validate_options(&options).is_err());
        options.insert("unexpected".into(), "value".into());
        assert!(schema.validate_options(&options).is_err());
    }
}
