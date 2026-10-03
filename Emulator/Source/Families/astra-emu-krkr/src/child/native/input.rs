use super::abi;
use crate::wire::{self, Failure, KeyCode, KeyState};

fn key_state(state: KeyState) -> u32 {
    match state {
        KeyState::Pressed => 0,
        KeyState::Released => 1,
        KeyState::Repeated => 2,
    }
}

/// NativeOwner.h's explicit semantic constants, not platform VK values or
/// Rust discriminants. Exhaustive matching makes every new wire key visible.
fn key_code(code: KeyCode) -> u32 {
    match code {
        KeyCode::Unknown => 0,
        KeyCode::A => 1,
        KeyCode::B => 2,
        KeyCode::C => 3,
        KeyCode::D => 4,
        KeyCode::E => 5,
        KeyCode::F => 6,
        KeyCode::G => 7,
        KeyCode::H => 8,
        KeyCode::I => 9,
        KeyCode::J => 10,
        KeyCode::K => 11,
        KeyCode::L => 12,
        KeyCode::M => 13,
        KeyCode::N => 14,
        KeyCode::O => 15,
        KeyCode::P => 16,
        KeyCode::Q => 17,
        KeyCode::R => 18,
        KeyCode::S => 19,
        KeyCode::T => 20,
        KeyCode::U => 21,
        KeyCode::V => 22,
        KeyCode::W => 23,
        KeyCode::X => 24,
        KeyCode::Y => 25,
        KeyCode::Z => 26,
        KeyCode::Digit0 => 27,
        KeyCode::Digit1 => 28,
        KeyCode::Digit2 => 29,
        KeyCode::Digit3 => 30,
        KeyCode::Digit4 => 31,
        KeyCode::Digit5 => 32,
        KeyCode::Digit6 => 33,
        KeyCode::Digit7 => 34,
        KeyCode::Digit8 => 35,
        KeyCode::Digit9 => 36,
        KeyCode::Escape => 37,
        KeyCode::Enter => 38,
        KeyCode::Tab => 39,
        KeyCode::Backspace => 40,
        KeyCode::Space => 41,
        KeyCode::Insert => 42,
        KeyCode::Delete => 43,
        KeyCode::Home => 44,
        KeyCode::End => 45,
        KeyCode::PageUp => 46,
        KeyCode::PageDown => 47,
        KeyCode::ArrowLeft => 48,
        KeyCode::ArrowRight => 49,
        KeyCode::ArrowUp => 50,
        KeyCode::ArrowDown => 51,
        KeyCode::ControlLeft => 52,
        KeyCode::ControlRight => 53,
        KeyCode::ShiftLeft => 54,
        KeyCode::ShiftRight => 55,
        KeyCode::AltLeft => 56,
        KeyCode::AltRight => 57,
        KeyCode::F1 => 58,
        KeyCode::F2 => 59,
        KeyCode::F3 => 60,
        KeyCode::F4 => 61,
        KeyCode::F5 => 62,
        KeyCode::F6 => 63,
        KeyCode::F7 => 64,
        KeyCode::F8 => 65,
        KeyCode::F9 => 66,
        KeyCode::F10 => 67,
        KeyCode::F11 => 68,
        KeyCode::F12 => 69,
        KeyCode::Numpad0 => 70,
        KeyCode::Numpad1 => 71,
        KeyCode::Numpad2 => 72,
        KeyCode::Numpad3 => 73,
        KeyCode::Numpad4 => 74,
        KeyCode::Numpad5 => 75,
        KeyCode::Numpad6 => 76,
        KeyCode::Numpad7 => 77,
        KeyCode::Numpad8 => 78,
        KeyCode::Numpad9 => 79,
        KeyCode::NumpadEnter => 80,
        KeyCode::NumpadAdd => 81,
        KeyCode::NumpadSubtract => 82,
        KeyCode::NumpadMultiply => 83,
        KeyCode::NumpadDivide => 84,
        KeyCode::Minus => 85,
        KeyCode::Equals => 86,
        KeyCode::Comma => 87,
        KeyCode::Period => 88,
        KeyCode::Slash => 89,
        KeyCode::Semicolon => 90,
        KeyCode::Apostrophe => 91,
        KeyCode::BracketLeft => 92,
        KeyCode::BracketRight => 93,
        KeyCode::Backslash => 94,
        KeyCode::Grave => 95,
    }
}

/// Text pointers borrow the supplied wire events through the synchronous
/// advance call. Pointer positions remain logical-stage coordinates (v10).
pub fn events(events: &[wire::Event]) -> Result<Vec<abi::Event>, Failure> {
    if events.len() > 4096 {
        return Err(Failure::Input);
    }
    let mut text_bytes = 0usize;
    let mut output = Vec::with_capacity(events.len());
    for event in events {
        let mut native = abi::Event::default();
        match event {
            wire::Event::Key {
                code,
                state,
                modifiers,
            } => {
                native.kind = 1;
                native.key_code = key_code(*code);
                native.key_state = key_state(*state);
                native.modifiers = u32::from(modifiers.shift)
                    | u32::from(modifiers.control) << 1
                    | u32::from(modifiers.alt) << 2
                    | u32::from(modifiers.super_key) << 3;
            }
            wire::Event::PointerMove { x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(Failure::Input);
                }
                native.kind = 2;
                native.x = *x;
                native.y = *y;
            }
            wire::Event::PointerButton { button, state } => {
                native.kind = 3;
                native.pointer_button = match button {
                    wire::PointerButton::Primary => 0,
                    wire::PointerButton::Secondary => 1,
                    wire::PointerButton::Middle => 2,
                    wire::PointerButton::Back => 3,
                    wire::PointerButton::Forward => 4,
                };
                native.key_state = key_state(*state);
            }
            wire::Event::Wheel { delta_x, delta_y } => {
                if !delta_x.is_finite() || !delta_y.is_finite() {
                    return Err(Failure::Input);
                }
                native.kind = 4;
                native.x = *delta_x;
                native.y = *delta_y;
            }
            wire::Event::Text(text) => {
                text_bytes = text_bytes
                    .checked_add(text.as_str().len())
                    .ok_or(Failure::Input)?;
                if text_bytes > wire::MAX_PACKET_BYTES {
                    return Err(Failure::Input);
                }
                native.kind = 5;
                native.text_utf8 = text.as_str().as_ptr();
                native.text_bytes = text.as_str().len() as u64;
            }
            wire::Event::Focused(value) => {
                native.kind = 6;
                native.value = u32::from(*value);
            }
            wire::Event::Resized { width, height } => {
                if *width == 0 || *height == 0 {
                    return Err(Failure::Input);
                }
                native.kind = 7;
                native.width = *width;
                native.height = *height;
            }
            wire::Event::Visible(value) => {
                native.kind = 8;
                native.value = u32::from(*value);
            }
            wire::Event::Suspended(value) => {
                native.kind = 9;
                native.value = u32::from(*value);
            }
            wire::Event::CloseRequested => native.kind = 10,
        }
        output.push(native);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_input_preserves_logical_positions_unicode_and_fractional_wheel() {
        let source = vec![
            wire::Event::PointerMove { x: 123.5, y: -2.0 },
            wire::Event::Wheel {
                delta_x: 0.25,
                delta_y: -0.5,
            },
            wire::Event::Text(wire::Text::new("文字🦀".to_owned()).unwrap()),
            wire::Event::Key {
                code: KeyCode::NumpadEnter,
                state: KeyState::Repeated,
                modifiers: wire::Modifiers {
                    shift: true,
                    control: false,
                    alt: true,
                    super_key: true,
                },
            },
        ];
        let native = events(&source).unwrap();
        assert_eq!((native[0].x, native[0].y), (123.5, -2.0));
        assert_eq!((native[1].x, native[1].y), (0.25, -0.5));
        let text = unsafe {
            std::slice::from_raw_parts(native[2].text_utf8, native[2].text_bytes as usize)
        };
        assert_eq!(text, "文字🦀".as_bytes());
        assert_eq!(
            (native[3].key_code, native[3].key_state, native[3].modifiers),
            (80, 2, 13)
        );
    }
    #[test]
    fn total_text_bound_is_checked_before_calling_native() {
        let event = wire::Event::Text(wire::Text::new("x".repeat(32768)).unwrap());
        assert!(events(&vec![event; 17]).is_err());
        assert!(events(&vec![wire::Event::CloseRequested; 4097]).is_err());
    }
}
