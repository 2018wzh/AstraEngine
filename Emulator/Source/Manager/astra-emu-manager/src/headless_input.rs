use astra_emu_manager_core::input_key_code;
use astra_emu_manager_core::model::{FamilyEvent, KeyModifiers, KeyState, PointerButton};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TimedInput {
    pub frame: u32,
    pub event: Input,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Input {
    Key { code: String, pressed: bool },
    PointerMove { x: f32, y: f32 },
    PointerButton { secondary: bool, pressed: bool },
    Control { action: String },
}

impl Input {
    fn to_event(&self) -> Result<FamilyEvent, String> {
        let state = |pressed| {
            if pressed {
                KeyState::Pressed
            } else {
                KeyState::Released
            }
        };
        Ok(match self {
            Self::Control { action } => FamilyEvent::Control {
                command: match action.as_str() {
                    "pause" => astra_emu_abi::CONTROL_PAUSE,
                    "resume" => astra_emu_abi::CONTROL_RESUME,
                    "retry" => astra_emu_abi::CONTROL_RETRY,
                    _ => return Err("ASTRA_EMU_HEADLESS_CONTROL".into()),
                },
            },
            Self::Key { code, pressed } => FamilyEvent::Key {
                logical: None,
                code: input_key_code(code).ok_or("ASTRA_EMU_HEADLESS_INPUT_KEY")?,
                state: state(*pressed),
                modifiers: KeyModifiers {
                    shift: false,
                    control: false,
                    alt: false,
                    super_key: false,
                },
            },
            Self::PointerMove { x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err("ASTRA_EMU_HEADLESS_INPUT_COORDINATES".into());
                }
                FamilyEvent::PointerMove {
                    x: *x,
                    y: *y,
                    displayed: None,
                }
            }
            Self::PointerButton { secondary, pressed } => FamilyEvent::PointerButton {
                button: if *secondary {
                    PointerButton::Secondary
                } else {
                    PointerButton::Primary
                },
                state: state(*pressed),
                displayed: None,
            },
        })
    }
}

pub(super) fn prepare(
    inputs: &[TimedInput],
    frames: u32,
) -> Result<std::collections::BTreeMap<u32, Vec<FamilyEvent>>, String> {
    let mut result = std::collections::BTreeMap::<u32, Vec<FamilyEvent>>::new();
    let mut previous = 0;
    for input in inputs {
        if input.frame >= frames || input.frame < previous {
            return Err("ASTRA_EMU_HEADLESS_INPUT_FRAME".into());
        }
        previous = input.frame;
        let events = result.entry(input.frame).or_default();
        if events.len() >= astra_emu_manager_core::model::MAX_INPUT_EVENTS {
            return Err("ASTRA_EMU_HEADLESS_INPUT_LIMIT".into());
        }
        events.push(input.event.to_event()?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_physical_event_order_and_rejects_invalid_sequences() {
        let inputs: Vec<TimedInput> = serde_json::from_str(
            r#"[{"frame":2,"event":{"type":"pointer_move","x":120,"y":80}},
                {"frame":2,"event":{"type":"pointer_button","secondary":false,"pressed":true}},
                {"frame":3,"event":{"type":"pointer_button","secondary":false,"pressed":false}}]"#,
        )
        .unwrap();
        let events = prepare(&inputs, 4).unwrap();
        assert!(matches!(
            events[&2][0],
            FamilyEvent::PointerMove {
                x: 120.0,
                y: 80.0,
                ..
            }
        ));
        assert!(matches!(
            events[&2][1],
            FamilyEvent::PointerButton {
                state: KeyState::Pressed,
                ..
            }
        ));
        assert!(matches!(
            events[&3][0],
            FamilyEvent::PointerButton {
                state: KeyState::Released,
                ..
            }
        ));
        assert!(prepare(&inputs, 3).is_err());
        let mut reversed = inputs;
        reversed.reverse();
        assert!(prepare(&reversed, 4).is_err());
        assert!(Input::Key {
            code: "choose".into(),
            pressed: true
        }
        .to_event()
        .is_err());
        assert!(Input::PointerMove {
            x: f32::NAN,
            y: 0.0
        }
        .to_event()
        .is_err());
    }
}
