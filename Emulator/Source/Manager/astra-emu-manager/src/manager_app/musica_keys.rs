use super::*;
use astra_emu_manager_core::model::ConfigValue;

const PLUGIN: &str = "astra.emu.musica";

impl AstraEmuManagerController {
    pub(super) fn musica_key_file_available(&self) -> bool {
        self.active.is_none()
            && self
                .selected_case_id
                .as_ref()
                .and_then(|id| self.candidates.get(id))
                .is_some_and(|candidate| candidate.descriptor.plugin_id == PLUGIN)
            && self
                .family_options
                .get("manager.emulator_id")
                .is_none_or(|plugin| plugin == PLUGIN)
    }

    pub(super) fn select_musica_key_file(&mut self) -> Result<ManagerViewModel, String> {
        if !self.musica_key_file_available() {
            return Err("ASTRA_EMU_MUSICA_KEY_SELECT_EMULATOR_FIRST".into());
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择 Musica key.toml")
            .add_filter("Musica key.toml", &["toml"])
            .pick_file()
        else {
            return self.model();
        };
        astra_emu_musica::load_musica_key_file(&path).map_err(|error| error.to_string())?;
        let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        let path = path.to_str().ok_or("ASTRA_EMU_MUSICA_KEY_PATH_UTF8")?;
        let game_id = self.selected_game()?.game_id;
        let (plugin, schema) = self.config_schema()?;
        let mut entries = self
            .library
            .family_configuration(&plugin, &game_id, &schema)
            .map_err(|error| error.to_string())?;
        let key = entries
            .iter_mut()
            .find(|entry| entry.id == "profile_file")
            .ok_or("ASTRA_EMU_MUSICA_KEY_CONFIG_FIELD")?;
        key.value = ConfigValue::String(path.into());
        self.library
            .save_family_configuration(&plugin, &game_id, &schema, &entries)
            .map_err(|error| error.to_string())?;
        self.family_options.clear();
        self.family_options
            .insert("manager.config_scope".into(), "game".into());
        self.diagnostic = "已为当前游戏应用 key.toml；原始文件未修改。".into();
        self.model()
    }
}
