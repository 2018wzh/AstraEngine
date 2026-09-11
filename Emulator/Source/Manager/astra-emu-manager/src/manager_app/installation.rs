use super::*;
use astra_emu_family_api::FamilyProvider;
use astra_emu_manager_core::{LoadedFamilyPlugin, VerifiedPluginInstall};

impl AstraEmuManagerController {
    pub(super) fn add_game_directory(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        self.scan_paths(&[path.to_owned()])?;
        let games = self.library.list_games().map_err(|e| e.to_string())?;
        if self.selected_case_id.is_none() {
            self.selected_case_id = games.first().map(|game| game.game_id.clone());
        }
        self.diagnostic = if games.is_empty() {
            "未发现受已安装插件支持的游戏。".into()
        } else {
            String::new()
        };
        self.model()
    }

    pub(super) fn install_family_plugin(
        &mut self,
        path: &Path,
    ) -> Result<ManagerViewModel, String> {
        if self.active.is_some() {
            return Err("ASTRA_EMU_PLUGIN_CLOSE_GAME_BEFORE_INSTALL".into());
        }
        let path = path
            .canonicalize()
            .map_err(|_| "ASTRA_EMU_PLUGIN_FILE_MISSING")?;
        let location = path
            .to_str()
            .ok_or("ASTRA_EMU_PLUGIN_PATH_UTF8")?
            .to_owned();
        let plugin = LoadedFamilyPlugin::load(&path).map_err(|e| e.to_string())?;
        let descriptor = plugin.descriptor().map_err(|e| e.to_string())?;
        let id = descriptor.plugin_id.to_string();
        self.registry
            .register_provider(plugin)
            .map_err(|e| e.to_string())?;
        let record = self
            .registry
            .descriptor(&id)
            .ok_or("ASTRA_EMU_PLUGIN_DESCRIPTOR_MISSING")?
            .clone();
        let result =
            VerifiedPluginInstall::from_verified_descriptor(record, location, unix_time_ms()?)
                .and_then(|verified| self.library.install_verified_plugin(&verified));
        if let Err(error) = result {
            self.registry.remove(&id);
            return Err(error.to_string());
        }
        self.diagnostic = "Family 插件已安装，重新扫描以发现支持的游戏。".into();
        self.model()
    }
}
