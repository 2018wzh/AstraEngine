use super::*;
use crate::{MusicaConfigChange, MusicaConfigControl, MusicaSystemPage};
impl MusicaSession {
    pub(super) fn config_event(&mut self, event: &FamilyEvent) -> FamilyResult<bool> {
        if self.vm.state().system_ui.page != MusicaSystemPage::Config {
            if matches!(
                event,
                FamilyEvent::Key {
                    code: KeyCode::F7,
                    state: KeyState::Pressed,
                    ..
                }
            ) && self.vm.state().system_ui.page == MusicaSystemPage::None
                && self.vm.state().launch_mode == crate::MusicaLaunchMode::Title
                && self.vm.state().wait.is_some()
            {
                self.vm.open_config().map_err(vm_error)?;
                self.clear_input();
                return Ok(true);
            }
            return Ok(false);
        }
        let control = match event {
            FamilyEvent::Key {
                code: KeyCode::Enter | KeyCode::Space,
                state: KeyState::Pressed,
                ..
            } => Some(MusicaConfigControl::Apply),
            FamilyEvent::Key {
                code: KeyCode::Escape,
                state: KeyState::Pressed,
                ..
            } => Some(MusicaConfigControl::Cancel),
            FamilyEvent::PointerMove { x, y } => {
                self.pointer = Some((*x, *y));
                if self.config_pointer_down {
                    crate::configuration::config_control_at(*x as i32, *y as i32).filter(
                        |control| {
                            matches!(
                                control,
                                MusicaConfigControl::MessageSpeedUnread(_)
                                    | MusicaConfigControl::MessageSpeedRead(_)
                                    | MusicaConfigControl::MessageSpeedAutoPlay(_)
                                    | MusicaConfigControl::BgmVolume(_)
                                    | MusicaConfigControl::VoiceVolume(_)
                                    | MusicaConfigControl::SeVolume(_)
                            )
                        },
                    )
                } else {
                    None
                }
            }
            FamilyEvent::PointerButton {
                button: PointerButton::Primary,
                state,
            } => {
                self.config_pointer_down = *state == KeyState::Pressed;
                if self.config_pointer_down {
                    self.pointer.and_then(|(x, y)| {
                        crate::configuration::config_control_at(x as i32, y as i32)
                    })
                } else {
                    None
                }
            }
            FamilyEvent::WindowFocused { .. }
            | FamilyEvent::WindowResized { .. }
            | FamilyEvent::WindowVisibility { .. }
            | FamilyEvent::WindowSuspended { .. }
            | FamilyEvent::WindowCloseRequested => return Ok(false),
            _ => None,
        };
        let Some(control) = control else {
            return Ok(true);
        };
        if control == MusicaConfigControl::Apply {
            let config = self.vm.config_for_presentation().map_err(vm_error)?;
            self.storage.write_configuration(self.game, config)?;
            if config.fullscreen != self.vm.config().fullscreen {
                self.pending_fullscreen = Some(config.fullscreen);
            }
        }
        match self.vm.apply_config_control(control).map_err(vm_error)? {
            MusicaConfigChange::Present => {}
            MusicaConfigChange::AudioParamsChanged => self.audio.set_preferences(
                self.vm
                    .config_for_presentation()
                    .map_err(vm_error)?
                    .audio_preferences(),
            )?,
            MusicaConfigChange::TestAudio(bus) => {
                let commands = self.vm.config_test_audio(bus).map_err(vm_error)?;
                self.audio.apply(commands)?;
            }
            MusicaConfigChange::Applied | MusicaConfigChange::Cancelled => {
                let commands = self.vm.stop_config_tests().map_err(vm_error)?;
                self.audio.apply(commands)?;
                self.audio
                    .set_preferences(self.vm.config().audio_preferences())?;
                self.scene.set_text_shadow(self.vm.config().text_shadow);
                self.progress_in_background = self.vm.config().progress_in_background;
                self.update_pause()?;
                self.title_focus = None;
                self.clear_input();
            }
        }
        tracing::debug!(event="astra.emu.musica.config.input",page=?self.vm.state().system_ui.page);
        Ok(true)
    }
}
