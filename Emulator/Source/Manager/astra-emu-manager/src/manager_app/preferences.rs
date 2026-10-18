use super::*;

impl AstraEmuManagerController {
    fn update_appearance(
        &mut self,
        update: impl FnOnce(&mut astra_emu_manager_core::AppearanceSettings),
    ) -> Result<(), String> {
        let mut candidate = self.appearance.clone();
        update(&mut candidate);
        candidate.validate()?;
        self.library
            .save_appearance_settings(&candidate)
            .map_err(|e| e.to_string())?;
        self.appearance = candidate;
        Ok(())
    }

    pub(super) fn set_theme(&mut self, dark: bool) -> Result<(), String> {
        self.update_appearance(|settings| {
            settings.theme_dark = dark;
            settings.theme_mode = if dark { "dark" } else { "light" }.into();
        })
    }
    pub(super) fn set_grid_columns(&mut self, columns: i32) -> Result<(), String> {
        self.update_appearance(|settings| settings.grid_columns = columns)
    }
    pub(super) fn set_theme_mode(&mut self, mode: &str) -> Result<(), String> {
        self.update_appearance(|settings| {
            settings.theme_mode = mode.into();
            match mode {
                "dark" => settings.theme_dark = true,
                "light" => settings.theme_dark = false,
                _ => {}
            }
        })
    }
    pub(super) fn set_accent(&mut self, accent: &str) -> Result<(), String> {
        self.update_appearance(|settings| settings.accent = accent.into())
    }
    pub(super) fn set_density(&mut self, density: &str) -> Result<(), String> {
        self.update_appearance(|settings| settings.density = density.into())
    }
    pub(super) fn set_view_mode(&mut self, view_mode: &str) -> Result<(), String> {
        self.update_appearance(|settings| settings.view_mode = view_mode.into())
    }

    pub(super) fn family_config_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        if key == "manager.config_scope" && matches!(value, "game" | "core") {
            self.family_options.clear();
            self.family_options.insert(key.into(), value.into());
            return Ok(());
        }
        if let Some(id) = key.strip_prefix("config.") {
            let (_, schema) = self.config_schema()?;
            if !schema.iter().any(|field| field.id == id)
                || value.len() > astra_emu_manager_core::model::MAX_CONFIG_TEXT_BYTES
            {
                return Err("ASTRA_EMU_FAMILY_CONFIG_FIELD_UNKNOWN".into());
            }
            self.family_options.insert(key.into(), value.into());
            return Ok(());
        }
        if key != "manager.emulator_id" {
            return Err("ASTRA_EMU_FAMILY_CONFIG_FIELD_UNKNOWN".into());
        }
        let game_id = self
            .selected_case_id
            .as_deref()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        let choices = self
            .probe_selection_for_game(game_id)
            .ok_or_else(|| "ASTRA_EMU_EMULATOR_PROBE_REQUIRED".to_owned())?;
        if !choices
            .iter()
            .any(|candidate| candidate.report.plugin_id == value)
        {
            return Err("ASTRA_EMU_EMULATOR_SELECTION_INVALID".into());
        }
        self.family_options
            .retain(|key, _| key == "manager.config_scope");
        self.family_options.insert(key.into(), value.into());
        Ok(())
    }

    pub(super) fn save_family_config(&mut self) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        let game_id = self
            .selected_case_id
            .clone()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        if let Some(plugin_id) = self.family_options.get("manager.emulator_id").cloned() {
            let current = self
                .candidates
                .get(&game_id)
                .map(|candidate| &candidate.report.plugin_id);
            if current != Some(&plugin_id) {
                self.install_probe_choice(&game_id, &plugin_id)?;
                self.family_options
                    .retain(|key, _| key == "manager.config_scope");
                self.diagnostic.clear();
                return self.model();
            }
            self.family_options.remove("manager.emulator_id");
        }
        if !self.candidates.contains_key(&game_id) {
            return Err("ASTRA_EMU_EMULATOR_SELECTION_REQUIRED".into());
        }
        self.save_typed_family_config()?;
        self.model()
    }

    pub(super) fn reset_family_config(&mut self) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        let (plugin, schema) = self.config_schema()?;
        let scope = self.config_scope().to_owned();
        self.library
            .save_family_configuration(&plugin, &scope, &schema, &[])
            .map_err(|error| error.to_string())?;
        self.family_options
            .retain(|key, _| key == "manager.config_scope");
        self.model()
    }
}
