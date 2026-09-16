use super::*;

pub fn parse_ps2a(source: &[u8]) -> Result<CmvsScript, CoreError> {
    let header = parse_header(source)?;
    let decoded = decode_ps2a(source, header)?;
    let name_index_bytes = checked_usize(header.name_index_count)?
        .checked_mul(4)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PS2A_INDEX", "PS2A name index overflowed"))?;
    let bytecode_start = HEADER_BYTES.checked_add(name_index_bytes).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_OFFSET",
            "PS2A bytecode offset overflowed",
        )
    })?;
    let name_index = parse_name_index(&decoded, header.name_index_count, bytecode_start)?;
    let bytecode_end = bytecode_start
        .checked_add(checked_usize(header.bytecode_size)?)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_OFFSET",
                "PS2A bytecode range overflowed",
            )
        })?;
    if bytecode_end > decoded.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_LAYOUT",
            "PS2A sections exceed decoded script bytes",
        ));
    }
    // `sub_4781B0` publishes the runtime data-segment base at
    // `this+15100 = bytecode_end` with the string pool at `this+15116`,
    // header-declared sizes apart, so the pool cannot start before the
    // declared segment ends.
    let strings_start = bytecode_end
        .checked_add(checked_usize(header.metadata_size)?)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_OFFSET",
                "PS2A data-segment range overflowed",
            )
        })?;
    if strings_start > decoded.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_LAYOUT",
            "PS2A sections exceed decoded script bytes",
        ));
    }
    let strings = parse_strings(&decoded[strings_start..], strings_start)?;
    Ok(CmvsScript {
        schema: "astra.emu.cmvs.ps2a_script.v1".into(),
        header,
        name_index,
        bytecode_offset: u32::try_from(bytecode_start).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_OFFSET",
                "PS2A bytecode offset is too large",
            )
        })?,
        bytecode: decoded[bytecode_start..bytecode_end].to_vec(),
        data_segment: decoded[bytecode_end..strings_start].to_vec(),
        string_pool: decoded[strings_start..].to_vec(),
        strings,
        decoded_size: u32::try_from(decoded.len()).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_SIZE",
                "PS2A decoded script is too large",
            )
        })?,
    })
}
