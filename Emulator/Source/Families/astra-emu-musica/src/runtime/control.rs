use super::*;

impl MusicaVm {
    /// Physical input is session-local and is never restored from a save.
    pub fn set_control_pressed(&mut self, pressed: bool) {
        self.control_pressed = pressed;
    }

    pub fn control_fast_forward_active(&self) -> bool {
        fast_forward_active(&self.state, self.control_pressed)
    }
}

pub(super) fn fast_forward_active(state: &MusicaRuntimeState, pressed: bool) -> bool {
    state.system_ui.skip_enabled && state.system_ui.control_enabled && pressed
}

pub(super) fn execute_pragma(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Pragma)?;
    let [pragma] = tokens.as_slice() else {
        return Err(MusicaRuntimeError::Pragma);
    };
    match pragma.as_str() {
        "enable_control" => state.system_ui.control_enabled = true,
        "disable_control" => state.system_ui.control_enabled = false,
        "skip_enable" => state.system_ui.skip_enabled = true,
        "skip_disable" => state.system_ui.skip_enabled = false,
        _ => return Err(MusicaRuntimeError::Pragma),
    }
    tracing::debug!(
        event = "astra.emu.musica.control.changed",
        control_enabled = state.system_ui.control_enabled,
        skip_enabled = state.system_ui.skip_enabled
    );
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_sc, ScOpcodeCatalog};

    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/control.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }

    #[test]
    fn pragma_gates_are_independent_and_saved_without_physical_keys() {
        let source = b".pragma skip_disable\r\n.pragma enable_control\r\n.wait 1\r\n.pragma skip_enable\r\n.wait 1\r\n.end\r\n";
        let mut original = vm(source);
        original.set_control_pressed(true);
        assert!(matches!(
            original.step(1).unwrap(),
            Some(MusicaVmEvent::Wait(_))
        ));
        assert!(!original.control_fast_forward_active());
        let save = original.encode_native_save().unwrap();
        let mut restored = vm(source);
        restored.restore_native_save(&save, 2).unwrap();
        assert!(restored.state().system_ui.control_enabled);
        assert!(!restored.state().system_ui.skip_enabled);
        let token = match restored.state().wait.as_ref().unwrap() {
            MusicaWaitState::Time { token_id, .. } => token_id.clone(),
            _ => unreachable!(),
        };
        restored.resolve_wait(&token).unwrap();
        // A saved held key must not cause the second wait to be skipped.
        assert!(matches!(
            restored.step(2).unwrap(),
            Some(MusicaVmEvent::Wait(_))
        ));
        original.resolve_wait(&token).unwrap();
        assert_eq!(original.step(2).unwrap(), Some(MusicaVmEvent::Terminal));
        assert!(original.control_fast_forward_active());
    }

    #[test]
    fn disable_control_blocks_held_key_and_unknown_pragmas_fail() {
        let mut machine = vm(b".pragma enable_control\r\n.pragma disable_control\r\n.wait 1\r\n");
        machine.set_control_pressed(true);
        assert!(matches!(
            machine.step(1).unwrap(),
            Some(MusicaVmEvent::Wait(_))
        ));
        assert!(!machine.control_fast_forward_active());
        for source in [
            b".pragma unknown\r\n".as_slice(),
            b".pragma enable_control extra\r\n",
            b".pragma\r\n",
        ] {
            assert_eq!(vm(source).step(1).unwrap_err(), MusicaRuntimeError::Pragma);
        }
    }
}
