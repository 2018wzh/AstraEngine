use astra_emu_sdk::CoreError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MGV1_MAGIC: &[u8; 4] = b"MGV1";
const MGV1_HEADER_BYTES: usize = 32;
const MGV1_INDEX_ENTRY_BYTES: usize = 4;
const MAX_MGV1_INDEX_BYTES: usize = 64 * 1024 * 1024;

/// Bounded, container-level view of an MGV1 source.
///
/// The original `mog3x` input path copies its fixed 32-byte header, then an
/// index of `header[4] * 4` bytes, then an optional embedded Ogg byte range.
/// Fields whose rendering semantics are not yet proven remain opaque rather
/// than being named as frame or timing values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Mgv1Container {
    pub header_bytes: u32,
    pub index_entry_count: u32,
    pub index_offset: u64,
    pub index_bytes: u64,
    pub ogg_offset: u64,
    pub ogg_bytes: u64,
    pub video_offset: u64,
    pub video_bytes: u64,
    pub opaque_word_2: u32,
    pub opaque_word_5: u32,
    pub opaque_word_6: u32,
    pub opaque_word_7: u32,
}

/// Parses the MGV1 framing required to safely hand its embedded Ogg stream and
/// remaining video payload to explicitly bound media providers.
pub fn parse_mgv1(source: &[u8]) -> Result<Mgv1Container, CoreError> {
    parse_mgv1_header(source, source.len() as u64)
}

/// Returns the embedded Ogg stream after validating the complete MGV1 framing.
///
/// This is an in-process handoff for the host's explicitly bound audio/media
/// provider.  It does not assign timestamps to the opaque video payload or
/// pretend that MGV's four-byte index is a packet table.
pub fn mgv1_embedded_ogg(source: &[u8]) -> Result<&[u8], CoreError> {
    let container = parse_mgv1(source)?;
    let start = usize::try_from(container.ogg_offset).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_MGV_OGG",
            "MGV1 embedded Ogg offset exceeds platform bounds",
        )
    })?;
    let length = usize::try_from(container.ogg_bytes).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_MGV_OGG",
            "MGV1 embedded Ogg length exceeds platform bounds",
        )
    })?;
    let ogg = source
        .get(
            start..start.checked_add(length).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_MGV_OGG",
                    "MGV1 embedded Ogg range overflowed",
                )
            })?,
        )
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_MGV_OGG", "MGV1 embedded Ogg is truncated"))?;
    if ogg.is_empty() || !ogg.starts_with(b"OggS") {
        return Err(invalid(
            "ASTRA_EMU_CMVS_MGV_OGG",
            "MGV1 embedded audio is not an Ogg stream",
        ));
    }
    Ok(ogg)
}

/// Parses only the fixed MGV1 header while validating every range against the
/// caller-provided, already verified source length.
pub fn parse_mgv1_header(header: &[u8], source_len: u64) -> Result<Mgv1Container, CoreError> {
    let header = header
        .get(..MGV1_HEADER_BYTES)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_MGV_HEADER", "MGV1 header is truncated"))?;
    if &header[..4] != MGV1_MAGIC {
        return Err(invalid(
            "ASTRA_EMU_CMVS_MGV_MAGIC",
            "MGV container magic is unsupported",
        ));
    }
    let header_bytes = le_u32(header, 4)?;
    if header_bytes as usize != MGV1_HEADER_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_MGV_HEADER",
            "MGV1 header length does not match the fixed input contract",
        ));
    }
    let index_entry_count = le_u32(header, 16)?;
    let index_bytes = usize::try_from(index_entry_count)
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_MGV_INDEX",
                "MGV1 index count exceeds platform bounds",
            )
        })?
        .checked_mul(MGV1_INDEX_ENTRY_BYTES)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_MGV_INDEX", "MGV1 index size overflowed"))?;
    if index_bytes > MAX_MGV1_INDEX_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_MGV_INDEX",
            "MGV1 index exceeds the bounded media policy",
        ));
    }
    let ogg_bytes = usize::try_from(le_u32(header, 12)?).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_MGV_OGG",
            "MGV1 Ogg size exceeds platform bounds",
        )
    })?;
    let index_offset = MGV1_HEADER_BYTES;
    let ogg_offset = index_offset
        .checked_add(index_bytes)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_MGV_RANGE", "MGV1 index range overflowed"))?;
    let video_offset = ogg_offset
        .checked_add(ogg_bytes)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_MGV_RANGE", "MGV1 Ogg range overflowed"))?;
    let source_len = usize::try_from(source_len).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_MGV_RANGE",
            "MGV1 source exceeds platform bounds",
        )
    })?;
    if video_offset > source_len {
        return Err(invalid(
            "ASTRA_EMU_CMVS_MGV_RANGE",
            "MGV1 index or embedded Ogg range exceeds the source",
        ));
    }
    Ok(Mgv1Container {
        header_bytes,
        index_entry_count,
        index_offset: index_offset as u64,
        index_bytes: index_bytes as u64,
        ogg_offset: ogg_offset as u64,
        ogg_bytes: ogg_bytes as u64,
        video_offset: video_offset as u64,
        video_bytes: (source_len - video_offset) as u64,
        opaque_word_2: le_u32(header, 8)?,
        opaque_word_5: le_u32(header, 20)?,
        opaque_word_6: le_u32(header, 24)?,
        opaque_word_7: le_u32(header, 28)?,
    })
}

fn le_u32(source: &[u8], offset: usize) -> Result<u32, CoreError> {
    let bytes = source.get(offset..offset + 4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_MGV_HEADER",
            "MGV1 header field is truncated",
        )
    })?;
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("bounded 4-byte slice"),
    ))
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0_u8; 32 + 12 + 5 + 9];
        bytes[..4].copy_from_slice(MGV1_MAGIC);
        bytes[4..8].copy_from_slice(&32_u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&5_u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&3_u32.to_le_bytes());
        bytes[44..48].copy_from_slice(b"OggS");
        bytes
    }

    #[test]
    fn parses_bounded_mgv1_ranges() {
        let parsed = parse_mgv1(&fixture()).expect("fixture should parse");
        assert_eq!(parsed.index_offset, 32);
        assert_eq!(parsed.index_bytes, 12);
        assert_eq!(parsed.ogg_offset, 44);
        assert_eq!(parsed.ogg_bytes, 5);
        assert_eq!(parsed.video_offset, 49);
        assert_eq!(parsed.video_bytes, 9);
    }

    #[test]
    fn header_parser_does_not_require_payload_bytes() {
        let bytes = fixture();
        let parsed = parse_mgv1_header(&bytes[..32], bytes.len() as u64)
            .expect("header and source length should be sufficient");
        assert_eq!(parsed.ogg_offset, 44);
    }

    #[test]
    fn rejects_a_truncated_embedded_ogg_range() {
        let mut bytes = fixture();
        bytes.truncate(48);
        let error = parse_mgv1(&bytes).expect_err("range must be bounded");
        assert_eq!(error.code(), "ASTRA_EMU_CMVS_MGV_RANGE");
    }

    #[test]
    fn returns_only_the_bounded_embedded_ogg_stream() {
        let bytes = fixture();
        assert_eq!(mgv1_embedded_ogg(&bytes).unwrap(), b"OggS\0");
    }

    #[test]
    fn rejects_a_non_ogg_embedded_audio_range() {
        let mut bytes = fixture();
        bytes[44..48].copy_from_slice(b"RIFF");
        assert_eq!(
            mgv1_embedded_ogg(&bytes).unwrap_err().code(),
            "ASTRA_EMU_CMVS_MGV_OGG"
        );
    }
}
