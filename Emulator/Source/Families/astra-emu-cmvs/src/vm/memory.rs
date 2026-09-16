use super::*;

/// The byte length the current frame's loaded script declares for its
/// runtime data segment.
pub(super) fn script_data_segment_bound(state: &CmvsPs2aVmState) -> Result<u32, CoreError> {
    state
        .script_data_segment_sizes
        .get(&state.current_frame)
        .copied()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
                "CMVS data-segment access has no loaded script frame",
            )
        })
}

/// Reads one data-segment dword. The loader's buffer keeps the decoded
/// bytes, so unwritten words read their loaded initial value, which is
/// zero for every recovered case; the provider materializes non-zero
/// initial dwords at script load.
pub(super) fn read_script_data_segment_word(
    state: &CmvsPs2aVmState,
    offset: u32,
) -> Result<u32, CoreError> {
    let end = offset.checked_add(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
            "CMVS data-segment read offset overflowed",
        )
    })?;
    if end > script_data_segment_bound(state)? {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
            "CMVS data-segment read is outside the loaded segment",
        ));
    }
    let value = state
        .script_data_segment_words
        .get(&state.current_frame)
        .and_then(|words| words.get(&offset))
        .copied()
        .unwrap_or(0);
    if tracing::enabled!(tracing::Level::TRACE) && offset <= 0x40 {
        tracing::trace!(
            event = "astra.emu.cmvs.vm.data_read",
            pc = state.program_counter,
            frame = state.current_frame
        );
    }
    Ok(value)
}

pub(super) fn write_script_data_segment_word(
    state: &mut CmvsPs2aVmState,
    offset: u32,
    value: u32,
) -> Result<(), CoreError> {
    let end = offset.checked_add(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
            "CMVS data-segment write offset overflowed",
        )
    })?;
    if end > script_data_segment_bound(state)? {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_DATA_SEGMENT",
            "CMVS data-segment write is outside the loaded segment",
        ));
    }
    state
        .script_data_segment_words
        .entry(state.current_frame)
        .or_default()
        .insert(offset, value);
    if tracing::enabled!(tracing::Level::TRACE) && offset <= 0x40 {
        tracing::trace!(
            event = "astra.emu.cmvs.vm.data_write",
            pc = state.program_counter,
            frame = state.current_frame
        );
    }
    Ok(())
}

/// Reinterprets an entry word as the signed frame-variable offset the
/// original applies to `variable_base + frame_offsets[frame]`.
pub(super) fn signed_word(value: u32) -> Result<i32, CoreError> {
    Ok(i32::from_ne_bytes(value.to_ne_bytes()))
}

/// Reads one frame-variable slot; the original buffer is zero-initialized,
/// so unwritten slots return zero.
pub(super) fn read_frame_local_word(
    state: &CmvsPs2aVmState,
    offset: i32,
) -> Result<u32, CoreError> {
    let start = frame_local_address(state, offset)?;
    let end = start.checked_add(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_FRAME_LOCAL",
            "CMVS frame-local read overflowed the variable buffer",
        )
    })?;
    if end > MAX_STACK_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_FRAME_LOCAL",
            "CMVS frame-local read exceeds the recovered variable-buffer budget",
        ));
    }
    let mut bytes = [0_u8; 4];
    if start < state.stack_bytes.len() {
        let available_end = end.min(state.stack_bytes.len());
        bytes[..available_end - start].copy_from_slice(&state.stack_bytes[start..available_end]);
    }
    Ok(u32::from_le_bytes(bytes))
}

pub(super) fn write_frame_local_word(
    state: &mut CmvsPs2aVmState,
    offset: i32,
    value: u32,
) -> Result<(), CoreError> {
    let start = frame_local_address(state, offset)?;
    let end = start.checked_add(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_FRAME_LOCAL",
            "CMVS frame-local write overflowed the variable buffer",
        )
    })?;
    if end > MAX_STACK_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_FRAME_LOCAL",
            "CMVS frame-local write exceeds the recovered variable-buffer budget",
        ));
    }
    ensure_stack_memory(state, end)?;
    state.stack_bytes[start..end].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

pub(super) fn frame_local_address(
    state: &CmvsPs2aVmState,
    offset: i32,
) -> Result<usize, CoreError> {
    let base = i64::from(*state.call_frame_bases.last().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_CALL_DEPTH",
            "CMVS VM has no active call-frame base",
        )
    })?);
    let address = base.checked_add(i64::from(offset)).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_FRAME_LOCAL",
            "CMVS frame-local address overflowed",
        )
    })?;
    usize::try_from(address).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_FRAME_LOCAL",
            "CMVS frame-local address precedes the variable buffer",
        )
    })
}

/// The recovered store dispatch behind the `0x170` postfix. Tag `0x101`
/// writes the process-indexed table; tag `0x102` sets or clears a process
/// flag bit; tags `0x108`/`0x103` write the current frame's local-word
/// buffer at positive/negative offsets; tag `0x12b` writes the process
/// float table; tag `0x10f` writes system registers 0..=6 and silently
/// ignores higher indices, matching the original.
pub(super) fn store_stack_expression_target(
    state: &mut CmvsPs2aVmState,
    target: StackExpressionEntry,
    value: u32,
) -> Result<(), CoreError> {
    match target.tag {
        0x101 => {
            if tracing::enabled!(tracing::Level::TRACE) && matches!(target.value, 0x4d | 0x6e) {
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.proc_write",
                    pc = state.program_counter,
                    frame = state.current_frame
                );
            }
            state.process_indexed_words.insert(target.value, value);
            Ok(())
        }
        0x108 => {
            let offset = signed_word(target.value)?;
            write_frame_local_word(state, offset, value)
        }
        0x104 | 0x106 | 0x107 => {
            // `sub_46E250` stores these tags through the same data-segment
            // base the reads use.
            write_script_data_segment_word(state, target.value, value)
        }
        0x103 => {
            let offset = signed_word(target.value)?.wrapping_neg();
            write_frame_local_word(state, offset, value)
        }
        0x12b => {
            // `sub_46E250` case 0x12B forwards through `sub_48AC30` into
            // the process float table; the stored word is reinterpreted as
            // the f32 bit pattern, matching the original's `float` cast.
            if target.value >= MAX_PROCESS_FLOAT_WORDS {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_VM_PROCESS_FLOAT",
                    "CMVS float variable index exceeds the recovered table budget",
                ));
            }
            state.process_float_words.insert(target.value, value);
            Ok(())
        }
        0x102 => {
            // `sub_48ABC0` sets or clears the process flag bit depending on
            // whether the assigned value is non-zero.
            if value != 0 {
                state.process_flag_bits.insert(target.value);
            } else {
                state.process_flag_bits.remove(&target.value);
            }
            Ok(())
        }
        0x10f if target.value <= 6 => {
            state
                .interpreter_words
                .insert(SYSTEM_REGISTER_FIELD_BASE + 4 * target.value, value);
            Ok(())
        }
        0x10f => Ok(()),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_STACK_EXPRESSION",
            "CMVS PS2A assignment target tag is not recovered",
        )),
    }
}
