use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterPreset {
    #[default]
    None,
    Scale,
    Sharpen,
    Anime4kRestoreUpscale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterConfiguration {
    pub preset: FilterPreset,
    pub scale: f32,
    pub strength: f32,
    pub parameters: BTreeMap<String, f32>,
}

impl Default for FilterConfiguration {
    fn default() -> Self {
        Self {
            preset: FilterPreset::None,
            scale: 1.0,
            strength: 0.35,
            parameters: BTreeMap::new(),
        }
    }
}

/// Private Manager settings; shader source never enters a Family or save file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterSettings {
    pub configuration: FilterConfiguration,
    pub source: Option<String>,
}

impl FilterSettings {
    pub fn validate(&self) -> Result<(), String> {
        let c = &self.configuration;
        if !c.scale.is_finite()
            || !(1.0..=4.0).contains(&c.scale)
            || !c.strength.is_finite()
            || !(0.0..=1.0).contains(&c.strength)
            || (c.preset == FilterPreset::Anime4kRestoreUpscale && c.scale != 2.0)
            || c.parameters.len() > 64
            || c.parameters
                .iter()
                .any(|(key, value)| key.is_empty() || key.len() > 128 || !value.is_finite())
            || self
                .source
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 1_048_576)
        {
            return Err("ASTRA_EMU_FILTER_SETTINGS_INVALID".into());
        }
        Ok(())
    }
}
