use super::*;

impl AstraEmuManagerController {
    pub(super) fn set_appearance(
        &mut self,
        settings: astra_emu_manager_core::AppearanceSettings,
    ) -> Result<ManagerViewModel, String> {
        settings.validate()?;
        self.library
            .save_appearance_settings(&settings)
            .map_err(|e| e.to_string())?;
        self.appearance = settings;
        self.model()
    }
}
