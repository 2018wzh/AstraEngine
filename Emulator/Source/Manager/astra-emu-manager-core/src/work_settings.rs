//! Per-game overrides for settings owned by the Manager.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::input_mapping::InputMapping;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mapping: Option<InputMapping>,
}

impl GameSettings {
    pub fn is_empty(&self) -> bool {
        self.input_mapping.is_none()
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(mapping) = &self.input_mapping {
            mapping.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_settings_round_trip() {
        let settings = GameSettings::default();
        assert!(settings.is_empty());
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<GameSettings>(&encoded).unwrap(),
            settings
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let result = serde_json::from_str::<GameSettings>(r#"{"old_patch_mode":"safe"}"#);
        assert!(result.is_err());
    }
}
