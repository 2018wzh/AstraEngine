use super::*;

pub(super) fn raise_interpreter_error_flag(state: &mut CmvsPs2aVmState) {
    let flags = state
        .interpreter_flag_words
        .entry(INTERPRETER_ERROR_FLAG_OFFSET)
        .or_insert(0);
    *flags |= INTERPRETER_ERROR_FLAG_MASK;
}

pub(super) fn raise_error_flag(
    state: &mut CmvsPs2aVmState,
    field_offset: u32,
    mask: u32,
) -> Result<(), CoreError> {
    let field_offset = u16::try_from(field_offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_ERROR_FLAG",
            "CMVS interpreter error-flag offset exceeds the retained field space",
        )
    })?;
    let flags = state
        .interpreter_flag_words
        .entry(field_offset)
        .or_insert(0);
    *flags |= mask;
    Ok(())
}

/// The exact field writes of the recovered settings reset helper
/// `sub_45AE00`, converted from its DWORD offsets to byte offsets.
pub(super) const SETTINGS_RESET_FIELDS: [(u16, u32); 14] = [
    (48, 0),
    (52, 0),
    (44, u32::MAX),
    (72, 0),
    (76, 0),
    (80, 0),
    (88, u32::MAX),
    (92, 0),
    (108, 0),
    (84, 0),
    (120, 0),
    (172, u32::MAX),
    (180, 0),
    (184, 0),
];

pub(super) fn reset_settings_object(state: &mut CmvsPs2aVmState) {
    for (offset, value) in SETTINGS_RESET_FIELDS {
        state.settings_words.insert(offset, value);
    }
}

pub(super) fn slot_object_key(table_byte_offset: u16, slot: u32) -> Result<u32, CoreError> {
    let slot = u8::try_from(slot).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_SLOT_OBJECTS",
            "CMVS slot object index exceeds the recovered table bounds",
        )
    })?;
    Ok((u32::from(table_byte_offset) << 8) | u32::from(slot))
}

/// Frees the case-544 filter-graph owner. `sub_47A660` clears the handle word
/// unconditionally on both resolve paths, and case 546 frees through the same
/// boundary.
pub(super) fn destroy_filter_graph_owner(
    state: &mut CmvsPs2aVmState,
    table_byte_offset: u16,
    slot: u8,
) {
    let key = (u32::from(table_byte_offset) << 8) | u32::from(slot);
    state.slot_objects.remove(&key);
    state.slot_object_seed_words.remove(&key);
    if table_byte_offset == 3252 {
        state
            .interpreter_words
            .insert(u32::from(table_byte_offset), 0);
    }
}

pub(super) fn texture_parent_is_live(
    state: &CmvsPs2aVmState,
    table_byte_offset: u32,
    max_parent_slot: u8,
    parent: u32,
) -> Result<bool, CoreError> {
    if parent > u32::from(max_parent_slot) {
        return Ok(false);
    }
    let table_byte_offset = u16::try_from(table_byte_offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_VM_TEXTURE",
            "CMVS texture table offset exceeds the retained slot-key space",
        )
    })?;
    Ok(state
        .slot_objects
        .contains_key(&slot_object_key(table_byte_offset, parent)?))
}

pub(super) fn mutate_process_flag_range(
    state: &mut CmvsPs2aVmState,
    start: u32,
    count: u32,
    enabled: bool,
) -> Result<(), CoreError> {
    let end = start.checked_add(count).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_PROCESS_FLAGS",
            "CMVS process flag range overflowed",
        )
    })?;
    if end > MAX_PROCESS_FLAG_BITS {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_PROCESS_FLAGS",
            "CMVS process flag range exceeds the recovered bitmap budget",
        ));
    }
    for bit in start..end {
        if enabled {
            state.process_flag_bits.insert(bit);
        } else {
            state.process_flag_bits.remove(&bit);
        }
    }
    Ok(())
}

pub(super) fn store_process_indexed_range(
    state: &mut CmvsPs2aVmState,
    start: u32,
    count: u32,
    value: u32,
) -> Result<(), CoreError> {
    let end = start.checked_add(count).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_PROCESS_INDEXED",
            "CMVS process indexed range overflowed",
        )
    })?;
    if end > MAX_PROCESS_INDEXED_WORDS {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_PROCESS_INDEXED",
            "CMVS process indexed range exceeds the recovered table budget",
        ));
    }
    for index in start..end {
        state.process_indexed_words.insert(index, value);
    }
    Ok(())
}

pub(super) fn store_process_float_range(
    state: &mut CmvsPs2aVmState,
    start: u32,
    count: u32,
    value: u32,
) -> Result<(), CoreError> {
    let end = start.checked_add(count).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_PROCESS_FLOAT",
            "CMVS process float range overflowed",
        )
    })?;
    if end > MAX_PROCESS_FLOAT_WORDS {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_PROCESS_FLOAT",
            "CMVS process float range exceeds the recovered table budget",
        ));
    }
    for index in start..end {
        state.process_float_words.insert(index, value);
    }
    Ok(())
}

pub(super) fn store_process_string_range(
    state: &mut CmvsPs2aVmState,
    start: u32,
    count: u32,
    value: CmvsPs2aPrivateStringReference,
) -> Result<(), CoreError> {
    let end = start.checked_add(count).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_VM_PROCESS_STRING",
            "CMVS process string range overflowed",
        )
    })?;
    let effective_end = end.min(MAX_PROCESS_STRING_SLOTS);
    if start < effective_end {
        for index in start..effective_end {
            state.process_string_slots.insert(index, value);
        }
    }
    Ok(())
}

pub(super) fn component_field_key(component_offset: u16, field_offset: u16) -> u32 {
    (u32::from(component_offset) << 16) | u32::from(field_offset)
}
