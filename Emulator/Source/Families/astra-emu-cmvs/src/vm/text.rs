use super::*;
use std::borrow::Cow;

#[cfg(test)]
mod tests;

pub(super) fn tag_zero_reference(value: u32) -> Result<CmvsPs2aPrivateStringReference, CoreError> {
    // This entry point accepts only the active frame's tag-zero pool.
    // Commands with other recovered domains use their explicit resolver.
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

/// Native global slots own bytes; resolving never follows mutable aliases.
pub(super) fn resolve_tagged_string_bytes(
    state: &CmvsPs2aVmState,
    word: u32,
) -> Result<Cow<'_, [u8]>, CoreError> {
    match word & 0xc000_0000 {
        0 => state
            .frame_string_values
            .get(&state.current_frame)
            .and_then(|pool| pool.get(&(word & 0x3fff_ffff)))
            .map(|value| Cow::Borrowed(value.as_slice()))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_STRING_POOL",
                    "string pool is not bound or offset is invalid",
                )
            }),
        0x4000_0000 => data_segment_string(state, word & 0x3fff_ffff).map(Cow::Owned),
        0x8000_0000 if word & 0x3fff_ffff < MAX_PROCESS_STRING_SLOTS => Ok(Cow::Borrowed(
            state
                .process_string_slots
                .get(&(word & 0x3fff_ffff))
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_TAG",
            "string tag or slot is outside the supported native domain",
        )),
    }
}

pub(super) fn validate_native_string_slot(bytes: &[u8]) -> Result<(), CoreError> {
    if bytes.len() >= 1024 || bytes.contains(&0) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_BOUND",
            "string exceeds native slot capacity or contains NUL",
        ));
    }
    let (_, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
    if malformed {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_ENCODING",
            "native string contains invalid Shift-JIS",
        ));
    }
    Ok(())
}

pub(super) fn resolve_resource_name(
    state: &CmvsPs2aVmState,
    value: u32,
) -> Result<String, CoreError> {
    let bytes = resolve_tagged_string_bytes(state, value)?;
    if bytes.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_SCRIPT_NAME",
            "script name is empty",
        ));
    }
    let (text, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&bytes);
    if malformed || bytes.contains(&0) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_ENCODING",
            "resource name contains invalid Shift-JIS",
        ));
    }
    Ok(text.into_owned())
}

pub(super) fn resolve_tagged_string_length(
    state: &CmvsPs2aVmState,
    word: u32,
) -> Result<u32, CoreError> {
    match word & 0xc000_0000 {
        0 => state
            .frame_string_lengths
            .get(&state.current_frame)
            .and_then(|table| table.get(&(word & 0x3fff_ffff)))
            .copied()
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_VM_STRING_LENGTH",
                    "string length is not bound",
                )
            }),
        0x4000_0000 | 0x8000_0000 => Ok(resolve_tagged_string_bytes(state, word)?.len() as u32),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_TAG",
            "external string pool is not bound",
        )),
    }
}

fn data_segment_string(state: &CmvsPs2aVmState, start: u32) -> Result<Vec<u8>, CoreError> {
    let size = state
        .script_data_segment_sizes
        .get(&state.current_frame)
        .copied()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_VM_STRING_POOL",
                "script data segment is not bound",
            )
        })?;
    if start >= size || size as usize > MAX_SCRIPT_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_STRING_BOUND",
            "data string offset is outside the segment",
        ));
    }
    let complete = size & !3;
    let words = state.script_data_segment_words.get(&state.current_frame);
    let tail = state.script_data_segment_tail.get(&state.current_frame);
    let mut bytes = Vec::new();
    for offset in start..size {
        let byte = if offset < complete {
            let word = words
                .and_then(|words| words.get(&(offset & !3)))
                .copied()
                .unwrap_or(0);
            word.to_le_bytes()[(offset & 3) as usize]
        } else {
            *tail
                .and_then(|tail| tail.get((offset - complete) as usize))
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_VM_STRING_POOL",
                        "script data tail is not bound",
                    )
                })?
        };
        if byte == 0 {
            return Ok(bytes);
        }
        bytes.push(byte);
    }
    Err(invalid(
        "ASTRA_EMU_CMVS_VM_STRING_BOUND",
        "data string has no terminator inside its segment",
    ))
}
