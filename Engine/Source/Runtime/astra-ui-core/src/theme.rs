use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{validate_id, validate_string, UiValidationError, ValidateUi};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiThemeValue {
    Color([u8; 4]),
    Number(f32),
    Text(String),
    Asset(String),
    Insets([f32; 4]),
    NineSlice {
        asset: String,
        border: [f32; 4],
    },
    Motion {
        duration_ms: u32,
        easing: String,
        reduced_motion_duration_ms: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiThemeManifest {
    pub schema: String,
    pub id: String,
    pub parent: Option<String>,
    pub tokens: BTreeMap<String, UiThemeValue>,
    pub high_contrast_tokens: BTreeMap<String, UiThemeValue>,
    pub revision: u64,
}

impl ValidateUi for UiThemeManifest {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_theme_manifest.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_THEME_SCHEMA",
                "theme schema must be astra.ui_theme_manifest.v1",
            ));
        }
        validate_id("theme.id", &self.id)?;
        if let Some(parent) = &self.parent {
            validate_id("theme.parent", parent)?;
            if parent == &self.id {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_THEME_SELF_PARENT",
                    "theme must not inherit itself",
                ));
            }
        }
        let mut names = BTreeSet::new();
        for (name, value) in self.tokens.iter().chain(&self.high_contrast_tokens) {
            validate_id("theme.token", name)?;
            validate_theme_value(value)?;
            names.insert(name);
        }
        if self.tokens.is_empty() {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_THEME_EMPTY",
                "theme must define at least one token",
            ));
        }
        if self.revision == 0 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_THEME_REVISION",
                "theme revision must be non-zero",
            ));
        }
        Ok(())
    }
}

fn validate_theme_value(value: &UiThemeValue) -> Result<(), UiValidationError> {
    match value {
        UiThemeValue::Number(value) if !value.is_finite() => Err(UiValidationError::invalid(
            "ASTRA_UI_THEME_NUMBER",
            "theme numeric token must be finite",
        )),
        UiThemeValue::Text(value) => validate_string("theme.text", value),
        UiThemeValue::Asset(value) => validate_id("theme.asset", value),
        UiThemeValue::Insets(values) => validate_floats("theme.insets", values),
        UiThemeValue::NineSlice { asset, border } => {
            validate_id("theme.nine_slice.asset", asset)?;
            validate_floats("theme.nine_slice.border", border)
        }
        UiThemeValue::Motion { easing, .. } => validate_id("theme.motion.easing", easing),
        UiThemeValue::Color(_) | UiThemeValue::Number(_) => Ok(()),
    }
}

fn validate_floats(field: &'static str, values: &[f32; 4]) -> Result<(), UiValidationError> {
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_THEME_METRIC",
            format!("{field} values must be finite and non-negative"),
        ));
    }
    Ok(())
}
