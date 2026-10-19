use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppearanceSettings {
    pub theme_mode: String,
    pub density: String,
    pub cover_size: String,
    pub view_mode: String,
    pub reduce_motion: bool,
    pub cover_badges: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme_mode: "system".into(),
            density: "comfortable".into(),
            cover_size: "medium".into(),
            view_mode: "grid".into(),
            reduce_motion: false,
            cover_badges: true,
        }
    }
}

impl AppearanceSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.theme_mode.as_str(), "system" | "light" | "dark")
            || !matches!(self.density.as_str(), "comfortable" | "compact")
            || !matches!(self.cover_size.as_str(), "large" | "medium" | "small")
            || !matches!(self.view_mode.as_str(), "grid" | "list")
        {
            return Err("ASTRA_EMU_APPEARANCE_INVALID".into());
        }
        Ok(())
    }
}
