use super::*;

/// Executes exactly one framed instruction. On error, `state` is unchanged.
/// Unsupported expressions, controls, tag domains and stack widths are
/// blocking by design; callers must not advance past them.
pub fn execute_cmvs390_frame(
    state: &mut CmvsPs2aVmState,
    frame: &CmvsPs2aInstructionFrame,
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    if state.dispatch_stopped {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STOPPED",
            "CMVS PS2A dispatch has already stopped",
        ));
    }
    if state.storage_await.is_some() || state.filter_graph_input_await {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STORAGE_AWAIT",
            "CMVS PS2A dispatch waits on an unresolved host request",
        ));
    }
    let mut next = state.clone();
    let depth_before = next.stack_cursor_bytes;
    let action = match frame {
        CmvsPs2aInstructionFrame::ValueExpression(expression) => {
            run_value_expression(&mut next, expression)?;
            advance(&mut next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::StackExpression(expression) => {
            run_stack_expression(&mut next, expression)?;
            advance(&mut next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::NumericExpression(expression) => {
            run_numeric_expression(&mut next, expression)?;
            advance(&mut next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::AbsoluteJump {
            target,
            ..
        })
        | CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWithOpaqueField {
            target,
            ..
        })
        | CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::NameIndexJump {
            target,
            ..
        }) => {
            next.program_counter = *target;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:644",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWhenFlagClear {
            target,
            ..
        }) => {
            // The original branch reads the persistent condition word
            // (`*(byte *)(this + 81212) & 1`), so keep the in-memory flag and
            // the interpreter word as one synchronized state.
            if next.current_frame != 0 && vm_trace_enabled() {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.branch",
                    pc = next.program_counter,
                    frame = next.current_frame
                );
            }
            if !next.interpreter_words.get(&81212).copied().unwrap_or(0) & 1 == 1 {
                next.program_counter = *target;
                if !next.program_counter.is_multiple_of(2) {
                    return Err(CoreError::invalid(
                        "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                        format!(
                            "CMVS PS2A PC set to odd {} at vm.rs:652",
                            next.program_counter
                        ),
                    ));
                }
            } else {
                advance(&mut next, frame)?;
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWhenFlagSet {
            target,
            ..
        }) => {
            if next.current_frame != 0 && vm_trace_enabled() {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.branch",
                    pc = next.program_counter,
                    frame = next.current_frame
                );
            }
            if next.interpreter_words.get(&81212).copied().unwrap_or(0) & 1 == 1 {
                next.program_counter = *target;
                if !next.program_counter.is_multiple_of(2) {
                    return Err(CoreError::invalid(
                        "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                        format!(
                            "CMVS PS2A PC set to odd {} at vm.rs:663",
                            next.program_counter
                        ),
                    ));
                }
            } else {
                advance(&mut next, frame)?;
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWhenValueEquals {
            expected_value,
            target,
            ..
        }) => {
            if next.current_value == Some(*expected_value) {
                next.program_counter = *target;
                if !next.program_counter.is_multiple_of(2) {
                    return Err(CoreError::invalid(
                        "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                        format!(
                            "CMVS PS2A PC set to odd {} at vm.rs:675",
                            next.program_counter
                        ),
                    ));
                }
            } else {
                advance(&mut next, frame)?;
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::NameIndexCall {
            target,
            ..
        }) => {
            let return_pc = next
                .program_counter
                .checked_add(u32::from(frame.span().byte_length))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_PC",
                        "CMVS PS2A program counter overflowed",
                    )
                })?;
            push_word(&mut next, return_pc)?;
            push_call_frame_base(&mut next)?;
            next.program_counter = *target;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:696",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StateTargetCall {
            ..
        }) => {
            // The original case 1046 reads the target from the persistent
            // expression-result word at byte offset 81216 (`mov eax,
            // [edi+13D40h]`), not from the evaluator's in-register result,
            // so the value survives intervening command side effects.
            let target = next
                .interpreter_words
                .get(&VALUE_EXPRESSION_RESULT_FIELD)
                .copied()
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_CURRENT_VALUE",
                        "CMVS PS2A state-target call requires an evaluator result",
                    )
                })?;
            let return_pc = next
                .program_counter
                .checked_add(u32::from(frame.span().byte_length))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_PC",
                        "CMVS PS2A program counter overflowed",
                    )
                })?;
            push_word(&mut next, return_pc)?;
            push_call_frame_base(&mut next)?;
            next.program_counter = target;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:727",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StackReturn {
            stack_adjust,
            ..
        }) => {
            // The original 0x411 (`sub_46DAF0`) pops the return PC, drops the
            // adjust bytes and decrements the frame counter; a coroutine
            // resume pushed by case 138 also returns through this path, so a
            // missing 0x410 frame base is legal and only the counter pops.
            if next.call_frame_bases.len() <= 1 {
                // The per-tick interrupt entry can reach 0x411 with the
                // per-frame queue already consumed; an empty value stack has
                // no return word, so end the dispatch and let the next tick
                // rebuild the queue instead of underflowing. An invalid
                // return word is likewise left on the stack so the per-frame
                // queue is not drained tick after tick.
                if next.stack_cursor_bytes < 4 {
                    next.dispatch_stopped = true;
                } else {
                    let current_pc = next.program_counter;
                    let return_pc = read_stack_word_from_top(&next, 4)?;
                    if return_pc == current_pc || return_pc == 0 || !return_pc.is_multiple_of(2) {
                        next.dispatch_stopped = true;
                    } else {
                        let _ = pop_word(&mut next)?;
                        drop_stack_bytes(&mut next, *stack_adjust)?;
                        next.program_counter = return_pc;
                    }
                }
                None
            } else if next.stack_cursor_bytes < 4 {
                next.dispatch_stopped = true;
                None
            } else {
                let current_pc = next.program_counter;
                let return_pc = read_stack_word_from_top(&next, 4)?;
                if return_pc == current_pc || return_pc == 0 {
                    // A self-return inside the event handler is the same
                    // stale-word case; end the dispatch rather than loop,
                    // leaving the stale word on the stack.
                    next.dispatch_stopped = true;
                } else if !return_pc.is_multiple_of(2) {
                    return Err(CoreError::invalid(
                        "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                        format!("CMVS PS2A PC set to odd {return_pc} at vm.rs:747"),
                    ));
                } else {
                    let _ = pop_word(&mut next)?;
                    drop_stack_bytes(&mut next, *stack_adjust)?;
                    next.call_frame_bases.pop();
                    next.program_counter = return_pc;
                }
                None
            }
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StackDrop {
            stack_bytes,
            ..
        }) => {
            drop_stack_bytes(&mut next, *stack_bytes)?;
            advance(&mut next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::FrameReturn { .. }) => {
            // Opcode 0x413 pops the return PC, then adjusts the stack by
            // word[81216] * 4 bytes and leaves the call frame. The original
            // dispatcher reads the adjustment from the persistent expression
            // result word (`mov ax, [edi+13D40h]; shl ax, 2`), not from the
            // evaluator's in-register value, so the two can diverge across
            // intervening command side effects.
            if next.call_frame_bases.len() <= 1 {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_CALL_DEPTH",
                    "CMVS frame return has no active call frame",
                ));
            }
            let return_pc = pop_word(&mut next)?;
            let adjust = next
                .interpreter_words
                .get(&VALUE_EXPRESSION_RESULT_FIELD)
                .copied()
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_CURRENT_VALUE",
                        "CMVS frame return requires an evaluator result",
                    )
                })?;
            let adjust_bytes = u16::try_from(u32::from(adjust as u16) * 4).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_STACK",
                    "CMVS frame return stack adjustment overflowed",
                )
            })?;
            drop_stack_bytes(&mut next, adjust_bytes)?;
            next.call_frame_bases.pop();
            next.program_counter = return_pc;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:784",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::ScriptReturn { .. }) => {
            // Opcode 0x414 pops the saved frame index, return PC and frame
            // counter pushed by the nested-script loader, in that order.
            let frame_word = pop_word(&mut next)?;
            let return_pc = pop_word(&mut next)?;
            let frame_counter = pop_word(&mut next)?;
            let frame = u16::try_from(frame_word).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_SCRIPT_FRAME",
                    "CMVS script return frame index overflowed",
                )
            })?;
            next.interpreter_words
                .insert(SCRIPT_FRAME_COUNTER_FIELD, frame_counter);
            next.current_frame = frame;
            next.program_counter = return_pc;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "ASTRA_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:802",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushCurrentCondition {
            ..
        }) => {
            let value = next.current_value.ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_CURRENT_VALUE",
                    "CMVS PS2A push requires an evaluator result",
                )
            })?;
            push_word(&mut next, value)?;
            advance(&mut next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            stack_bytes,
            value,
            ..
        }) => {
            push_immediate(&mut next, *value, *stack_bytes)?;
            advance(&mut next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Command { command_id, .. } => {
            let contract = cmvs390_command_contract(*command_id).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_COMMAND",
                    "CMVS PS2A command has no recovered execution contract",
                )
            })?;
            // Case 138 transfers dispatch to the registered record address
            // itself; the interpreter loop must not advance the PC again
            // after its effect runs. `command 0` returns `0xC000` in the
            // original dispatcher: the negative bit ends the frame, but the
            // `0x4000` pop flag still applies, so the PC advances past the
            // command word before the dispatch yields.
            let program_counter_overridden = matches!(
                contract.effect_kind,
                CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord { .. }
            );
            let action = execute_command(
                &mut next,
                contract.effect_kind,
                contract.stack_pop_bytes,
                &contract.stack_words,
            )?;
            if !program_counter_overridden {
                advance(&mut next, frame)?;
            }
            action
        }
    };
    let depth_after = next.stack_cursor_bytes;
    let trace_pc = next.program_counter;
    let trace_frame = next.current_frame;
    *state = next;
    if trace_frame != 0 && vm_trace_enabled() {
        tracing::trace!(
            event = "astra.emu.cmvs.vm.ctl_trace",
            pc = trace_pc,
            frame = trace_frame,
            depth_before,
            depth_after
        );
    }
    if depth_after != depth_before && vm_trace_enabled() {
        tracing::trace!(
            event = "astra.emu.cmvs.vm.depth_trace",
            pc = trace_pc,
            frame = trace_frame,
            depth_before,
            depth_after
        );
    }
    Ok(action)
}
