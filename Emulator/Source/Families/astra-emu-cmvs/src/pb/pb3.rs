use super::*;

pub fn parse_pb3_metadata(source: &[u8]) -> Result<PbMetadata, CoreError> {
    let header = decrypt_pb3_header(source)?;
    let metadata_header = &header[..PB3_HEADER_BYTES];
    if &metadata_header[..4] != PB3_MAGIC {
        return Err(invalid("ASTRA_EMU_CMVS_PB3_MAGIC", "PB3 magic is invalid"));
    }
    let metadata = PbMetadata {
        version: 3,
        input_size: le_u32(metadata_header, 4)?,
        image_type: le_u16(metadata_header, 0x1c)?,
        width: le_u16(metadata_header, 0x1e)?,
        height: le_u16(metadata_header, 0x20)?,
        bits_per_pixel: le_u16(metadata_header, 0x22)?,
        offset_1: None,
        offset_2: None,
        frame_count: None,
        auxiliary_field: Some(le_u32(metadata_header, 0x18)?),
    };
    if !matches!(metadata.image_type, 2 | 3)
        && usize::try_from(metadata.input_size)
            .ok()
            .is_none_or(|size| size > source.len())
    {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_INPUT_SIZE",
            "PB3 input size exceeds its container",
        ));
    }
    validate_metadata(metadata)
}

/// Decodes the PB3 type-5 delta/LZSS variant into RGBA pixels.
///
/// The type-5 contract is separate from metadata parsing because all other
/// PB3 variants have different stream layouts.  Unknown variants remain a
/// stable blocking error instead of being interpreted as this layout.
pub fn decode_pb3_type5(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = parse_pb3_metadata(source)?;
    if metadata.image_type != 5 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_VARIANT",
            "PB3 decoder received a non-type-5 image",
        ));
    }
    let channels = usize::from(metadata.bits_per_pixel / 8);
    if !matches!(channels, 3 | 4) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_BPP",
            "PB3 type-5 requires 24-bit or 32-bit pixels",
        ));
    }
    let decoded = decrypt_pb3_source(source)?;
    let width = usize::from(metadata.width);
    let height = usize::from(metadata.height);
    let pixel_count = width.checked_mul(height).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_PIXELS",
            "PB3 image pixel count overflowed",
        )
    })?;
    let bgra_len = pixel_count.checked_mul(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_PIXELS",
            "PB3 image byte count overflowed",
        )
    })?;
    let mut bgra = vec![0_u8; bgra_len];
    for channel in 0..4 {
        let table = 0x34usize
            .checked_add(channel * 8)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB3_LAYOUT", "PB3 table offset overflowed"))?;
        let bit_offset = usize::try_from(le_u32(&decoded, table)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 bit stream offset exceeds bounds",
            )
        })?;
        let data_offset = usize::try_from(le_u32(&decoded, table + 4)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 data stream offset exceeds bounds",
            )
        })?;
        decode_pb3_type5_channel(&decoded, bit_offset, data_offset, &mut bgra, channel)?;
    }
    let mut rgba = Vec::with_capacity(bgra_len);
    for pixel in bgra.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[
            pixel[2],
            pixel[1],
            pixel[0],
            if channels == 4 { pixel[3] } else { 255 },
        ]);
    }
    RgbaImage::from_raw(u32::from(metadata.width), u32::from(metadata.height), rgba).ok_or_else(
        || {
            invalid(
                "ASTRA_EMU_CMVS_PB3_PIXELS",
                "PB3 RGBA image construction failed",
            )
        },
    )
}

/// Decodes the PB3 type-1 block/plane LZSS variant into RGBA pixels.
pub fn decode_pb3_type1(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = parse_pb3_metadata(source)?;
    if metadata.image_type != 1 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_VARIANT",
            "PB3 decoder received a non-type-1 image",
        ));
    }
    let channels = usize::from(metadata.bits_per_pixel / 8);
    if !matches!(channels, 3 | 4) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_BPP",
            "PB3 type-1 requires 24-bit or 32-bit pixels",
        ));
    }
    let decoded = decrypt_pb3_source(source)?;
    let width = usize::from(metadata.width);
    let height = usize::from(metadata.height);
    let pixel_count = width.checked_mul(height).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_PB3_PIXELS",
            "PB3 image pixel count overflowed",
        )
    })?;
    let mut bgra = vec![
        0_u8;
        pixel_count.checked_mul(4).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_PIXELS",
                "PB3 image byte count overflowed",
            )
        })?
    ];
    let data1 = pb3_offset(&decoded, 0x2c)?;
    let data2 = pb3_offset(&decoded, 0x30)?;
    let blocks_x = width.div_ceil(16);
    let blocks_y = height.div_ceil(16);
    for channel in 0..channels {
        let channel_offset = pb3_channel_offset(&decoded, data1, channels, channel)?;
        let block_header = data1.checked_add(channel_offset).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 block header offset overflowed",
            )
        })?;
        let flags_bytes = usize::try_from(le_u32(&decoded, block_header)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 flags length exceeds bounds",
            )
        })?;
        let plane_offset = usize::try_from(le_u32(&decoded, block_header + 4)?).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 plane offset exceeds bounds",
            )
        })?;
        let plane_size = usize::try_from(le_u32(&decoded, block_header + 8)?)
            .map_err(|_| invalid("ASTRA_EMU_CMVS_PB3_LAYOUT", "PB3 plane size exceeds bounds"))?;
        if plane_size > pixel_count {
            return Err(invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 plane exceeds its image pixel budget",
            ));
        }
        let control_offset = block_header
            .checked_add(12)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB3_LAYOUT", "PB3 control offset overflowed"))?;
        let compressed_control = control_offset
            .checked_add(flags_bytes)
            .and_then(|value| value.checked_add(plane_offset))
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB3_LAYOUT", "PB3 control stream overflowed"))?;
        let compressed_data = data2
            .checked_add(pb3_channel_offset(&decoded, data2, channels, channel)?)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_PB3_LAYOUT", "PB3 data stream overflowed"))?;
        let plane = decode_pb_lzss(&decoded, compressed_control, compressed_data, plane_size)?;
        let mut plane_cursor = 0usize;
        let mut flag_offset = control_offset;
        let mut flag_mask = 0x80_u8;
        let mut constant_offset = control_offset.checked_add(flags_bytes).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 constant stream overflowed",
            )
        })?;
        for block_y in 0..blocks_y {
            for block_x in 0..blocks_x {
                let flag = *decoded.get(flag_offset).ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_PB3_CONTROL",
                        "PB3 block flag stream is truncated",
                    )
                })?;
                let block_width = (width - block_x * 16).min(16);
                let block_height = (height - block_y * 16).min(16);
                if flag & flag_mask != 0 {
                    let value = *decoded.get(constant_offset).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PB3_DATA",
                            "PB3 constant stream is truncated",
                        )
                    })?;
                    constant_offset += 1;
                    for y in 0..block_height {
                        for x in 0..block_width {
                            bgra[((block_y * 16 + y) * width + block_x * 16 + x) * 4 + channel] =
                                value;
                        }
                    }
                } else {
                    let required = block_width.checked_mul(block_height).ok_or_else(|| {
                        invalid("ASTRA_EMU_CMVS_PB3_OUTPUT", "PB3 block size overflowed")
                    })?;
                    if plane_cursor
                        .checked_add(required)
                        .is_none_or(|end| end > plane.len())
                    {
                        return Err(invalid(
                            "ASTRA_EMU_CMVS_PB3_OUTPUT",
                            "PB3 plane is shorter than its block map",
                        ));
                    }
                    for y in 0..block_height {
                        for x in 0..block_width {
                            let value = plane[plane_cursor];
                            plane_cursor += 1;
                            bgra[((block_y * 16 + y) * width + block_x * 16 + x) * 4 + channel] =
                                value;
                        }
                    }
                }
                flag_mask >>= 1;
                if flag_mask == 0 {
                    flag_mask = 0x80;
                    flag_offset = flag_offset.checked_add(1).ok_or_else(|| {
                        invalid(
                            "ASTRA_EMU_CMVS_PB3_CONTROL",
                            "PB3 block flag offset overflowed",
                        )
                    })?;
                }
            }
        }
    }
    bgra_to_rgba(bgra, metadata.width, metadata.height, channels)
}

/// Decodes the PB3 type-6 base-image overlay variant.
pub fn decode_pb3_type6(
    source: &[u8],
    resolver: &dyn PbImageResolver,
) -> Result<PbDecodedImage, CoreError> {
    let metadata = parse_pb3_metadata(source)?;
    if metadata.image_type != 6 || metadata.bits_per_pixel != 32 {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_VARIANT",
            "PB3 decoder received an unsupported type-6 layout",
        ));
    }
    let decoded = decrypt_pb3_source(source)?;
    let reference = pb3_type6_base_reference(&decoded)?;
    let base = resolver.resolve_pb3_base(&reference)?;
    if base.width() != u32::from(metadata.width) || base.height() != u32::from(metadata.height) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_BASE_DIMENSIONS",
            "PB3 type-6 base image dimensions do not match the overlay",
        ));
    }
    let bit_offset = pb3_offset(&decoded, 0x0c)?
        .checked_add(0x20)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 overlay control offset overflowed",
            )
        })?;
    let data_offset = bit_offset
        .checked_add(pb3_offset(&decoded, 0x2c)?)
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 overlay data offset overflowed",
            )
        })?;
    let overlay_size =
        usize::try_from(metadata.auxiliary_field.unwrap_or_default()).map_err(|_| {
            invalid(
                "ASTRA_EMU_CMVS_PB3_LAYOUT",
                "PB3 overlay size exceeds platform bounds",
            )
        })?;
    let overlay = decode_pb_lzss(&decoded, bit_offset, data_offset, overlay_size)?;
    apply_pb3_type6_overlay(base, &overlay)
}

/// Decodes PB3 type-2 and type-3 JBP containers. The JBP stream carries its
/// own Huffman, quantization, DCT, and optional alpha data, so it remains
/// distinct from PB LZSS variants.
pub fn decode_pb3_jbp(source: &[u8]) -> Result<PbDecodedImage, CoreError> {
    let metadata = parse_pb3_metadata(source)?;
    if !matches!(metadata.image_type, 2 | 3) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_PB3_VARIANT",
            "PB3 decoder received a non-JBP image",
        ));
    }
    let decoded = decrypt_pb3_source(source)?;
    crate::jbp::decode_pb3_jbp(
        &decoded,
        metadata.width,
        metadata.height,
        metadata.bits_per_pixel,
        decoded.len(),
    )
}
