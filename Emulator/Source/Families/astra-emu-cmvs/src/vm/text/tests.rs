use super::*;

fn bind(state: &mut CmvsPs2aVmState, frame: u16, bytes: &[u8]) {
    state
        .frame_string_values
        .insert(frame, BTreeMap::from([(42, bytes.to_vec())]));
}
fn command(state: &mut CmvsPs2aVmState, id: u16) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: crate::CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: id,
        },
    )
}
#[test]
fn global_assignment_copies_bytes_before_slot_and_script_replacement() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"first.ps3");
    append_value_entry_to_buffer(&mut state, 0x120, 42).unwrap();
    materialize_message_buffer(&mut state, 0x121, 0).unwrap();
    state.message_buffer.clear();
    append_value_entry_to_buffer(&mut state, 0x121, 0).unwrap();
    materialize_message_buffer(&mut state, 0x121, 1).unwrap();
    bind(&mut state, 0, b"replacement.ps3");
    state
        .process_string_slots
        .insert(0, b"changed.ps3".to_vec());
    state.current_frame = 3;
    push_word(&mut state, 0x8000_0001).unwrap();
    assert_eq!(
        command(&mut state, 128).unwrap(),
        Some(CmvsPs2aVmAction::ReloadRootScript {
            name: "first.ps3".into()
        })
    );
    assert_eq!(state.current_frame, 0);
}
#[test]
fn call_copies_name_before_switching_to_destination_frame() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 3, b"caller.ps3");
    bind(&mut state, 1, b"wrong.ps3");
    state.current_frame = 3;
    push_word(&mut state, 42).unwrap();
    push_word(&mut state, 0).unwrap();
    assert_eq!(
        command(&mut state, 129).unwrap(),
        Some(CmvsPs2aVmAction::CallScript {
            frame: 1,
            name: "caller.ps3".into()
        })
    );
    assert_eq!(state.current_frame, 1);
}
#[test]
fn native_slots_reject_overflow_nul_and_invalid_encoding_without_overwrite() {
    for bytes in [vec![b'x'; 1024], vec![0], vec![0x81]] {
        let mut state = CmvsPs2aVmState::new(0);
        state.process_string_slots.insert(0, b"safe".to_vec());
        state.message_buffer = bytes;
        assert!(materialize_message_buffer(&mut state, 0x121, 0).is_err());
        assert_eq!(state.process_string_slots[&0], b"safe");
    }
}
#[test]
fn slot_concatenation_and_length_use_copied_native_bytes() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(0, b"chapter".to_vec());
    bind(&mut state, 0, b".ps3");
    append_value_entry_to_buffer(&mut state, 0x121, 0).unwrap();
    append_value_entry_to_buffer(&mut state, 0x120, 42).unwrap();
    materialize_message_buffer(&mut state, 0x121, 1).unwrap();
    assert_eq!(
        resolve_resource_name(&state, 0x8000_0001).unwrap(),
        "chapter.ps3"
    );
    assert_eq!(
        resolve_tagged_string_length(&state, 0x8000_0001).unwrap(),
        11
    );
}
#[test]
fn restore_retains_slot_values_without_serializing_script_pool() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"private-pool.ps3");
    state.process_string_slots.insert(0, b"saved.ps3".to_vec());
    let encoded = postcard::to_allocvec(&state).unwrap();
    assert!(!encoded
        .windows(b"private-pool.ps3".len())
        .any(|s| s == b"private-pool.ps3"));
    let restored: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
    assert_eq!(
        resolve_resource_name(&restored, 0x8000_0000).unwrap(),
        "saved.ps3"
    );
    assert!(resolve_resource_name(&restored, 42).is_err());
}

#[test]
fn legacy_and_unknown_snapshot_versions_are_rejected_at_the_prefix() {
    for bytes in [vec![0], vec![1], vec![2], vec![3], vec![4], vec![6]] {
        assert!(postcard::from_bytes::<CmvsPs2aVmState>(&bytes).is_err());
    }
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(128, b"invalid".to_vec());
    assert!(validate_cmvs390_vm_state(&state).is_err());
    state.process_string_slots.clear();
    state.process_string_slots.insert(0, vec![0x81]);
    assert!(validate_cmvs390_vm_state(&state).is_err());
}

#[test]
fn configured_path_is_owned_across_script_and_global_replacement() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"save\\");
    state.process_string_slots.insert(1, b"save\\".to_vec());
    push_word(&mut state, 0x8000_0001).unwrap();
    command(&mut state, 22).unwrap();
    bind(&mut state, 0, b"different\\");
    state.process_string_slots.insert(1, b"other\\".to_vec());
    state.current_frame = 2;
    let restored: CmvsPs2aVmState =
        postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
    assert_eq!(
        restored.interpreter_string_buffers[&5404],
        vec![
            CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 },
            CmvsStringSegment::OwnedString("save\\".into()),
        ]
    );
}

#[test]
fn unsafe_path_does_not_replace_the_previous_owned_buffer() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"save/");
    push_word(&mut state, 42).unwrap();
    command(&mut state, 22).unwrap();
    let previous = state.interpreter_string_buffers[&5404].clone();
    bind(&mut state, 0, b"../outside/");
    push_word(&mut state, 42).unwrap();
    assert_eq!(
        command(&mut state, 22).unwrap_err().code(),
        "ASTRA_EMU_CMVS_RESOURCE_PATH"
    );
    assert_eq!(state.interpreter_string_buffers[&5404], previous);
}
#[test]
fn missing_or_external_pool_never_falls_back_to_a_different_frame() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 1, b"wrong.ps3");
    assert!(resolve_resource_name(&state, 42).is_err());
    assert!(resolve_resource_name(&state, 0xc000_002a).is_err());
    assert!(resolve_resource_name(&state, 0x8000_0000).is_err());
    assert!(resolve_tagged_string_bytes(&state, 0x8000_0080).is_err());
}

#[test]
fn native_range_copy_reads_source_once_even_when_overwriting_it() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(0, b"copied".to_vec());
    store_process_string_range(&mut state, 0, 3, 0x8000_0000).unwrap();
    state.process_string_slots.insert(0, b"changed".to_vec());
    assert_eq!(state.process_string_slots[&1], b"copied");
    assert_eq!(state.process_string_slots[&2], b"copied");
}

#[test]
fn data_string_domain_reads_mutable_segment_and_preserves_partial_tail() {
    let mut state = CmvsPs2aVmState::new(0);
    state.install_script_data(0, b"name\0x\0").unwrap();
    assert_eq!(resolve_resource_name(&state, 0x4000_0000).unwrap(), "name");
    assert_eq!(resolve_resource_name(&state, 0x4000_0005).unwrap(), "x");
    append_value_entry_to_buffer(&mut state, 0x125, 0).unwrap();
    materialize_message_buffer(&mut state, 0x121, 0).unwrap();
    state
        .script_data_segment_words
        .get_mut(&0)
        .unwrap()
        .insert(0, u32::from_le_bytes(*b"edit"));
    assert_eq!(resolve_resource_name(&state, 0x4000_0000).unwrap(), "edit");
    assert_eq!(resolve_resource_name(&state, 0x8000_0000).unwrap(), "name");
    let restored: CmvsPs2aVmState =
        postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
    assert_eq!(resolve_resource_name(&restored, 0x4000_0005).unwrap(), "x");
    state.install_script_data(0, b"unterminated").unwrap();
    assert!(resolve_resource_name(&state, 0x4000_0000).is_err());
}

#[test]
fn native_search_configuration_is_owned_isolated_and_restorable() {
    let mut state = CmvsPs2aVmState::new(0);
    for (id, bank, path) in [
        (16, CmvsResourceSearchBank::Script, "scripts.cpz"),
        (17, CmvsResourceSearchBank::Texture, "image.cpz"),
        (18, CmvsResourceSearchBank::Native0d0c, "voice.cpz"),
        (19, CmvsResourceSearchBank::CrossfadeAudio, "audio.cpz"),
        (20, CmvsResourceSearchBank::Native0d14, "other.cpz"),
        (23, CmvsResourceSearchBank::Native0d00, "second.cpz"),
        (24, CmvsResourceSearchBank::Native0d04, "third.cpz"),
        (26, CmvsResourceSearchBank::Native0d18, "fourth.cpz"),
    ] {
        state
            .process_string_slots
            .insert(0, path.as_bytes().to_vec());
        push_word(&mut state, 0x8000_0000).unwrap();
        push_word(&mut state, 0).unwrap();
        assert_eq!(command(&mut state, id).unwrap(), None);
        assert_eq!(state.resource_search_paths[&bank][&0], path);
    }
    state.process_string_slots.clear();
    state.current_frame = 1;
    let bytes = postcard::to_allocvec(&state).unwrap();
    let restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
    validate_cmvs390_vm_state(&restored).unwrap();
    assert_eq!(restored.resource_search_paths, state.resource_search_paths);
    assert_eq!(restored.resource_search_paths.len(), 8);
}

#[test]
fn invalid_search_path_preserves_previous_binding_and_invalid_slot_is_native_noop() {
    let mut state = CmvsPs2aVmState::new(0);
    state.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(0, "first.cpz".into())]),
    );
    state
        .process_string_slots
        .insert(0, b"../escape.cpz".to_vec());
    push_word(&mut state, 0x8000_0000).unwrap();
    push_word(&mut state, 0).unwrap();
    assert_eq!(
        command(&mut state, 16).unwrap_err().code(),
        "ASTRA_EMU_CMVS_RESOURCE_PATH"
    );
    assert_eq!(
        state.resource_search_paths[&CmvsResourceSearchBank::Script][&0],
        "first.cpz"
    );
    let mut state = CmvsPs2aVmState::new(0);
    push_word(&mut state, 0xc000_0000).unwrap();
    push_word(&mut state, 5).unwrap();
    assert_eq!(command(&mut state, 16).unwrap(), None);
    assert!(state.resource_search_paths.is_empty());
    state.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(5, "a.cpz".into())]),
    );
    assert_eq!(
        validate_cmvs390_vm_state(&state).unwrap_err().code(),
        "ASTRA_EMU_CMVS_RESOURCE_SLOT"
    );
}
