use super::*;

pub(super) fn parse_header(source: &[u8]) -> Result<Ps2aHeader, CoreError> {
    if source.len() < HEADER_BYTES || &source[..4] != PS2A_MAGIC {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_MAGIC",
            "PS2A script magic is invalid",
        ));
    }
    let header_length = le_u32(source, 4)?;
    let name_index_count = le_u32(source, 0x10)?;
    let bytecode_size = le_u32(source, 0x14)?;
    let metadata_size = le_u32(source, 0x18)?;
    let name_index_size = le_u32(source, 0x1c)?;
    let compressed_size = le_u32(source, 0x24)?;
    let uncompressed_size = le_u32(source, 0x28)?;
    if header_length as usize != HEADER_BYTES
        || compressed_size == 0
        || uncompressed_size == 0
        || compressed_size as usize > source.len().saturating_sub(HEADER_BYTES)
        || uncompressed_size as usize > MAX_SCRIPT_BYTES
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_HEADER",
            "PS2A header sizes are invalid",
        ));
    }
    Ok(Ps2aHeader {
        header_length,
        program_key: le_u32(source, 0x0c)?,
        name_index_count,
        bytecode_size,
        metadata_size,
        name_index_size,
        initial_pc: le_u32(source, 0x20)?,
        compressed_size,
        uncompressed_size,
    })
}

pub(crate) fn parse_header_for_archive_key(source: &[u8]) -> Result<Ps2aHeader, CoreError> {
    parse_header(source)
}

pub(super) fn decode_payload(source: &[u8], header: Ps2aHeader) -> Result<Vec<u8>, CoreError> {
    let compressed_end = HEADER_BYTES
        .checked_add(checked_usize(header.compressed_size)?)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PS2A_SIZE",
                "PS2A compressed size overflowed",
            )
        })?;
    let mut encrypted = source[HEADER_BYTES..compressed_end].to_vec();
    let xor = ((header.program_key >> 24) + (header.program_key >> 3)) as u8;
    let shifts = (header.program_key >> 20) % 5 + 1;
    for byte in &mut encrypted {
        let value = byte.wrapping_sub(0x7c) ^ xor;
        *byte = value.rotate_right(shifts);
    }
    let expected = checked_usize(header.uncompressed_size)?;
    let mut output = vec![0; expected];
    lzss_decode(&encrypted, &mut output)?;
    let mut decoded = Vec::with_capacity(HEADER_BYTES + output.len());
    decoded.extend_from_slice(&source[..HEADER_BYTES]);
    decoded.append(&mut output);
    Ok(decoded)
}

pub(super) fn lzss_decode(source: &[u8], output: &mut [u8]) -> Result<(), CoreError> {
    let mut window = [0u8; LZSS_WINDOW_BYTES];
    let mut window_pos = 0x7dfusize;
    let mut input_pos = 0usize;
    let mut output_pos = 0usize;
    let mut flags = 0u16;
    while output_pos < output.len() {
        flags >>= 1;
        if flags & 0x100 == 0 {
            let byte = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_LZSS",
                    "PS2A compressed stream ended in flag byte",
                )
            })?;
            input_pos += 1;
            flags = u16::from(byte) | 0xff00;
        }
        if flags & 1 != 0 {
            let byte = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_LZSS",
                    "PS2A compressed stream ended in literal",
                )
            })?;
            input_pos += 1;
            write_lzss_byte(output, &mut output_pos, &mut window, &mut window_pos, byte)?;
        } else {
            let lo = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_LZSS",
                    "PS2A compressed stream ended in match offset",
                )
            })?;
            let hi = *source.get(input_pos + 1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_LZSS",
                    "PS2A compressed stream ended in match length",
                )
            })?;
            input_pos += 2;
            let offset = usize::from(lo) | (usize::from(hi & 0xe0) << 3);
            let count = usize::from(hi & 0x1f) + 2;
            for i in 0..count {
                let byte = window[(offset + i) & (LZSS_WINDOW_BYTES - 1)];
                write_lzss_byte(output, &mut output_pos, &mut window, &mut window_pos, byte)?;
            }
        }
    }
    Ok(())
}

pub(super) fn write_lzss_byte(
    output: &mut [u8],
    output_pos: &mut usize,
    window: &mut [u8; LZSS_WINDOW_BYTES],
    window_pos: &mut usize,
    byte: u8,
) -> Result<(), CoreError> {
    let slot = output.get_mut(*output_pos).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PS2A_LZSS",
            "PS2A LZSS stream expands beyond declared size",
        )
    })?;
    *slot = byte;
    *output_pos += 1;
    window[*window_pos] = byte;
    *window_pos = (*window_pos + 1) & (LZSS_WINDOW_BYTES - 1);
    Ok(())
}

pub(super) fn parse_strings(bytes: &[u8], base: usize) -> Result<Vec<CmvsScriptString>, CoreError> {
    let mut result = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|relative| cursor + relative)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_STRINGS",
                    "PS2A string pool is unterminated",
                )
            })?;
        let raw = &bytes[cursor..end];
        let (_, malformed) = SHIFT_JIS.decode_without_bom_handling(raw);
        if malformed {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PS2A_ENCODING",
                "PS2A string pool contains invalid Shift-JIS",
            ));
        }
        let text_hash: [u8; 32] = Sha256::digest(raw).into();
        result.push(CmvsScriptString {
            offset: u32::try_from(base + cursor).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_OFFSET",
                    "PS2A string offset is too large",
                )
            })?,
            byte_length: u32::try_from(raw.len())
                .map_err(|_| invalid("ASTRA_EMU_CMVS_PS2A_SIZE", "PS2A string is too large"))?,
            text_hash,
        });
        cursor = end + 1;
    }
    Ok(result)
}

pub(super) fn parse_name_index(
    decoded: &[u8],
    count: u32,
    bytecode_start: usize,
) -> Result<Vec<u32>, CoreError> {
    if bytecode_start > decoded.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PS2A_INDEX",
            "PS2A name index table exceeds decoded script bytes",
        ));
    }
    let mut entries = Vec::with_capacity(checked_usize(count)?);
    for index in 0..checked_usize(count)? {
        let offset = HEADER_BYTES
            .checked_add(index.checked_mul(4).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_INDEX",
                    "PS2A name index offset overflowed",
                )
            })?)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PS2A_INDEX",
                    "PS2A name index offset overflowed",
                )
            })?;
        entries.push(le_u32_at(decoded, offset)?);
    }
    Ok(entries)
}
