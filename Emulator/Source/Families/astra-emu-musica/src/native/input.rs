use super::*;
use astra_emu_family_api::{FamilyEvent, KeyCode, KeyModifiers, KeyState, PointerButton};

pub(super) unsafe fn translate(
    handle: &Handle,
    source: &[abi::InputEvent],
) -> Result<Vec<FamilyEvent>, Failure> {
    let mut events = Vec::with_capacity(source.len());
    for event in source {
        if event.pressed > 1 || event.repeat > 1 {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        let state = if event.pressed == 0 {
            KeyState::Released
        } else if event.repeat != 0 {
            KeyState::Repeated
        } else {
            KeyState::Pressed
        };
        match event.kind {
            abi::INPUT_KEY => {
                let physical = unsafe { event.physical_key.text(128) }.map_err(invalid)?;
                let code = key(physical);
                if code == KeyCode::F9 && state == KeyState::Pressed {
                    handle.shared.services.invalidate();
                }
                events.push(FamilyEvent::Key {
                    code,
                    state,
                    modifiers: KeyModifiers {
                        shift: false,
                        control: false,
                        alt: false,
                        super_key: false,
                    },
                });
            }
            abi::INPUT_POINTER_MOVE | abi::INPUT_POINTER_BUTTON => {
                if event.displayed.session_epoch != handle.shared.services.session
                    || event.displayed.output_epoch != 1
                    || event.displayed.generation == 0
                {
                    return Err(invalid(abi::STALE_CONTEXT));
                }
                if !event.x.is_finite() || !event.y.is_finite() {
                    return Err(invalid(abi::INVALID_ARGUMENT));
                }
                let button = if event.kind == abi::INPUT_POINTER_BUTTON {
                    Some(match event.button {
                        abi::BUTTON_PRIMARY => PointerButton::Primary,
                        abi::BUTTON_SECONDARY => PointerButton::Secondary,
                        abi::BUTTON_MIDDLE => PointerButton::Middle,
                        _ => return Err(invalid(abi::INVALID_ARGUMENT)),
                    })
                } else {
                    None
                };
                let raster = [event.x as f32, event.y as f32];
                if !handle.shared.canvas.contains_raster_point(raster) {
                    if state == KeyState::Released {
                        if let Some(button) = button {
                            events.push(FamilyEvent::PointerButton { button, state });
                        }
                    }
                    continue;
                }
                let [x, y] = handle
                    .shared
                    .canvas
                    .raster_to_logical_point(raster)
                    .map_err(|_| invalid(abi::INVALID_ARGUMENT))?;
                events.push(FamilyEvent::PointerMove { x, y });
                if let Some(button) = button {
                    events.push(FamilyEvent::PointerButton { button, state });
                }
            }
            abi::INPUT_WHEEL => {
                if !event.x.is_finite() || !event.y.is_finite() {
                    return Err(invalid(abi::INVALID_ARGUMENT));
                }
                events.push(FamilyEvent::Wheel {
                    delta_x: event.x as f32,
                    delta_y: event.y as f32,
                });
            }
            abi::INPUT_WINDOW => {
                if event.window.focused > 1 || event.window.suspended > 1 {
                    return Err(invalid(abi::INVALID_ARGUMENT));
                }
                events.push(FamilyEvent::WindowFocused {
                    focused: event.window.focused != 0,
                });
                events.push(FamilyEvent::WindowSuspended {
                    suspended: event.window.suspended != 0,
                });
            }
            _ => return Err(invalid(abi::UNSUPPORTED)),
        }
    }
    Ok(events)
}
fn key(value: &str) -> KeyCode {
    match value {
        "Equal" => return KeyCode::Equals,
        "Backquote" => return KeyCode::Grave,
        "Quote" => return KeyCode::Apostrophe,
        _ => {}
    }
    let value = value.strip_prefix("Key").unwrap_or(value);
    macro_rules! keys { ($($name:ident),* $(,)?) => { match value { $(stringify!($name) => KeyCode::$name,)* _ => KeyCode::Unknown } }; }
    keys!(
        A,
        B,
        C,
        D,
        E,
        F,
        G,
        H,
        I,
        J,
        K,
        L,
        M,
        N,
        O,
        P,
        Q,
        R,
        S,
        T,
        U,
        V,
        W,
        X,
        Y,
        Z,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Escape,
        Enter,
        Tab,
        Backspace,
        Space,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
        ArrowLeft,
        ArrowRight,
        ArrowUp,
        ArrowDown,
        ControlLeft,
        ControlRight,
        ShiftLeft,
        ShiftRight,
        AltLeft,
        AltRight,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadEnter,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        Minus,
        Comma,
        Period,
        Slash,
        Semicolon,
        BracketLeft,
        BracketRight,
        Backslash
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_physical_letter_keys_reach_game_bindings() {
        assert_eq!(key("KeyA"), KeyCode::A);
        assert_eq!(key("KeyZ"), KeyCode::Z);
        assert_eq!(key("Digit1"), KeyCode::Digit1);
        assert_eq!(key("Enter"), KeyCode::Enter);
        assert_eq!(key("Equal"), KeyCode::Equals);
    }
}
