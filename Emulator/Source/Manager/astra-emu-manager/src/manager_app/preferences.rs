use super::*;

impl AstraEmuManagerController {
    pub(super) fn set_audio_device(&mut self, value: &str) -> Result<(), String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_AUDIO_DEVICE_SESSION_ACTIVE".into());
        }
        self.audio_device = audio_executor::AudioDeviceKind::parse(value)?;
        self.diagnostic.clear();
        tracing::info!(
            event = "astra.emu.audio.device_selected",
            device_kind = self.audio_device.as_str()
        );
        Ok(())
    }
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
        if key != "family.plugin_id" {
            return Err("ASTRA_EMU_FAMILY_CONFIG_FIELD_UNKNOWN".into());
        }
        let game_id = self
            .selected_case_id
            .as_deref()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        let choices = self
            .probe_selection_for_game(game_id)
            .ok_or_else(|| "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_NOT_REQUIRED".to_owned())?;
        if !choices
            .iter()
            .any(|candidate| candidate.report.plugin_id == value)
        {
            return Err("ASTRA_EMU_FAMILY_PROVIDER_SELECTION_INVALID".into());
        }
        self.family_options.insert(key.into(), value.into());
        Ok(())
    }

    pub(super) fn save_family_config(&mut self) -> Result<ManagerViewModel, String> {
        let game_id = self
            .selected_case_id
            .clone()
            .ok_or_else(|| "ASTRA_EMU_CASE_SELECTION_MISSING".to_owned())?;
        if self.probe_choices.contains_key(&game_id) {
            let plugin_id = self
                .family_options
                .get("family.plugin_id")
                .cloned()
                .ok_or_else(|| "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED".to_owned())?;
            self.install_probe_choice(&game_id, &plugin_id)?;
            self.family_options.remove("family.plugin_id");
            self.diagnostic.clear();
        } else if let Some(value) = self.family_options.get("family.plugin_id") {
            let selected = self
                .selected_case_id
                .as_deref()
                .and_then(|id| self.candidates.get(id))
                .ok_or_else(|| "ASTRA_EMU_FAMILY_PROBE_REQUIRED".to_owned())?;
            if value != &selected.descriptor.plugin_id {
                return Err("ASTRA_EMU_FAMILY_PROVIDER_SELECTION_INVALID".into());
            }
        }
        self.model()
    }

    pub(super) fn reset_family_config(&mut self) -> Result<ManagerViewModel, String> {
        self.family_options.clear();
        self.model()
    }
}
