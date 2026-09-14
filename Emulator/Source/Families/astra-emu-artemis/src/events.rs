//! Family event to Artemis runtime input translation.
//!
//! The runtime consumes Windows virtual-key codes and stage-coordinate mouse
//! input; mouse button codes follow the Artemis convention (1=left, 2=right,
//! 4=middle). Text input and wheel have no runtime feed and are ignored.

use art3m1s_core::runtime::CoreRuntime;
use astra_emu_family_api::{FamilyEvent, KeyCode, KeyState, PointerButton};

/// Artemis mouse button codes.
const MOUSE_LEFT: u32 = 1;
const MOUSE_RIGHT: u32 = 2;
const MOUSE_MIDDLE: u32 = 4;
/// Windows virtual-key code of Enter, the keyboard decide key.
const KEY_ENTER: u32 = 0x0D;

/// Applies input and reports whether a decide edge (left press / Enter)
/// was delivered this tick, for the host stop-wake path.
pub(crate) fn apply(rt: &CoreRuntime, events: &[FamilyEvent]) -> bool {
    let mut decide = false;
    for event in events {
        match event {
            FamilyEvent::PointerMove { x, y } => {
                rt.feed_mouse(*x as i32, *y as i32);
            }
            FamilyEvent::PointerButton { button, state } => {
                let Some(code) = translate_button(button) else {
                    continue;
                };
                let pressed = matches!(state, KeyState::Pressed | KeyState::Repeated);
                rt.feed_mouse_button(code, pressed);
                // The script layer's isDecide reads the synthetic click flag
                // (or the Enter/Space key edges), not the mouse button edge;
                // a primary press must also produce that decide edge or the
                // scenario's click waits never release.
                if pressed && code == MOUSE_LEFT {
                    rt.feed_click();
                    decide = true;
                }
            }
            FamilyEvent::Key { code, state, .. } => {
                let Some(virtual_key) = translate_key(code) else {
                    continue;
                };
                match state {
                    KeyState::Pressed | KeyState::Repeated => {
                        rt.feed_key_down(virtual_key);
                        if virtual_key == KEY_ENTER {
                            decide = true;
                        }
                    }
                    KeyState::Released => rt.feed_key_up(virtual_key),
                }
            }
            // Wheel and text input have no runtime feed; focus, visibility,
            // suspension, and close requests carry no engine input because
            // lifecycle is owned by the host.
            FamilyEvent::Wheel { .. }
            | FamilyEvent::TextInput { .. }
            | FamilyEvent::WindowFocused { .. }
            | FamilyEvent::WindowResized { .. }
            | FamilyEvent::WindowVisibility { .. }
            | FamilyEvent::WindowSuspended { .. }
            | FamilyEvent::WindowCloseRequested => {}
        }
    }
    decide
}

fn translate_button(button: &PointerButton) -> Option<u32> {
    Some(match button {
        PointerButton::Primary => MOUSE_LEFT,
        PointerButton::Secondary => MOUSE_RIGHT,
        PointerButton::Middle => MOUSE_MIDDLE,
        PointerButton::Back | PointerButton::Forward => return None,
    })
}

/// Windows virtual-key codes; Artemis scripts bind through the same table.
fn translate_key(code: &KeyCode) -> Option<u32> {
    Some(match code {
        KeyCode::Escape => 0x1B,
        KeyCode::Enter | KeyCode::NumpadEnter => 0x0D,
        KeyCode::Tab => 0x09,
        KeyCode::Backspace => 0x08,
        KeyCode::Space => 0x20,
        KeyCode::Insert => 0x2D,
        KeyCode::Delete => 0x2E,
        KeyCode::Home => 0x24,
        KeyCode::End => 0x23,
        KeyCode::PageUp => 0x21,
        KeyCode::PageDown => 0x22,
        KeyCode::ArrowLeft => 0x25,
        KeyCode::ArrowUp => 0x26,
        KeyCode::ArrowRight => 0x27,
        KeyCode::ArrowDown => 0x28,
        KeyCode::ControlLeft | KeyCode::ControlRight => 0x11,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => 0x10,
        KeyCode::AltLeft | KeyCode::AltRight => 0x12,
        KeyCode::A => 0x41,
        KeyCode::B => 0x42,
        KeyCode::C => 0x43,
        KeyCode::D => 0x44,
        KeyCode::E => 0x45,
        KeyCode::F => 0x46,
        KeyCode::G => 0x47,
        KeyCode::H => 0x48,
        KeyCode::I => 0x49,
        KeyCode::J => 0x4A,
        KeyCode::K => 0x4B,
        KeyCode::L => 0x4C,
        KeyCode::M => 0x4D,
        KeyCode::N => 0x4E,
        KeyCode::O => 0x4F,
        KeyCode::P => 0x50,
        KeyCode::Q => 0x51,
        KeyCode::R => 0x52,
        KeyCode::S => 0x53,
        KeyCode::T => 0x54,
        KeyCode::U => 0x55,
        KeyCode::V => 0x56,
        KeyCode::W => 0x57,
        KeyCode::X => 0x58,
        KeyCode::Y => 0x59,
        KeyCode::Z => 0x5A,
        KeyCode::Digit0 => 0x30,
        KeyCode::Digit1 => 0x31,
        KeyCode::Digit2 => 0x32,
        KeyCode::Digit3 => 0x33,
        KeyCode::Digit4 => 0x34,
        KeyCode::Digit5 => 0x35,
        KeyCode::Digit6 => 0x36,
        KeyCode::Digit7 => 0x37,
        KeyCode::Digit8 => 0x38,
        KeyCode::Digit9 => 0x39,
        KeyCode::Numpad0 => 0x60,
        KeyCode::Numpad1 => 0x61,
        KeyCode::Numpad2 => 0x62,
        KeyCode::Numpad3 => 0x63,
        KeyCode::Numpad4 => 0x64,
        KeyCode::Numpad5 => 0x65,
        KeyCode::Numpad6 => 0x66,
        KeyCode::Numpad7 => 0x67,
        KeyCode::Numpad8 => 0x68,
        KeyCode::Numpad9 => 0x69,
        KeyCode::NumpadAdd => 0x6B,
        KeyCode::NumpadSubtract => 0x6D,
        KeyCode::NumpadMultiply => 0x6A,
        KeyCode::NumpadDivide => 0x6F,
        KeyCode::F1 => 0x70,
        KeyCode::F2 => 0x71,
        KeyCode::F3 => 0x72,
        KeyCode::F4 => 0x73,
        KeyCode::F5 => 0x74,
        KeyCode::F6 => 0x75,
        KeyCode::F7 => 0x76,
        KeyCode::F8 => 0x77,
        KeyCode::F9 => 0x78,
        KeyCode::F10 => 0x79,
        KeyCode::F11 => 0x7A,
        KeyCode::F12 => 0x7B,
        KeyCode::Minus => 0xBD,
        KeyCode::Equals => 0xBB,
        KeyCode::Comma => 0xBC,
        KeyCode::Period => 0xBE,
        KeyCode::Slash => 0xBF,
        KeyCode::Semicolon => 0xBA,
        KeyCode::Apostrophe => 0xDE,
        KeyCode::BracketLeft => 0xDB,
        KeyCode::BracketRight => 0xDD,
        KeyCode::Backslash => 0xDC,
        KeyCode::Grave => 0xC0,
        KeyCode::Unknown => return None,
    })
}
