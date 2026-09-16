mod pb2;
pub use pb2::*;
mod pb3;
pub use pb3::*;

use astra_emu_sdk::CoreError;
use image::RgbaImage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const PB2_MAGIC: &[u8; 4] = b"PB2A";
pub const PB3_MAGIC: &[u8; 4] = b"PB3B";
const PB2_HEADER_BYTES: usize = 0x20;
const PB2_KEY_BYTES: usize = 27;
const PB3_HEADER_BYTES: usize = 0x24;
const PB3_DECRYPT_BYTES: usize = 52;
const PB3_TAIL_KEY_BYTES: usize = 47;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
const LZSS_FRAME_BYTES: usize = 0x800;
const LZSS_INITIAL_OFFSET: usize = 0x7de;

/// Header data common to PB2 and PB3.  Pixel payload decoding is deliberately
/// separate: callers must not turn unvalidated offsets into image reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PbMetadata {
    pub version: u8,
    pub input_size: u32,
    pub image_type: u16,
    pub width: u16,
    pub height: u16,
    pub bits_per_pixel: u16,
    pub offset_1: Option<u32>,
    pub offset_2: Option<u32>,
    pub frame_count: Option<u32>,
    /// Type-specific field at PB3 offset 0x18.  It is only a subtype for
    /// variants whose decoder contract says so; callers must not generalize
    /// that meaning across the format family.
    pub auxiliary_field: Option<u32>,
}

/// Fully decoded pixels are intentionally an in-process result.  The caller
/// must keep the originating VFS URI and content identity outside this value;
/// no source name or private profile data can cross into presentation state.
pub type PbDecodedImage = RgbaImage;

/// Resolves a type-6 base image within the same host-owned VFS session.  The
/// reference is format data and must stay in-process; resolvers must reject
/// traversal, cycles, ambiguous entries and identity drift.
pub trait PbImageResolver {
    fn resolve_pb3_base(&self, reference: &str) -> Result<PbDecodedImage, CoreError>;
}

fn require_pb2_variant(source: &[u8], image_type: u16) -> Result<PbMetadata, CoreError> {
    let metadata = parse_pb2_metadata(source)?;
    if metadata.image_type != image_type {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB2_VARIANT",
            "PB2 decoder received a different image variant",
        ));
    }
    Ok(metadata)
}

fn pb2_channels(metadata: PbMetadata) -> Result<usize, CoreError> {
    let channels = usize::from(metadata.bits_per_pixel / 8);
    if !matches!(channels, 3 | 4) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB2_BPP",
            "PB2 decoder requires 24-bit or 32-bit pixels",
        ));
    }
    Ok(channels)
}

fn pb2_offset(offset: Option<u32>, message: &'static str) -> Result<usize, CoreError> {
    offset
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB2_LAYOUT", message))
        .and_then(|value| {
            usize::try_from(value).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_PB2_LAYOUT",
                    "PB2 offset exceeds platform bounds",
                )
            })
        })
}

fn checked_image_bytes(
    width: usize,
    height: usize,
    channels: usize,
    code: &'static str,
) -> Result<usize, CoreError> {
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(channels))
        .filter(|bytes| *bytes <= MAX_DECODED_BYTES)
        .ok_or_else(|| invalid(code, "PB image byte count exceeds the decoder budget"))
}

fn packed_bgr_to_rgba(
    packed: Vec<u8>,
    width: u16,
    height: u16,
    channels: usize,
    code: &'static str,
) -> Result<PbDecodedImage, CoreError> {
    let mut rgba = Vec::with_capacity(
        usize::from(width)
            .checked_mul(usize::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| invalid(code, "PB RGBA output size overflowed"))?,
    );
    for pixel in packed.chunks_exact(channels) {
        rgba.extend_from_slice(&[
            pixel[2],
            pixel[1],
            pixel[0],
            if channels == 4 { pixel[3] } else { 0xff },
        ]);
    }
    if packed.len() != usize::from(width) * usize::from(height) * channels {
        return Err(invalid(
            code,
            "PB packed image size does not match dimensions",
        ));
    }
    RgbaImage::from_raw(u32::from(width), u32::from(height), rgba)
        .ok_or_else(|| invalid(code, "PB RGBA image construction failed"))
}

fn decrypt_pb2_source(source: &[u8]) -> Result<Vec<u8>, CoreError> {
    let _ = parse_pb2_metadata(source)?;
    let mut decoded = source.to_vec();
    let key_offset = decoded.len() - PB2_KEY_BYTES;
    let key = decoded[key_offset..].to_vec();
    for index in (8..PB2_HEADER_BYTES).step_by(2) {
        decoded[index] ^= key[24];
        decoded[index] = decoded[index].wrapping_sub(key[index - 8]);
        decoded[index + 1] ^= key[25];
        decoded[index + 1] = decoded[index + 1].wrapping_sub(key[index - 7]);
    }
    Ok(decoded)
}

fn pb3_type6_base_reference(source: &[u8]) -> Result<String, CoreError> {
    const NAME_KEY: [u8; 16] = [
        0xa6, 0x75, 0xf3, 0x9c, 0xc5, 0x69, 0x78, 0xa3, 0x3e, 0xa5, 0x4f, 0x79, 0x59, 0xfe, 0x3a,
        0xc7,
    ];
    let encrypted = source.get(0x34..0x54).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_LAYOUT",
            "PB3 type-6 base reference is truncated",
        )
    })?;
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = encrypted[index] ^ NAME_KEY[index & 15];
    }
    let end = bytes.iter().position(|byte| *byte == 0).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_BASE_REFERENCE",
            "PB3 type-6 base reference is not terminated",
        )
    })?;
    if end == 0 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_BASE_REFERENCE",
            "PB3 type-6 base reference is empty",
        ));
    }
    let (name, _, had_errors) = encoding_rs::SHIFT_JIS.decode(&bytes[..end]);
    if had_errors || name.contains(['/', '\\']) || name.contains("..") {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_BASE_REFERENCE",
            "PB3 type-6 base reference is not a safe CP932 basename",
        ));
    }
    Ok(format!("{name}.pb3"))
}

fn apply_pb3_type6_overlay(
    mut base: PbDecodedImage,
    overlay: &[u8],
) -> Result<PbDecodedImage, CoreError> {
    let width = usize::try_from(base.width())
        .map_err(|_| invalid("ASTRA_EMU_CMVS_PB3_PIXELS", "PB3 width exceeds bounds"))?;
    let height = usize::try_from(base.height())
        .map_err(|_| invalid("ASTRA_EMU_CMVS_PB3_PIXELS", "PB3 height exceeds bounds"))?;
    let mut flag_offset = 8usize;
    let mut data_offset = 8usize.checked_add(pb3_offset(overlay, 0)?).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_LAYOUT",
            "PB3 overlay data offset overflowed",
        )
    })?;
    let mut flag_mask = 0x80_u8;
    let pixels: &mut [u8] = base.as_mut();
    for block_y in 0..height.div_ceil(8) {
        for block_x in 0..width.div_ceil(8) {
            let flag = *overlay.get(flag_offset).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB3_CONTROL",
                    "PB3 overlay flag stream is truncated",
                )
            })?;
            let block_width = (width - block_x * 8).min(8);
            let block_height = (height - block_y * 8).min(8);
            if flag & flag_mask == 0 {
                for y in 0..block_height {
                    let count = block_width.checked_mul(4).ok_or_else(|| {
                        invalid("ASTRA_EMU_CMVS_PB3_OUTPUT", "PB3 overlay row overflowed")
                    })?;
                    let input = overlay
                        .get(data_offset..data_offset + count)
                        .ok_or_else(|| {
                            invalid(
                                "ASTRA_EMU_CMVS_PB3_DATA",
                                "PB3 overlay pixels are truncated",
                            )
                        })?;
                    let destination = ((block_y * 8 + y) * width + block_x * 8) * 4;
                    for (index, pixel) in input.as_chunks::<4>().0.iter().enumerate() {
                        let offset = destination + index * 4;
                        pixels[offset..offset + 4]
                            .copy_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                    }
                    data_offset += count;
                }
            }
            flag_mask >>= 1;
            if flag_mask == 0 {
                flag_mask = 0x80;
                flag_offset += 1;
            }
        }
    }
    Ok(base)
}

fn decode_pb3_type5_channel(
    source: &[u8],
    bit_offset: usize,
    data_offset: usize,
    output: &mut [u8],
    channel: usize,
) -> Result<(), CoreError> {
    let mut frame = [0_u8; LZSS_FRAME_BYTES];
    let mut frame_offset = LZSS_INITIAL_OFFSET;
    let mut bit_offset = 0x54usize.checked_add(bit_offset).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_LAYOUT",
            "PB3 bit stream offset overflowed",
        )
    })?;
    let mut data_offset = 0x54usize.checked_add(data_offset).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_LAYOUT",
            "PB3 data stream offset overflowed",
        )
    })?;
    let mut bit_mask = 0x80_u8;
    let mut accumulator = 0_u8;
    let mut destination = channel;
    while destination < output.len() {
        let control = *source.get(bit_offset).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_CONTROL",
                "PB3 control stream is truncated",
            )
        })?;
        if control & bit_mask == 0 {
            let value = *source.get(data_offset).ok_or_else(|| {
                invalid("ASTRA_EMU_CMVS_PB3_DATA", "PB3 literal stream is truncated")
            })?;
            data_offset += 1;
            emit_pb3_type5_value(
                value,
                &mut frame,
                &mut frame_offset,
                &mut accumulator,
                output,
                &mut destination,
            )?;
        } else {
            let pair = source.get(data_offset..data_offset + 2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB3_DATA",
                    "PB3 back-reference stream is truncated",
                )
            })?;
            data_offset += 2;
            let value = u16::from_le_bytes(pair.try_into().expect("bounded PB3 pair"));
            let count = usize::from(value & 0x1f) + 3;
            let remaining = (output.len() - destination).div_ceil(4);
            if count > remaining {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_PB3_OUTPUT",
                    "PB3 back-reference exceeds the output channel",
                ));
            }
            let reference = usize::from(value >> 5);
            for index in 0..count {
                let value = frame[(reference + index) & (LZSS_FRAME_BYTES - 1)];
                emit_pb3_type5_value(
                    value,
                    &mut frame,
                    &mut frame_offset,
                    &mut accumulator,
                    output,
                    &mut destination,
                )?;
            }
        }
        bit_mask >>= 1;
        if bit_mask == 0 {
            bit_mask = 0x80;
            bit_offset = bit_offset.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB3_CONTROL",
                    "PB3 control offset overflowed",
                )
            })?;
        }
    }
    Ok(())
}

fn pb3_offset(source: &[u8], offset: usize) -> Result<usize, CoreError> {
    usize::try_from(le_u32(source, offset)?).map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_LAYOUT",
            "PB3 offset exceeds platform bounds",
        )
    })
}

fn pb3_channel_offset(
    source: &[u8],
    table: usize,
    channels: usize,
    channel: usize,
) -> Result<usize, CoreError> {
    let mut offset = channels.checked_mul(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_LAYOUT",
            "PB3 channel table size overflowed",
        )
    })?;
    for index in 0..channel {
        let entry = table.checked_add(index * 4).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 channel table offset overflowed",
            )
        })?;
        offset = offset
            .checked_add(pb3_offset(source, entry)?)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB3_LAYOUT", "PB3 channel offset overflowed"))?;
    }
    Ok(offset)
}

fn bgra_to_rgba(
    bgra: Vec<u8>,
    width: u16,
    height: u16,
    channels: usize,
) -> Result<PbDecodedImage, CoreError> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[
            pixel[2],
            pixel[1],
            pixel[0],
            if channels == 4 { pixel[3] } else { 255 },
        ]);
    }
    RgbaImage::from_raw(u32::from(width), u32::from(height), rgba).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_PIXELS",
            "PB3 RGBA image construction failed",
        )
    })
}

fn emit_pb3_type5_value(
    value: u8,
    frame: &mut [u8; LZSS_FRAME_BYTES],
    frame_offset: &mut usize,
    accumulator: &mut u8,
    output: &mut [u8],
    destination: &mut usize,
) -> Result<(), CoreError> {
    *accumulator = accumulator.wrapping_add(value);
    let slot = output
        .get_mut(*destination)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB3_OUTPUT", "PB3 output range is invalid"))?;
    *slot = *accumulator;
    *destination = destination
        .checked_add(4)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB3_OUTPUT", "PB3 output offset overflowed"))?;
    frame[*frame_offset] = value;
    *frame_offset = (*frame_offset + 1) & (LZSS_FRAME_BYTES - 1);
    Ok(())
}

/// CMVS PB3 protects the first 52 bytes after CPZ entry decryption with a
/// short trailer-derived transform.  The original reader mutates the complete
/// container; metadata inspection only needs this bounded prefix and rejects
/// overlapping header/trailer layouts instead of relying on mutation order.
fn decrypt_pb3_header(source: &[u8]) -> Result<[u8; PB3_DECRYPT_BYTES], CoreError> {
    if source.len() < PB3_DECRYPT_BYTES + PB3_TAIL_KEY_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_SIZE",
            "PB3 container is too small for its protected header and trailer",
        ));
    }
    let mut header: [u8; PB3_DECRYPT_BYTES] = source[..PB3_DECRYPT_BYTES]
        .try_into()
        .expect("bounded PB3 header");
    let key_offset = source.len() - 3;
    let key = u16::from_le_bytes(
        source[key_offset..key_offset + 2]
            .try_into()
            .expect("bounded PB3 trailer key"),
    );
    for offset in (8..PB3_DECRYPT_BYTES).step_by(2) {
        let word = u16::from_le_bytes(
            header[offset..offset + 2]
                .try_into()
                .expect("bounded PB3 header word"),
        ) ^ key;
        header[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
    }
    let tail_offset = source.len() - PB3_TAIL_KEY_BYTES;
    for (offset, key) in (8..PB3_DECRYPT_BYTES).zip(&source[tail_offset..]) {
        header[offset] = header[offset].wrapping_sub(*key);
    }
    Ok(header)
}

fn decrypt_pb3_source(source: &[u8]) -> Result<Vec<u8>, CoreError> {
    let _ = decrypt_pb3_header(source)?;
    let mut decoded = source.to_vec();
    let key_offset = decoded.len() - 3;
    let key = u16::from_le_bytes(
        decoded[key_offset..key_offset + 2]
            .try_into()
            .expect("bounded PB3 trailer key"),
    );
    for offset in (8..PB3_DECRYPT_BYTES).step_by(2) {
        let word = u16::from_le_bytes(
            decoded[offset..offset + 2]
                .try_into()
                .expect("bounded PB3 source word"),
        ) ^ key;
        decoded[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
    }
    let tail_offset = decoded.len() - PB3_TAIL_KEY_BYTES;
    let tail = decoded[tail_offset..].to_vec();
    for (offset, key) in (8..PB3_DECRYPT_BYTES).zip(tail) {
        decoded[offset] = decoded[offset].wrapping_sub(key);
    }
    Ok(decoded)
}

/// Decodes the bounded PB LZSS variant used by the documented PB2/PB3
/// payload layouts. Callers remain responsible for validating their variant's
/// control and data offsets before associating bytes with pixels.
pub fn decode_pb_lzss(
    source: &[u8],
    mut bit_offset: usize,
    mut data_offset: usize,
    output_bytes: usize,
) -> Result<Vec<u8>, CoreError> {
    if output_bytes > MAX_DECODED_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB_LZSS_SIZE",
            "PB LZSS output exceeds the bounded decoder policy",
        ));
    }
    let mut frame = [0_u8; LZSS_FRAME_BYTES];
    let mut frame_offset = LZSS_INITIAL_OFFSET;
    let mut bit_mask = 0x80_u8;
    let mut output = Vec::with_capacity(output_bytes);
    while output.len() < output_bytes {
        let control = *source.get(bit_offset).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB_LZSS_CONTROL",
                "PB LZSS control stream is truncated",
            )
        })?;
        if control & bit_mask == 0 {
            let literal = *source.get(data_offset).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB_LZSS_DATA",
                    "PB LZSS literal stream is truncated",
                )
            })?;
            data_offset += 1;
            output.push(literal);
            frame[frame_offset] = literal;
            frame_offset = (frame_offset + 1) & (LZSS_FRAME_BYTES - 1);
        } else {
            let pair = source.get(data_offset..data_offset + 2).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB_LZSS_DATA",
                    "PB LZSS back-reference stream is truncated",
                )
            })?;
            data_offset += 2;
            let value = u16::from_le_bytes(pair.try_into().expect("bounded pair"));
            let count = usize::from(value & 0x1f) + 3;
            if count > output_bytes - output.len() {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_PB_LZSS_OUTPUT",
                    "PB LZSS back-reference exceeds its declared output",
                ));
            }
            let reference = usize::from(value >> 5);
            for index in 0..count {
                let byte = frame[(reference + index) & (LZSS_FRAME_BYTES - 1)];
                output.push(byte);
                frame[frame_offset] = byte;
                frame_offset = (frame_offset + 1) & (LZSS_FRAME_BYTES - 1);
            }
        }
        bit_mask >>= 1;
        if bit_mask == 0 {
            bit_mask = 0x80;
            bit_offset = bit_offset.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB_LZSS_CONTROL",
                    "PB LZSS control offset overflowed",
                )
            })?;
        }
    }
    Ok(output)
}

fn validate_metadata(metadata: PbMetadata) -> Result<PbMetadata, CoreError> {
    if metadata.width == 0 || metadata.height == 0 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB_DIMENSIONS",
            "PB metadata has zero image dimensions",
        ));
    }
    if metadata.bits_per_pixel == 0
        || metadata.bits_per_pixel > 64
        || !metadata.bits_per_pixel.is_multiple_of(8)
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB_BPP",
            "PB metadata has an unsupported bits-per-pixel value",
        ));
    }
    if u64::from(metadata.width) * u64::from(metadata.height) > MAX_PIXELS {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB_PIXELS",
            "PB image dimensions exceed the bounded decoder policy",
        ));
    }
    Ok(metadata)
}

fn le_u16(source: &[u8], offset: usize) -> Result<u16, CoreError> {
    let bytes = source
        .get(offset..offset + 2)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB_HEADER", "PB header field is truncated"))?;
    Ok(u16::from_le_bytes(
        bytes.try_into().expect("bounded 2-byte slice"),
    ))
}

fn le_u32(source: &[u8], offset: usize) -> Result<u32, CoreError> {
    let bytes = source
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB_HEADER", "PB header field is truncated"))?;
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("bounded 4-byte slice"),
    ))
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests;
