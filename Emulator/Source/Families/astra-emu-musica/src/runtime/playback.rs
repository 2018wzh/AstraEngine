use super::*;

pub(super) fn auto_wait(token_id: String, units: u8) -> MusicaWaitState {
    let timer_ticks = u32::from(units).max(1);
    MusicaWaitState::Time {
        token_id,
        timer_ticks,
        milliseconds: timer_ticks * 10,
    }
}
impl MusicaVm {
    pub(crate) fn auto_delay_units(&self) -> u8 {
        self.auto_delay_units
    }
    pub(crate) fn set_auto_delay_units(&mut self, value: u8) -> Result<(), MusicaRuntimeError> {
        if value > 100 {
            return Err(MusicaRuntimeError::State);
        }
        self.auto_delay_units = value;
        self.rebind_auto_wait();
        Ok(())
    }
    pub fn toggle_play_mode(&mut self, mode: MusicaPlayMode) -> Result<bool, MusicaRuntimeError> {
        if mode == MusicaPlayMode::Normal
            || self.state.system_ui.page != MusicaSystemPage::None
            || self.state.terminal
        {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.play_mode = if self.state.system_ui.play_mode == mode {
            MusicaPlayMode::Normal
        } else {
            mode
        };
        let rebound = self.rebind_auto_wait();
        tracing::debug!(event = "astra.emu.musica.play_mode.changed", mode = ?self.state.system_ui.play_mode);
        Ok(rebound)
    }
    fn rebind_auto_wait(&mut self) -> bool {
        let Some(message) = &self.state.message else {
            return false;
        };
        if message.auto_advance || message.wait_for_voice || self.state.choice.is_some() {
            return false;
        }
        let token_id = match &self.state.wait {
            Some(MusicaWaitState::Input { token_id } | MusicaWaitState::Time { token_id, .. }) => {
                token_id.clone()
            }
            _ => return false,
        };
        if !token_id.starts_with("musica.message.") {
            return false;
        }
        self.state.wait = Some(if self.state.system_ui.play_mode == MusicaPlayMode::Auto {
            auto_wait(token_id, self.auto_delay_units)
        } else {
            MusicaWaitState::Input { token_id }
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }
    #[test]
    fn auto_mode_does_not_rebind_script_wait_or_voice_markers() {
        for suffix in ["", "\\a", "\\v"] {
            let source = format!(".message 1  speaker First{suffix}\r\n.wait 80\r\n.end\r\n");
            let mut machine = vm(source.as_bytes());
            machine.step(1).unwrap();
            let initial = machine.state().wait.clone();
            assert_eq!(
                machine.toggle_play_mode(MusicaPlayMode::Auto).unwrap(),
                suffix.is_empty()
            );
            if !suffix.is_empty() {
                assert_eq!(machine.state().wait, initial);
            }
            let token = match machine.state().wait.as_ref().unwrap() {
                MusicaWaitState::Input { token_id } | MusicaWaitState::Time { token_id, .. } => {
                    token_id.clone()
                }
                _ => unreachable!(),
            };
            machine.resolve_wait(&token).unwrap();
            machine.step(2).unwrap();
            let authored = machine.state().wait.clone();
            assert!(!machine.toggle_play_mode(MusicaPlayMode::Auto).unwrap());
            assert_eq!(machine.state().wait, authored);
        }
    }
    #[test]
    fn auto_delay_bounds_and_saved_mode() {
        let mut machine = vm(b".message 1  speaker First\r\n.end\r\n");
        assert!(machine.set_auto_delay_units(101).is_err());
        machine.set_auto_delay_units(0).unwrap();
        machine.toggle_play_mode(MusicaPlayMode::Auto).unwrap();
        machine.step(1).unwrap();
        assert!(matches!(
            machine.state().wait,
            Some(MusicaWaitState::Time {
                milliseconds: 10,
                ..
            })
        ));
        let state = MusicaVm::decode_native_save(&machine.encode_native_save().unwrap()).unwrap();
        assert_eq!(state.system_ui.play_mode, MusicaPlayMode::Auto);
    }
}
