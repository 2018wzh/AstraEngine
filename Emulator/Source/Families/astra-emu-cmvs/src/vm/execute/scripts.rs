use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::CallScript,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            if *slot >= crate::CMVS_MAX_SCRIPT_CALL_INDEX {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_SCRIPT_FRAME",
                    "CMVS nested script frame index exceeds the recovered bound",
                ));
            }
            let frame = u16::try_from(*slot + 1).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_SCRIPT_FRAME",
                    "CMVS nested script frame index overflowed",
                )
            })?;
            let source_frame = state.current_frame;
            let name = script_name_reference(state, *name)?;
            // The loader pops both operands through the contract's
            // `stack_pop_bytes` before this branch runs; the dispatcher
            // result 0 adds no further pop.  It then pushes the frame
            // counter, the advanced PC and the previous frame index before
            // the host transfers dispatch to the loaded script.
            let frame_counter = state
                .interpreter_words
                .get(&SCRIPT_FRAME_COUNTER_FIELD)
                .copied()
                .unwrap_or(0);
            let return_pc = state.program_counter.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_PC",
                    "CMVS PS2A program counter overflowed",
                )
            })?;
            push_word(state, frame_counter)?;
            push_word(state, return_pc)?;
            push_word(state, u32::from(state.current_frame))?;
            if vm_trace_enabled() {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.stack_trace",
                    pc = state.program_counter,
                    frame = state.current_frame
                );
            }
            state.current_frame = frame;
            Ok(Some(CmvsPs2aVmAction::CallScript {
                source_frame,
                frame,
                name,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ReloadRootScript,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            let source_frame = state.current_frame;
            let name = script_name_reference(state, *name)?;
            // `sub_4781B0` resets the data stack, the frame stack-table
            // words and the current frame before the new root script takes
            // over; the expression-result field is zeroed with its flag bit.
            state.stack_bytes.clear();
            state.stack_initialized.clear();
            state.stack_cursor_bytes = 0;
            state.call_frame_bases.clear();
            state.call_frame_bases.push(0);
            state.current_frame = 0;
            state.current_value = None;
            state.condition_flag = false;
            state.interpreter_words.insert(80952, 0);
            state.interpreter_words.insert(81208, 0);
            state.interpreter_words.insert(81212, 0);
            state.interpreter_words.insert(81216, 0);
            // The same loader invalidates the coroutine table lazily: a
            // record whose frame id is still the constructor default keeps
            // only its previous-slot flag cleared, with the PC word set to
            // the all-ones unregistered value (`sub_4781B0`).
            for label in 0..COROUTINE_RECORD_COUNT {
                let base = COROUTINE_RECORD_TABLE_BASE + COROUTINE_RECORD_STRIDE * label;
                let frame_id = state
                    .interpreter_words
                    .get(&(base + 4))
                    .copied()
                    .unwrap_or(0);
                if frame_id == 0 {
                    state.interpreter_words.insert(base + 8, u32::MAX);
                    state.interpreter_words.insert(base, 0);
                }
            }
            Ok(Some(CmvsPs2aVmAction::ReloadRootScript {
                source_frame,
                name,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord {
                record_table_base_offset,
                record_stride_bytes,
                max_index_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, label)],
        ) => {
            // `sub_47CB00` masks the label like the record writer does and
            // indexes the same 28-byte table.
            let label = *label & u32::from(max_index_mask);
            let base = u32::from(record_table_base_offset)
                .checked_add(u32::from(record_stride_bytes).wrapping_mul(label))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_FIELD",
                        "CMVS coroutine record offset overflowed",
                    )
                })?;
            let (recorded_frame, recorded_pc) = match (
                state.interpreter_words.get(&(base + 4)),
                state.interpreter_words.get(&(base + 8)),
            ) {
                (Some(frame), Some(pc)) => (*frame, *pc),
                // The original would jump to an unregistered address; the
                // recovered subset blocks instead of emulating that fault.
                _ => {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_VM_COROUTINE",
                        "CMVS coroutine label has no registered continuation record",
                    ))
                }
            };
            if recorded_pc == u32::MAX {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_COROUTINE",
                    "CMVS coroutine label was invalidated before the resume",
                ));
            }
            let frame = u16::try_from(recorded_frame).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_COROUTINE",
                    "CMVS coroutine record frame index overflowed",
                )
            })?;
            // The original advances the PC past the opcode before pushing
            // the return record (`sub_47CB00`); the frame-counter value is
            // read after that advance, matching the original field order.
            let return_pc = state.program_counter.checked_add(2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_PC",
                    "CMVS PS2A program counter overflowed",
                )
            })?;
            let frame_counter = state
                .interpreter_words
                .get(&SCRIPT_FRAME_COUNTER_FIELD)
                .copied()
                .unwrap_or(0);
            push_word(state, frame_counter)?;
            push_word(state, return_pc)?;
            push_word(state, u32::from(state.current_frame))?;
            if vm_trace_enabled() {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.stack_trace",
                    pc = state.program_counter,
                    frame = state.current_frame
                );
            }
            state
                .interpreter_words
                .insert(SCRIPT_FRAME_COUNTER_FIELD, 0);
            state.current_frame = frame;
            state.program_counter = recorded_pc;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy {
                slot_table_dword_index,
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _selector)],
        ) => {
            let _ = slot_table_dword_index;
            if *slot > max_slot {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let key = slot_object_key(
                u16::try_from(slot_table_dword_index).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot table offset overflowed",
                    )
                })?,
                *slot,
            )?;
            let occupied = state.slot_objects.contains_key(&key);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(occupied));
            Ok(None)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "CMVS PS2A command stack words do not match the recovered effect contract",
        )),
    }
}
