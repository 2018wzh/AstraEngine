//! Family event to Siglus engine input translation.

use astra_emu_family_api::{FamilyEvent, KeyCode, KeyState, PointerButton};
use siglus_scene_vm::host::SiglusHost;
use siglus_scene_vm::runtime::input::{VmKey, VmMouseButton};

pub(crate) fn apply(host: &mut SiglusHost, events: &[FamilyEvent]) {
    for event in events {
        match event {
            FamilyEvent::Key { code, state, .. } => {
                let Some(key) = translate_key(code) else {
                    continue;
                };
                match state {
                    KeyState::Pressed | KeyState::Repeated => host.key_down(key),
                    KeyState::Released => host.key_up(key),
                }
            }
            FamilyEvent::PointerMove { x, y } => {
                host.mouse_move(*x as f64, *y as f64);
            }
            FamilyEvent::PointerButton { button, state } => {
                let Some(button) = translate_button(button) else {
                    continue;
                };
                match state {
                    KeyState::Pressed | KeyState::Repeated => host.mouse_down(button),
                    KeyState::Released => host.mouse_up(button),
                }
            }
            FamilyEvent::Wheel { delta_y, .. } => {
                host.mouse_wheel(delta_y.round() as i32);
            }
            FamilyEvent::TextInput { text } => {
                host.text_input(text.as_str());
            }
            FamilyEvent::WindowResized { width, height } => {
                host.resize(*width, *height, 1.0);
            }
            // Focus, visibility, suspension, and close requests carry no
            // engine input; lifecycle is owned by the host.
            FamilyEvent::WindowFocused { .. }
            | FamilyEvent::WindowVisibility { .. }
            | FamilyEvent::WindowSuspended { .. }
            | FamilyEvent::WindowCloseRequested => {}
        }
    }
}

fn translate_button(button: &PointerButton) -> Option<VmMouseButton> {
    Some(match button {
        PointerButton::Primary => VmMouseButton::Left,
        PointerButton::Secondary => VmMouseButton::Right,
        PointerButton::Middle => VmMouseButton::Middle,
        PointerButton::Back | PointerButton::Forward => return None,
    })
}

fn translate_key(code: &KeyCode) -> Option<VmKey> {
    Some(match code {
        KeyCode::Escape => VmKey::Escape,
        KeyCode::Enter => VmKey::Enter,
        KeyCode::Space => VmKey::Space,
        KeyCode::Backspace => VmKey::Backspace,
        KeyCode::Delete => VmKey::Delete,
        KeyCode::Tab => VmKey::Tab,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => VmKey::Shift,
        KeyCode::ControlLeft | KeyCode::ControlRight => VmKey::Control,
        KeyCode::AltLeft | KeyCode::AltRight => VmKey::Alt,
        KeyCode::Home => VmKey::Home,
        KeyCode::End => VmKey::End,
        KeyCode::ArrowLeft => VmKey::ArrowLeft,
        KeyCode::ArrowRight => VmKey::ArrowRight,
        KeyCode::ArrowUp => VmKey::ArrowUp,
        KeyCode::ArrowDown => VmKey::ArrowDown,
        KeyCode::Digit0 => VmKey::Digit(0),
        KeyCode::Digit1 => VmKey::Digit(1),
        KeyCode::Digit2 => VmKey::Digit(2),
        KeyCode::Digit3 => VmKey::Digit(3),
        KeyCode::Digit4 => VmKey::Digit(4),
        KeyCode::Digit5 => VmKey::Digit(5),
        KeyCode::Digit6 => VmKey::Digit(6),
        KeyCode::Digit7 => VmKey::Digit(7),
        KeyCode::Digit8 => VmKey::Digit(8),
        KeyCode::Digit9 => VmKey::Digit(9),
        KeyCode::A => VmKey::Letter('A'),
        KeyCode::B => VmKey::Letter('B'),
        KeyCode::C => VmKey::Letter('C'),
        KeyCode::D => VmKey::Letter('D'),
        KeyCode::E => VmKey::Letter('E'),
        KeyCode::F => VmKey::Letter('F'),
        KeyCode::G => VmKey::Letter('G'),
        KeyCode::H => VmKey::Letter('H'),
        KeyCode::I => VmKey::Letter('I'),
        KeyCode::J => VmKey::Letter('J'),
        KeyCode::K => VmKey::Letter('K'),
        KeyCode::L => VmKey::Letter('L'),
        KeyCode::M => VmKey::Letter('M'),
        KeyCode::N => VmKey::Letter('N'),
        KeyCode::O => VmKey::Letter('O'),
        KeyCode::P => VmKey::Letter('P'),
        KeyCode::Q => VmKey::Letter('Q'),
        KeyCode::R => VmKey::Letter('R'),
        KeyCode::S => VmKey::Letter('S'),
        KeyCode::T => VmKey::Letter('T'),
        KeyCode::U => VmKey::Letter('U'),
        KeyCode::V => VmKey::Letter('V'),
        KeyCode::W => VmKey::Letter('W'),
        KeyCode::X => VmKey::Letter('X'),
        KeyCode::Y => VmKey::Letter('Y'),
        KeyCode::Z => VmKey::Letter('Z'),
        KeyCode::F1 => VmKey::F(1),
        KeyCode::F2 => VmKey::F(2),
        KeyCode::F3 => VmKey::F(3),
        KeyCode::F4 => VmKey::F(4),
        KeyCode::F5 => VmKey::F(5),
        KeyCode::F6 => VmKey::F(6),
        KeyCode::F7 => VmKey::F(7),
        KeyCode::F8 => VmKey::F(8),
        KeyCode::F9 => VmKey::F(9),
        KeyCode::F10 => VmKey::F(10),
        KeyCode::F11 => VmKey::F(11),
        KeyCode::F12 => VmKey::F(12),
        _ => return None,
    })
}
