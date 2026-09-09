use astra_emu_family_api::{
    FamilyEvent, FamilyResult, KeyCode as FamilyKey, KeyModifiers, KeyState, PointerButton,
    WindowState,
};
use rfvp::host_api::{InputModifiers, KeyCode, PointerButton as RfvpButton, RfvpEvent};

/// State that belongs to the family session rather than one `advance` call.
///
/// The family ABI sends a pointer button without coordinates. RFVP's event
/// wire still carries the coordinates, so retaining the last converted move
/// is required for a button delivered by a later advance to hit the same game
/// location. Window dimensions are retained for the same reason: resize is a
/// host-window lifecycle event, while RFVP's game viewport remains the
/// virtual frame configured during boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InputState {
    pub(crate) last_pointer: (i32, i32),
    pub(crate) window: WindowState,
    suspended: bool,
}

impl InputState {
    pub(crate) const fn new(window: WindowState) -> Self {
        Self {
            last_pointer: (0, 0),
            window,
            suspended: false,
        }
    }

    fn effective_focus(self) -> bool {
        self.window.focused && self.window.visible && !self.suspended
    }
}

fn modifiers(value: KeyModifiers) -> InputModifiers {
    let mut out = InputModifiers::empty();
    if value.shift {
        out = out.union(InputModifiers::SHIFT);
    }
    if value.control {
        out = out.union(InputModifiers::CONTROL);
    }
    if value.alt {
        out = out.union(InputModifiers::ALT);
    }
    if value.super_key {
        out = out.union(InputModifiers::SUPER);
    }
    out
}

fn key(value: FamilyKey) -> KeyCode {
    use FamilyKey::*;
    match value {
        Escape => KeyCode::Escape,
        Enter => KeyCode::Return,
        Space => KeyCode::Space,
        Backspace => KeyCode::Backspace,
        Tab => KeyCode::Tab,
        ArrowLeft => KeyCode::Left,
        ArrowRight => KeyCode::Right,
        ArrowUp => KeyCode::Up,
        ArrowDown => KeyCode::Down,
        PageUp => KeyCode::PageUp,
        PageDown => KeyCode::PageDown,
        Home => KeyCode::Home,
        End => KeyCode::End,
        Insert => KeyCode::Insert,
        Delete => KeyCode::Delete,
        ShiftLeft | ShiftRight => KeyCode::Shift,
        ControlLeft | ControlRight => KeyCode::Control,
        AltLeft | AltRight => KeyCode::Alt,
        A => KeyCode::Character('a'),
        B => KeyCode::Character('b'),
        C => KeyCode::Character('c'),
        D => KeyCode::Character('d'),
        E => KeyCode::Character('e'),
        F => KeyCode::Character('f'),
        G => KeyCode::Character('g'),
        H => KeyCode::Character('h'),
        I => KeyCode::Character('i'),
        J => KeyCode::Character('j'),
        K => KeyCode::Character('k'),
        L => KeyCode::Character('l'),
        M => KeyCode::Character('m'),
        N => KeyCode::Character('n'),
        O => KeyCode::Character('o'),
        P => KeyCode::Character('p'),
        Q => KeyCode::Character('q'),
        R => KeyCode::Character('r'),
        S => KeyCode::Character('s'),
        T => KeyCode::Character('t'),
        U => KeyCode::Character('u'),
        V => KeyCode::Character('v'),
        W => KeyCode::Character('w'),
        X => KeyCode::Character('x'),
        Y => KeyCode::Character('y'),
        Z => KeyCode::Character('z'),
        Digit0 => KeyCode::Character('0'),
        Digit1 => KeyCode::Character('1'),
        Digit2 => KeyCode::Character('2'),
        Digit3 => KeyCode::Character('3'),
        Digit4 => KeyCode::Character('4'),
        Digit5 => KeyCode::Character('5'),
        Digit6 => KeyCode::Character('6'),
        Digit7 => KeyCode::Character('7'),
        Digit8 => KeyCode::Character('8'),
        Digit9 => KeyCode::Character('9'),
        F1 => KeyCode::Function(1),
        F2 => KeyCode::Function(2),
        F3 => KeyCode::Function(3),
        F4 => KeyCode::Function(4),
        F5 => KeyCode::Function(5),
        F6 => KeyCode::Function(6),
        F7 => KeyCode::Function(7),
        F8 => KeyCode::Function(8),
        F9 => KeyCode::Function(9),
        F10 => KeyCode::Function(10),
        F11 => KeyCode::Function(11),
        F12 => KeyCode::Function(12),
        _ => KeyCode::Unknown(0),
    }
}

fn pointer_button(value: PointerButton) -> RfvpButton {
    match value {
        PointerButton::Primary => RfvpButton::Left,
        PointerButton::Secondary => RfvpButton::Right,
        PointerButton::Middle => RfvpButton::Middle,
        PointerButton::Back => RfvpButton::Other(4),
        PointerButton::Forward => RfvpButton::Other(5),
    }
}

fn coordinate(value: f32) -> i32 {
    value.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

pub(crate) fn convert(
    events: &[FamilyEvent],
    input_state: &mut InputState,
) -> FamilyResult<Vec<RfvpEvent>> {
    let mut out = Vec::new();
    for event in events {
        match event {
            FamilyEvent::Key {
                code,
                state,
                modifiers: mods,
            } => match state {
                KeyState::Pressed => out.push(RfvpEvent::KeyDown {
                    key: key(*code),
                    repeat: false,
                    modifiers: modifiers(*mods),
                }),
                KeyState::Repeated => out.push(RfvpEvent::KeyDown {
                    key: key(*code),
                    repeat: true,
                    modifiers: modifiers(*mods),
                }),
                KeyState::Released => out.push(RfvpEvent::KeyUp {
                    key: key(*code),
                    modifiers: modifiers(*mods),
                }),
            },
            FamilyEvent::PointerMove { x, y } => {
                let pointer = (coordinate(*x), coordinate(*y));
                input_state.last_pointer = pointer;
                out.push(RfvpEvent::PointerMove {
                    x: pointer.0,
                    y: pointer.1,
                    in_screen: true,
                });
            }
            FamilyEvent::PointerButton { button, state } => match state {
                KeyState::Released => out.push(RfvpEvent::PointerUp {
                    button: pointer_button(*button),
                    x: input_state.last_pointer.0,
                    y: input_state.last_pointer.1,
                }),
                KeyState::Pressed | KeyState::Repeated => out.push(RfvpEvent::PointerDown {
                    button: pointer_button(*button),
                    x: input_state.last_pointer.0,
                    y: input_state.last_pointer.1,
                }),
            },
            FamilyEvent::Wheel { delta_x, delta_y } => out.push(RfvpEvent::Wheel {
                delta_x: coordinate(*delta_x),
                delta_y: coordinate(*delta_y),
            }),
            FamilyEvent::TextInput { text } => {
                out.extend(text.chars().map(|ch| RfvpEvent::TextInput { ch }))
            }
            FamilyEvent::WindowFocused { focused } => {
                input_state.window.focused = *focused;
                out.push(if *focused {
                    RfvpEvent::FocusGained
                } else {
                    RfvpEvent::FocusLost
                });
            }
            FamilyEvent::WindowResized { width, height } => {
                // The event's dimensions are physical host-window pixels.
                // RFVP receives virtual game coordinates and keeps its
                // configured virtual viewport; preserve the host dimensions
                // in session state instead of silently discarding the event.
                input_state.window.width = *width;
                input_state.window.height = *height;
            }
            FamilyEvent::WindowVisibility { visible } => {
                let was_focused = input_state.effective_focus();
                input_state.window.visible = *visible;
                let is_focused = input_state.effective_focus();
                if was_focused != is_focused {
                    out.push(if is_focused {
                        RfvpEvent::FocusGained
                    } else {
                        RfvpEvent::FocusLost
                    });
                }
            }
            FamilyEvent::WindowSuspended { suspended } => {
                let was_focused = input_state.effective_focus();
                input_state.suspended = *suspended;
                let is_focused = input_state.effective_focus();
                if was_focused != is_focused {
                    out.push(if is_focused {
                        RfvpEvent::FocusGained
                    } else {
                        RfvpEvent::FocusLost
                    });
                }
            }
            FamilyEvent::WindowCloseRequested => out.push(RfvpEvent::Quit),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> WindowState {
        WindowState {
            width: 800,
            height: 600,
            focused: true,
            visible: true,
        }
    }

    #[test]
    fn pointer_buttons_keep_latest_move_between_advances() {
        let mut state = InputState::new(window());

        let first = convert(
            &[FamilyEvent::PointerMove { x: 120.0, y: 80.0 }],
            &mut state,
        )
        .expect("pointer move converts");
        assert_eq!(
            first,
            vec![RfvpEvent::PointerMove {
                x: 120,
                y: 80,
                in_screen: true,
            }]
        );

        let second = convert(
            &[
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
                FamilyEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Released,
                },
            ],
            &mut state,
        )
        .expect("pointer buttons convert");
        assert_eq!(
            second,
            vec![
                RfvpEvent::PointerDown {
                    button: RfvpButton::Left,
                    x: 120,
                    y: 80,
                },
                RfvpEvent::PointerUp {
                    button: RfvpButton::Left,
                    x: 120,
                    y: 80,
                },
            ]
        );
    }

    #[test]
    fn visibility_and_suspend_follow_rfvp_focus_lifecycle() {
        let mut state = InputState::new(window());
        assert_eq!(
            convert(
                &[FamilyEvent::WindowVisibility { visible: false }],
                &mut state
            )
            .unwrap(),
            vec![RfvpEvent::FocusLost]
        );
        assert_eq!(
            convert(
                &[FamilyEvent::WindowSuspended { suspended: true }],
                &mut state
            )
            .unwrap(),
            Vec::<RfvpEvent>::new()
        );
        assert_eq!(
            convert(
                &[FamilyEvent::WindowVisibility { visible: true }],
                &mut state
            )
            .unwrap(),
            Vec::<RfvpEvent>::new()
        );
        assert_eq!(
            convert(
                &[FamilyEvent::WindowSuspended { suspended: false }],
                &mut state
            )
            .unwrap(),
            vec![RfvpEvent::FocusGained]
        );
    }
}
