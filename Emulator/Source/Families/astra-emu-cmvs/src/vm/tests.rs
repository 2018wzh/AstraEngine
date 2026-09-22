use super::*;
use crate::{CmvsPs2aSourceSpan, CmvsPs2aValueExpression, CmvsPs2aValueExpressionToken};

const SPAN: CmvsPs2aSourceSpan = CmvsPs2aSourceSpan {
    offset: 0,
    byte_length: 2,
};

fn tag_zero_value(offset: u32) -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::ValueExpression(CmvsPs2aValueExpression {
        span: SPAN,
        tokens: vec![CmvsPs2aValueExpressionToken::Value {
            span: CmvsPs2aSourceSpan {
                offset: 2,
                byte_length: 6,
            },
            opcode: 0x120,
            payload: offset,
        }],
    })
}

fn push_current() -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushCurrentCondition {
        span: SPAN,
    })
}

fn push_immediate(value: u32) -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
        span: CmvsPs2aSourceSpan {
            offset: 0,
            byte_length: 8,
        },
        stack_bytes: 4,
        value,
    })
}

#[test]
fn emits_crossfade_audio_with_resource_reference_and_fade() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(&mut state, &push_immediate(1)).unwrap();
    execute_cmvs390_frame(&mut state, &push_immediate(7)).unwrap();
    execute_cmvs390_frame(&mut state, &tag_zero_value(19)).unwrap();
    execute_cmvs390_frame(&mut state, &push_current()).unwrap();
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 160,
        },
    )
    .unwrap();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::PlayCrossfadeAudio {
            secondary: None,
            primary: CmvsPs2aPrivateStringReference {
                relative_offset: 19,
            },
            fade_ms: 7,
            playback_flag: true,
        })
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn rejects_execution_and_snapshot_after_an_unrecovered_expression() {
    let mut state = CmvsPs2aVmState::new(4);
    let before = state.clone();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(crate::CmvsPs2aStackExpression {
            span: SPAN,
            nodes: Vec::new(),
        }),
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION");
    assert!(state.execution_failed);
    assert_eq!(
        execute_cmvs390_frame(&mut state, &push_immediate(1))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_VM_FAILED"
    );
    assert_eq!(
        validate_cmvs390_vm_state(&state).unwrap_err().code(),
        "ASTRA_EMU_CMVS_VM_FAILED"
    );
    // Restoring a separately validated successful snapshot is still allowed.
    validate_cmvs390_vm_state(&before).unwrap();
    state = before;
    execute_cmvs390_frame(&mut state, &push_immediate(1)).unwrap();
}

#[test]
fn instruction_dispatch_reuses_large_vm_allocations() {
    let mut state = CmvsPs2aVmState::new(0);
    state.stack_bytes = vec![0; MAX_STACK_BYTES];
    state.stack_initialized = vec![false; MAX_STACK_BYTES];
    let allocation = state.stack_bytes.as_ptr();
    for value in 0..1024 {
        execute_cmvs390_frame(&mut state, &push_immediate(value)).unwrap();
        assert_eq!(state.stack_bytes.as_ptr(), allocation);
    }
    assert_eq!(state.stack_cursor_bytes, 4096);
    validate_cmvs390_vm_state(&state).unwrap();
}

#[test]
fn partial_instruction_failure_cannot_resume_by_resetting_dispatch_stop() {
    let mut state = CmvsPs2aVmState::new(u32::MAX - 1);
    assert!(execute_cmvs390_frame(&mut state, &push_immediate(42)).is_err());
    assert_eq!(state.stack_cursor_bytes, 4);
    state.dispatch_stopped = false;
    state.program_counter = 0;
    assert_eq!(
        execute_cmvs390_frame(&mut state, &push_immediate(1))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_VM_FAILED"
    );
    assert_eq!(state.stack_cursor_bytes, 4);
}

#[test]
fn dispatch_preconditions_do_not_poison_a_waiting_machine() {
    let mut state = CmvsPs2aVmState::new(0);
    state.filter_graph_input_await = true;
    assert_eq!(
        execute_cmvs390_frame(&mut state, &push_immediate(1))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_VM_STORAGE_AWAIT"
    );
    state.filter_graph_input_await = false;
    state.dispatch_stopped = true;
    assert_eq!(
        execute_cmvs390_frame(&mut state, &push_immediate(1))
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_VM_STOPPED"
    );
    state.dispatch_stopped = false;
    execute_cmvs390_frame(&mut state, &push_immediate(1)).unwrap();
    assert_eq!(state.stack_cursor_bytes, 4);
}

#[test]
fn evaluates_the_proven_literal_stack_expression_before_pushing() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(crate::CmvsPs2aStackExpression {
            span: CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 8,
            },
            nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
                span: CmvsPs2aSourceSpan {
                    offset: 2,
                    byte_length: 6,
                },
                opcode: 0x100,
                payload: 11,
            }],
        }),
    )
    .unwrap();
    execute_cmvs390_frame(&mut state, &push_current()).unwrap();
    assert_eq!(state.stack_bytes, 11u32.to_le_bytes());
}

#[test]
fn evaluates_the_proven_numeric_float_literal_and_branch_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    let frame = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aNumericExpressionNode::Value {
            span: SPAN,
            opcode: 0x12a,
            payload: 1.5_f32.to_bits(),
        }],
    });
    execute_cmvs390_frame(&mut state, &frame).unwrap();
    assert_eq!(state.current_value, Some(1.5_f32.to_bits()));
    assert!(state.condition_flag);
    let zero = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aNumericExpressionNode::Value {
            span: SPAN,
            opcode: 0x12a,
            payload: 0.0_f32.to_bits(),
        }],
    });
    execute_cmvs390_frame(&mut state, &zero).unwrap();
    assert!(!state.condition_flag);
}

#[test]
fn assigns_float_literal_through_the_recovered_numeric_variable_target() {
    let mut state = CmvsPs2aVmState::new(0);
    // Mirrors the recovered boundary form: the nested stack expression
    // computes the float-table index, then `floatvar[index] = 1.0`.
    let frame = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aNumericExpressionNode::Prefix {
                span: SPAN,
                opcode: 0x12b,
                payload: None,
                index: None,
                operands: vec![crate::CmvsPs2aStackExpression {
                    span: SPAN,
                    nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
                        span: SPAN,
                        opcode: 0x100,
                        payload: 120,
                    }],
                }],
            },
            crate::CmvsPs2aNumericExpressionNode::Value {
                span: SPAN,
                opcode: 0x12a,
                payload: 1.0_f32.to_bits(),
            },
            crate::CmvsPs2aNumericExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    });
    execute_cmvs390_frame(&mut state, &frame).unwrap();
    assert_eq!(
        state.process_float_words.get(&120),
        Some(&1.0_f32.to_bits())
    );
    assert_eq!(state.current_value, Some(1.0_f32.to_bits()));
    assert!(state.condition_flag);
}

#[test]
fn blocks_numeric_assignment_targets_outside_the_recovered_subset() {
    let mut state = CmvsPs2aVmState::new(0);
    let frame = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aNumericExpressionNode::Value {
                span: SPAN,
                opcode: 0x12c,
                payload: 4,
            },
            crate::CmvsPs2aNumericExpressionNode::Value {
                span: SPAN,
                opcode: 0x12a,
                payload: 1.0_f32.to_bits(),
            },
            crate::CmvsPs2aNumericExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    });
    let error = execute_cmvs390_frame(&mut state, &frame).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_NUMERIC_EXPRESSION");
}

#[test]
fn assigns_and_reads_frame_local_words_through_the_active_call_base() {
    let mut state = CmvsPs2aVmState::new(0);
    let assign = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x108,
                payload: 8,
            },
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x100,
                payload: 7,
            },
            crate::CmvsPs2aStackExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    };
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(assign),
    )
    .unwrap();
    assert_eq!(&state.stack_bytes[8..12], &7_u32.to_le_bytes());
    assert_eq!(state.current_value, Some(7));
    // Script identity does not create an independent variable buffer;
    // only an intra-script call changes the recovered frame base.
    state.current_frame = 1;
    let read = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
            span: SPAN,
            opcode: 0x108,
            payload: 8,
        }],
    };
    execute_cmvs390_frame(&mut state, &CmvsPs2aInstructionFrame::StackExpression(read)).unwrap();
    assert_eq!(state.current_value, Some(7));
}

#[test]
fn tracks_wider_immediate_stack_slots_without_inventing_gap_values() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 8,
            value: 1,
        }),
    )
    .unwrap();
    assert_eq!(state.stack_bytes.len(), 8);
    assert_eq!(
        state.stack_initialized,
        vec![true, true, true, true, false, false, false, false]
    );
    let error = read_stack_word_from_top(&state, 4).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_STACK_UNINITIALIZED");
}

#[test]
fn truncating_the_stack_keeps_initialization_tracking_in_lockstep() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [1_u32, 2, 3] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    assert_eq!(state.stack_bytes.len(), 12);
    assert_eq!(state.stack_initialized.len(), 12);
    truncate_stack_to(&mut state, 4);
    assert_eq!(state.stack_bytes.len(), 4);
    assert_eq!(state.stack_initialized.len(), 4);
    assert_eq!(state.stack_cursor_bytes, 4);
    // The snapshot validation must hold after the per-frame queue drop.
    validate_cmvs390_vm_state(&state).unwrap();
}

#[test]
fn stores_command_30_word_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0x1234_5678,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 30,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&10712), Some(&0x1234_5678));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_751_reads_input_modes_without_inventing_gpu_readiness() {
    for (skip, auto, force, expected) in [
        (false, false, 0, 0),
        (true, false, 0, 1),
        (false, true, 0, 1),
        (false, false, 1, 1),
    ] {
        let mut state = CmvsPs2aVmState::new(0);
        state.input_skip_mode = skip;
        state.input_auto_mode = auto;
        state.interpreter_words.insert(1464, force);
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: 751,
            },
        )
        .unwrap();
        assert_eq!(state.interpreter_words.get(&81220), Some(&expected));
        assert_eq!(state.stack_cursor_bytes, 0);
    }
}

#[test]
fn command_51_blocks_on_an_unoccupied_script_slot() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 3,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 51,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
}

#[test]
fn command_51_reports_a_slot_with_a_decoded_texture() {
    let mut state = CmvsPs2aVmState::new(0);
    state
        .slot_objects
        .insert(slot_object_key(1924, 3).unwrap(), 1);
    state
        .texture_parents
        .insert(3, CmvsTextureParentState::default());
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 3,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 51,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_334_stores_a_changed_playback_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert((742_u32 << 8) | 2, 7);
    for flag in [1_u32, 1, 0] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value: flag,
            }),
        )
        .unwrap();
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value: 2,
            }),
        )
        .unwrap();
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: 334,
            },
        )
        .unwrap();
    }
    assert_eq!(state.interpreter_words.get(&(0x7420_0000 + 2)), Some(&0));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_334_blocks_on_an_unoccupied_channel() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 1,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 334,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x0001_0000));
}

#[test]
fn stores_command_29_word_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0xabcd_1234,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 29,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&1600), Some(&0xabcd_1234));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_command_144_constant_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 144,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&2940), Some(&1));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_138_resumes_the_registered_coroutine_record() {
    const RECORD_BASE: u32 = 13272 + 28 * 5;
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 2;
    state.program_counter = 0x100;
    for value in [9_u32, 8, 7, 0x4321, 5] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 136,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&RECORD_BASE), Some(&0));
    assert_eq!(state.interpreter_words.get(&(RECORD_BASE + 4)), Some(&2));
    assert_eq!(
        state.interpreter_words.get(&(RECORD_BASE + 8)),
        Some(&0x4321)
    );
    assert_eq!(state.interpreter_words.get(&(RECORD_BASE + 12)), Some(&7));
    assert_eq!(state.interpreter_words.get(&(RECORD_BASE + 16)), Some(&8));
    assert_eq!(state.interpreter_words.get(&(RECORD_BASE + 20)), Some(&9));
    assert_eq!(state.stack_cursor_bytes, 0);

    // Resume from a different frame with an outstanding frame counter;
    // the label is masked like the original `& 0x3F`.
    state.current_frame = 0;
    state
        .interpreter_words
        .insert(SCRIPT_FRAME_COUNTER_FIELD, 0x77);
    // The label is masked like the original `& 0x3F`: 0x145 selects the
    // same record as 5.
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0x145,
        }),
    )
    .unwrap();
    state.program_counter = 0x200;
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 138,
        },
    )
    .unwrap();
    assert_eq!(state.program_counter, 0x4321);
    assert_eq!(state.current_frame, 2);
    assert_eq!(
        state.interpreter_words.get(&SCRIPT_FRAME_COUNTER_FIELD),
        Some(&0)
    );
    assert_eq!(&state.stack_bytes[0..4], &0x77_u32.to_le_bytes());
    assert_eq!(&state.stack_bytes[4..8], &0x202_u32.to_le_bytes());
    assert_eq!(&state.stack_bytes[8..12], &0_u32.to_le_bytes());
    assert_eq!(state.stack_cursor_bytes, 12);
}

#[test]
fn stack_expression_reads_and_writes_the_loaded_data_segment() {
    let mut state = CmvsPs2aVmState::new(0);
    state.script_data_segment_sizes.insert(0, 12);
    let assign = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x106,
                payload: 4,
            },
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x100,
                payload: 0x55,
            },
            crate::CmvsPs2aStackExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    };
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(assign),
    )
    .unwrap();
    assert_eq!(
        state
            .script_data_segment_words
            .get(&0)
            .and_then(|words| words.get(&4)),
        Some(&0x55)
    );
    // Unwritten words read zero through the same tag family.
    let read = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
            span: SPAN,
            opcode: 0x104,
            payload: 8,
        }],
    };
    execute_cmvs390_frame(&mut state, &CmvsPs2aInstructionFrame::StackExpression(read)).unwrap();
    assert_eq!(state.current_value, Some(0));
    // An offset whose dword end exceeds the declared length blocks.
    let out_of_bounds = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
            span: SPAN,
            opcode: 0x107,
            payload: 12,
        }],
    };
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(out_of_bounds.clone()),
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_DATA_SEGMENT");
    // A frame without a declared segment blocks as well.
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 3;
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(out_of_bounds.clone()),
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_DATA_SEGMENT");
}

#[test]
fn reload_root_script_invalidates_frame_zero_coroutine_records() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [9_u32, 8, 7, 0x4321, 5] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 136,
        },
    )
    .unwrap();
    // Reload: the root-frame record loses its registered PC, so the
    // resume keeps blocking like the original unregistered jump.
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 128,
        },
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 138,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_COROUTINE");
}

#[test]
fn body_update_commands_write_their_result_registers() {
    let mut state = CmvsPs2aVmState::new(0);
    let run = |state: &mut CmvsPs2aVmState, id: u16| {
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    };
    let push = |state: &mut CmvsPs2aVmState, values: &[u32]| {
        for value in values {
            execute_cmvs390_frame(
                state,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
    };
    // 718 copies the pinned word; 750 canonicalizes it.
    state.interpreter_words.insert(11792, 0xabc);
    run(&mut state, 718);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0xabc));
    state.interpreter_words.insert(1596, 5);
    run(&mut state, 750);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // 741 flag round trip: store then read back.
    push(&mut state, &[1, 1]);
    run(&mut state, 741);
    assert_eq!(state.interpreter_words.get(&1500), Some(&1));
    push(&mut state, &[0, 0]);
    run(&mut state, 741);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // 846/304 publish canonical zeros; 426 clears the latch pair.
    state.interpreter_words.insert(81220, 9);
    run(&mut state, 846);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    state.interpreter_words.insert(81220, 9);
    run(&mut state, 304);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    state.interpreter_words.insert(81236, 7);
    run(&mut state, 426);
    assert_eq!(state.interpreter_words.get(&81236), Some(&0));
    // 397 needs the effect channel object; an occupied slot reports
    // canonical zero quad fields.
    state.slot_objects.insert((742_u32 << 8) | 7, 1);
    push(&mut state, &[0xffff_ffff, 7]);
    run(&mut state, 397);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    assert_eq!(state.interpreter_words.get(&81252), Some(&0));
}

#[test]
fn effect_quad_visibility_drives_the_case_405_gate() {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert((742_u32 << 8) | 7, 1);
    let run = |state: &mut CmvsPs2aVmState, id: u16| {
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    };
    let push = |state: &mut CmvsPs2aVmState, values: &[u32]| {
        for value in values {
            execute_cmvs390_frame(
                state,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
    };
    let quad_active = |state: &mut CmvsPs2aVmState| {
        state.interpreter_words.insert(81220, 9);
        push(state, &[0, 7]);
        run(state, 405);
        state.interpreter_words.get(&81220).copied()
    };

    // A never-shown quad reads as hidden: case 405 stores 1.
    assert_eq!(quad_active(&mut state), Some(1));
    // Case 402 shows quad 0, so case 405 stores 0.
    push(&mut state, &[0, 7]);
    run(&mut state, 402);
    assert_eq!(quad_active(&mut state), Some(0));
    // Case 403 hides it again, so case 405 stores 1.
    push(&mut state, &[0, 7]);
    run(&mut state, 403);
    assert_eq!(quad_active(&mut state), Some(1));
}

#[test]
fn scene_layer_queries_publish_boolean_pairs_and_clears_leave_the_middle_word() {
    let mut state = CmvsPs2aVmState::new(0);
    let run = |state: &mut CmvsPs2aVmState, id: u16| {
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    };

    // Case 426 publishes `(scene[298] != 0, scene[299] != 0)`.
    state.scene_words.insert(298, 7);
    run(&mut state, 426);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    assert_eq!(state.interpreter_words.get(&81236), Some(&0));
    state.scene_words.insert(299, 3);
    run(&mut state, 426);
    assert_eq!(state.interpreter_words.get(&81236), Some(&1));

    // Case 427 clears base 298 and base + 2 (300), keeping 299.
    state.scene_words.insert(300, 9);
    run(&mut state, 427);
    assert_eq!(state.scene_words.get(&298), None);
    assert_eq!(state.scene_words.get(&300), None);
    assert_eq!(state.scene_words.get(&299), Some(&3));

    // Case 424 reads the separate group at base 289.
    state.scene_words.insert(289, 1);
    run(&mut state, 424);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));

    // Case 432 reads the last group at base 307; case 433 clears 307 and 309.
    state.scene_words.insert(308, 5);
    run(&mut state, 432);
    assert_eq!(state.interpreter_words.get(&81236), Some(&1));
    run(&mut state, 433);
    assert_eq!(state.scene_words.get(&308), Some(&5));
}

#[test]
fn effect_channel_quad_geometry_and_rect_use_the_recovered_word_offsets() {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert((742_u32 << 8) | 7, 1);
    let run = |state: &mut CmvsPs2aVmState, id: u16| {
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    };
    let push = |state: &mut CmvsPs2aVmState, values: &[u32]| {
        for value in values {
            execute_cmvs390_frame(
                state,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
    };

    // Case 400 -> `sub_466CB0`: the channel is the top word, then quad, sub
    // selector and six `__int16` values stored at word `38*quad + 6*sub + 22`.
    push(&mut state, &[60, 50, 40, 30, 20, 10, 0, 0, 7]);
    run(&mut state, 400);
    let channel = state.effect_channels.get(&7).expect("channel created");
    assert_eq!(channel.words.get(&22).copied(), Some(10));
    assert_eq!(channel.words.get(&27).copied(), Some(60));

    // Case 401 -> `sub_466D10`: four `__int16` hit-rectangle words at
    // `38*quad + 52`.
    push(&mut state, &[4, 3, 2, 1, 0, 7]);
    run(&mut state, 401);
    assert_eq!(
        state.effect_channels.get(&7).unwrap().quad_rect(0),
        [1, 2, 3, 4]
    );

    // Case 406 -> `sub_47F330`: the animation frame word at `38*quad + 38`.
    push(&mut state, &[3, 0, 7]);
    run(&mut state, 406);
    assert_eq!(state.effect_channels.get(&7).unwrap().quad_frame(0), 3);
}

#[test]
fn pointer_commands_read_the_streamed_position_and_hit_test_shapes() {
    let mut state = CmvsPs2aVmState::new(0);
    state.pointer_x = 100;
    state.pointer_y = 50;
    // Case 200 publishes the pointer into system registers 1/2.
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 200,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81224), Some(&100));
    assert_eq!(state.interpreter_words.get(&81228), Some(&50));

    let push = |state: &mut CmvsPs2aVmState, values: &[u32]| {
        for value in values {
            execute_cmvs390_frame(
                state,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
    };
    // Case 202: inclusive rectangle (x=10, y=10, w=200, h=100).
    push(&mut state, &[100, 200, 10, 10]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 202,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // A point strictly outside the same rectangle misses.
    state.pointer_x = 211;
    push(&mut state, &[100, 200, 10, 10]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 202,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));

    // Case 203 type 1: half-open rectangle (10,10)-(210,110).
    state.pointer_x = 100;
    push(&mut state, &[0, 0, 110, 210, 10, 10, 1]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // The right edge is exclusive for a GDI rectangle region.
    state.pointer_x = 210;
    push(&mut state, &[0, 0, 110, 210, 10, 10, 1]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));

    // The curved and polygon shapes stay blocking.
    state.pointer_x = 100;
    push(&mut state, &[0, 0, 110, 210, 10, 10, 2]);
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_POINTER_REGION");
}

#[test]
fn command_138_blocks_on_a_missing_or_invalidated_coroutine_record() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [5_u32, 0x4321, 7, 8, 9] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    // No record was ever registered for label 5.
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    let successful_snapshot = state.clone();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 138,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_COROUTINE");

    // Case 139 invalidates the record PC; the resume must keep blocking.
    validate_cmvs390_vm_state(&successful_snapshot).unwrap();
    let mut state = successful_snapshot;
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 136,
        },
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 139,
        },
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 138,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_COROUTINE");
}

#[test]
fn stores_command_31_process_global_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0x89ab_cdef,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 31,
        },
    )
    .unwrap();
    assert_eq!(
        state.process_global_words.get(&0x004f_0578),
        Some(&0x89ab_cdef)
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_command_940_as_a_canonical_process_global_boolean() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 7,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 940,
        },
    )
    .unwrap();
    assert_eq!(state.process_global_words.get(&0x004f_0580), Some(&1));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn mutates_the_bounded_process_flag_range_from_command_145() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [5, 3, 1] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 145,
        },
    )
    .unwrap();
    assert_eq!(state.process_flag_bits, BTreeSet::from([1, 2, 3]));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_the_bounded_process_indexed_range_from_command_146() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [0xfeed_beef, 3, 5] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 146,
        },
    )
    .unwrap();
    for index in 5..8 {
        assert_eq!(state.process_indexed_words.get(&index), Some(&0xfeed_beef));
    }
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_raw_float_bits_from_command_147() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [1.25_f32.to_bits(), 2, 7] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 147,
        },
    )
    .unwrap();
    assert_eq!(state.process_float_words.get(&7), Some(&1.25_f32.to_bits()));
    assert_eq!(state.process_float_words.get(&8), Some(&1.25_f32.to_bits()));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_private_string_references_from_command_148() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [11, 2, 3] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 148,
        },
    )
    .unwrap();
    let reference = CmvsPs2aPrivateStringReference {
        relative_offset: 11,
    };
    assert_eq!(state.process_string_slots.get(&3), Some(&reference));
    assert_eq!(state.process_string_slots.get(&4), Some(&reference));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_command_21_prefixed_string_segments_without_payload() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 72,
        }),
    )
    .unwrap();
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 21,
        },
    )
    .unwrap();
    assert_eq!(action, None);
    assert_eq!(
        state.interpreter_string_buffers.get(&3356),
        Some(&vec![
            CmvsStringSegment::InterpreterPrefixField {
                field_offset: 10764
            },
            CmvsStringSegment::PrivateString(CmvsPs2aPrivateStringReference {
                relative_offset: 72,
            }),
        ])
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_22_emits_a_directory_request_for_its_path_buffer() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 88);
    let action = run_command_with_action(&mut state, 22);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::EnsurePathDirectories {
            buffer_offset: 5404
        })
    );
    assert_eq!(
        state.interpreter_string_buffers.get(&5404),
        Some(&vec![
            CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 },
            CmvsStringSegment::PrivateString(CmvsPs2aPrivateStringReference {
                relative_offset: 88,
            }),
        ])
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn blocks_command_21_tagged_string_outside_tag_zero() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0x8000_0010,
        }),
    )
    .unwrap();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 21,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_STRING_TAG");
    assert!(state.interpreter_string_buffers.is_empty());
    assert_eq!(state.stack_bytes.len(), 4);
}

fn push(state: &mut CmvsPs2aVmState, value: u32) {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value,
        }),
    )
    .unwrap();
}

fn run_command(state: &mut CmvsPs2aVmState, command_id: u16) {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id,
        },
    )
    .unwrap();
}

fn run_command_with_action(
    state: &mut CmvsPs2aVmState,
    command_id: u16,
) -> Option<CmvsPs2aVmAction> {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id,
        },
    )
    .unwrap()
}

#[test]
fn command_32_creates_and_replaces_slot_objects_with_stable_identity() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    run_command(&mut state, 32);
    let key = slot_object_key(1924, 7).unwrap();
    assert_eq!(state.slot_objects.get(&key), Some(&1));
    assert_eq!(
        state.texture_parents.get(&7),
        Some(&CmvsTextureParentState::default())
    );
    assert_eq!(state.texture_children.get(&7), Some(&BTreeMap::new()));
    push(&mut state, 7);
    run_command(&mut state, 32);
    assert_eq!(state.slot_objects.get(&key), Some(&2));
    assert_eq!(state.texture_children.get(&7), Some(&BTreeMap::new()));
    assert_eq!(state.stack_cursor_bytes, 0);
    assert!(state.interpreter_flag_words.is_empty());
}

#[test]
fn commands_48_and_64_bind_and_initialize_a_live_texture_parent() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    run_command(&mut state, 32);

    let resource = CmvsPs2aPrivateStringReference {
        relative_offset: 23,
    };
    // Handler stack order is resource below the topmost parent id.
    push(&mut state, resource.relative_offset);
    push(&mut state, 7);
    assert_eq!(
        run_command_with_action(&mut state, 48),
        Some(CmvsPs2aVmAction::LoadTextureParentResource {
            parent_slot: 7,
            resource,
        })
    );
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    assert_eq!(
        state.texture_parents.get(&7).unwrap().resource,
        Some(resource)
    );

    push(&mut state, 7);
    run_command(&mut state, 64);
    assert!(state.texture_parents.get(&7).unwrap().surface_initialized);
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_32_out_of_bounds_slot_only_raises_the_error_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 256);
    run_command(&mut state, 32);
    assert!(state.slot_objects.is_empty());
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_33_destroys_only_the_requested_slot_object() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 3);
    run_command(&mut state, 32);
    push(&mut state, 4);
    run_command(&mut state, 32);
    push(&mut state, 3);
    run_command(&mut state, 33);
    assert_eq!(state.slot_objects.len(), 1);
    assert_eq!(
        state.slot_objects.get(&slot_object_key(1924, 4).unwrap()),
        Some(&2)
    );
    assert!(!state.texture_children.contains_key(&3));
    assert!(state.texture_children.contains_key(&4));
}

#[test]
fn command_40_moves_the_source_object_and_clears_both_source_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    run_command(&mut state, 32);
    // Stack order: source pushed first, destination on top.
    push(&mut state, 5);
    push(&mut state, 9);
    run_command(&mut state, 40);
    assert_eq!(
        state.slot_objects.get(&slot_object_key(1924, 9).unwrap()),
        Some(&1)
    );
    assert!(!state
        .slot_objects
        .contains_key(&slot_object_key(1924, 5).unwrap()));
    assert!(!state.texture_children.contains_key(&5));
    assert!(state.texture_children.contains_key(&9));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn texture_child_commands_retain_the_recovered_hierarchy_and_fields() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    run_command(&mut state, 32);

    // case 34: child first, parent on top.
    push(&mut state, 11);
    push(&mut state, 7);
    run_command(&mut state, 34);
    assert_eq!(
        state
            .texture_children
            .get(&7)
            .and_then(|children| children.get(&11)),
        Some(&CmvsTextureChildState::default())
    );

    // case 56: resource, child, parent.
    push(&mut state, 41);
    push(&mut state, 11);
    push(&mut state, 7);
    assert_eq!(
        run_command_with_action(&mut state, 56),
        Some(CmvsPs2aVmAction::LoadTextureResource {
            parent_slot: 7,
            child_id: 11,
            resource: CmvsPs2aPrivateStringReference {
                relative_offset: 41,
            },
        })
    );

    push(&mut state, 11);
    push(&mut state, 7);
    run_command(&mut state, 80);

    // case 68: four fields, child, parent.
    for value in [104, 103, 102, 101, 11, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 68);
    // case 70: two fields, child, parent.
    for value in [202, 201, 11, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 70);
    // case 69: auxiliary pair, child, parent.
    for value in [302, 301, 11, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 69);
    // case 71: auxiliary word, child, parent.
    for value in [401, 11, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 71);

    let child = &state.texture_children[&7][&11];
    assert_eq!(
        child.resource,
        Some(CmvsPs2aPrivateStringReference {
            relative_offset: 41,
        })
    );
    assert!(child.surface_initialized);
    assert_eq!(child.rect_words, Some([101, 102, 103, 104]));
    assert_eq!(child.position_words, Some([201, 202]));
    assert_eq!(child.auxiliary_pair_words, Some([301, 302]));
    assert_eq!(child.auxiliary_word, Some(401));
}

#[test]
fn negative_texture_selector_updates_the_parent_surface_without_error() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    run_command(&mut state, 32);
    push(&mut state, 7);
    run_command(&mut state, 64);

    for value in [104, 103, 102, 101, u32::MAX, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 68);
    for value in [202, 201, u32::MAX, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 70);
    for value in [302, 301, u32::MAX, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 69);
    for value in [401, u32::MAX, 7] {
        push(&mut state, value);
    }
    run_command(&mut state, 71);

    let parent = &state.texture_parents[&7];
    assert_eq!(parent.rect_words, Some([101, 102, 103, 104]));
    assert_eq!(parent.position_words, Some([201, 202]));
    assert_eq!(parent.auxiliary_pair_words, Some([301, 302]));
    assert_eq!(parent.auxiliary_word, Some(401));
    assert!(state.interpreter_flag_words.is_empty());
}

#[test]
fn texture_child_bounds_follow_parent_error_and_child_noop_contracts() {
    let mut missing_parent = CmvsPs2aVmState::new(0);
    push(&mut missing_parent, 4);
    push(&mut missing_parent, 2);
    run_command(&mut missing_parent, 34);
    assert_eq!(
        missing_parent.interpreter_flag_words.get(&10532),
        Some(&0x10)
    );

    let mut out_of_range_child = CmvsPs2aVmState::new(0);
    push(&mut out_of_range_child, 2);
    run_command(&mut out_of_range_child, 32);
    push(&mut out_of_range_child, 1024);
    push(&mut out_of_range_child, 2);
    run_command(&mut out_of_range_child, 34);
    assert!(out_of_range_child.texture_children[&2].is_empty());
    assert!(out_of_range_child.interpreter_flag_words.is_empty());
}

#[test]
fn command_47_clears_all_top_level_texture_containers_and_children() {
    let mut state = CmvsPs2aVmState::new(0);
    for parent in [2, 7] {
        push(&mut state, parent);
        run_command(&mut state, 32);
        push(&mut state, 11);
        push(&mut state, parent);
        run_command(&mut state, 34);
    }
    run_command(&mut state, 47);
    assert!(state.texture_children.is_empty());
    assert!(!state.slot_objects.keys().any(|key| (*key >> 16) == 1924));
}

#[test]
fn command_143_emits_a_window_caption_request_with_prefix_segments() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 40);
    let action = run_command_with_action(&mut state, 143);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::SetWindowCaption {
            segments: vec![
                CmvsStringSegment::InterpreterPrefixField {
                    field_offset: 10764
                },
                CmvsStringSegment::PrivateString(CmvsPs2aPrivateStringReference {
                    relative_offset: 40,
                }),
            ],
        })
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_352_emits_a_raw_caption_request_without_prefix() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 55);
    let action = run_command_with_action(&mut state, 352);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::SetWindowCaption {
            segments: vec![CmvsStringSegment::PrivateString(
                CmvsPs2aPrivateStringReference {
                    relative_offset: 55,
                }
            )],
        })
    );
}

#[test]
fn command_715_stores_the_component_word_from_the_stack() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 0x2026);
    run_command(&mut state, 715);
    assert_eq!(
        state.component_words.get(&component_field_key(1920, 76)),
        Some(&0x2026)
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_848_resets_the_settings_object_before_storing() {
    let mut state = CmvsPs2aVmState::new(0);
    state.settings_words.insert(48, 999);
    push(&mut state, 1);
    run_command(&mut state, 848);
    assert_eq!(state.settings_words.get(&0), Some(&1));
    assert_eq!(state.settings_words.get(&48), Some(&0));
    assert_eq!(state.settings_words.get(&44), Some(&u32::MAX));
    assert_eq!(state.settings_words.get(&172), Some(&u32::MAX));
}

#[test]
fn command_849_stores_both_booleans_after_the_reset() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    push(&mut state, 0);
    run_command(&mut state, 849);
    assert_eq!(state.settings_words.get(&24), Some(&0));
    assert_eq!(state.settings_words.get(&28), Some(&1));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_858_and_859_respect_their_recovered_bounds() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 91);
    run_command(&mut state, 858);
    assert!(!state.settings_words.contains_key(&36));
    push(&mut state, 90);
    run_command(&mut state, 858);
    assert_eq!(state.settings_words.get(&36), Some(&90));
    push(&mut state, u32::MAX); // -1 signed
    run_command(&mut state, 859);
    assert!(!state.settings_words.contains_key(&32));
    push(&mut state, 3);
    run_command(&mut state, 859);
    assert_eq!(state.settings_words.get(&32), Some(&3));
}

#[test]
fn command_136_records_the_fixed_session_clock() {
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 3;
    state.session_clock_millis = 1_234;
    push(&mut state, 50);
    push(&mut state, 40);
    push(&mut state, 30);
    push(&mut state, 20);
    push(&mut state, 2);
    run_command(&mut state, 136);
    let base = 13_272 + 28 * 2;
    assert_eq!(state.interpreter_words.get(&base), Some(&0));
    assert_eq!(state.interpreter_words.get(&(base + 4)), Some(&3));
    assert_eq!(state.interpreter_words.get(&(base + 8)), Some(&20));
    assert_eq!(state.interpreter_words.get(&(base + 12)), Some(&30));
    assert_eq!(state.interpreter_words.get(&(base + 16)), Some(&40));
    assert_eq!(state.interpreter_words.get(&(base + 20)), Some(&50));
    assert_eq!(state.interpreter_words.get(&(base + 24)), Some(&1_234));
}

#[test]
fn command_869_copies_the_complete_settings_snapshot() {
    let mut state = CmvsPs2aVmState::new(0);
    for (offset, value) in [(100, 1), (104, 2), (124, 3), (128, 4), (176, 5)] {
        state.settings_words.insert(offset, value);
    }
    state.settings_words.insert(120, 1);
    run_command(&mut state, 869);
    assert_eq!(state.interpreter_words.get(&81236), Some(&1));
    assert_eq!(state.interpreter_words.get(&81240), Some(&2));
    assert_eq!(state.interpreter_words.get(&81224), Some(&3));
    assert_eq!(state.interpreter_words.get(&81228), Some(&4));
    assert_eq!(state.interpreter_words.get(&81244), Some(&5));
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));

    state.settings_words.insert(120, 0);
    state.settings_words.insert(176, 9);
    state.interpreter_words.insert(81236, 77);
    run_command(&mut state, 869);
    assert_eq!(state.interpreter_words.get(&81236), Some(&77));
    assert_eq!(state.interpreter_words.get(&81244), Some(&9));
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
}

#[test]
fn commands_528_530_and_531_manage_bounded_filter_chain_records() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    assert_eq!(state.filter_chain_channels.get(&2), Some(&7));

    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 530);
    assert_eq!(
        state
            .filter_chain_records
            .get(&2)
            .and_then(|records| records.get(&44))
            .map(|record| record.words),
        Some([0; 4])
    );
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));

    push(&mut state, 4);
    push(&mut state, 3);
    push(&mut state, 2);
    push(&mut state, 1);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 531);
    assert_eq!(
        state
            .filter_chain_records
            .get(&2)
            .and_then(|records| records.get(&44))
            .map(|record| record.words),
        Some([1, 2, 3, 4])
    );

    push(&mut state, 8);
    push(&mut state, 7);
    push(&mut state, 6);
    push(&mut state, 5);
    push(&mut state, 99);
    push(&mut state, 2);
    run_command(&mut state, 531);
    assert!(!state
        .filter_chain_records
        .get(&2)
        .is_some_and(|records| records.contains_key(&99)));

    push(&mut state, 4);
    push(&mut state, 3);
    push(&mut state, 2);
    push(&mut state, 1);
    push(&mut state, 44);
    push(&mut state, 6);
    run_command(&mut state, 531);
    assert_eq!(state.interpreter_words.get(&10532), Some(&0x100000));
}

#[test]
fn command_529_destroys_the_complete_filter_chain_bank() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 530);
    state.filter_chain_active_records.insert(2, 44);
    state.filter_chain_apply_revisions.insert(2, 3);
    state.filter_chain_selection_await = Some(2);

    push(&mut state, 2);
    run_command(&mut state, 529);
    assert!(!state
        .slot_objects
        .contains_key(&slot_object_key(3196, 2).unwrap()));
    assert!(!state.filter_chain_records.contains_key(&2));
    assert!(!state.filter_chain_record_order.contains_key(&2));
    assert!(!state.filter_chain_active_records.contains_key(&2));
    assert!(!state.filter_chain_channels.contains_key(&2));
    assert!(!state.filter_chain_apply_revisions.contains_key(&2));
    assert_eq!(state.filter_chain_selection_await, None);

    push(&mut state, 6);
    run_command(&mut state, 529);
    assert_eq!(state.interpreter_words.get(&10532), Some(&0x100000));
}

#[test]
fn command_532_uses_the_recovered_backend_parameter_layout() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 530);

    for value in [6, 5, 4, 3, 2, 1, 99, 1, 44, 2] {
        push(&mut state, value);
    }
    run_command(&mut state, 532);
    let block = CmvsFilterChainParameterBlock {
        word: 99,
        short_words: [1, 2, 3, 4, 5, 6],
    };
    let record = state
        .filter_chain_records
        .get(&2)
        .and_then(|records| records.get(&44))
        .unwrap();
    assert_eq!(record.parameter_blocks.get(&1), Some(&block));
    assert!(!record.parameter_blocks.contains_key(&2));

    state.process_global_words.insert(0x004f_057c, 1);
    for value in [16, 15, 14, 13, 12, 11, 199, 1, 44, 2] {
        push(&mut state, value);
    }
    run_command(&mut state, 532);
    let mirrored = CmvsFilterChainParameterBlock {
        word: 199,
        short_words: [11, 12, 13, 14, 15, 16],
    };
    let record = state
        .filter_chain_records
        .get(&2)
        .and_then(|records| records.get(&44))
        .unwrap();
    assert_eq!(record.parameter_blocks.get(&1), Some(&mirrored));
    assert_eq!(record.parameter_blocks.get(&2), Some(&mirrored));
}

#[test]
fn command_533_applies_only_a_live_filter_chain_bank() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);

    push(&mut state, 2);
    run_command(&mut state, 533);
    push(&mut state, 2);
    run_command(&mut state, 533);
    assert_eq!(state.filter_chain_apply_revisions.get(&2), Some(&2));

    push(&mut state, 6);
    run_command(&mut state, 533);
    assert_eq!(state.interpreter_words.get(&10532), Some(&0x100000));
    assert!(!state.filter_chain_apply_revisions.contains_key(&6));
}

#[test]
fn command_535_waits_on_a_live_chain_and_preserves_insertion_order() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    for record in [44, 11, 29] {
        push(&mut state, record);
        push(&mut state, 2);
        run_command(&mut state, 530);
    }
    push(&mut state, 11);
    push(&mut state, 2);
    run_command(&mut state, 530);
    assert_eq!(
        state.filter_chain_record_order.get(&2),
        Some(&vec![44, 11, 29])
    );

    push(&mut state, 2);
    assert_eq!(
        run_command_with_action(&mut state, 535),
        Some(CmvsPs2aVmAction::FilterChainSelectionWait { bank: 2 })
    );
    assert_eq!(state.filter_chain_selection_await, Some(2));
    assert_eq!(state.interpreter_words.get(&81220), Some(&u32::MAX));

    state.filter_chain_selection_await = None;
    state.filter_chain_active_records.insert(2, 11);
    push(&mut state, 2);
    run_command(&mut state, 534);
    assert_eq!(state.interpreter_words.get(&81220), Some(&11));
    state.filter_chain_active_records.remove(&2);
    push(&mut state, 2);
    run_command(&mut state, 534);
    assert_eq!(state.interpreter_words.get(&81220), Some(&u32::MAX));

    let mut missing = CmvsPs2aVmState::new(0);
    push(&mut missing, 6);
    assert_eq!(run_command_with_action(&mut missing, 535), None);
    assert_eq!(missing.interpreter_words.get(&10532), Some(&0x100000));
}

#[test]
fn command_548_waits_for_host_input_when_effects_are_enabled() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [40, 30, 20, 1] {
        push(&mut state, value);
    }
    run_command(&mut state, 544);
    // `sub_47A4F0` publishes the owner handle at the slot-table word.
    assert!(state.interpreter_words.get(&3252).copied().unwrap_or(0) != 0);
    push(&mut state, 1);
    assert_eq!(
        run_command_with_action(&mut state, 548),
        Some(CmvsPs2aVmAction::FilterGraphInputWait)
    );
    assert!(state.filter_graph_input_await);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
}

#[test]
fn command_548_zero_control_still_waits_like_the_original_dispatcher() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [40, 30, 20, 1] {
        push(&mut state, value);
    }
    run_command(&mut state, 544);
    push(&mut state, 0);
    assert_eq!(
        run_command_with_action(&mut state, 548),
        Some(CmvsPs2aVmAction::FilterGraphInputWait)
    );
    assert!(state.filter_graph_input_await);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
}

#[test]
fn command_548_uses_the_recovered_skip_flag_without_waiting() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [40, 30, 20, 1] {
        push(&mut state, value);
    }
    run_command(&mut state, 544);
    state.interpreter_words.insert(1464, 1);
    push(&mut state, 1);
    assert_eq!(run_command_with_action(&mut state, 548), None);
    assert!(!state.filter_graph_input_await);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // The resolve path destroys the owner through `sub_47A660`.
    assert!(!state.slot_objects.contains_key(&(3252_u32 << 8)));
    assert_eq!(state.interpreter_words.get(&3252), Some(&0));
}

#[test]
fn command_548_without_an_owner_handle_pops_and_continues() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 1);
    assert_eq!(run_command_with_action(&mut state, 548), None);
    assert!(!state.filter_graph_input_await);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
}

#[test]
fn command_692_emits_the_presentation_mode_toggle_request() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 1);
    let action = run_command_with_action(&mut state, 692);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::TogglePresentationMode {
            first_field_offset: 1440,
            second_field_offset: 15064,
            enabled: true,
        })
    );
}

fn stack_expression(nodes: Vec<crate::CmvsPs2aStackExpressionNode>) -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::StackExpression(crate::CmvsPs2aStackExpression { span: SPAN, nodes })
}

fn literal_node(payload: u32) -> crate::CmvsPs2aStackExpressionNode {
    crate::CmvsPs2aStackExpressionNode::Value {
        span: SPAN,
        opcode: 0x100,
        payload,
    }
}

#[test]
fn stack_expression_literal_sets_value_result_slot_and_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(&mut state, &stack_expression(vec![literal_node(9)])).unwrap();
    assert_eq!(state.current_value, Some(9));
    assert!(state.condition_flag);
    assert_eq!(state.process_indexed_words.get(&4096), Some(&9));
}

#[test]
fn stack_expression_assignment_writes_the_process_indexed_table() {
    let mut state = CmvsPs2aVmState::new(0);
    // [0x101, 0x100, 0x100, 0x170]: process_var[5] := 77
    let expression = stack_expression(vec![
        crate::CmvsPs2aStackExpressionNode::Prefix {
            span: SPAN,
            opcode: 0x101,
            payload: None,
            index: None,
            operands: vec![crate::CmvsPs2aStackExpression {
                span: SPAN,
                nodes: vec![literal_node(5)],
            }],
        },
        literal_node(77),
        crate::CmvsPs2aStackExpressionNode::Operator {
            span: SPAN,
            opcode: 0x170,
        },
    ]);
    execute_cmvs390_frame(&mut state, &expression).unwrap();
    assert_eq!(state.process_indexed_words.get(&5), Some(&77));
    assert_eq!(state.current_value, Some(77));
    assert_eq!(state.process_indexed_words.get(&4096), Some(&77));
    assert!(state.condition_flag);
}

#[test]
fn stack_expression_assignment_to_system_registers_matches_original_bounds() {
    // Tag 0x10f targets write interpreter system registers 0..=6 and
    // silently ignore higher indices, matching the original store.
    let mut state = CmvsPs2aVmState::new(0);
    store_stack_expression_target(
        &mut state,
        super::StackExpressionEntry {
            tag: 0x10f,
            value: 2,
        },
        33,
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81228), Some(&33));
    store_stack_expression_target(
        &mut state,
        super::StackExpressionEntry {
            tag: 0x10f,
            value: 9,
        },
        44,
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81256), None);
}

#[test]
fn stack_expression_unrecovered_operator_stays_fail_closed() {
    let mut state = CmvsPs2aVmState::new(0);
    let expression = stack_expression(vec![
        literal_node(1),
        literal_node(2),
        crate::CmvsPs2aStackExpressionNode::Operator {
            span: SPAN,
            opcode: 0x173,
        },
    ]);
    let error = execute_cmvs390_frame(&mut state, &expression).unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION");
    assert!(state.current_value.is_none());
    assert!(state.process_indexed_words.is_empty());
}

#[test]
fn command_296_blocks_dispatch_behind_a_storage_request() {
    let mut state = CmvsPs2aVmState::new(0);
    let action = run_command_with_action(&mut state, 296);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::StorageRequest(
            CmvsPs2aStorageRequest::LoadSystemState {
                path_buffer_offset: 5404
            }
        ))
    );
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 1,
        }),
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_STORAGE_AWAIT");
}

#[test]
fn stack_expression_binary_operators_follow_the_original_order() {
    // Left operand is pushed first, right operand second (on top); the
    // evaluator computes LEFT op RIGHT.
    fn eval_binary(opcode: u16, left: u32, right: u32) -> CmvsPs2aVmState {
        let mut state = CmvsPs2aVmState::new(0);
        let expression = crate::CmvsPs2aStackExpression {
            span: SPAN,
            nodes: vec![
                crate::CmvsPs2aStackExpressionNode::Value {
                    span: SPAN,
                    opcode: 0x100,
                    payload: left,
                },
                crate::CmvsPs2aStackExpressionNode::Value {
                    span: SPAN,
                    opcode: 0x100,
                    payload: right,
                },
                crate::CmvsPs2aStackExpressionNode::Operator { span: SPAN, opcode },
            ],
        };
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::StackExpression(expression),
        )
        .unwrap();
        state
    }
    // 0x16a: signed greater-than, left vs right.
    assert_eq!(eval_binary(0x16a, 7, 3).current_value, Some(1));
    assert_eq!(eval_binary(0x16a, 3, 7).current_value, Some(0));
    assert_eq!(eval_binary(0x16a, 3, 3).current_value, Some(0));
    // 0x164: left - right.
    assert_eq!(eval_binary(0x164, 9, 4).current_value, Some(5));
    assert_eq!(
        eval_binary(0x164, 4, 9).current_value,
        Some(u32::from_ne_bytes((-5_i32).to_ne_bytes()))
    );
    // 0x161: division with the guarded zero-divisor path.
    assert_eq!(eval_binary(0x161, 8, 2).current_value, Some(4));
    assert_eq!(eval_binary(0x161, 8, 0).current_value, Some(0));
    // 0x168: left shift masked to five bits.
    assert_eq!(eval_binary(0x168, 1, 4).current_value, Some(16));
    assert_eq!(eval_binary(0x168, 1, 36).current_value, Some(16));
    // 0x171 / 0x172 equality pair.
    assert_eq!(eval_binary(0x171, 5, 5).current_value, Some(1));
    assert_eq!(eval_binary(0x172, 5, 6).current_value, Some(1));
    // The terminator stores the result in process index 4096.
    let state = eval_binary(0x163, 2, 3);
    assert_eq!(state.process_indexed_words.get(&4096), Some(&5));
    assert!(state.condition_flag);
}

#[test]
fn stack_expression_flag_reads_and_writes_use_the_process_bitmap() {
    // Prefix 0x102 pushes the flag index; coercion reads the bitmap.
    let mut state = CmvsPs2aVmState::new(0);
    state.process_flag_bits.insert(41);
    let expression = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Prefix {
            span: SPAN,
            opcode: 0x102,
            payload: None,
            index: None,
            operands: vec![crate::CmvsPs2aStackExpression {
                span: SPAN,
                nodes: vec![literal_node(41)],
            }],
        }],
    };
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(expression),
    )
    .unwrap();
    assert_eq!(state.current_value, Some(1));

    // Assignment through tag 0x102 sets and clears flag bits.
    fn assign_flag(state: &mut CmvsPs2aVmState, index: u32, value: u32) {
        let expression = crate::CmvsPs2aStackExpression {
            span: SPAN,
            nodes: vec![
                crate::CmvsPs2aStackExpressionNode::Prefix {
                    span: SPAN,
                    opcode: 0x102,
                    payload: None,
                    index: None,
                    operands: vec![crate::CmvsPs2aStackExpression {
                        span: SPAN,
                        nodes: vec![literal_node(index)],
                    }],
                },
                literal_node(value),
                crate::CmvsPs2aStackExpressionNode::Operator {
                    span: SPAN,
                    opcode: 0x170,
                },
            ],
        };
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::StackExpression(expression),
        )
        .unwrap();
    }
    assign_flag(&mut state, 77, 1);
    assert!(state.process_flag_bits.contains(&77));
    assign_flag(&mut state, 41, 0);
    assert!(!state.process_flag_bits.contains(&41));
}

#[test]
fn apply_system_save_overwrites_flag_and_word_regions() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_flag_bits.insert(100);
    state.process_flag_bits.insert(3000);
    state.process_indexed_words.insert(2500, 9);
    let mut flags = vec![0_u8; 0x7F00];
    flags[0] = 0b101;
    flags[1] = 0x80;
    let mut cg_words = vec![0_u32; 2048];
    cg_words[500] = 1234;
    let save = crate::CmvsSystemSave {
        flags,
        cg_words,
        bgm_words: vec![0_u32; 1024],
        title_slot_count: 64,
        list_a_count: 0,
        list_b_count: 0,
    };
    apply_system_save(&mut state, save).unwrap();
    // Bits below the region survive.
    assert!(state.process_flag_bits.contains(&100));
    // Bits inside the region are replaced by the save bytes.
    assert!(!state.process_flag_bits.contains(&3000));
    assert!(state.process_flag_bits.contains(&2048));
    assert!(state.process_flag_bits.contains(&2050));
    assert!(state.process_flag_bits.contains(&(2048 + 15)));
    // The CG word table lands at process indexed words 2048..4096.
    assert_eq!(state.process_indexed_words.get(&2500), Some(&0));
    assert_eq!(state.process_indexed_words.get(&(2048 + 500)), Some(&1234));
    assert!(state.system_save.is_some());
}

#[test]
fn command_152_appends_to_the_string_list_and_reports_length() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 21);
    run_command(&mut state, 152);
    push(&mut state, 22);
    run_command(&mut state, 152);
    let list = state.private_string_lists.get(&152).unwrap();
    assert_eq!(
        list.as_slice(),
        [
            CmvsPs2aPrivateStringReference {
                relative_offset: 21
            },
            CmvsPs2aPrivateStringReference {
                relative_offset: 22
            },
        ]
    );
    assert_eq!(state.interpreter_words.get(&81220), Some(&2));
}

#[test]
fn command_212_stores_deterministic_random_draws_modulo_the_bound() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    run_command(&mut state, 212);
    // MSVC LCG from seed 1: first draw is 41, reduced modulo 5.
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    push(&mut state, 5);
    run_command(&mut state, 212);
    assert_eq!(state.interpreter_words.get(&81220), Some(&2));
    // A zero bound blocks instead of inventing a draw.
    push(&mut state, 0);
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 212,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "ASTRA_EMU_CMVS_VM_RANDOM");
}

#[test]
fn command_153_skips_the_action_when_the_cursor_index_is_unchanged() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 2);
    let action = run_command_with_action(&mut state, 153);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::SelectSystemCursor { index: 2 })
    );
    push(&mut state, 2);
    let action = run_command_with_action(&mut state, 153);
    assert_eq!(action, None);
    push(&mut state, 3);
    let action = run_command_with_action(&mut state, 153);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::SelectSystemCursor { index: 3 })
    );
}

#[test]
fn command_177_writes_the_channel_visibility_record_without_notify() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 3);
    let action = run_command_with_action(&mut state, 177);
    assert_eq!(action, None);
    assert_eq!(state.interpreter_words.get(&(1640 + 52 * 3)), Some(&1));
    assert_eq!(state.interpreter_words.get(&(1656 + 52 * 3)), Some(&1));
    assert!(state.interpreter_flag_words.is_empty());
}

#[test]
fn command_177_out_of_bounds_channel_raises_mask_0x200() {
    let mut state = CmvsPs2aVmState::new(0);
    let interpreter_words = state.interpreter_words.clone();
    push(&mut state, 1);
    push(&mut state, 6);
    run_command(&mut state, 177);
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x200));
    assert_eq!(state.interpreter_words, interpreter_words);
}

#[test]
fn command_688_stores_the_frame_pair_and_active_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 12);
    push(&mut state, 0);
    run_command(&mut state, 688);
    assert_eq!(state.interpreter_words.get(&10296), Some(&0));
    assert_eq!(state.interpreter_words.get(&10300), Some(&12));
    assert_eq!(state.interpreter_words.get(&10292), Some(&0));
}

#[test]
fn command_691_clamps_the_presentation_size_and_requests_layout() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, u32::MAX); // -1 signed
    let action = run_command_with_action(&mut state, 691);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::ApplyPresentationLayout {
            size_field_offset: 10452
        })
    );
    assert_eq!(state.interpreter_words.get(&10452), Some(&10));
    push(&mut state, 400);
    run_command(&mut state, 691);
    assert_eq!(state.interpreter_words.get(&10452), Some(&256));
    push(&mut state, 128);
    run_command(&mut state, 691);
    assert_eq!(state.interpreter_words.get(&10452), Some(&128));
}

#[test]
fn command_40_out_of_bounds_pair_only_raises_the_error_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    run_command(&mut state, 32);
    push(&mut state, 5);
    push(&mut state, 300);
    run_command(&mut state, 40);
    assert_eq!(
        state.slot_objects.get(&slot_object_key(1924, 5).unwrap()),
        Some(&1)
    );
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
}

#[test]
fn stores_component_constant_from_command_845() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 845,
        },
    )
    .unwrap();
    assert_eq!(
        state
            .component_words
            .get(&component_field_key(0x0cc8, 0x0568)),
        Some(&1)
    );
}

#[test]
fn starts_the_recovered_resource_channel_from_command_19() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [17, 2] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 19,
        },
    )
    .unwrap();
    let reference = CmvsPs2aPrivateStringReference {
        relative_offset: 17,
    };
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::StartResourceChannel {
            bank: CmvsResourceChannelBank::FiveSlotSetup,
            channel: 2,
            resource: reference,
        })
    );
    assert_eq!(
        state
            .resource_channels
            .get(&CmvsResourceChannelBank::FiveSlotSetup)
            .and_then(|bank| bank.get(&2)),
        Some(&reference)
    );
}

#[test]
fn executes_a_name_index_call_and_exact_stack_return() {
    let mut state = CmvsPs2aVmState::new(10);
    for value in [u32::MAX, 2, 9] {
        push_word(&mut state, value).unwrap();
    }
    let call = CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::NameIndexCall {
        span: CmvsPs2aSourceSpan {
            offset: 10,
            byte_length: 4,
        },
        name_index: 0,
        target: 40,
    });
    execute_cmvs390_frame(&mut state, &call).unwrap();
    assert_eq!(state.program_counter, 40);
    assert_eq!(state.stack_cursor_bytes, 16);
    assert_eq!(state.call_frame_bases, vec![0, 16]);
    assert_eq!(read_frame_local_word(&state, -16).unwrap(), u32::MAX);
    assert_eq!(read_frame_local_word(&state, -12).unwrap(), 2);
    assert_eq!(read_frame_local_word(&state, -8).unwrap(), 9);
    let returned = CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StackReturn {
        span: SPAN,
        stack_adjust: 12,
    });
    execute_cmvs390_frame(&mut state, &returned).unwrap();
    assert_eq!(state.program_counter, 14);
    assert_eq!(state.stack_cursor_bytes, 0);
    assert_eq!(state.call_frame_bases, vec![0]);
}

#[test]
fn native_input_poll_retains_edges_until_explicit_consume() {
    let mut state = CmvsPs2aVmState::new(0);
    state.input_confirm_held = true;
    state.input_advance_press = true;
    state.input_advance_release = true;
    let command = |command_id| CmvsPs2aInstructionFrame::Command {
        span: SPAN,
        command_id,
    };
    execute_cmvs390_frame(&mut state, &command(416)).unwrap();
    assert_eq!(state.interpreter_words[&81220], 1);
    assert_eq!(state.interpreter_words[&81236], 1);
    execute_cmvs390_frame(&mut state, &command(416)).unwrap();
    assert_eq!(state.interpreter_words[&81220], 1);
    execute_cmvs390_frame(&mut state, &command(417)).unwrap();
    assert!(!state.input_advance_press);
    assert!(!state.input_advance_release);
    assert!(state.input_confirm_held);
    execute_cmvs390_frame(&mut state, &command(416)).unwrap();
    assert_eq!(state.interpreter_words[&81220], 0);
    assert_eq!(state.interpreter_words[&81236], 1);
}

#[test]
fn native_crossfade_stop_preserves_duration_and_consumes_only_its_argument() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(&mut state, &push_immediate(17)).unwrap();
    execute_cmvs390_frame(&mut state, &push_immediate(250)).unwrap();
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 162,
        },
    )
    .unwrap();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::FadeOutAudio { fade_ms: 250 })
    );
    assert_eq!(state.stack_cursor_bytes, 4);
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 161,
        },
    )
    .unwrap();
    assert_eq!(action, Some(CmvsPs2aVmAction::StopAudio));
    assert_eq!(state.stack_cursor_bytes, 4);
}
