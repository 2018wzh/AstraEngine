//! Per-game settings overrides.
//!
//! A work (VN title) may override a subset of the global Manager settings.
//! Every field is optional: `None` means "inherit the global value". The
//! overrides are persisted as JSON in the `work_settings` table and resolved
//! on top of the global settings when a game launches.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::input_mapping::InputMapping;

/// Per-game settings overrides. `None` fields inherit the global settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkSettings {
    /// Per-game device-to-key input mapping override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mapping: Option<InputMapping>,
    /// Per-game filter preset override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_preset: Option<String>,
    /// Per-game patch mode override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_mode: Option<String>,
}

impl WorkSettings {
    /// Whether no override is set at all.
    pub fn is_empty(&self) -> bool {
        self.input_mapping.is_none() && self.filter_preset.is_none() && self.patch_mode.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_mapping::default_vn_preset;

    #[test]
    fn empty_by_default_and_round_trips() {
        let settings = WorkSettings::default();
        assert!(settings.is_empty());
        let json = serde_json::to_string(&settings).unwrap();
        let restored: WorkSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, settings);
    }

    #[test]
    fn input_mapping_override_round_trips() {
        let settings = WorkSettings {
            input_mapping: Some(default_vn_preset()),
            ..WorkSettings::default()
        };
        assert!(!settings.is_empty());
        let json = serde_json::to_string(&settings).unwrap();
        let restored: WorkSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, settings);
        assert!(restored.input_mapping.is_some());
    }

    #[test]
    fn absent_fields_deserialize_as_none() {
        let restored: WorkSettings = serde_json::from_str("{}").unwrap();
        assert!(restored.is_empty());
    }
}
