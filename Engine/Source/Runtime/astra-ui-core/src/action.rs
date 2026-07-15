use std::collections::BTreeMap;

use astra_core::Hash256;
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
            Self::Map(values) => {
                for (key, value) in values {
                    validate_id("action.value.key", key)?;
                    value.validate_depth(depth + 1)?;
                }
                Ok(())
            }
            Self::Null | Self::Bool(_) | Self::Integer(_) | Self::Number(_) => Ok(()),
        }
    }
}

impl ValidateUi for UiValue {
    fn validate(&self) -> Result<(), UiValidationError> {
        self.validate_depth(0)?;
        crate::validate_serialized_size(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiActionEnvelope {
    pub schema: String,
    pub input_sequence: u64,
    pub semantic_target_id: String,
    pub action_id: String,
    pub arguments: BTreeMap<String, UiValue>,
    pub semantic_snapshot_hash: Hash256,
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
        for (key, value) in &self.arguments {
            validate_id("action.argument", key)?;
            value.validate_depth(0)?;
        }
        crate::validate_serialized_size(self)
    }
}
