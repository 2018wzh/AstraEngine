//! Bounded, startup-only Family configuration shared by host and core.
use crate::{FamilyError, FamilyResult};
use abi_stable::{
    std_types::{RString, RVec},
    StableAbi,
};
use serde::{Deserialize, Serialize};

pub const MAX_CONFIG_FIELDS: usize = 128;
pub const MAX_CONFIG_TEXT_BYTES: usize = 4096;
pub const MAX_CONFIG_CHOICES: usize = 128;

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi, Serialize, Deserialize)]
pub enum ConfigValue {
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(RString),
    Enum(RString),
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi, Serialize, Deserialize)]
pub enum ConfigKind {
    Bool,
    Integer { min: i64, max: i64 },
    Number { min: f64, max: f64 },
    String { max_bytes: u32 },
    Enum { choices: RVec<RString> },
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigField {
    pub id: RString,
    pub label: RString,
    /// Display group; empty means the ungrouped section. Groups do not nest.
    pub group: RString,
    pub kind: ConfigKind,
    pub default: ConfigValue,
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, StableAbi, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigEntry {
    pub id: RString,
    pub value: ConfigValue,
}

fn error(index: usize, reason: &str) -> FamilyError {
    FamilyError::invalid(
        "ASTRA_EMU_FAMILY_CONFIG",
        format!("configuration field at index {index}: {reason}"),
    )
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

impl ConfigKind {
    pub fn accepts(&self, value: &ConfigValue) -> bool {
        match (self, value) {
            (Self::Bool, ConfigValue::Bool(_)) => true,
            (Self::Integer { min, max }, ConfigValue::Integer(value)) => {
                min <= value && value <= max
            }
            (Self::Number { min, max }, ConfigValue::Number(value)) => {
                value.is_finite() && min <= value && value <= max
            }
            (Self::String { max_bytes }, ConfigValue::String(value)) => {
                value.len() <= *max_bytes as usize && !value.contains('\0')
            }
            (Self::Enum { choices }, ConfigValue::Enum(value)) => choices.contains(value),
            _ => false,
        }
    }
}

pub fn validate_config_schema(fields: &[ConfigField]) -> FamilyResult<()> {
    if fields.len() > MAX_CONFIG_FIELDS {
        return Err(error(0, "too many fields"));
    }
    for (index, field) in fields.iter().enumerate() {
        if !valid_id(&field.id)
            || fields[..index]
                .iter()
                .any(|previous| previous.id == field.id)
        {
            return Err(error(index, "invalid or duplicate ID"));
        }
        if field.label.is_empty()
            || field.label.len() > 256
            || field.label.chars().any(char::is_control)
            || field.group.len() > 256
            || field.group.chars().any(char::is_control)
        {
            return Err(error(index, "invalid label or group"));
        }
        let valid = match &field.kind {
            ConfigKind::Bool => true,
            ConfigKind::Integer { min, max } => min <= max,
            ConfigKind::Number { min, max } => min.is_finite() && max.is_finite() && min <= max,
            ConfigKind::String { max_bytes } => *max_bytes as usize <= MAX_CONFIG_TEXT_BYTES,
            ConfigKind::Enum { choices } => {
                !choices.is_empty()
                    && choices.len() <= MAX_CONFIG_CHOICES
                    && choices
                        .iter()
                        .enumerate()
                        .all(|(i, choice)| valid_id(choice) && !choices[..i].contains(choice))
            }
        };
        if !valid || !field.kind.accepts(&field.default) {
            return Err(error(index, "invalid range, choices, or default"));
        }
    }
    Ok(())
}

/// Validate explicit entries and materialize omitted defaults in schema order.
/// Duplicate keys are rejected before any lookup, including in persisted input.
pub fn resolve_config(
    fields: &[ConfigField],
    entries: &[ConfigEntry],
) -> FamilyResult<RVec<ConfigEntry>> {
    validate_config_schema(fields)?;
    if entries.len() > MAX_CONFIG_FIELDS {
        return Err(error(0, "too many values"));
    }
    for (index, entry) in entries.iter().enumerate() {
        if entries[..index]
            .iter()
            .any(|previous| previous.id == entry.id)
        {
            return Err(error(index, "duplicate value ID"));
        }
        let field = fields
            .iter()
            .find(|field| field.id == entry.id)
            .ok_or_else(|| error(index, "unknown value ID"))?;
        if !field.kind.accepts(&entry.value) {
            return Err(error(index, "value type or range mismatch"));
        }
    }
    Ok(fields
        .iter()
        .map(|field| ConfigEntry {
            id: field.id.clone(),
            value: entries
                .iter()
                .find(|entry| entry.id == field.id)
                .map(|entry| &entry.value)
                .unwrap_or(&field.default)
                .clone(),
        })
        .collect::<Vec<_>>()
        .into())
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
