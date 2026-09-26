use super::*;

const MAX_GLYPHS: usize = 65_535;

fn failure() -> CoreError {
    invalid(
        "ASTRA_EMU_CMVS_TEXT_LAYOUT",
        "invalid native static text or layout bounds",
    )
}

type Measure<'a> = Option<&'a mut dyn FnMut(&str) -> Result<u32, CoreError>>;

/// CMVS 390's static parser (465C10), including both native advance policies.
/// Font rasterization is deliberately separate from this script layout policy.
pub(crate) fn append(
    surface: &mut CmvsNativeTextSurface,
    source: &str,
    mut measure: Measure<'_>,
) -> Result<(), CoreError> {
    if source.is_empty() {
        return Ok(());
    }
    if !(1..=4096).contains(&surface.size) || surface.rect[2] <= 0 {
        return Err(failure());
    }
    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(source);
    if invalid || bytes.len() > MAX_GLYPHS * 4 || bytes.contains(&0) {
        return Err(failure());
    }
    let mut cursor = surface.cursor;
    let mut glyphs = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                index += 1;
                let control = *bytes.get(index).ok_or_else(failure)?;
                index += 1;
                match control {
                    b'n' => newline(surface, &mut cursor)?,
                    b'c' | b't' => {}
                    b'w' => {
                        bytes.get(index).ok_or_else(failure)?;
                        index += 1;
                    }
                    b'"' | b'%' | b'@' | b'{' | b'}' => {
                        let literal = if control == b'@' { b'\\' } else { control };
                        emit(
                            surface,
                            &mut cursor,
                            &mut glyphs,
                            &[literal],
                            false,
                            &mut measure,
                        )?;
                    }
                    _ => return Err(failure()),
                }
            }
            b'{' => {
                index += 1;
                loop {
                    let byte = *bytes.get(index).ok_or_else(failure)?;
                    if byte == b'/' {
                        index += 1;
                        break;
                    }
                    let length = encoded_length(byte);
                    let value = bytes.get(index..index + length).ok_or_else(failure)?;
                    // Unlike command 266, static ruby ignores single-byte base
                    // characters and the complete annotation after the slash.
                    if length == 2 {
                        emit(surface, &mut cursor, &mut glyphs, value, true, &mut measure)?;
                    }
                    index += length;
                }
                loop {
                    let byte = *bytes.get(index).ok_or_else(failure)?;
                    if byte == b'}' {
                        index += 1;
                        break;
                    }
                    let length = encoded_length(byte);
                    bytes.get(index..index + length).ok_or_else(failure)?;
                    index += length;
                }
            }
            byte => {
                let length = encoded_length(byte);
                let value = bytes.get(index..index + length).ok_or_else(failure)?;
                emit(
                    surface,
                    &mut cursor,
                    &mut glyphs,
                    value,
                    false,
                    &mut measure,
                )?;
                index += length;
            }
        }
        if surface.glyphs.len() + glyphs.len() > MAX_GLYPHS {
            return Err(failure());
        }
    }
    surface.cursor = cursor;
    surface.glyphs.extend(glyphs);
    Ok(())
}

fn encoded_length(byte: u8) -> usize {
    if (0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte) {
        2
    } else {
        1
    }
}

fn newline(surface: &CmvsNativeTextSurface, cursor: &mut [i32; 2]) -> Result<(), CoreError> {
    cursor[0] = surface.rect[0];
    cursor[1] = i32::try_from(
        i64::from(cursor[1]) + i64::from(surface.size) + i64::from(surface.line_spacing),
    )
    .map_err(|_| failure())?;
    Ok(())
}

fn emit(
    surface: &CmvsNativeTextSurface,
    cursor: &mut [i32; 2],
    glyphs: &mut Vec<CmvsNativeTextGlyph>,
    bytes: &[u8],
    ruby_base: bool,
    measure: &mut Measure<'_>,
) -> Result<(), CoreError> {
    let code = if bytes.len() == 2 {
        u16::from_be_bytes([bytes[0], bytes[1]])
    } else {
        u16::from(bytes[0])
    };
    let text = encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(bytes)
        .ok_or_else(failure)?;
    let threshold = i64::from(surface.rect[0]) + i64::from(surface.rect[2])
        - i64::from(surface.character_spacing)
        - i64::from(surface.size);
    let prohibited = surface.kinsoku && matches!(code, 0x8141 | 0x8142 | 0x8176 | 0x8178);
    if i64::from(cursor[0]) >= threshold && !prohibited {
        newline(surface, cursor)?;
    }
    let narrow_punctuation = !ruby_base && code == 0x814a;
    let mut position = *cursor;
    if narrow_punctuation {
        position[0] = position[0]
            .checked_sub((surface.size / 5) as i32)
            .ok_or_else(failure)?;
    }
    let width = if narrow_punctuation {
        surface.size / 2 - surface.size / 5
    } else if ruby_base {
        surface.size
    } else if let Some(measure) = measure {
        measure(&text)?
    } else if bytes.len() == 1 {
        surface.size / 2
    } else {
        surface.size
    };
    glyphs.push(CmvsNativeTextGlyph {
        text: text.into_owned(),
        position,
        font: surface.font.clone(),
        size: surface.size,
        colors: surface.colors,
        style: surface.style,
        color_mode: surface.color_mode,
    });
    cursor[0] = i32::try_from(
        i64::from(cursor[0]) + i64::from(width) + i64::from(surface.character_spacing),
    )
    .map_err(|_| failure())?;
    Ok(())
}
