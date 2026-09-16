use super::*;

pub(crate) fn decode_jbp(
    source: &[u8],
    jbp_offset: usize,
    width: u16,
    height: u16,
    bits_per_pixel: u16,
    bounds: JbpBounds,
) -> Result<PbDecodedImage, CoreError> {
    if !matches!(bits_per_pixel, 24 | 32) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_JBP_BPP",
            "JBP requires 24-bit or 32-bit pixels",
        ));
    }
    if bounds.payload_end > source.len() || bounds.payload_end < jbp_offset {
        return Err(invalid(
            "ASTRA_EMU_CMVS_JBP_LAYOUT",
            "JBP payload boundary is invalid",
        ));
    }
    let bitstream_end = match (bits_per_pixel, bounds.alpha.as_ref()) {
        (32, Some(alpha)) if alpha.start <= alpha.end && alpha.end <= source.len() => alpha.start,
        (32, Some(_)) => {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_LAYOUT",
                "JBP alpha boundary exceeds its container",
            ));
        }
        _ => bounds.payload_end,
    };
    let mut decoder = JbpDecoder::new(source, jbp_offset, bitstream_end)?;
    let bgra = decoder.decode()?;
    let source_stride = decoder.stride;
    let width = usize::from(width);
    let height = usize::from(height);
    let destination_stride = width.checked_mul(4).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_JBP_OUTPUT",
            "JBP destination stride overflowed",
        )
    })?;
    let cropped_len = destination_stride.checked_mul(height).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_JBP_OUTPUT",
            "JBP destination size overflowed",
        )
    })?;
    let mut cropped = vec![0_u8; cropped_len];
    for row in 0..height {
        let source_start = row
            .checked_mul(source_stride)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_OUTPUT", "JBP source row overflowed"))?;
        let destination_start = row.checked_mul(destination_stride).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_JBP_OUTPUT",
                "JBP destination row overflowed",
            )
        })?;
        cropped[destination_start..destination_start + destination_stride]
            .copy_from_slice(&bgra[source_start..source_start + destination_stride]);
    }
    if bits_per_pixel == 32 {
        if let Some(alpha) = bounds.alpha {
            apply_alpha(source, alpha.start, alpha.end, &mut cropped)?;
        } else {
            for pixel in cropped.as_chunks_mut::<4>().0 {
                pixel[3] = 0xff;
            }
        }
    } else {
        for pixel in cropped.as_chunks_mut::<4>().0 {
            pixel[3] = 0xff;
        }
    }
    for pixel in cropped.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    image::RgbaImage::from_raw(
        u32::try_from(width)
            .map_err(|_| invalid("ASTRA_EMU_CMVS_JBP_OUTPUT", "JBP width exceeds bounds"))?,
        u32::try_from(height)
            .map_err(|_| invalid("ASTRA_EMU_CMVS_JBP_OUTPUT", "JBP height exceeds bounds"))?,
        cropped,
    )
    .ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_JBP_OUTPUT",
            "JBP RGBA image construction failed",
        )
    })
}
