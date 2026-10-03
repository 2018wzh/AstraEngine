use super::error;
use crate::wire;
use abi_stable::std_types::ROption;
use astra_emu_family_api::*;

pub(super) fn events(source: &[FamilyEvent]) -> FamilyResult<wire::Items<wire::Event, 4096>> {
    if source.len() > 4096 {
        return Err(error(
            "ASTRA_EMU_KRKR_INPUT_BOUND",
            "input event count exceeds native bound",
        ));
    }
    let mut result = Vec::with_capacity(source.len());
    let mut text_bytes = 0_usize;
    for event in source {
        let converted = match event {
            FamilyEvent::Key {
                code,
                state,
                modifiers,
            } => wire::Event::Key {
                code: key(*code),
                state: key_state(*state),
                modifiers: wire::Modifiers {
                    shift: modifiers.shift,
                    control: modifiers.control,
                    alt: modifiers.alt,
                    super_key: modifiers.super_key,
                },
            },
            FamilyEvent::PointerMove { x, y } => wire::Event::PointerMove { x: *x, y: *y },
            FamilyEvent::PointerButton { button, state } => wire::Event::PointerButton {
                button: match button {
                    PointerButton::Primary => wire::PointerButton::Primary,
                    PointerButton::Secondary => wire::PointerButton::Secondary,
                    PointerButton::Middle => wire::PointerButton::Middle,
                    PointerButton::Back => wire::PointerButton::Back,
                    PointerButton::Forward => wire::PointerButton::Forward,
                },
                state: key_state(*state),
            },
            FamilyEvent::Wheel { delta_x, delta_y } => {
                if *delta_x != 0.0 {
                    return Err(error(
                        "ASTRA_EMU_KRKR_HORIZONTAL_WHEEL_UNSUPPORTED",
                        "KrKr does not support horizontal wheel input",
                    ));
                }
                wire::Event::Wheel {
                    delta_x: 0.0,
                    delta_y: *delta_y,
                }
            }
            FamilyEvent::TextInput { text } => {
                if text.len() > 32768 {
                    return Err(error(
                        "ASTRA_EMU_KRKR_INPUT_BOUND",
                        "input text exceeds native bound",
                    ));
                }
                text_bytes = text_bytes
                    .checked_add(text.len())
                    .filter(|n| *n <= 512 * 1024)
                    .ok_or_else(|| {
                        error(
                            "ASTRA_EMU_KRKR_INPUT_BOUND",
                            "input text total exceeds native bound",
                        )
                    })?;
                wire::Event::Text(wire::Text::new(text.to_string()).map_err(|_| {
                    error(
                        "ASTRA_EMU_KRKR_INPUT_BOUND",
                        "input text exceeds native bound",
                    )
                })?)
            }
            FamilyEvent::WindowFocused { focused } => wire::Event::Focused(*focused),
            FamilyEvent::WindowResized { width, height } => wire::Event::Resized {
                width: *width,
                height: *height,
            },
            FamilyEvent::WindowVisibility { visible } => wire::Event::Visible(*visible),
            FamilyEvent::WindowSuspended { suspended } => wire::Event::Suspended(*suspended),
            FamilyEvent::WindowCloseRequested => wire::Event::CloseRequested,
        };
        result.push(converted);
    }
    let result = wire::Items::new(result).map_err(|_| {
        error(
            "ASTRA_EMU_KRKR_INPUT_BOUND",
            "input event count exceeds native bound",
        )
    })?;
    let packet = wire::ParentPacket::Command(wire::Command::Advance {
        elapsed_ns: 0,
        events: result,
    });
    packet
        .validate()
        .map_err(|_| error("ASTRA_EMU_KRKR_INPUT", "native input values are invalid"))?;
    let wire::ParentPacket::Command(wire::Command::Advance { events, .. }) = packet else {
        unreachable!()
    };
    Ok(events)
}
fn key_state(state: KeyState) -> wire::KeyState {
    match state {
        KeyState::Pressed => wire::KeyState::Pressed,
        KeyState::Released => wire::KeyState::Released,
        KeyState::Repeated => wire::KeyState::Repeated,
    }
}
fn key(code: KeyCode) -> wire::KeyCode {
    match code {
        KeyCode::Unknown => wire::KeyCode::Unknown,
        KeyCode::A => wire::KeyCode::A,
        KeyCode::B => wire::KeyCode::B,
        KeyCode::C => wire::KeyCode::C,
        KeyCode::D => wire::KeyCode::D,
        KeyCode::E => wire::KeyCode::E,
        KeyCode::F => wire::KeyCode::F,
        KeyCode::G => wire::KeyCode::G,
        KeyCode::H => wire::KeyCode::H,
        KeyCode::I => wire::KeyCode::I,
        KeyCode::J => wire::KeyCode::J,
        KeyCode::K => wire::KeyCode::K,
        KeyCode::L => wire::KeyCode::L,
        KeyCode::M => wire::KeyCode::M,
        KeyCode::N => wire::KeyCode::N,
        KeyCode::O => wire::KeyCode::O,
        KeyCode::P => wire::KeyCode::P,
        KeyCode::Q => wire::KeyCode::Q,
        KeyCode::R => wire::KeyCode::R,
        KeyCode::S => wire::KeyCode::S,
        KeyCode::T => wire::KeyCode::T,
        KeyCode::U => wire::KeyCode::U,
        KeyCode::V => wire::KeyCode::V,
        KeyCode::W => wire::KeyCode::W,
        KeyCode::X => wire::KeyCode::X,
        KeyCode::Y => wire::KeyCode::Y,
        KeyCode::Z => wire::KeyCode::Z,
        KeyCode::Digit0 => wire::KeyCode::Digit0,
        KeyCode::Digit1 => wire::KeyCode::Digit1,
        KeyCode::Digit2 => wire::KeyCode::Digit2,
        KeyCode::Digit3 => wire::KeyCode::Digit3,
        KeyCode::Digit4 => wire::KeyCode::Digit4,
        KeyCode::Digit5 => wire::KeyCode::Digit5,
        KeyCode::Digit6 => wire::KeyCode::Digit6,
        KeyCode::Digit7 => wire::KeyCode::Digit7,
        KeyCode::Digit8 => wire::KeyCode::Digit8,
        KeyCode::Digit9 => wire::KeyCode::Digit9,
        KeyCode::Escape => wire::KeyCode::Escape,
        KeyCode::Enter => wire::KeyCode::Enter,
        KeyCode::Tab => wire::KeyCode::Tab,
        KeyCode::Backspace => wire::KeyCode::Backspace,
        KeyCode::Space => wire::KeyCode::Space,
        KeyCode::Insert => wire::KeyCode::Insert,
        KeyCode::Delete => wire::KeyCode::Delete,
        KeyCode::Home => wire::KeyCode::Home,
        KeyCode::End => wire::KeyCode::End,
        KeyCode::PageUp => wire::KeyCode::PageUp,
        KeyCode::PageDown => wire::KeyCode::PageDown,
        KeyCode::ArrowLeft => wire::KeyCode::ArrowLeft,
        KeyCode::ArrowRight => wire::KeyCode::ArrowRight,
        KeyCode::ArrowUp => wire::KeyCode::ArrowUp,
        KeyCode::ArrowDown => wire::KeyCode::ArrowDown,
        KeyCode::ControlLeft => wire::KeyCode::ControlLeft,
        KeyCode::ControlRight => wire::KeyCode::ControlRight,
        KeyCode::ShiftLeft => wire::KeyCode::ShiftLeft,
        KeyCode::ShiftRight => wire::KeyCode::ShiftRight,
        KeyCode::AltLeft => wire::KeyCode::AltLeft,
        KeyCode::AltRight => wire::KeyCode::AltRight,
        KeyCode::F1 => wire::KeyCode::F1,
        KeyCode::F2 => wire::KeyCode::F2,
        KeyCode::F3 => wire::KeyCode::F3,
        KeyCode::F4 => wire::KeyCode::F4,
        KeyCode::F5 => wire::KeyCode::F5,
        KeyCode::F6 => wire::KeyCode::F6,
        KeyCode::F7 => wire::KeyCode::F7,
        KeyCode::F8 => wire::KeyCode::F8,
        KeyCode::F9 => wire::KeyCode::F9,
        KeyCode::F10 => wire::KeyCode::F10,
        KeyCode::F11 => wire::KeyCode::F11,
        KeyCode::F12 => wire::KeyCode::F12,
        KeyCode::Numpad0 => wire::KeyCode::Numpad0,
        KeyCode::Numpad1 => wire::KeyCode::Numpad1,
        KeyCode::Numpad2 => wire::KeyCode::Numpad2,
        KeyCode::Numpad3 => wire::KeyCode::Numpad3,
        KeyCode::Numpad4 => wire::KeyCode::Numpad4,
        KeyCode::Numpad5 => wire::KeyCode::Numpad5,
        KeyCode::Numpad6 => wire::KeyCode::Numpad6,
        KeyCode::Numpad7 => wire::KeyCode::Numpad7,
        KeyCode::Numpad8 => wire::KeyCode::Numpad8,
        KeyCode::Numpad9 => wire::KeyCode::Numpad9,
        KeyCode::NumpadEnter => wire::KeyCode::NumpadEnter,
        KeyCode::NumpadAdd => wire::KeyCode::NumpadAdd,
        KeyCode::NumpadSubtract => wire::KeyCode::NumpadSubtract,
        KeyCode::NumpadMultiply => wire::KeyCode::NumpadMultiply,
        KeyCode::NumpadDivide => wire::KeyCode::NumpadDivide,
        KeyCode::Minus => wire::KeyCode::Minus,
        KeyCode::Equals => wire::KeyCode::Equals,
        KeyCode::Comma => wire::KeyCode::Comma,
        KeyCode::Period => wire::KeyCode::Period,
        KeyCode::Slash => wire::KeyCode::Slash,
        KeyCode::Semicolon => wire::KeyCode::Semicolon,
        KeyCode::Apostrophe => wire::KeyCode::Apostrophe,
        KeyCode::BracketLeft => wire::KeyCode::BracketLeft,
        KeyCode::BracketRight => wire::KeyCode::BracketRight,
        KeyCode::Backslash => wire::KeyCode::Backslash,
        KeyCode::Grave => wire::KeyCode::Grave,
    }
}
pub(super) fn response(reply: wire::Reply) -> FamilyResult<AdvanceResponse> {
    let wire::Reply::Advanced {
        status,
        window,
        reset_clock,
    } = reply
    else {
        return Err(error(
            "ASTRA_EMU_KRKR_ADVANCE",
            "native advance reply is invalid",
        ));
    };
    let response = AdvanceResponse {
        status: match status {
            wire::Status::Running => FamilyStatus::Running,
            wire::Status::Waiting => FamilyStatus::Waiting,
            wire::Status::Finished => FamilyStatus::Finished,
        },
        window_command: match window {
            None => ROption::RNone,
            Some(wire::WindowCommand::SetFullscreen(value)) => {
                ROption::RSome(FamilyWindowCommand::SetFullscreen(value))
            }
            Some(wire::WindowCommand::SetTitle(value)) => {
                ROption::RSome(FamilyWindowCommand::SetTitle(value.as_str().into()))
            }
            Some(wire::WindowCommand::SetIcon {
                width,
                height,
                rgba,
            }) => ROption::RSome(FamilyWindowCommand::SetIcon(FamilyWindowIcon {
                width,
                height,
                rgba: rgba.into_vec().into(),
            })),
        },
        reset_clock,
    };
    response.validate()?;
    Ok(response)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_keeps_logical_pointer_unicode_and_fractional_vertical_wheel() {
        let source = [
            FamilyEvent::PointerMove { x: 123.25, y: 45.5 },
            FamilyEvent::TextInput {
                text: "日🙂".into(),
            },
            FamilyEvent::Wheel {
                delta_x: 0.0,
                delta_y: -0.125,
            },
            FamilyEvent::WindowCloseRequested,
        ];
        let result = events(&source).unwrap();
        assert!(matches!(
            result.as_slice()[0],
            wire::Event::PointerMove { x: 123.25, y: 45.5 }
        ));
        assert!(matches!(
            result.as_slice()[2],
            wire::Event::Wheel {
                delta_x: 0.0,
                delta_y: -0.125
            }
        ));
        assert!(matches!(result.as_slice()[3], wire::Event::CloseRequested));
        assert_eq!(
            events(&[FamilyEvent::Wheel {
                delta_x: 0.1,
                delta_y: 0.0
            }])
            .unwrap_err()
            .code(),
            "ASTRA_EMU_KRKR_HORIZONTAL_WHEEL_UNSUPPORTED"
        );
    }
    #[test]
    fn native_window_command_and_reset_reach_family_response() {
        let output = response(wire::Reply::Advanced {
            status: wire::Status::Waiting,
            window: Some(wire::WindowCommand::SetTitle(
                wire::Text::new("caption".into()).unwrap(),
            )),
            reset_clock: true,
        })
        .unwrap();
        assert_eq!(output.status, FamilyStatus::Waiting);
        assert!(output.reset_clock);
        assert!(matches!(
            output.window_command,
            ROption::RSome(FamilyWindowCommand::SetTitle(_))
        ));
    }
}
