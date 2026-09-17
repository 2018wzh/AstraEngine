use super::*;
const MAX_READ_MESSAGES: usize = 131_072;

pub(super) fn identity(
    script: Hash256,
    source: crate::SourceSpan,
    message_id: i64,
    text_hash: Hash256,
) -> Hash256 {
    let mut material = Vec::with_capacity(160);
    material.extend_from_slice(b"astra.emu.musica.message.read.v1\0");
    material.extend_from_slice(script.as_bytes());
    material.extend_from_slice(&source.offset.to_le_bytes());
    material.extend_from_slice(&source.length.to_le_bytes());
    material.extend_from_slice(&message_id.to_le_bytes());
    material.extend_from_slice(text_hash.as_bytes());
    Hash256::from_sha256(&material)
}
pub(super) fn is_read(state: &MusicaRuntimeState) -> bool {
    state.message.as_ref().is_some_and(|message| {
        state
            .read_message_identities
            .binary_search(&message.read_identity)
            .is_ok()
    })
}
pub(super) fn validate(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if state.read_message_identities.len() > MAX_READ_MESSAGES
        || state
            .read_message_identities
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(MusicaRuntimeError::State);
    }
    if let Some(message) = &state.message {
        let entry = state.backlog.last().ok_or(MusicaRuntimeError::State)?;
        if entry.source != message.source
            || entry.message_id != message.message_id
            || identity(
                state.script_hash,
                message.source,
                message.message_id,
                entry.text_hash,
            ) != message.read_identity
        {
            return Err(MusicaRuntimeError::State);
        }
    }
    Ok(())
}
impl MusicaVm {
    pub fn active_message_is_read(&self) -> Result<bool, MusicaRuntimeError> {
        if self.state.message.is_none() {
            return Err(MusicaRuntimeError::State);
        }
        Ok(is_read(&self.state))
    }
    pub(super) fn mark_active_message_read(&mut self) -> Result<(), MusicaRuntimeError> {
        let identity = self
            .state
            .message
            .as_ref()
            .ok_or(MusicaRuntimeError::State)?
            .read_identity;
        if let Err(index) = self.state.read_message_identities.binary_search(&identity) {
            if self.state.read_message_identities.len() >= MAX_READ_MESSAGES {
                return Err(MusicaRuntimeError::State);
            }
            self.state.read_message_identities.insert(index, identity);
        }
        Ok(())
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
    fn finish(vm: &mut MusicaVm) {
        let MusicaWaitState::Input { token_id } = vm.state().wait.clone().unwrap() else {
            panic!("expected message");
        };
        vm.resolve_wait(&token_id).unwrap();
    }
    #[test]
    fn read_skip_marks_completion_and_distinguishes_source_locations() {
        let source = b".message 1  speaker Same\r\n.message 1  speaker Same\r\n.end\r\n";
        let mut vm = vm(source);
        vm.toggle_play_mode(MusicaPlayMode::Skip).unwrap();
        vm.step(1).unwrap();
        assert!(!vm.active_message_is_read().unwrap());
        assert!(!vm.fast_forward_active());
        finish(&mut vm);
        assert!(vm.active_message_is_read().unwrap());
        assert!(vm.fast_forward_active());
        vm.step(2).unwrap();
        assert!(!vm.active_message_is_read().unwrap());
        assert!(!vm.fast_forward_active());
        finish(&mut vm);
        assert_eq!(vm.state().read_message_identities.len(), 2);
        let saved = vm.encode_native_save().unwrap();
        for kind in 0..2 {
            let mut state = vm.state().clone();
            if kind == 0 {
                state
                    .read_message_identities
                    .push(state.read_message_identities[0]);
            } else {
                state.message.as_mut().unwrap().read_identity = Hash256::from_sha256(b"invalid");
            }
            assert!(vm
                .restore_native_save(&postcard::to_allocvec(&state).unwrap(), 1)
                .is_err());
            assert_eq!(vm.encode_native_save().unwrap(), saved);
        }
    }
    #[test]
    fn read_identity_is_revision_bound_and_modes_are_exclusive() {
        let source = b".label start\r\n.message 1  speaker First\r\n.goto start\r\n";
        let mut vm = vm(source);
        vm.step(1).unwrap();
        finish(&mut vm);
        let read = vm.state().read_message_identities[0];
        vm.toggle_play_mode(MusicaPlayMode::Auto).unwrap();
        vm.toggle_play_mode(MusicaPlayMode::Skip).unwrap();
        assert_eq!(vm.state().system_ui.play_mode, MusicaPlayMode::Skip);
        vm.step(2).unwrap();
        assert_eq!(vm.state().message.as_ref().unwrap().read_identity, read);
        assert!(vm.fast_forward_active());
        vm.state.system_ui.skip_enabled = false;
        assert!(!vm.fast_forward_active());
        let changed = b".label start\r\n.message 1  speaker First\r\n.goto start\r\n;revision\r\n";
        vm.replace_script(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(changed),
            crate::parse_sc(changed, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            None,
        )
        .unwrap();
        vm.step(3).unwrap();
        assert!(!vm.active_message_is_read().unwrap());
    }
}
