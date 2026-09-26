use super::*;
use crate::{CmvsPs2aStorageRequest, CmvsPs2aVmState, CmvsStringSegment};

fn failure() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_SYSTEM_SAVE_PATH",
        "native system save is invalid or unavailable",
    )
}

impl CmvsArchive {
    pub(crate) fn load_native_system_state(
        &self,
        vm: &mut CmvsPs2aVmState,
        buffer: u16,
    ) -> Result<(), CoreError> {
        if vm.storage_await
            != Some(CmvsPs2aStorageRequest::LoadSystemState {
                path_buffer_offset: buffer,
            })
            || vm.interpreter_words.get(&10756).copied().unwrap_or(0) != 0
            || vm.interpreter_words.get(&10532).copied().unwrap_or(0) != 0
        {
            return Err(failure());
        }
        let root = self.game_root.as_deref().ok_or_else(failure)?;
        let segments = vm
            .interpreter_string_buffers
            .get(&buffer)
            .ok_or_else(failure)?;
        let [CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 }, CmvsStringSegment::OwnedString(prefix)] =
            segments.as_slice()
        else {
            return Err(failure());
        };
        native::validate_search_path(prefix)?;
        let relative = format!("{}system.dat", prefix.replace('\\', "/"));
        match std::fs::metadata(root.join(&relative)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure()),
            Ok(metadata)
                if metadata.is_file()
                    && metadata.len() <= crate::system_save::MAX_SYSTEM_SAVE_BYTES =>
            {
                let path = astra_emu_sdk::resolve_game_file(root, &relative)?;
                let bytes = native::read_native_range(&path, 0, metadata.len())?;
                let identity = vm.interpreter_words.get(&1600).copied().unwrap_or(0);
                let candidate = crate::parse_cmvs_system_save(&bytes, identity)?;
                crate::apply_system_save(vm, candidate)?;
            }
            Ok(_) => return Err(failure()),
        }
        vm.storage_await = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_save::tests::{encode_container, layout_payload};

    fn request() -> CmvsPs2aVmState {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.interpreter_string_buffers.insert(
            5404,
            vec![
                CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 },
                CmvsStringSegment::OwnedString("save/".into()),
            ],
        );
        vm.storage_await = Some(CmvsPs2aStorageRequest::LoadSystemState {
            path_buffer_offset: 5404,
        });
        vm.process_float_words.insert(1024, 77);
        vm.process_string_slots.insert(64, b"old".to_vec());
        vm.native_system_lists[0].push(b"retained".to_vec());
        vm
    }

    #[test]
    fn missing_file_preserves_globals_and_does_not_create_a_file() {
        let (root, mut archive) = super::super::cache_tests::fixture();
        archive.bind_game_root(root.path().to_path_buf()).unwrap();
        let mut vm = request();
        let mut expected = vm.clone();
        expected.storage_await = None;
        archive.load_native_system_state(&mut vm, 5404).unwrap();
        assert_eq!(vm, expected);
        assert!(!root.path().join("save").exists());
    }

    #[test]
    fn native_load_updates_live_owners_and_restores_them_without_a_sidecar() {
        let (root, mut archive) = super::super::cache_tests::fixture();
        archive.bind_game_root(root.path().to_path_buf()).unwrap();
        std::fs::create_dir(root.path().join("save")).unwrap();
        let mut payload = layout_payload();
        payload.splice(44800..44801, b"title\0".iter().copied());
        let bytes = encode_container(&payload, 0);
        let path = root.path().join("save/system.dat");
        std::fs::write(&path, &bytes).unwrap();
        let mut vm = request();
        archive.load_native_system_state(&mut vm, 5404).unwrap();
        assert_eq!(vm.process_float_words.get(&1024), Some(&0x2000_0000));
        assert_eq!(vm.process_string_slots.get(&64), Some(&b"title".to_vec()));
        assert_eq!(vm.native_system_lists[0], vec![b"retained".to_vec()]);
        let restored: CmvsPs2aVmState =
            postcard::from_bytes(&postcard::to_allocvec(&vm).unwrap()).unwrap();
        assert_eq!(restored, vm);
        crate::validate_cmvs390_vm_state(&restored).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn invalid_candidate_leaves_file_request_and_all_globals_unchanged() {
        let (root, mut archive) = super::super::cache_tests::fixture();
        archive.bind_game_root(root.path().to_path_buf()).unwrap();
        std::fs::create_dir(root.path().join("save")).unwrap();
        let path = root.path().join("save/system.dat");
        let valid = encode_container(&layout_payload(), 0);
        let mut checksum = valid.clone();
        checksum[360] ^= 1;
        let mut version = valid.clone();
        version[76] = 2;
        let mut count = valid.clone();
        count[88..92].copy_from_slice(&u32::MAX.to_le_bytes());
        for bytes in [
            checksum,
            version,
            count,
            encode_container(&layout_payload()[..44863], 0),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            let mut vm = request();
            let expected = vm.clone();
            assert!(archive.load_native_system_state(&mut vm, 5404).is_err());
            assert_eq!(vm, expected);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
}
