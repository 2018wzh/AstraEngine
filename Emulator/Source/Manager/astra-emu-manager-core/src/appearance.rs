use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppearanceSettings {
    pub theme_dark: bool,
    pub theme_mode: String,
    pub accent: String,
    pub grid_columns: i32,
    pub density: String,
    pub view_mode: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme_dark: true,
            theme_mode: "system".into(),
            accent: "blue".into(),
            grid_columns: 3,
            density: "comfortable".into(),
            view_mode: "grid".into(),
        }
    }
}

impl AppearanceSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.theme_mode.as_str(), "system" | "dark" | "light")
            || !matches!(self.accent.as_str(), "blue" | "purple")
            || !matches!(self.density.as_str(), "comfortable" | "compact")
            || !matches!(self.view_mode.as_str(), "grid" | "list")
            || !(1..=8).contains(&self.grid_columns)
        {
            return Err("ASTRA_EMU_APPEARANCE_INVALID".into());
        }
        Ok(())
    }
}
