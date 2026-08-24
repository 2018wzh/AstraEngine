use astra_emu_family_api::LegacySurfaceFormatV9;

pub fn copy_surface_to_straight_rgba8(
    pixels: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    format: LegacySurfaceFormatV9,
) -> Result<Vec<u8>, &'static str> {
    let row_bytes = width
        .checked_mul(4)
        .ok_or("ASTRA_EMU_SURFACE_PIXELS_BOUNDS")?;
    let expected = usize::try_from(u64::from(stride) * u64::from(height))
        .map_err(|_| "ASTRA_EMU_SURFACE_PIXELS_BOUNDS")?;
    if stride < row_bytes || pixels.len() != expected {
        return Err("ASTRA_EMU_SURFACE_PIXELS_LENGTH");
    }
    let output_len = usize::try_from(u64::from(row_bytes) * u64::from(height))
        .map_err(|_| "ASTRA_EMU_SURFACE_PIXELS_BOUNDS")?;
    let mut output = Vec::with_capacity(output_len);
    for y in 0..height {
        let start = usize::try_from(u64::from(y) * u64::from(stride))
            .map_err(|_| "ASTRA_EMU_SURFACE_PIXELS_BOUNDS")?;
        let end = start
            .checked_add(row_bytes as usize)
            .ok_or("ASTRA_EMU_SURFACE_PIXELS_BOUNDS")?;
        let row = pixels
            .get(start..end)
            .ok_or("ASTRA_EMU_SURFACE_PIXELS_LENGTH")?;
        match format {
            LegacySurfaceFormatV9::Rgba8SrgbPremultiplied => output.extend_from_slice(row),
            LegacySurfaceFormatV9::Bgra8SrgbPremultiplied => output.extend(
                row.as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]]),
            ),
        }
    }
    for pixel in output.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_stride_converts_bgra_and_unpremultiplies() {
        let pixels = [10, 20, 30, 40, 0, 0, 0, 0, 50, 60, 70, 80, 0, 0, 0, 0];
        let rgba = copy_surface_to_straight_rgba8(
            &pixels,
            1,
            2,
            8,
            LegacySurfaceFormatV9::Bgra8SrgbPremultiplied,
        )
        .unwrap();
        assert_eq!(rgba, [191, 128, 64, 40, 223, 191, 159, 80]);
    }

    #[test]
    fn rejects_short_rows() {
        assert_eq!(
            copy_surface_to_straight_rgba8(
                &[0; 4],
                2,
                1,
                4,
                LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
            ),
            Err("ASTRA_EMU_SURFACE_PIXELS_LENGTH")
        );
    }
}
