use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{validate_id, validate_string, UiValidationError, ValidateUi};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiValue {
    Null,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    List(Vec<UiValue>),
    Map(BTreeMap<String, UiValue>),
}

impl UiValue {
    pub fn validate_map_entries(
        values: &BTreeMap<String, UiValue>,
    ) -> Result<(), UiValidationError> {
        for (key, value) in values {
            validate_id("action.value.key", key)?;
            value.validate_depth(1)?;
        }
        Ok(())
    }

    pub fn map_retained_bytes(values: &BTreeMap<String, UiValue>) -> usize {
        values.iter().fold(
            values
                .len()
                .saturating_mul(std::mem::size_of::<(String, UiValue)>()),
            |bytes, (key, value)| {
                bytes
                    .saturating_add(key.len())
                    .saturating_add(value.retained_bytes())
            },
        )
    }

    /// Returns the retained heap payload used by this value without encoding it.
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Null | Self::Bool(_) | Self::Integer(_) | Self::Number(_) => 0,
            Self::String(value) => value.len(),
            Self::List(values) => values.iter().fold(
                values.len().saturating_mul(std::mem::size_of::<UiValue>()),
                |bytes, value| bytes.saturating_add(value.retained_bytes()),
            ),
            Self::Map(values) => Self::map_retained_bytes(values),
        }
    }

    fn validate_depth(&self, depth: usize) -> Result<(), UiValidationError> {
        if depth > 16 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VALUE_DEPTH",
                "UI value nesting exceeds 16",
            ));
        }
        match self {
            Self::Number(value) if !value.is_finite() => Err(UiValidationError::invalid(
                "ASTRA_UI_VALUE_NON_FINITE",
                "UI numeric value must be finite",
            )),
            Self::String(value) => validate_string("action.value", value),
            Self::List(values) => {
                for value in values {
                    value.validate_depth(depth + 1)?;
                }
                Ok(())
            }
            Self::Map(values) => Self::validate_map_entries_at_depth(values, depth + 1),
            Self::Null | Self::Bool(_) | Self::Integer(_) | Self::Number(_) => Ok(()),
        }
    }

    fn validate_map_entries_at_depth(
        values: &BTreeMap<String, UiValue>,
        depth: usize,
    ) -> Result<(), UiValidationError> {
        for (key, value) in values {
            validate_id("action.value.key", key)?;
            value.validate_depth(depth)?;
        }
        Ok(())
    }
}

impl ValidateUi for UiValue {
    fn validate(&self) -> Result<(), UiValidationError> {
        self.validate_depth(0)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiActionEnvelope {
    pub schema: String,
    pub input_sequence: u64,
    pub semantic_target_id: String,
    pub action_id: String,
    pub arguments: BTreeMap<String, UiValue>,
    pub semantic_generation: u64,
}

impl ValidateUi for UiActionEnvelope {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_action_envelope.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_ACTION_SCHEMA",
                "action schema must be astra.ui_action_envelope.v1",
            ));
        }
        validate_id("action.semantic_target_id", &self.semantic_target_id)?;
        validate_id("action.action_id", &self.action_id)?;
        if self.semantic_generation == 0 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_ACTION_SEMANTIC_GENERATION",
                "action semantic generation must be non-zero",
            ));
        }
        for (key, value) in &self.arguments {
            validate_id("action.argument", key)?;
            value.validate_depth(0)?;
        }
        Ok(())
    }
}
