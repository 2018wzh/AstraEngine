use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_MESSAGE_CONTROLS: usize = 128;
const MAX_INLINE_LOAD_DELAY_MS: u32 = 60_000;
const MAX_INLINE_LOAD_TRANSITION_MS: u32 = 60_000;
const MAX_CHARACTER_SLOT_ID: u32 = 4096;
const MAX_RESOURCE_NAME_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinoriMessageMarkup {
    pub visible_text: String,
    pub controls: Vec<MinoriMessageControl>,
}

impl MinoriMessageMarkup {
    pub fn auto_advance(&self) -> bool {
        self.controls
            .iter()
            .any(|control| matches!(control, MinoriMessageControl::AutoAdvance { .. }))
    }

    pub fn waits_for_voice(&self) -> bool {
        self.controls
            .iter()
            .any(|control| matches!(control, MinoriMessageControl::WaitForVoice { .. }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MinoriMessageControl {
    WaitForVoice {
        visible_byte_offset: u32,
    },
    AutoAdvance {
        visible_byte_offset: u32,
    },
    LoadCharacter {
        visible_byte_offset: u32,
        delay_ms: u32,
        slot_id: u32,
        resource: String,
        transition_ms: u32,
        opacity_255: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MinoriMessageMarkupError {
    #[error("ASTRA_EMU_MINORI_MESSAGE_CONTROL_TRUNCATED: message control is truncated")]
    Truncated,
    #[error("ASTRA_EMU_MINORI_MESSAGE_CONTROL_UNSUPPORTED: message control is not verified")]
    Unsupported,
    #[error("ASTRA_EMU_MINORI_MESSAGE_CONTROL_BOUNDS: message controls exceed their bounds")]
    Bounds,
    #[error("ASTRA_EMU_MINORI_MESSAGE_LOAD_SCHEMA: inline character load is malformed")]
    LoadSchema,
}

pub fn parse_minori_message_markup(
    source: &str,
) -> Result<MinoriMessageMarkup, MinoriMessageMarkupError> {
    let mut visible_text = String::with_capacity(source.len());
    let mut controls = Vec::new();
    let mut cursor = 0usize;
    while cursor < source.len() {
        let suffix = &source[cursor..];
        let character = suffix
            .chars()
            .next()
            .ok_or(MinoriMessageMarkupError::Truncated)?;
        if character != '\\' {
            visible_text.push(character);
            cursor = cursor
                .checked_add(character.len_utf8())
                .ok_or(MinoriMessageMarkupError::Bounds)?;
            continue;
        }

        let command_offset = cursor
            .checked_add(character.len_utf8())
            .ok_or(MinoriMessageMarkupError::Bounds)?;
        let command = source
            .get(command_offset..)
            .and_then(|remaining| remaining.chars().next())
            .ok_or(MinoriMessageMarkupError::Truncated)?;
        cursor = command_offset
            .checked_add(command.len_utf8())
            .ok_or(MinoriMessageMarkupError::Bounds)?;
        let visible_byte_offset =
            u32::try_from(visible_text.len()).map_err(|_| MinoriMessageMarkupError::Bounds)?;
        let control = match command {
            'v' => MinoriMessageControl::WaitForVoice {
                visible_byte_offset,
            },
            'a' => MinoriMessageControl::AutoAdvance {
                visible_byte_offset,
            },
            'x' => {
                let payload = source
                    .get(cursor..)
                    .and_then(|remaining| remaining.strip_prefix('{'))
                    .ok_or(MinoriMessageMarkupError::LoadSchema)?;
                let close = payload
                    .find('}')
                    .ok_or(MinoriMessageMarkupError::Truncated)?;
                let value = &payload[..close];
                cursor = cursor
                    .checked_add(1)
                    .and_then(|value| value.checked_add(close))
                    .and_then(|value| value.checked_add(1))
                    .ok_or(MinoriMessageMarkupError::Bounds)?;
                parse_inline_load(value, visible_byte_offset)?
            }
            _ => return Err(MinoriMessageMarkupError::Unsupported),
        };
        if controls.len() >= MAX_MESSAGE_CONTROLS {
            return Err(MinoriMessageMarkupError::Bounds);
        }
        controls.push(control);
    }
    Ok(MinoriMessageMarkup {
        visible_text,
        controls,
    })
}

fn parse_inline_load(
    payload: &str,
    visible_byte_offset: u32,
) -> Result<MinoriMessageControl, MinoriMessageMarkupError> {
    let operands = payload.split(',').collect::<Vec<_>>();
    if !(4..=6).contains(&operands.len()) || operands[0] != "load" {
        return Err(MinoriMessageMarkupError::LoadSchema);
    }
    let delay_ms = parse_bounded_u32(operands[1], MAX_INLINE_LOAD_DELAY_MS)?;
    let signed_slot = operands[2]
        .parse::<i32>()
        .ok()
        .filter(|slot| *slot != 0 && slot.unsigned_abs() <= MAX_CHARACTER_SLOT_ID)
        .ok_or(MinoriMessageMarkupError::LoadSchema)?;
    validate_resource_name(operands[3])?;
    let transition_ms = operands.get(4).map_or(Ok(0), |value| {
        parse_bounded_u32(value, MAX_INLINE_LOAD_TRANSITION_MS)
    })?;
    let opacity_255 = operands.get(5).map_or(Ok(255), |value| {
        value
            .parse::<u8>()
            .map_err(|_| MinoriMessageMarkupError::LoadSchema)
    })?;
    Ok(MinoriMessageControl::LoadCharacter {
        visible_byte_offset,
        delay_ms,
        slot_id: signed_slot.unsigned_abs(),
        resource: operands[3].to_owned(),
        transition_ms,
        opacity_255,
    })
}

fn parse_bounded_u32(value: &str, maximum: u32) -> Result<u32, MinoriMessageMarkupError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= maximum)
        .ok_or(MinoriMessageMarkupError::LoadSchema)
}

fn validate_resource_name(value: &str) -> Result<(), MinoriMessageMarkupError> {
    if value.is_empty()
        || value.len() > MAX_RESOURCE_NAME_BYTES
        || value.starts_with('/')
        || value.contains(['/', '\\', ':', '\0'])
        || value == "."
        || value == ".."
        || !value.to_ascii_lowercase().ends_with(".png")
    {
        return Err(MinoriMessageMarkupError::LoadSchema);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_voice_auto_suffix_is_typed_and_not_visible() {
        let parsed = parse_minori_message_markup("body\\v\\a").unwrap();
        assert_eq!(parsed.visible_text, "body");
        assert!(parsed.waits_for_voice());
        assert!(parsed.auto_advance());
        assert_eq!(parsed.controls.len(), 2);
    }

    #[test]
    fn verified_inline_load_preserves_timing_layer_and_transition() {
        let parsed = parse_minori_message_markup("a\\x{load,120,21,Face.png,40,255}b").unwrap();
        assert_eq!(parsed.visible_text, "ab");
        assert_eq!(
            parsed.controls,
            [MinoriMessageControl::LoadCharacter {
                visible_byte_offset: 1,
                delay_ms: 120,
                slot_id: 21,
                resource: "Face.png".into(),
                transition_ms: 40,
                opacity_255: 255,
            }]
        );
    }

    #[test]
    fn unknown_or_malformed_controls_fail_closed() {
        assert_eq!(
            parse_minori_message_markup("body\\q").unwrap_err(),
            MinoriMessageMarkupError::Unsupported
        );
        assert_eq!(
            parse_minori_message_markup("body\\x{load,1,2,Face.png").unwrap_err(),
            MinoriMessageMarkupError::Truncated
        );
        assert_eq!(
            parse_minori_message_markup("body\\x{other,1,2,Face.png}").unwrap_err(),
            MinoriMessageMarkupError::LoadSchema
        );
    }
}
