use super::*;

/// The interpreter field holding the script frame counter pushed by the
/// nested-script loader (byte offset 13224).
pub(super) const SCRIPT_FRAME_COUNTER_FIELD: u32 = 13224;

/// The coroutine record table `sub_47CB00` indexes through
/// `sub_47CBE0`/`sub_47CC80`: 64 records of 28 bytes whose registered
/// frame id sits at byte offset 13276 and PC at 13280.
pub(super) const COROUTINE_RECORD_TABLE_BASE: u32 = 13272;
pub(super) const COROUTINE_RECORD_COUNT: u32 = 64;
pub(super) const COROUTINE_RECORD_STRIDE: u32 = 28;

/// Applies one validated system save to the VM state, mirroring the
/// recovered loader `sub_427B30`:
///
/// * the flag region (0x7F00 bytes) overwrites the process flag bitmap at
///   byte offset 256 (bit indices 2048..=262143),
/// * the CG word table (2048 words) overwrites the process indexed-word
///   table at indices 2048..=4095, the same table tag `0x101` reads,
/// * the BGM table and string pools stay in the retained save record until
///   their consumers are recovered.
///
/// Existing flag bits and words inside the overwritten ranges are dropped
/// first, matching the loader's in-place `memmove` semantics.
pub fn apply_system_save(
    state: &mut CmvsPs2aVmState,
    save: crate::CmvsSystemSave,
) -> Result<(), CoreError> {
    const FLAG_REGION_START_BIT: u32 = 2048;
    let region_bits = u32::try_from(save.flags.len())
        .ok()
        .and_then(|len| len.checked_mul(8))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                "system save flag region overflows the process flag bitmap",
            )
        })?;
    let region_end_bit = FLAG_REGION_START_BIT
        .checked_add(region_bits)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                "system save flag region overflows the process flag bitmap",
            )
        })?;
    state
        .process_flag_bits
        .retain(|bit| *bit < FLAG_REGION_START_BIT || *bit >= region_end_bit);
    for (byte_index, byte) in save.flags.iter().enumerate() {
        for bit in 0..8_u32 {
            if byte & (1 << bit) != 0 {
                let flag_bit = FLAG_REGION_START_BIT
                    + u32::try_from(byte_index).map_err(|_| {
                        invalid(
                            "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                            "system save flag region index overflows",
                        )
                    })? * 8
                    + bit;
                state.process_flag_bits.insert(flag_bit);
            }
        }
    }
    const CG_REGION_START_INDEX: u32 = 2048;
    for (index, word) in save.cg_words.iter().enumerate() {
        let table_index = CG_REGION_START_INDEX
            + u32::try_from(index).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                    "system save CG table index overflows",
                )
            })?;
        state.process_indexed_words.insert(table_index, *word);
    }
    state.system_save = Some(save);
    Ok(())
}

/// Number of per-frame entry slots the original scheduler scans each frame
/// (`sub_46DF10` iterates 32 of the 64 coroutine record slots).
pub(super) const FRAME_ENTRY_QUEUE_SLOTS: u32 = 32;

/// Effect channel object table (`this[742 + channel]`). Slots 7-14 of the
/// per-frame scheduler test the handle at interpreter dword 735+i, which is
/// this table at channel `i-7`.
pub(super) const EFFECT_CHANNEL_TABLE_DWORD_INDEX: u32 = 742;

/// Per-frame scan indexes (queue slot minus three) that the original
/// scheduler always reactivates.
pub(super) const FRAME_ENTRY_ALWAYS_ACTIVE: [u32; 5] = [0, 1, 2, 3, 25];

/// Interpreter word that gates the per-frame entry queue rebuild. Script
/// `command 137` (`sub_47CCB0`) stores its boolean operand here, and
/// `sub_46DF10` only re-queues the suspended triple and the per-frame
/// entries while this word is non-zero.
pub(super) const FRAME_ENTRY_SCHEDULER_ENABLED_WORD: u32 = 3317;

/// Drops the rebuilt per-frame entry queue from the value stack, keeping the
/// initialization tracking and cursor in lockstep so the snapshot validation
/// holds (`ensure_stack_memory` grows both vectors together).
#[cfg_attr(not(test), allow(dead_code))]
pub fn truncate_stack_to(state: &mut CmvsPs2aVmState, baseline: u32) {
    let baseline = baseline as usize;
    if state.stack_cursor_bytes as usize > baseline {
        state.stack_bytes.truncate(baseline);
        state.stack_initialized.truncate(baseline);
        state.stack_cursor_bytes = baseline as u32;
    }
}

pub fn rebuild_frame_entry_queue(state: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    let scheduler_enabled = state
        .interpreter_words
        .get(&FRAME_ENTRY_SCHEDULER_ENABLED_WORD)
        .copied()
        .unwrap_or_default()
        != 0;
    if scheduler_enabled {
        let suspended_delay = state
            .interpreter_words
            .get(&SCRIPT_FRAME_COUNTER_FIELD)
            .copied()
            .unwrap_or_default();
        push_word(state, suspended_delay)?;
        push_word(state, state.program_counter)?;
        push_word(state, u32::from(state.current_frame))?;
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                event = "astra.emu.cmvs.vm.queue",
                pc = state.program_counter,
                frame = state.current_frame
            );
        }
        for slot in 0..FRAME_ENTRY_QUEUE_SLOTS {
            let base = COROUTINE_RECORD_TABLE_BASE + COROUTINE_RECORD_STRIDE * slot;
            let entry_pc = state.interpreter_words.get(&(base + 8)).copied();
            let Some(entry_pc) = entry_pc else {
                continue;
            };
            if entry_pc == u32::MAX {
                continue;
            }
            if tracing::enabled!(tracing::Level::TRACE) {
                let entry_frame = state.interpreter_words.get(&(base + 4)).copied();
                tracing::trace!(
                    event = "astra.emu.cmvs.vm.entry_slot",
                    pc = entry_pc,
                    slot,
                    has_frame = entry_frame.is_some()
                );
            }
            // The original scan index runs -3..=28 over the 32 queue slots;
            // all switch cases below are expressed in that index. Queue slot
            // `s` here corresponds to scan index `s - 3`.
            let scan_index = slot as i32 - 3;
            let active = if FRAME_ENTRY_ALWAYS_ACTIVE
                .iter()
                .any(|index| *index as i32 == scan_index)
            {
                true
            } else if (7..=14).contains(&scan_index) {
                // Scan indexes 7-14 mirror the eight effect channels: the
                // original tests the object handle at interpreter dword
                // 735+scan, which is `this[742 + (scan-7)]` — the same
                // handle `command 349` publishes and `command 321` clears.
                // Occupancy of the `(742<<8)|channel` slot object activates
                // the entry.
                let channel = (scan_index - 7) as u32;
                state
                    .slot_objects
                    .contains_key(&(EFFECT_CHANNEL_TABLE_DWORD_INDEX << 8 | channel))
            } else if scan_index == 23 {
                state
                    .interpreter_words
                    .get(&base)
                    .copied()
                    .unwrap_or_default()
                    != 0
            } else {
                false
            };
            if !active {
                continue;
            }
            let entry_frame = state
                .interpreter_words
                .get(&(base + 4))
                .copied()
                .unwrap_or_default();
            push_word(state, 0)?;
            push_word(state, entry_pc)?;
            push_word(state, entry_frame)?;
        }
        // The original decrements the stack cursor past the topmost triple
        // and reads `(frame, pc, delay)` back from it, so exactly one
        // 12-byte triple is consumed here regardless of how many entries
        // queued.
        let entry_frame = pop_word(state)?;
        let entry_pc = pop_word(state)?;
        let entry_delay = pop_word(state)?;
        let frame = u16::try_from(entry_frame).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_VM_COROUTINE",
                "CMVS frame entry queue restored an out-of-range frame id",
            )
        })?;
        state.current_frame = frame;
        state.program_counter = entry_pc;
        state
            .interpreter_words
            .insert(SCRIPT_FRAME_COUNTER_FIELD, entry_delay);
    }
    for label in 0..COROUTINE_RECORD_COUNT {
        let base = COROUTINE_RECORD_TABLE_BASE + COROUTINE_RECORD_STRIDE * label;
        state.interpreter_words.insert(base, 0);
    }
    Ok(())
}
