use super::*;
use crate::CmvsNativeTextOperation;

/// Native text-object parameters. Font names and source text stay inside the
/// Family. CPU/GPU font handles never enter a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeTextSurface {
    pub font: String,
    pub capacity_hint: u32,
    pub rect: [i32; 4],
    pub cursor: [i32; 2],
    pub size: u32,
    pub character_spacing: u32,
    pub line_spacing: u32,
    pub colors: [u32; 2],
    pub kinsoku: bool,
    pub style: u32,
    pub color_mode: u32,
}

impl CmvsNativeTextSurface {
    fn new(font: String, capacity_hint: u32) -> Self {
        Self {
            font,
            capacity_hint,
            rect: [0; 4],
            cursor: [0; 2],
            size: 0,
            character_spacing: 0,
            line_spacing: 0,
            colors: [0x00ff_ffff; 2],
            kinsoku: true,
            style: 2,
            color_mode: 0,
        }
    }
}

/// Product projection must handle these operations synchronously. A Family
/// without its font/renderer binding rejects the action and closes the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsNativeTextAction {
    Create { slot: u8 },
    Destroy { slot: u8 },
    Clear { slot: u8 },
    SetFont { slot: u8 },
    DrawStatic { slot: u8, source: String },
}

fn font_name(state: &CmvsPs2aVmState, value: u32) -> Result<String, CoreError> {
    let bytes = text::resolve_tagged_string_bytes(state, value)?;
    if bytes.len() >= 64 || bytes.contains(&0) {
        return Err(invalid(
            "ASTRA_EMU_CMVS_TEXT_FONT",
            "native font name exceeds its buffer",
        ));
    }
    text::resolve_string_value(state, value)
}

fn allocate(state: &mut CmvsPs2aVmState, table: u32, slot: u8) -> Result<(), CoreError> {
    let id = state.next_slot_object_id;
    state.next_slot_object_id = id.checked_add(1).ok_or_else(|| {
        invalid(
            "ASTRA_EMU_CMVS_TEXT_OBJECT",
            "native text object identity exhausted",
        )
    })?;
    state.slot_objects.insert(table << 8 | u32::from(slot), id);
    Ok(())
}

pub(super) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    let fail = || {
        invalid(
            "ASTRA_EMU_CMVS_TEXT_OBJECT",
            "native text snapshot has inconsistent owners or fields",
        )
    };
    for (slot, font) in &state.native_text_fonts {
        let (bytes, _, invalid_encoding) = encoding_rs::SHIFT_JIS.encode(font);
        if *slot >= 12
            || invalid_encoding
            || bytes.len() >= 64
            || bytes.contains(&0)
            || !state
                .slot_objects
                .contains_key(&(762 << 8 | u32::from(*slot)))
        {
            return Err(fail());
        }
    }
    for (slot, surface) in &state.native_text {
        if *slot >= 12
            || state.native_text_fonts.get(slot) != Some(&surface.font)
            || !state
                .slot_objects
                .contains_key(&(750 << 8 | u32::from(*slot)))
            || surface.colors.iter().any(|color| color & 0xff000000 != 0)
            || surface.size > i32::MAX as u32
            || surface.character_spacing > i32::MAX as u32
            || surface.line_spacing > i32::MAX as u32
        {
            return Err(fail());
        }
    }
    Ok(())
}

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    operation: CmvsNativeTextOperation,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    use CmvsNativeTextOperation::*;
    let expected = match operation {
        Create | Colors | Cursor => 3,
        Rect => 5,
        Metrics => 4,
        Destroy | Clear => 1,
        Font | Kinsoku | DrawStatic => 2,
    };
    if values.len() != expected {
        return Err(invalid(
            "ASTRA_EMU_CMVS_VM_COMMAND_CONTRACT",
            "native text argument count differs",
        ));
    }
    // Native string resolution precedes the slot guard in these handlers.
    let string = match operation {
        Create | Font => Some(font_name(state, values[1].1)?),
        DrawStatic => Some(text::resolve_string_value(state, values[1].1)?),
        _ => None,
    };
    let index = values[0].1;
    if index >= 12 {
        raise_error_flag(state, 10532, 0x1000)?;
        return Ok(None);
    }
    let slot = index as u8;
    let key = 750 << 8 | index;
    let action = match operation {
        Create => {
            let font = string.unwrap();
            if !state.slot_objects.contains_key(&(762 << 8 | index)) {
                allocate(state, 762, slot)?;
            }
            allocate(state, 750, slot)?;
            state.native_text_fonts.insert(slot, font.clone());
            state
                .native_text
                .insert(slot, CmvsNativeTextSurface::new(font, values[2].1));
            state.effect_playback.insert(
                slot,
                CmvsEffectPlaybackRecord {
                    value: 12,
                    ..Default::default()
                },
            );
            state.interpreter_words.insert(81220, 1);
            Some(CmvsNativeTextAction::Create { slot })
        }
        Destroy => {
            let existed = state.slot_objects.remove(&key).is_some();
            state.native_text.remove(&slot);
            state.effect_playback.remove(&slot);
            existed.then_some(CmvsNativeTextAction::Destroy { slot })
        }
        _ => {
            if !state.slot_objects.contains_key(&key) {
                raise_error_flag(state, 10532, 0x1000)?;
                return Ok(None);
            }
            let surface = state.native_text.get_mut(&slot).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_CMVS_TEXT_OBJECT",
                    "native text object is not initialized",
                )
            })?;
            match operation {
                Rect => {
                    surface.rect = [
                        values[1].1 as i32,
                        values[2].1 as i32,
                        values[3].1 as i32,
                        values[4].1 as i32,
                    ];
                    surface.cursor = [surface.rect[0], surface.rect[1]];
                    None
                }
                Clear => {
                    surface.cursor = [surface.rect[0], surface.rect[1]];
                    Some(CmvsNativeTextAction::Clear { slot })
                }
                Metrics => {
                    if values[1].1 as i32 > 0 {
                        surface.size = values[1].1;
                    }
                    if values[2].1 as i32 >= 0 {
                        surface.character_spacing = values[2].1;
                    }
                    if values[3].1 as i32 >= 0 {
                        surface.line_spacing = values[3].1;
                    }
                    None
                }
                Font => {
                    let font = string.unwrap();
                    surface.font.clone_from(&font);
                    state.native_text_fonts.insert(slot, font);
                    Some(CmvsNativeTextAction::SetFont { slot })
                }
                Colors => {
                    for (color, value) in surface.colors.iter_mut().zip(&values[1..]) {
                        if value.1 & 0xff00_0000 == 0 {
                            *color = value.1;
                        }
                    }
                    None
                }
                Kinsoku => {
                    surface.kinsoku = values[1].1 != 0;
                    None
                }
                Cursor => {
                    surface.cursor = [values[1].1 as i32, values[2].1 as i32];
                    None
                }
                DrawStatic => Some(CmvsNativeTextAction::DrawStatic {
                    slot,
                    source: string.unwrap(),
                }),
                Create | Destroy => unreachable!(),
            }
        }
    };
    Ok(action.map(CmvsPs2aVmAction::NativeText))
}

#[cfg(test)]
mod tests;
