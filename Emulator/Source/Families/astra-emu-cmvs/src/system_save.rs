//! CMVS system save (`system.dat`) container support.
//!
//! The container layout, checksum, cipher and decompression are recovered
//! from the CMVS 3.90 loader `sub_427B30` and its helpers.  The cipher key
//! string is a fixed engine constant, not case-private material.  Parsed
//! tables keep only player-progress state; pooled strings are counted but
//! never retained.

use astra_emu_sdk::CoreError;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The fixed engine key string proven by `sub_427B30`/`sub_427E80`.
const SYSTEM_SAVE_KEY: &[u8] = b"YUKO_KAWAI_PURPLE_SYSTEM_SAVE_CHECK";
/// The fixed header region length; the encrypted payload starts right after.
const SYSTEM_SAVE_HEADER_BYTES: usize = 360;
/// Decompressed byte offset of the CG table (2048 dwords).
const CG_TABLE_OFFSET: usize = 32512;
/// Decompressed byte offset of the BGM table (1024 dwords).
const BGM_TABLE_OFFSET: usize = 40704;
/// Decompressed byte offset of the pooled title strings.
const TITLE_STRINGS_OFFSET: usize = 44800;
/// The recovered flag region byte count copied by `sub_48AB00`.
pub const SYSTEM_SAVE_FLAG_BYTES: usize = 0x7F00;
/// The recovered CG table word count copied by `sub_48AB70`.
pub const SYSTEM_SAVE_CG_WORDS: usize = 2048;
/// The recovered BGM table word count copied by `sub_48AB40`.
pub const SYSTEM_SAVE_BGM_WORDS: usize = 1024;
/// The fixed number of pooled title strings copied by the loader.
pub const SYSTEM_SAVE_TITLE_SLOTS: usize = 64;
/// The bounded size accepted for a system save container.
pub const MAX_SYSTEM_SAVE_BYTES: u64 = 16 * 1024 * 1024;

/// Player-progress tables recovered from one validated system save.
///
/// Pooled strings are intentionally reduced to counts: they are textual
/// payload and must not enter snapshots, reports or packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsSystemSave {
    pub flags: Vec<u8>,
    pub cg_words: Vec<u32>,
    pub bgm_words: Vec<u32>,
    pub title_slot_count: u32,
    pub list_a_count: u32,
    pub list_b_count: u32,
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, CoreError> {
    let chunk: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
                "system save header is truncated",
            )
        })?
        .try_into()
        .unwrap();
    Ok(u16::from_le_bytes(chunk))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, CoreError> {
    let chunk: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
                "system save header is truncated",
            )
        })?
        .try_into()
        .unwrap();
    Ok(u32::from_le_bytes(chunk))
}

/// Parses and validates one `system.dat` container.
///
/// `current_identity` is the interpreter identity field the original loader
/// compares against the container's identity word; a non-zero mismatch
/// reproduces the loader's rejection.  Checksum and identity failures return
/// dedicated diagnostic codes so the host can mirror the loader's error-flag
/// behavior instead of faulting.
pub fn parse_cmvs_system_save(
    source: &[u8],
    current_identity: u32,
) -> Result<CmvsSystemSave, CoreError> {
    if source.len() < SYSTEM_SAVE_HEADER_BYTES + 2 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save is smaller than its header",
        ));
    }
    let declared = le_u32(source, 100)?;
    if declared
        .checked_add(2)
        .is_none_or(|total| total as usize != source.len())
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save length does not match its declared checksum span",
        ));
    }
    let checksum = u16::from_le_bytes(
        source[source.len() - 2..]
            .try_into()
            .expect("bounds checked above"),
    );
    let mut sum = 0_u16;
    for byte in &source[SYSTEM_SAVE_HEADER_BYTES..source.len() - 2] {
        sum = sum.wrapping_add(u16::from(*byte));
    }
    if checksum != sum {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_CHECKSUM",
            "system save checksum does not match its payload",
        ));
    }
    let identity = le_u32(source, 68)?;
    if identity != 0 && identity != current_identity {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_IDENTITY",
            "system save identity does not match the running interpreter",
        ));
    }
    if le_u16(source, 76)? != 1 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_VERSION",
            "system save uses an unrecovered container version",
        ));
    }
    let decompressed_size = le_u32(source, 92)? as usize;
    let payload_size = le_u32(source, 96)? as usize;
    let payload_end = SYSTEM_SAVE_HEADER_BYTES
        .checked_add(payload_size)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
                "system save payload range overflowed",
            )
        })?;
    if payload_end > source.len() - 2 || decompressed_size > MAX_SYSTEM_SAVE_BYTES as usize {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save payload bounds are invalid",
        ));
    }
    let rot = rotation_amount(source)?;
    let addend = source[78];
    let xor_base = source[80];
    let mut payload = source[SYSTEM_SAVE_HEADER_BYTES..payload_end].to_vec();
    for (index, byte) in payload.iter_mut().enumerate() {
        let rotated = byte.rotate_right(rot);
        let key = xor_base.wrapping_add(SYSTEM_SAVE_KEY[index % SYSTEM_SAVE_KEY.len()]);
        *byte = rotated.wrapping_sub(addend) ^ key;
        *byte = byte.wrapping_sub(64);
    }
    let decompressed = decompress_system_save(&payload, decompressed_size)?;
    if decompressed.len() < TITLE_STRINGS_OFFSET {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
            "system save payload is smaller than the recovered layout",
        ));
    }
    let flags = decompressed[..SYSTEM_SAVE_FLAG_BYTES].to_vec();
    let cg_words = read_word_table(&decompressed, CG_TABLE_OFFSET, SYSTEM_SAVE_CG_WORDS)?;
    let bgm_words = read_word_table(&decompressed, BGM_TABLE_OFFSET, SYSTEM_SAVE_BGM_WORDS)?;
    // Count the pooled strings without retaining their payload.
    let mut cursor = TITLE_STRINGS_OFFSET;
    let mut title_slot_count = 0_u32;
    for _ in 0..SYSTEM_SAVE_TITLE_SLOTS {
        let length = nul_length(&decompressed, cursor)?;
        cursor += length + 1;
        title_slot_count += 1;
    }
    let list_a_count = le_u32(source, 88)?;
    let list_b_count = le_u32(source, 84)?;
    for _ in 0..list_a_count + list_b_count {
        let length = nul_length(&decompressed, cursor)?;
        cursor += length + 1;
    }
    if cursor > decompressed.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
            "system save string pools exceed the payload",
        ));
    }
    Ok(CmvsSystemSave {
        flags,
        cg_words,
        bgm_words,
        title_slot_count,
        list_a_count,
        list_b_count,
    })
}

fn rotation_amount(source: &[u8]) -> Result<u32, CoreError> {
    let word78 = le_u16(source, 78)?;
    let word82 = le_u16(source, 82)?;
    let shift = u32::from((word78 >> 1) & 7);
    Ok(u32::from((word82 >> shift) & 3) + 2)
}

fn read_word_table(
    decompressed: &[u8],
    offset: usize,
    words: usize,
) -> Result<Vec<u32>, CoreError> {
    let end = offset
        .checked_add(words * 4)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                "system save table range overflowed",
            )
        })?
        .min(decompressed.len());
    if end < offset + words * 4 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
            "system save table lies outside the payload",
        ));
    }
    let mut table = Vec::with_capacity(words);
    for index in 0..words {
        table.push(le_u32(decompressed, offset + index * 4)?);
    }
    Ok(table)
}

fn nul_length(bytes: &[u8], start: usize) -> Result<usize, CoreError> {
    bytes
        .get(start..)
        .and_then(|slice| slice.iter().position(|byte| *byte == 0))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                "system save string pool is unterminated",
            )
        })
}

/// The recovered `sub_491600` decompressor: an LZSS variant with a
/// 1024-byte window seeded at position 976, LSB-first flag bytes, literal
/// bits and two-byte matches whose offset is assembled from the low byte and
/// the top two bits of the high byte, and whose count is `(hi & 0x3f) + 2`.
/// Decoding stops when the source is consumed, matching the original.
pub fn decompress_system_save(source: &[u8], expected_size: usize) -> Result<Vec<u8>, CoreError> {
    if expected_size > MAX_SYSTEM_SAVE_BYTES as usize {
        return Err(invalid(
            "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save decompression size is outside the bounded policy",
        ));
    }
    let mut window = [0_u8; 1024];
    let mut window_pos = 976_usize;
    let mut output = Vec::with_capacity(expected_size);
    let mut input_pos = 0_usize;
    let mut flags = 0_u16;
    macro_rules! emit {
        ($byte:expr) => {{
            if output.len() >= expected_size {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream expands beyond its declared size",
                ));
            }
            output.push($byte);
            window[window_pos] = $byte;
            window_pos = (window_pos + 1) & 0x3ff;
        }};
    }
    while input_pos < source.len() {
        flags >>= 1;
        if flags & 0x100 == 0 {
            let byte = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a flag byte",
                )
            })?;
            input_pos += 1;
            flags = u16::from(byte) | 0xff00;
        }
        if flags & 1 != 0 {
            let byte = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a literal",
                )
            })?;
            input_pos += 1;
            emit!(byte);
        } else {
            let lo = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a match offset",
                )
            })?;
            let hi = *source.get(input_pos + 1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a match length",
                )
            })?;
            input_pos += 2;
            let offset = usize::from(lo) | (usize::from(hi & 0xc0) << 2);
            let count = usize::from(hi & 0x3f) + 2;
            for step in 0..count {
                let byte = window[(offset + step) & 0x3ff];
                emit!(byte);
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a valid synthetic container around one decompressed payload.
    /// The payload is stored with an all-literal LZSS stream and the engine
    /// cipher inverted from the recovered decrypt path.
    pub(crate) fn encode_container(decompressed: &[u8], identity: u32) -> Vec<u8> {
        // All-literal LZSS: flag byte 0xff followed by up to eight literals.
        let mut plain = Vec::new();
        for chunk in decompressed.chunks(8) {
            plain.push(0xff_u8);
            plain.extend_from_slice(chunk);
        }
        let rot = 3_u32;
        let addend = 5_u8;
        let xor_base = 9_u8;
        let payload: Vec<u8> = plain
            .iter()
            .enumerate()
            .map(|(index, byte)| {
                let key = xor_base.wrapping_add(SYSTEM_SAVE_KEY[index % SYSTEM_SAVE_KEY.len()]);
                let t = byte.wrapping_add(64) ^ key;
                let t = t.wrapping_add(addend);
                t.rotate_left(rot)
            })
            .collect();
        let total = SYSTEM_SAVE_HEADER_BYTES + payload.len() + 2;
        let mut source = vec![0_u8; total];
        source[68..72].copy_from_slice(&identity.to_le_bytes());
        source[76..78].copy_from_slice(&1_u16.to_le_bytes());
        // word@78 supplies the addend byte; keep the rotation derivation
        // stable: word@78 low bits choose shift 0, word@82 provides rot-2.
        source[78] = addend;
        source[79] = 0;
        source[80] = xor_base;
        let shift = u16::from((addend >> 1) & 7);
        let word82 = (((rot - 2) & 3) as u16) << shift;
        source[82..84].copy_from_slice(&word82.to_le_bytes());
        source[84..88].copy_from_slice(&0_u32.to_le_bytes()); // list B count
        source[88..92].copy_from_slice(&0_u32.to_le_bytes()); // list A count
        source[92..96].copy_from_slice(&(decompressed.len() as u32).to_le_bytes());
        source[96..100].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        source[100..104].copy_from_slice(&((total - 2) as u32).to_le_bytes());
        source[SYSTEM_SAVE_HEADER_BYTES..SYSTEM_SAVE_HEADER_BYTES + payload.len()]
            .copy_from_slice(&payload);
        let mut sum = 0_u16;
        for byte in &source[SYSTEM_SAVE_HEADER_BYTES..total - 2] {
            sum = sum.wrapping_add(u16::from(*byte));
        }
        source[total - 2..].copy_from_slice(&sum.to_le_bytes());
        source
    }

    pub(crate) fn layout_payload() -> Vec<u8> {
        let mut payload = vec![0_u8; TITLE_STRINGS_OFFSET + SYSTEM_SAVE_TITLE_SLOTS];
        for (index, byte) in payload[..SYSTEM_SAVE_FLAG_BYTES].iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        for index in 0..SYSTEM_SAVE_CG_WORDS {
            let word = (index as u32) | 0x1000_0000;
            payload[CG_TABLE_OFFSET + index * 4..CG_TABLE_OFFSET + index * 4 + 4]
                .copy_from_slice(&word.to_le_bytes());
        }
        for index in 0..SYSTEM_SAVE_BGM_WORDS {
            let word = (index as u32) | 0x2000_0000;
            payload[BGM_TABLE_OFFSET + index * 4..BGM_TABLE_OFFSET + index * 4 + 4]
                .copy_from_slice(&word.to_le_bytes());
        }
        // Zero-filled title slots parse as 64 empty strings.
        payload
    }

    #[test]
    fn parses_a_valid_synthetic_system_save() {
        let payload = layout_payload();
        let container = encode_container(&payload, 0);
        let save = parse_cmvs_system_save(&container, 0).unwrap();
        assert_eq!(save.flags.len(), SYSTEM_SAVE_FLAG_BYTES);
        assert_eq!(save.flags[251], 0);
        assert_eq!(save.flags[250], 250);
        assert_eq!(save.cg_words.len(), SYSTEM_SAVE_CG_WORDS);
        assert_eq!(save.cg_words[3], 3 | 0x1000_0000);
        assert_eq!(save.bgm_words.len(), SYSTEM_SAVE_BGM_WORDS);
        assert_eq!(save.bgm_words[5], 5 | 0x2000_0000);
        assert_eq!(save.title_slot_count, SYSTEM_SAVE_TITLE_SLOTS as u32);
        assert_eq!(save.list_a_count, 0);
        assert_eq!(save.list_b_count, 0);
    }

    #[test]
    fn rejects_a_corrupted_checksum() {
        let payload = layout_payload();
        let mut container = encode_container(&payload, 0);
        let last = container.len() - 3;
        container[last] ^= 0xff;
        let error = parse_cmvs_system_save(&container, 0).unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_CMVS_SYSTEM_SAVE_CHECKSUM");
    }

    #[test]
    fn rejects_a_mismatched_identity() {
        let payload = layout_payload();
        let container = encode_container(&payload, 77);
        let error = parse_cmvs_system_save(&container, 7).unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_CMVS_SYSTEM_SAVE_IDENTITY");
        let save = parse_cmvs_system_save(&container, 77).unwrap();
        assert_eq!(save.cg_words.len(), SYSTEM_SAVE_CG_WORDS);
    }

    #[test]
    fn rejects_a_truncated_header() {
        let error = parse_cmvs_system_save(&[0_u8; 16], 0).unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_CMVS_SYSTEM_SAVE_BOUNDS");
    }

    #[test]
    fn decompressor_reproduces_all_literal_streams() {
        let payload = b"abcdefgh01234567".to_vec();
        let mut stream = Vec::new();
        for chunk in payload.chunks(8) {
            stream.push(0xff_u8);
            stream.extend_from_slice(chunk);
        }
        let decoded = decompress_system_save(&stream, payload.len()).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn decompressor_reproduces_window_matches() {
        // Emit eight literals, then copy them back through the window. The
        // window starts at position 976, so the literals occupy 976..984.
        let mut stream = vec![0xff_u8];
        stream.extend_from_slice(b"abcdefgh");
        // The next token needs a fresh flag byte; bit 0 selects the match.
        stream.push(0xfe_u8);
        let offset = 976_usize;
        let copies = 8_usize;
        stream.push((offset & 0xff) as u8);
        stream.push((((offset >> 2) & 0xc0) | ((copies - 2) & 0x3f)) as u8);
        let decoded = decompress_system_save(&stream, 16).unwrap();
        assert_eq!(decoded, b"abcdefghabcdefgh".to_vec());
    }
}
