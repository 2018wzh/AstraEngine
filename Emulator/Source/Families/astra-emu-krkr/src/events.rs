//! Translation from `FamilyEvent` to the engine input ABI.
//!
//! Kirikiri's key codes are Windows virtual-key codes, so the mapping is a
//! plain `KeyCode` → `VK_*` table. Pointer coordinates arrive in game-frame
//! pixels from the host and pass through unchanged. Text input is UTF-8 on
//! the ABI; the engine consumes one UTF-32 character per event.

use astra_emu_family_api::{FamilyEvent, KeyCode, KeyState, PointerButton};

use crate::engine_ffi::{
    AstraKrkrInputEvent, ASTRA_KRKR_INPUT_KEY_DOWN, ASTRA_KRKR_INPUT_KEY_UP,
    ASTRA_KRKR_INPUT_MOUSE_DOWN, ASTRA_KRKR_INPUT_MOUSE_MOVE, ASTRA_KRKR_INPUT_MOUSE_UP,
    ASTRA_KRKR_INPUT_TEXT, ASTRA_KRKR_INPUT_WHEEL,
};

/// Session-scoped pointer/modifier tracking. The ABI delivers button events
/// without coordinates; the engine still needs the position of the last move.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct InputState {
    pub(crate) pointer: (i32, i32),
    shift: bool,
    control: bool,
}

pub(crate) fn translate(
    events: &[FamilyEvent],
    state: &mut InputState,
) -> Vec<AstraKrkrInputEvent> {
    let mut out = Vec::new();
    for event in events {
        match event {
            FamilyEvent::Key {
                code,
                state: key_state,
                modifiers,
            } => {
                let Some(vk) = windows_virtual_key(*code) else {
                    continue;
                };
                match key_state {
                    KeyState::Pressed => {
                        state.shift = modifiers.shift;
                        state.control = modifiers.control;
                        push_key(&mut out, state, vk, ASTRA_KRKR_INPUT_KEY_DOWN);
                    }
                    KeyState::Released => {
                        push_key(&mut out, state, vk, ASTRA_KRKR_INPUT_KEY_UP);
                        state.shift = modifiers.shift;
                        state.control = modifiers.control;
                    }
                    KeyState::Repeated => {
                        push_key(&mut out, state, vk, ASTRA_KRKR_INPUT_KEY_DOWN);
                    }
                }
            }
            FamilyEvent::TextInput { text } => {
                for character in text.as_str().chars() {
                    push_event(&mut out, state, ASTRA_KRKR_INPUT_TEXT, |event| {
                        event.utf32 = character as u32
                    });
                }
            }
            FamilyEvent::PointerMove { x, y } => {
                state.pointer = (*x as i32, *y as i32);
                push_event(&mut out, state, ASTRA_KRKR_INPUT_MOUSE_MOVE, |event| {
                    event.x = state.pointer.0;
                    event.y = state.pointer.1;
                });
            }
            FamilyEvent::PointerButton {
                button,
                state: button_state,
            } => match button {
                PointerButton::Back | PointerButton::Forward => {
                    let vk = if matches!(button, PointerButton::Back) {
                        0x05
                    } else {
                        0x06
                    };
                    let kind = match button_state {
                        KeyState::Pressed | KeyState::Repeated => ASTRA_KRKR_INPUT_KEY_DOWN,
                        KeyState::Released => ASTRA_KRKR_INPUT_KEY_UP,
                    };
                    push_key(&mut out, state, vk, kind);
                }
                mapped => {
                    let engine_button = match mapped {
                        PointerButton::Primary => 0_u8,
                        PointerButton::Secondary => 1,
                        PointerButton::Middle => 2,
                        PointerButton::Back | PointerButton::Forward => unreachable!(),
                    };
                    let kind = match button_state {
                        KeyState::Pressed | KeyState::Repeated => ASTRA_KRKR_INPUT_MOUSE_DOWN,
                        KeyState::Released => ASTRA_KRKR_INPUT_MOUSE_UP,
                    };
                    push_event(&mut out, state, kind, |event| {
                        event.button = engine_button;
                        event.x = state.pointer.0;
                        event.y = state.pointer.1;
                    });
                }
            },
            FamilyEvent::Wheel { delta_x, delta_y } => {
                push_event(&mut out, state, ASTRA_KRKR_INPUT_WHEEL, |event| {
                    event.wheel = delta_y.round() as i32;
                    event.x = delta_x.round() as i32;
                });
            }
            _ => {}
        }
    }
    out
}

fn push_key(out: &mut Vec<AstraKrkrInputEvent>, state: &InputState, vk: u16, kind: u8) {
    push_event(out, state, kind, |event| {
        event.key = vk;
        if vk == VK_SHIFT {
            event.shift = 1;
        } else if vk == VK_CONTROL {
            event.control = 1;
        }
    });
}

fn push_event(
    out: &mut Vec<AstraKrkrInputEvent>,
    state: &InputState,
    kind: u8,
    fill: impl FnOnce(&mut AstraKrkrInputEvent),
) {
    let mut event = AstraKrkrInputEvent {
        kind,
        button: 0,
        shift: u8::from(state.shift),
        control: u8::from(state.control),
        key: 0,
        utf32: 0,
        x: state.pointer.0,
        y: state.pointer.1,
        wheel: 0,
    };
    fill(&mut event);
    out.push(event);
}

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;

fn windows_virtual_key(key: KeyCode) -> Option<u16> {
    use KeyCode::*;
    Some(match key {
        Unknown => return None,
        Backspace => 0x08,
        Tab => 0x09,
        Enter => 0x0D,
        Escape => 0x1B,
        Space => 0x20,
        PageUp => 0x21,
        PageDown => 0x22,
        End => 0x23,
        Home => 0x24,
        ArrowLeft => 0x25,
        ArrowUp => 0x26,
        ArrowRight => 0x27,
        ArrowDown => 0x28,
        Insert => 0x2D,
        Delete => 0x2E,
        Digit0 => b'0' as u16,
        Digit1 => b'1' as u16,
        Digit2 => b'2' as u16,
        Digit3 => b'3' as u16,
        Digit4 => b'4' as u16,
        Digit5 => b'5' as u16,
        Digit6 => b'6' as u16,
        Digit7 => b'7' as u16,
        Digit8 => b'8' as u16,
        Digit9 => b'9' as u16,
        A => b'A' as u16,
        B => b'B' as u16,
        C => b'C' as u16,
        D => b'D' as u16,
        E => b'E' as u16,
        F => b'F' as u16,
        G => b'G' as u16,
        H => b'H' as u16,
        I => b'I' as u16,
        J => b'J' as u16,
        K => b'K' as u16,
        L => b'L' as u16,
        M => b'M' as u16,
        N => b'N' as u16,
        O => b'O' as u16,
        P => b'P' as u16,
        Q => b'Q' as u16,
        R => b'R' as u16,
        S => b'S' as u16,
        T => b'T' as u16,
        U => b'U' as u16,
        V => b'V' as u16,
        W => b'W' as u16,
        X => b'X' as u16,
        Y => b'Y' as u16,
        Z => b'Z' as u16,
        Numpad0 => 0x60,
        Numpad1 => 0x61,
        Numpad2 => 0x62,
        Numpad3 => 0x63,
        Numpad4 => 0x64,
        Numpad5 => 0x65,
        Numpad6 => 0x66,
        Numpad7 => 0x67,
        Numpad8 => 0x68,
        Numpad9 => 0x69,
        NumpadMultiply => 0x6A,
        NumpadAdd => 0x6B,
        NumpadSubtract => 0x6D,
        NumpadDivide => 0x6F,
        NumpadEnter => 0x0D,
        F1 => 0x70,
        F2 => 0x71,
        F3 => 0x72,
        F4 => 0x73,
        F5 => 0x74,
        F6 => 0x75,
        F7 => 0x76,
        F8 => 0x77,
        F9 => 0x78,
        F10 => 0x79,
        F11 => 0x7A,
        F12 => 0x7B,
        ShiftLeft | ShiftRight => VK_SHIFT,
        ControlLeft | ControlRight => VK_CONTROL,
        AltLeft | AltRight => 0x12,
        Semicolon => 0xBA,
        Equals => 0xBB,
        Comma => 0xBC,
        Minus => 0xBD,
        Period => 0xBE,
        Slash => 0xBF,
        Grave => 0xC0,
        BracketLeft => 0xDB,
        Backslash => 0xDC,
        BracketRight => 0xDD,
        Apostrophe => 0xDE,
    })
}
