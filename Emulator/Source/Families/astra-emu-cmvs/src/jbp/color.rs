use super::*;

pub(super) fn inverse_dct(
    values: &mut [i16; 64],
    quantization: &[i16; 64],
) -> Result<(), CoreError> {
    for row in 0..8 {
        let p = row;
        let q = row;
        if values[p + 0x08] == 0
            && values[p + 0x10] == 0
            && values[p + 0x18] == 0
            && values[p + 0x20] == 0
            && values[p + 0x28] == 0
            && values[p + 0x30] == 0
            && values[p + 0x38] == 0
        {
            let value = wrap_i16(i64::from(values[p]) * i64::from(quantization[q]));
            for index in (0..64).step_by(8) {
                values[p + index] = value;
            }
        } else {
            let c = i64::from(quantization[q + 0x10]) * i64::from(values[p + 0x10]);
            let d = i64::from(quantization[q + 0x30]) * i64::from(values[p + 0x30]);
            let x = ((c + d) * 35467) >> 16;
            let c = ((c * 50159) >> 16) + x;
            let d = ((d * -121094) >> 16) + x;
            let a = i64::from(values[p]) * i64::from(quantization[q]);
            let b = i64::from(values[p + 0x20]) * i64::from(quantization[q + 0x20]);
            let w = a + b + c;
            let x = a + b - c;
            let y = a - b + d;
            let z = a - b - d;
            let c = i64::from(quantization[q + 0x38]) * i64::from(values[p + 0x38]);
            let d = i64::from(quantization[q + 0x28]) * i64::from(values[p + 0x28]);
            let a = i64::from(quantization[q + 0x18]) * i64::from(values[p + 0x18]);
            let b = i64::from(quantization[q + 0x08]) * i64::from(values[p + 0x08]);
            let n = ((a + b + c + d) * 77062) >> 16;
            let u =
                n + ((c * 19571) >> 16) + (((c + a) * -128553) >> 16) + (((c + b) * -58980) >> 16);
            let v =
                n + ((d * 134553) >> 16) + (((d + b) * -25570) >> 16) + (((d + a) * -167963) >> 16);
            let t =
                n + ((b * 98390) >> 16) + (((d + b) * -25570) >> 16) + (((c + b) * -58980) >> 16);
            let s = n
                + ((a * 201373) >> 16)
                + (((c + a) * -128553) >> 16)
                + (((d + a) * -167963) >> 16);
            values[p] = wrap_i16(w + t);
            values[p + 0x38] = wrap_i16(w - t);
            values[p + 0x08] = wrap_i16(y + s);
            values[p + 0x30] = wrap_i16(y - s);
            values[p + 0x10] = wrap_i16(z + v);
            values[p + 0x28] = wrap_i16(z - v);
            values[p + 0x18] = wrap_i16(x + u);
            values[p + 0x20] = wrap_i16(x - u);
        }
    }
    for row in 0..8 {
        let p = row * 8;
        let a = i64::from(values[p]);
        let c = i64::from(values[p + 2]);
        let b = i64::from(values[p + 4]);
        let d = i64::from(values[p + 6]);
        let x = ((c + d) * 35467) >> 16;
        let c = ((c * 50159) >> 16) + x;
        let d = ((d * -121094) >> 16) + x;
        let w = a + b + c;
        let x = a + b - c;
        let y = a - b + d;
        let z = a - b - d;
        let d = i64::from(values[p + 5]);
        let b = i64::from(values[p + 1]);
        let c = i64::from(values[p + 7]);
        let a = i64::from(values[p + 3]);
        let n = ((a + b + c + d) * 77062) >> 16;
        let s =
            n + ((a * 201373) >> 16) + (((a + c) * -128553) >> 16) + (((a + d) * -167963) >> 16);
        let t = n + ((b * 98390) >> 16) + (((b + c) * -58980) >> 16) + (((b + d) * -25570) >> 16);
        let u = n + ((c * 19571) >> 16) + (((b + c) * -58980) >> 16) + (((a + c) * -128553) >> 16);
        let v = n + ((d * 134553) >> 16) + (((b + d) * -25570) >> 16) + (((a + d) * -167963) >> 16);
        values[p] = wrap_i16((w + t) >> 3);
        values[p + 7] = wrap_i16((w - t) >> 3);
        values[p + 1] = wrap_i16((y + s) >> 3);
        values[p + 6] = wrap_i16((y - s) >> 3);
        values[p + 2] = wrap_i16((z + v) >> 3);
        values[p + 5] = wrap_i16((z - v) >> 3);
        values[p + 3] = wrap_i16((x + u) >> 3);
        values[p + 4] = wrap_i16((x - u) >> 3);
    }
    Ok(())
}

pub(super) fn ycc_to_bgr(
    output: &mut [u8],
    stride: usize,
    mut dc: usize,
    mut ac: usize,
    planes: [&[i16; 64]; 3],
    mut chroma: usize,
) -> Result<(), CoreError> {
    let mut y_source = 0_usize;
    for _ in 0..4 {
        for _ in 0..4 {
            let cb_value = i64::from(planes[1][chroma]);
            let cr_value = i64::from(planes[2][chroma]);
            let red = (cr_value * 0x166f0) >> 16;
            let green = ((cb_value * 0x5810) >> 16) + ((cr_value * 0xb6c0) >> 16);
            let blue = (cb_value * 0x1c590) >> 16;
            write_bgr(
                output,
                dc,
                i64::from(planes[0][y_source]) + 0x180,
                blue,
                green,
                red,
            )?;
            write_bgr(
                output,
                ac + 4 - stride,
                i64::from(planes[0][y_source + 1]) + 0x180,
                blue,
                green,
                red,
            )?;
            write_bgr(
                output,
                ac,
                i64::from(planes[0][y_source + 8]) + 0x180,
                blue,
                green,
                red,
            )?;
            write_bgr(
                output,
                ac + 4,
                i64::from(planes[0][y_source + 9]) + 0x180,
                blue,
                green,
                red,
            )?;
            y_source += 2;
            dc += 8;
            ac += 8;
            chroma += 1;
        }
        dc = dc
            .checked_add(stride * 2 - 32)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_OUTPUT", "JBP row offset overflowed"))?;
        ac = ac
            .checked_add(stride * 2 - 32)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_OUTPUT", "JBP row offset overflowed"))?;
        y_source += 8;
        chroma += 4;
    }
    Ok(())
}

pub(super) fn write_bgr(
    output: &mut [u8],
    offset: usize,
    y: i64,
    blue: i64,
    green: i64,
    red: i64,
) -> Result<(), CoreError> {
    let pixel = output.get_mut(offset..offset + 3).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_JBP_OUTPUT",
            "JBP pixel write exceeds output",
        )
    })?;
    pixel[0] = clamp(y + blue);
    pixel[1] = clamp(y - green);
    pixel[2] = clamp(y + red);
    Ok(())
}

pub(super) fn apply_alpha(
    source: &[u8],
    mut offset: usize,
    end: usize,
    output: &mut [u8],
) -> Result<(), CoreError> {
    if offset > end || end > source.len() {
        return Err(invalid(
            "ASTRA_EMU_CMVS_JBP_ALPHA",
            "JBP alpha range exceeds the source",
        ));
    }
    let mut pixel_index = 0usize;
    while pixel_index < output.len() / 4 {
        let alpha = *source
            .get(offset)
            .filter(|_| offset < end)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_ALPHA", "JBP alpha stream is truncated"))?;
        offset += 1;
        if alpha != 0 && alpha != 0xff {
            output[pixel_index * 4 + 3] = alpha;
            pixel_index += 1;
            continue;
        }
        let count =
            usize::from(*source.get(offset).filter(|_| offset < end).ok_or_else(|| {
                invalid("ASTRA_EMU_CMVS_JBP_ALPHA", "JBP alpha run is truncated")
            })?);
        offset += 1;
        if count == 0 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_ALPHA",
                "JBP alpha run is empty",
            ));
        }
        if count > output.len() / 4 - pixel_index {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_ALPHA",
                "JBP alpha run exceeds its image",
            ));
        }
        for pixel in output[pixel_index * 4..(pixel_index + count) * 4]
            .as_chunks_mut::<4>()
            .0
        {
            pixel[3] = alpha;
        }
        pixel_index += count;
    }
    Ok(())
}
