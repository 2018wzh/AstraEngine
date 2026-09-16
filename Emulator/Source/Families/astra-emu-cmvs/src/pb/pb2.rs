use super::*;

pub fn parse_pb2_metadata(source: &[u8]) -> Result<PbMetadata, CoreError> {
    if source.len() < PB2_HEADER_BYTES + PB2_KEY_BYTES {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB2_SIZE",
            "PB2 source is truncated",
        ));
    }
    let mut header = source[..PB2_HEADER_BYTES].to_vec();
    if &header[..4] != PB2_MAGIC {
        return Err(invalid("ASTRA_EMU_CMVS_PB2_MAGIC", "PB2 magic is invalid"));
    }
    let key = &source[source.len() - PB2_KEY_BYTES..];
    for index in (8..PB2_HEADER_BYTES).step_by(2) {
        header[index] ^= key[24];
        header[index] = header[index].wrapping_sub(key[index - 8]);
        header[index + 1] ^= key[25];
        header[index + 1] = header[index + 1].wrapping_sub(key[index - 7]);
    }
    let metadata = PbMetadata {
        version: 2,
        input_size: le_u32(&header, 4)?,
        frame_count: Some(le_u32(&header, 8)?),
        image_type: le_u16(&header, 0x10)?,
        width: le_u16(&header, 0x12)?,
        height: le_u16(&header, 0x14)?,
        bits_per_pixel: le_u16(&header, 0x16)?,
        offset_1: Some(le_u32(&header, 0x18)?),
        offset_2: Some(le_u32(&header, 0x1c)?),
        auxiliary_field: None,
    };
    // GARbro records this field but PB2 decoding reads the complete container.
    // It is not a source-length contract, so treating it as one rejects valid
    // PB2 variants before their type-specific reader has validated offsets.
    validate_metadata(metadata)
}

/// Decodes the PB2 variants documented by the GARbro reference reader.
/// Unsupported PB2 variants remain blocking: PB2 type numbers do not share
/// PB3's payload contracts.
pub fn decode_pb2_image(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = parse_pb2_metadata(source)?;
    match metadata.image_type {
        1 => decode_pb2_type1(source),
        2 => decode_pb2_type2(source),
        4 => decode_pb2_jbp(source),
        6 => decode_pb2_type6(source),
        _ => Err(invalid(
            "ASTRA_EMU_CMVS_PB2_VARIANT",
            "CMVS PB2 image variant is not implemented by the reference contract",
        )),
    }
}

/// Decodes PB2 type-1 8x8 planar blocks.
pub fn decode_pb2_type1(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = require_pb2_variant(source, 1)?;
    let channels = pb2_channels(metadata)?;
    let decoded = decrypt_pb2_source(source)?;
    let width = usize::from(metadata.width);
    let height = usize::from(metadata.height);
    let stride =
        checked_image_bytes(width, height, channels, "ASTRA_EMU_CMVS_PB2_PIXELS")? / height;
    let offset_1 = pb2_offset(metadata.offset_1, "PB2 type-1 control offset is missing")?;
    let offset_2 = pb2_offset(metadata.offset_2, "PB2 type-1 data offset is missing")?;
    let block_data = decode_pb_lzss(
        &decoded,
        offset_1,
        offset_2,
        stride.checked_mul(height).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_PIXELS",
                "PB2 type-1 output size overflowed",
            )
        })?,
    )?;
    let mut output = vec![0_u8; block_data.len()];
    let mut source_cursor = 0usize;
    for channel in 0..channels {
        for block_y in 0..height.div_ceil(8) {
            let block_height = (height - block_y * 8).min(8);
            for block_x in 0..width.div_ceil(8) {
                let block_width = (width - block_x * 8).min(8);
                for y in 0..block_height {
                    for x in 0..block_width {
                        let value = *block_data.get(source_cursor).ok_or_else(|| {
                            invalid(
                                "ASTRA_EMU_CMVS_PB2_OUTPUT",
                                "PB2 type-1 block data is truncated",
                            )
                        })?;
                        source_cursor += 1;
                        output[((block_y * 8 + y) * stride)
                            + (block_x * 8 + x) * channels
                            + channel] = value;
                    }
                }
            }
        }
    }
    if source_cursor != block_data.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB2_OUTPUT",
            "PB2 type-1 block data does not exactly cover its pixels",
        ));
    }
    packed_bgr_to_rgba(
        output,
        metadata.width,
        metadata.height,
        channels,
        "ASTRA_EMU_CMVS_PB2_PIXELS",
    )
}

/// Decodes PB2 type-2 constant-or-LZSS 8x8 planar blocks.
pub fn decode_pb2_type2(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = require_pb2_variant(source, 2)?;
    let channels = pb2_channels(metadata)?;
    let decoded = decrypt_pb2_source(source)?;
    let width = usize::from(metadata.width);
    let height = usize::from(metadata.height);
    let stride =
        checked_image_bytes(width, height, channels, "ASTRA_EMU_CMVS_PB2_PIXELS")? / height;
    let base_control = pb2_offset(metadata.offset_1, "PB2 type-2 control offset is missing")?;
    let base_data = pb2_offset(metadata.offset_2, "PB2 type-2 data offset is missing")?;
    let mut output = vec![0_u8; stride * height];
    let blocks_x = width.div_ceil(8);
    let blocks_y = height.div_ceil(8);
    let mut control_header = base_control.checked_add(channels * 4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB2_LAYOUT",
            "PB2 type-2 control header overflowed",
        )
    })?;
    let mut data_header = base_data.checked_add(channels * 4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB2_LAYOUT",
            "PB2 type-2 data header overflowed",
        )
    })?;
    for channel in 0..channels {
        let control_table = base_control.checked_add(channel * 4).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 control table overflowed",
            )
        })?;
        let data_table = base_data.checked_add(channel * 4).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 data table overflowed",
            )
        })?;
        let control_delta = usize::try_from(le_u32(&decoded, control_table)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 control delta exceeds bounds",
            )
        })?;
        let data_delta = usize::try_from(le_u32(&decoded, data_table)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 data delta exceeds bounds",
            )
        })?;
        let bit_prefix = usize::try_from(le_u32(&decoded, control_header)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 bit offset exceeds bounds",
            )
        })?;
        let bit_delta = usize::try_from(le_u32(&decoded, control_header + 4)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 bit delta exceeds bounds",
            )
        })?;
        let compressed_control = control_header
            .checked_add(bit_prefix)
            .and_then(|value| value.checked_add(bit_delta))
            .and_then(|value| value.checked_add(12))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB2_LAYOUT",
                    "PB2 type-2 compressed control overflowed",
                )
            })?;
        let unpacked_size =
            usize::try_from(le_u32(&decoded, control_header + 8)?).map_err(|_| {
                invalid(
                    "ASTRA_EMU_CMVS_PB2_LAYOUT",
                    "PB2 type-2 plane size exceeds bounds",
                )
            })?;
        if unpacked_size
            > width
                .checked_mul(height)
                .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB2_PIXELS", "PB2 pixel count overflowed"))?
        {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 plane exceeds pixel budget",
            ));
        }
        let compressed_data = data_header.checked_add(data_delta).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 compressed data overflowed",
            )
        })?;
        let block_data =
            decode_pb_lzss(&decoded, compressed_control, compressed_data, unpacked_size)?;
        let mut bit_offset = control_header
            .checked_add(12)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB2_LAYOUT", "PB2 type-2 bit map overflowed"))?;
        let mut bit_mask = 0x80_u8;
        let mut literal_offset = control_header
            .checked_add(control_delta)
            .and_then(|value| value.checked_add(12))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB2_LAYOUT",
                    "PB2 type-2 literal stream overflowed",
                )
            })?;
        let mut block_cursor = 0usize;
        for block_y in 0..blocks_y {
            let block_height = (height - block_y * 8).min(8);
            for block_x in 0..blocks_x {
                let block_width = (width - block_x * 8).min(8);
                let flag = *decoded.get(bit_offset).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PB2_CONTROL",
                        "PB2 type-2 block map is truncated",
                    )
                })?;
                if flag & bit_mask != 0 {
                    let value = *decoded.get(literal_offset).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PB2_DATA",
                            "PB2 type-2 literal stream is truncated",
                        )
                    })?;
                    literal_offset += 1;
                    for y in 0..block_height {
                        for x in 0..block_width {
                            output[((block_y * 8 + y) * stride)
                                + (block_x * 8 + x) * channels
                                + channel] = value;
                        }
                    }
                } else {
                    let required = block_width.checked_mul(block_height).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PB2_OUTPUT",
                            "PB2 type-2 block size overflowed",
                        )
                    })?;
                    if block_cursor
                        .checked_add(required)
                        .is_none_or(|end| end > block_data.len())
                    {
                        return Err(invalid(
                            "ASTRA_EMU_CMVS_PB2_OUTPUT",
                            "PB2 type-2 decoded plane is truncated",
                        ));
                    }
                    for y in 0..block_height {
                        for x in 0..block_width {
                            output[((block_y * 8 + y) * stride)
                                + (block_x * 8 + x) * channels
                                + channel] = block_data[block_cursor];
                            block_cursor += 1;
                        }
                    }
                }
                bit_mask >>= 1;
                if bit_mask == 0 {
                    bit_mask = 0x80;
                    bit_offset = bit_offset.checked_add(1).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PB2_CONTROL",
                            "PB2 type-2 bit map overflowed",
                        )
                    })?;
                }
            }
        }
        if block_cursor != block_data.len() {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PB2_OUTPUT",
                "PB2 type-2 decoded plane has trailing bytes",
            ));
        }
        control_header = control_header.checked_add(control_delta).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 next control header overflowed",
            )
        })?;
        data_header = data_header.checked_add(data_delta).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-2 next data header overflowed",
            )
        })?;
    }
    packed_bgr_to_rgba(
        output,
        metadata.width,
        metadata.height,
        channels,
        "ASTRA_EMU_CMVS_PB2_PIXELS",
    )
}

/// Decodes PB2 type-4 JBP with the bounded alpha stream defined by the
/// container end.  PB2 has no standalone alpha-length field.
pub fn decode_pb2_jbp(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = require_pb2_variant(source, 4)?;
    let decoded = decrypt_pb2_source(source)?;
    let alpha_offset = pb2_offset(metadata.offset_1, "PB2 JBP alpha offset is missing")?;
    crate::jbp::decode_jbp(
        &decoded,
        PB2_HEADER_BYTES,
        metadata.width,
        metadata.height,
        metadata.bits_per_pixel,
        crate::jbp::JbpBounds {
            payload_end: decoded.len(),
            alpha: (metadata.bits_per_pixel == 32 && alpha_offset != 0)
                .then_some(alpha_offset..decoded.len()),
        },
    )
}

/// Decodes PB2 type-6 four-channel XOR planes.
pub fn decode_pb2_type6(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = require_pb2_variant(source, 6)?;
    if metadata.bits_per_pixel != 32 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB2_BPP",
            "PB2 type-6 requires 32-bit pixels",
        ));
    }
    let decoded = decrypt_pb2_source(source)?;
    let width = usize::from(metadata.width);
    let height = usize::from(metadata.height);
    let channel_size = width.checked_mul(height).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB2_PIXELS",
            "PB2 type-6 pixel count overflowed",
        )
    })?;
    let first_zero = decoded
        .iter()
        .enumerate()
        .skip(0x24)
        .find_map(|(index, byte)| (*byte == 0).then_some(index))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-6 channel table terminator is missing",
            )
        })?;
    let table = first_zero
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-6 table alignment overflowed",
            )
        })?;
    let mut channels = [[0_u8; 0]; 4].map(|_| vec![0_u8; channel_size]);
    for (channel, channel_output) in channels.iter_mut().enumerate() {
        let entry = table.checked_add(channel * 8).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-6 table offset overflowed",
            )
        })?;
        let control_delta = usize::try_from(le_u32(&decoded, entry)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-6 control delta exceeds bounds",
            )
        })?;
        let data_delta = usize::try_from(le_u32(&decoded, entry + 4)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB2_LAYOUT",
                "PB2 type-6 data delta exceeds bounds",
            )
        })?;
        let control = table
            .checked_add(0x20)
            .and_then(|value| value.checked_add(control_delta))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB2_LAYOUT",
                    "PB2 type-6 control offset overflowed",
                )
            })?;
        let data = table
            .checked_add(0x20)
            .and_then(|value| value.checked_add(data_delta))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_PB2_LAYOUT",
                    "PB2 type-6 data offset overflowed",
                )
            })?;
        *channel_output = decode_pb_lzss(&decoded, control, data, channel_size)?;
    }
    let mut rgba = vec![
        0_u8;
        channel_size.checked_mul(4).ok_or_else(|| invalid(
            "ASTRA_EMU_CMVS_PB2_PIXELS",
            "PB2 type-6 byte count overflowed"
        ))?
    ];
    for index in 0..channel_size {
        let red = channels[2][index] ^ channels[3][index];
        let green = channels[1][index] ^ red;
        let blue = channels[0][index] ^ green;
        rgba[index * 4..index * 4 + 4].copy_from_slice(&[red, green, blue, channels[3][index]]);
    }
    RgbaImage::from_raw(u32::from(metadata.width), u32::from(metadata.height), rgba).ok_or_else(
        || {
            invalid(
                "ASTRA_EMU_CMVS_PB2_PIXELS",
                "PB2 type-6 RGBA image construction failed",
            )
        },
    )
}
