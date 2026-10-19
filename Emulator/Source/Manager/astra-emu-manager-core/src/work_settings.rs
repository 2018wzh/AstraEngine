//! Per-game overrides for settings owned by the Manager.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::input_mapping::InputMapping;

pub const RENDER_PRESET_HEIGHTS: [u32; 4] = [720, 1080, 1440, 2160];
pub const MAX_RENDER_EXTENT: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RenderPreference {
    Default,
    Adaptive,
    Preset { height: u32 },
    Custom { width: u32, height: u32 },
}

impl RenderPreference {
    pub fn validate(&self) -> Result<(), String> {
        let valid = match *self {
            Self::Default | Self::Adaptive => true,
            Self::Preset { height } => RENDER_PRESET_HEIGHTS.contains(&height),
            Self::Custom { width, height } => {
                (1..=MAX_RENDER_EXTENT).contains(&width)
                    && (1..=MAX_RENDER_EXTENT).contains(&height)
            }
        };
        if valid {
            Ok(())
        } else {
            Err("ASTRA_EMU_RENDER_PREFERENCE_INVALID".to_owned())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRecord {
    pub logical_width: u32,
    pub logical_height: u32,
    pub raster_width: u32,
    pub raster_height: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mapping: Option<InputMapping>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emulator_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<RenderPreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_surface: Option<SurfaceRecord>,
}

impl GameSettings {
    pub fn is_empty(&self) -> bool {
        self.input_mapping.is_none()
            && self.emulator_id.is_none()
            && self.render.is_none()
            && self.last_surface.is_none()
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(mapping) = &self.input_mapping {
            mapping.validate()?;
        }
        if let Some(id) = &self.emulator_id {
            crate::library::validate_id(id).map_err(|_| "ASTRA_EMU_EMULATOR_ID".to_owned())?;
        }
        if let Some(render) = &self.render {
            render.validate()?;
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

    #[test]
    fn render_preference_uses_tagged_snake_case_and_validates_bounds() {
        let custom = RenderPreference::Custom {
            width: 2560,
            height: 1440,
        };
        assert_eq!(
            serde_json::to_value(custom).unwrap(),
            serde_json::json!({"mode": "custom", "width": 2560, "height": 1440})
        );
        assert_eq!(
            serde_json::from_str::<RenderPreference>(r#"{"mode":"preset","height":1080}"#).unwrap(),
            RenderPreference::Preset { height: 1080 }
        );
        assert_eq!(
            serde_json::to_value(RenderPreference::Default).unwrap(),
            serde_json::json!({"mode": "default"})
        );
        for valid in [
            RenderPreference::Default,
            RenderPreference::Adaptive,
            RenderPreference::Preset { height: 2160 },
            RenderPreference::Custom {
                width: 1,
                height: 16_384,
            },
        ] {
            valid.validate().unwrap();
        }
        for invalid in [
            RenderPreference::Preset { height: 900 },
            RenderPreference::Custom {
                width: 0,
                height: 720,
            },
            RenderPreference::Custom {
                width: 1280,
                height: 16_385,
            },
        ] {
            assert_eq!(
                invalid.validate().unwrap_err(),
                "ASTRA_EMU_RENDER_PREFERENCE_INVALID"
            );
            let settings = GameSettings {
                render: Some(invalid),
                ..Default::default()
            };
            assert!(settings.validate().is_err());
        }
    }
}
