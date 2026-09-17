use super::*;
use crate::{MusicaConfigChange, MusicaConfigControl, MusicaConfigState};

pub(super) struct ConfigEdit {
    draft: MusicaConfigState,
    return_page: MusicaSystemPage,
}

impl MusicaVm {
    pub fn config(&self) -> &MusicaConfigState {
        &self.config
    }

    pub fn set_config(&mut self, config: MusicaConfigState) -> Result<(), MusicaRuntimeError> {
        config.validate()?;
        if self.config_edit.is_some() {
            return Err(MusicaRuntimeError::State);
        }
        self.config = config;
        Ok(())
    }

    pub fn open_config(&mut self) -> Result<(), MusicaRuntimeError> {
        let page = self.state.system_ui.page.clone();
        if self.state.launch_mode != MusicaLaunchMode::Title
            || self.state.terminal
            || self.config_edit.is_some()
            || !(page == MusicaSystemPage::Title
                || (page == MusicaSystemPage::None && self.state.wait.is_some()))
        {
            return Err(MusicaRuntimeError::State);
        }
        self.config_edit = Some(ConfigEdit {
            draft: self.config.clone(),
            return_page: page,
        });
        self.state.system_ui.page = MusicaSystemPage::Config;
        self.state.system_ui.focus_index = 0;
        Ok(())
    }

    pub fn config_for_presentation(&self) -> Result<&MusicaConfigState, MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Config {
            return Err(MusicaRuntimeError::State);
        }
        self.config_edit
            .as_ref()
            .map(|edit| &edit.draft)
            .ok_or(MusicaRuntimeError::State)
    }

    pub fn apply_config_control(
        &mut self,
        control: MusicaConfigControl,
    ) -> Result<MusicaConfigChange, MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::Config || self.state.terminal {
            return Err(MusicaRuntimeError::State);
        }
        let edit = self.config_edit.as_mut().ok_or(MusicaRuntimeError::State)?;
        if !matches!(
            control,
            MusicaConfigControl::Apply | MusicaConfigControl::Cancel
        ) {
            return edit.draft.edit(control);
        }
        edit.draft.validate()?;
        let edit = self.config_edit.take().ok_or(MusicaRuntimeError::State)?;
        self.state.system_ui.page = edit.return_page;
        self.state.system_ui.focus_index = 0;
        if control == MusicaConfigControl::Apply {
            self.config = edit.draft;
            Ok(MusicaConfigChange::Applied)
        } else {
            Ok(MusicaConfigChange::Cancelled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_config_draft_apply_cancel_and_restore_keep_preferences_outside_slots() {
        let source = b".message 1   Test\r\n.end\r\n";
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            0,
        )
        .unwrap();
        assert!(vm.open_config().is_err());
        vm.begin_title_launch().unwrap();
        let slot = vm.encode_native_save().unwrap();
        vm.open_config().unwrap();
        assert!(vm.open_config().is_err());
        assert!(vm.encode_native_save().is_err());
        vm.apply_config_control(MusicaConfigControl::MessageSpeedAutoPlay(25))
            .unwrap();
        vm.apply_config_control(MusicaConfigControl::ToggleCharacterVoice(0))
            .unwrap();
        assert_eq!(vm.config().message_speed_auto_play, 50);
        assert!(vm.voice_preferences().character_voice_enabled[0]);
        let before = vm.config_for_presentation().unwrap().clone();
        assert!(vm
            .apply_config_control(MusicaConfigControl::BgmVolume(101))
            .is_err());
        assert!(vm
            .apply_config_control(MusicaConfigControl::ToggleCharacterVoice(5))
            .is_err());
        assert_eq!(vm.config_for_presentation().unwrap(), &before);
        vm.apply_config_control(MusicaConfigControl::Cancel)
            .unwrap();
        assert_eq!(vm.config(), &MusicaConfigState::default());
        vm.open_config().unwrap();
        vm.apply_config_control(MusicaConfigControl::MessageSpeedAutoPlay(25))
            .unwrap();
        vm.apply_config_control(MusicaConfigControl::ToggleCharacterVoice(0))
            .unwrap();
        vm.apply_config_control(MusicaConfigControl::Apply).unwrap();
        assert_eq!(vm.config().message_speed_auto_play, 25);
        assert!(!vm.voice_preferences().character_voice_enabled[0]);
        vm.open_config().unwrap();
        assert!(vm.restore_native_save(&[0], 1).is_err());
        assert!(vm.config_for_presentation().is_ok());
        vm.restore_native_save(&slot, 1).unwrap();
        assert_eq!(vm.config().message_speed_auto_play, 25);
        assert!(vm.config_edit.is_none());
        assert_eq!(vm.state.system_ui.page, MusicaSystemPage::Title);
        vm.start_menu_script(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
        )
        .unwrap();
        vm.step(1).unwrap();
        let wait = vm.state.wait.clone();
        vm.open_config().unwrap();
        vm.apply_config_control(MusicaConfigControl::Cancel)
            .unwrap();
        assert_eq!(vm.state.system_ui.page, MusicaSystemPage::None);
        assert_eq!(vm.state.wait, wait);
    }
}
