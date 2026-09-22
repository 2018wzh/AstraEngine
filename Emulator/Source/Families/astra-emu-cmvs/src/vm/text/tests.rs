use super::*;

fn pool(offset: u32) -> CmvsMessageBufferSegment {
    CmvsMessageBufferSegment::PoolString {
        reference: CmvsPs2aPrivateStringReference {
            relative_offset: offset,
        },
    }
}

fn reload(state: &mut CmvsPs2aVmState) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    push_word(state, 0x8000_0000)?;
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: crate::CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: 128,
        },
    )
}

#[test]
fn reload_resolves_one_nested_script_name_before_resetting_the_root() {
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 3;
    state
        .message_string_slots
        .insert(0, vec![CmvsMessageBufferSegment::StringSlot { index: 1 }]);
    state.message_string_slots.insert(1, vec![pool(42)]);
    assert_eq!(
        reload(&mut state).unwrap(),
        Some(CmvsPs2aVmAction::ReloadRootScript {
            source_frame: 3,
            name: CmvsPs2aPrivateStringReference {
                relative_offset: 42,
            },
        })
    );
    assert_eq!(state.current_frame, 0);
    assert_eq!(state.stack_cursor_bytes, 0);
    assert!(!state.execution_failed);
}

#[test]
fn unresolved_script_names_fail_before_root_replacement() {
    for segments in [
        None,
        Some(vec![]),
        Some(vec![pool(1), pool(2)]),
        Some(vec![CmvsMessageBufferSegment::StringSlot { index: 1 }]),
        Some(vec![CmvsMessageBufferSegment::StringSlot { index: 0 }]),
    ] {
        let mut state = CmvsPs2aVmState::new(0);
        state.current_frame = 3;
        if let Some(segments) = segments {
            state.message_string_slots.insert(0, segments);
        }
        assert_eq!(
            reload(&mut state).unwrap_err().code(),
            "ASTRA_EMU_CMVS_VM_SCRIPT_NAME"
        );
        assert_eq!(state.current_frame, 3);
        assert!(state.execution_failed);
        assert_eq!(
            reload(&mut state).unwrap_err().code(),
            "ASTRA_EMU_CMVS_VM_FAILED"
        );
    }
}

#[test]
fn script_name_nesting_keeps_the_declared_bound() {
    let mut state = CmvsPs2aVmState::new(0);
    for index in 0..7 {
        state.message_string_slots.insert(
            index,
            vec![CmvsMessageBufferSegment::StringSlot { index: index + 1 }],
        );
    }
    state.message_string_slots.insert(7, vec![pool(42)]);
    assert_eq!(
        script_name_reference(&state, 0x8000_0000)
            .unwrap()
            .relative_offset,
        42
    );
    state
        .message_string_slots
        .insert(7, vec![CmvsMessageBufferSegment::StringSlot { index: 8 }]);
    state.message_string_slots.insert(8, vec![pool(42)]);
    assert_eq!(
        script_name_reference(&state, 0x8000_0000)
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_VM_SCRIPT_NAME"
    );
}

#[test]
fn call_retains_name_source_before_switching_to_destination_frame() {
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 3;
    push_word(&mut state, 42).unwrap();
    push_word(&mut state, 0).unwrap();
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: crate::CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: 129,
        },
    )
    .unwrap();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::CallScript {
            source_frame: 3,
            frame: 1,
            name: CmvsPs2aPrivateStringReference {
                relative_offset: 42
            },
        })
    );
    assert_eq!(state.current_frame, 1);
}
