use super::*;

pub(super) fn tag_zero_reference(value: u32) -> Result<CmvsPs2aPrivateStringReference, CoreError> {
    // Tag 0x00 is the active frame's private pool and tag 0x40 is the
    // alternate frame pool; both resolve through the same frame-owned
    // string store, so the alternate tag folds onto the masked offset.
    // Tags 0x80 (process-global message slots) and 0xC0 stay unrecovered
    // here; command 48 resolves them through the message slot store.
    if value & 0xc000_0000 != 0 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_TAG",
            "CMVS PS2A string reference uses an unrecovered tag domain",
        ));
    }
    Ok(CmvsPs2aPrivateStringReference {
        relative_offset: value & 0x3fff_ffff,
    })
}

/// Resolves the script-name operand of `command 128`/`command 129`. Beyond
/// the frame-pool tags above, tag 0x80 names a process-global message slot;
/// when that slot holds exactly one pool-string segment the script name is
/// that segment's private pool offset. Any other shape stays fail-closed
/// (`sub_478080` resolves the name text before loading).
pub(super) fn script_name_reference(
    state: &CmvsPs2aVmState,
    value: u32,
) -> Result<CmvsPs2aPrivateStringReference, CoreError> {
    match value & 0xc000_0000 {
        0 | 0x4000_0000 => Ok(CmvsPs2aPrivateStringReference {
            relative_offset: value & 0x3fff_ffff,
        }),
        0x8000_0000 => {
            let index = value & 0x3fff_ffff;
            let segments = state
                .message_string_slots
                .get(&index)
                .cloned()
                .unwrap_or_default();
            if let Some(reference) = single_pool_string_in_segments(state, &segments, 0) {
                return Ok(reference);
            }
            // The native engine fills this slot from interpreter-external
            // state (the menu confirm path copies the scenario name before
            // `command 128` runs). The sentinel offset defers the name
            // resolution to the provider, which maps the active filter-chain
            // selection to its scenario.
            Ok(CmvsPs2aPrivateStringReference {
                relative_offset: u32::MAX,
            })
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_TAG",
            "CMVS PS2A string reference uses an unrecovered tag domain",
        )),
    }
}

/// Follows nested message-slot segments until exactly one pool string
/// remains; any branching or non-pool leaf keeps the reference unresolved.
fn single_pool_string_in_segments(
    state: &CmvsPs2aVmState,
    segments: &[CmvsMessageBufferSegment],
    depth: u32,
) -> Option<CmvsPs2aPrivateStringReference> {
    if depth >= 8 || segments.len() != 1 {
        return None;
    }
    match &segments[0] {
        CmvsMessageBufferSegment::PoolString { reference } => Some(*reference),
        CmvsMessageBufferSegment::StringSlot { index } => {
            let nested = state.message_string_slots.get(index)?;
            single_pool_string_in_segments(state, nested, depth + 1)
        }
    }
}

/// Resolves the `lstrlenA` byte length of one tagged string the way
/// `sub_470D50` dispatches the four tag domains: 0x00 private pool and 0x40
/// alternate frame pool are keyed by the active frame's payload-free length
/// table, 0x80 is the process-global 128x1024 string slot assembled from
/// message segments, and 0xC0 is the unrecovered external/root pool.
pub(super) fn resolve_tagged_string_length(
    state: &CmvsPs2aVmState,
    word: u32,
) -> Result<u32, CoreError> {
    let pool_length = |offset: u32| {
        state
            .frame_string_lengths
            .get(&state.current_frame)
            .and_then(|table| table.get(&offset))
            .copied()
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_STRING_LENGTH",
                    "CMVS string length is not known for this reference",
                )
            })
    };
    match word & 0xc000_0000 {
        0 => pool_length(word),
        0x4000_0000 => pool_length(word & 0x3fff_ffff),
        0x8000_0000 => {
            let index = word & 0x3fff_ffff;
            let mut total = 0_u32;
            let segments = state
                .message_string_slots
                .get(&index)
                .cloned()
                .unwrap_or_default();
            for segment in &segments {
                total = total
                    .checked_add(string_segment_length(state, segment, 0)?)
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_STRING_LENGTH",
                            "CMVS string slot length overflowed",
                        )
                    })?;
            }
            Ok(total)
        }
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_LENGTH",
            "CMVS external string pool length is not recovered",
        )),
    }
}

pub(super) fn string_segment_length(
    state: &CmvsPs2aVmState,
    segment: &CmvsMessageBufferSegment,
    depth: u32,
) -> Result<u32, CoreError> {
    if depth >= 8 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_LENGTH",
            "CMVS string slot nesting exceeds the recovered depth",
        ));
    }
    match segment {
        CmvsMessageBufferSegment::PoolString { reference } => state
            .frame_string_lengths
            .get(&state.current_frame)
            .and_then(|table| table.get(&reference.relative_offset))
            .copied()
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_STRING_LENGTH",
                    "CMVS string segment length is not known for this reference",
                )
            }),
        CmvsMessageBufferSegment::StringSlot { index } => {
            let nested = state
                .message_string_slots
                .get(index)
                .cloned()
                .unwrap_or_default();
            let mut total = 0_u32;
            for segment in &nested {
                total = total
                    .checked_add(string_segment_length(state, segment, depth + 1)?)
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_VM_STRING_LENGTH",
                            "CMVS string slot length overflowed",
                        )
                    })?;
            }
            Ok(total)
        }
    }
}
