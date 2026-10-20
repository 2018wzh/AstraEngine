use std::{cell::RefCell, rc::Rc};

use astra_emu_manager_core::model::{FamilyEvent, KeyCode, KeyModifiers, KeyState, WindowState};
use astra_emu_manager_ui_slint::{
    report_error, App, GameSettings, ManagerViewModel, ManagerWindow, Settings, SlintManagerAdapter,
};
use slint::{
    winit_030::{EventResult, WinitWindowAccessor},
    ComponentHandle,
};
use winit::event::{ElementState, WindowEvent};

use super::ManagerController;
use crate::gamepad::GameInputPump;

pub(super) fn system_theme(window: &slint::Window) -> Result<bool, String> {
    window
        .with_winit_window(|window| window.theme())
        .flatten()
        .map(|theme| theme == winit::window::Theme::Dark)
        .ok_or_else(|| "ASTRA_EMU_SYSTEM_THEME_UNAVAILABLE".into())
}

pub(super) fn current_window_state(window: &slint::Window) -> Result<WindowState, String> {
    window
        .with_winit_window(|window| {
            let size = window.inner_size();
            Ok(WindowState {
                width: size.width,
                height: size.height,
                scale_factor: window.scale_factor(),
                focused: window.has_focus(),
                visible: window
                    .is_visible()
                    .ok_or("ASTRA_EMU_WINDOW_VISIBILITY_UNAVAILABLE")?,
                suspended: window.is_minimized() == Some(true),
            })
        })
        .ok_or_else(|| "ASTRA_EMU_WINDOW_UNAVAILABLE".to_owned())?
}

pub(super) fn install<C: ManagerController>(
    adapter: &Rc<SlintManagerAdapter>,
    controller: Rc<RefCell<C>>,
    pointer: Rc<RefCell<super::pointer::HostPointer>>,
    gamepad: Rc<RefCell<GameInputPump>>,
) {
    let weak = adapter.window().as_weak();
    let weak_adapter = Rc::downgrade(adapter);
    let mut modifiers = KeyModifiers {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    };
    let mut occluded = false;
    adapter
        .window()
        .window()
        .on_winit_window_event(move |native_window, event| {
            let Some(window) = weak.upgrade() else {
                return EventResult::Propagate;
            };
            let target = capture_target(&window);
            match sync_capture(&controller, target.as_ref()) {
                Ok(Some(model)) => super::apply_model(&weak_adapter, &model),
                Ok(None) => {}
                Err(error) => {
                    clear_capture(&window);
                    report_error(&window, &error);
                }
            }
            if let (Some(target), WindowEvent::KeyboardInput { event, .. }) = (&target, event) {
                if event.state == ElementState::Pressed && !event.repeat {
                    let code = match event.physical_key {
                        winit::keyboard::PhysicalKey::Code(code) => family_key(code),
                        winit::keyboard::PhysicalKey::Unidentified(_) => None,
                    };
                    let result = controller
                        .borrow_mut()
                        .capture_key(code.unwrap_or(KeyCode::Unknown))
                        .and_then(|model| {
                            let mapping = controller.borrow().input_mapping();
                            gamepad.borrow_mut().set_mapping(mapping)?;
                            Ok(model)
                        });
                    match result {
                        Ok(model) => super::apply_model(&weak_adapter, &model),
                        Err(error) => report_error(&window, &error),
                    }
                    let hint = controller
                        .borrow()
                        .key_capture()
                        .map(|state| slint::SharedString::from(state.hint));
                    match hint {
                        Some(hint) if target.game_id.is_some() => {
                            window.global::<GameSettings>().set_capture_hint(hint)
                        }
                        Some(hint) => window.global::<Settings>().set_capture_hint(hint),
                        None => clear_capture(&window),
                    }
                }
                // Every key, Esc included, is a binding candidate rather than a UI shortcut.
                return EventResult::PreventDefault;
            }
            let input_active = super::session_input_open(&window);
            super::pointer::HostPointer::observe(&pointer, &window, event);
            let mut consumed = false;
            let keyboard = matches!(event, WindowEvent::KeyboardInput { .. });
            let result = match event {
                WindowEvent::ThemeChanged(_) => {
                    if let Ok(dark) = system_theme(native_window) {
                        window.global::<App>().set_system_dark(dark);
                    }
                    Ok(())
                }
                WindowEvent::Focused(focused) => {
                    current_window_state(native_window).and_then(|mut state| {
                        state.suspended |= occluded;
                        state.focused = *focused;
                        controller.borrow_mut().set_window_state(state)
                    })
                }
                WindowEvent::Resized(_)
                | WindowEvent::Occluded(_)
                | WindowEvent::ScaleFactorChanged { .. } => current_window_state(native_window)
                    .and_then(|mut state| {
                        if let WindowEvent::Occluded(value) = event {
                            occluded = *value;
                        }
                        state.suspended |= occluded;
                        controller.borrow_mut().set_window_state(state)
                    }),
                WindowEvent::ModifiersChanged(value) => {
                    let state = value.state();
                    modifiers = KeyModifiers {
                        shift: state.shift_key(),
                        control: state.control_key(),
                        alt: state.alt_key(),
                        super_key: state.super_key(),
                    };
                    Ok(())
                }
                WindowEvent::KeyboardInput { event, .. } if input_active => {
                    if let winit::keyboard::PhysicalKey::Code(code) = event.physical_key {
                        if let Some(code) = family_key(code) {
                            consumed = true;
                            controller.borrow_mut().physical_event(FamilyEvent::Key {
                                logical: match &event.logical_key {
                                    winit::keyboard::Key::Character(text) => Some(text.to_string()),
                                    _ => None,
                                },
                                code,
                                modifiers,
                                state: match (event.state, event.repeat) {
                                    (ElementState::Released, _) => KeyState::Released,
                                    (ElementState::Pressed, true) => KeyState::Repeated,
                                    (ElementState::Pressed, false) => KeyState::Pressed,
                                },
                            })
                        } else {
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                if keyboard {
                    super::report_input_error(&window, &error);
                } else if controller.borrow().is_game_active() {
                    super::terminate_game(&controller, &weak_adapter, &window, error);
                } else {
                    report_error(&window, &error);
                }
            }
            if consumed {
                EventResult::PreventDefault
            } else {
                EventResult::Propagate
            }
        });
}

fn family_key(code: winit::keyboard::KeyCode) -> Option<KeyCode> {
    use winit::keyboard::KeyCode as W;
    Some(match code {
        W::KeyA => KeyCode::A,
        W::KeyB => KeyCode::B,
        W::KeyC => KeyCode::C,
        W::KeyD => KeyCode::D,
        W::KeyE => KeyCode::E,
        W::KeyF => KeyCode::F,
        W::KeyG => KeyCode::G,
        W::KeyH => KeyCode::H,
        W::KeyI => KeyCode::I,
        W::KeyJ => KeyCode::J,
        W::KeyK => KeyCode::K,
        W::KeyL => KeyCode::L,
        W::KeyM => KeyCode::M,
        W::KeyN => KeyCode::N,
        W::KeyO => KeyCode::O,
        W::KeyP => KeyCode::P,
        W::KeyQ => KeyCode::Q,
        W::KeyR => KeyCode::R,
        W::KeyS => KeyCode::S,
        W::KeyT => KeyCode::T,
        W::KeyU => KeyCode::U,
        W::KeyV => KeyCode::V,
        W::KeyW => KeyCode::W,
        W::KeyX => KeyCode::X,
        W::KeyY => KeyCode::Y,
        W::KeyZ => KeyCode::Z,
        W::Digit0 => KeyCode::Digit0,
        W::Digit1 => KeyCode::Digit1,
        W::Digit2 => KeyCode::Digit2,
        W::Digit3 => KeyCode::Digit3,
        W::Digit4 => KeyCode::Digit4,
        W::Digit5 => KeyCode::Digit5,
        W::Digit6 => KeyCode::Digit6,
        W::Digit7 => KeyCode::Digit7,
        W::Digit8 => KeyCode::Digit8,
        W::Digit9 => KeyCode::Digit9,
        W::Escape => KeyCode::Escape,
        W::Enter => KeyCode::Enter,
        W::Tab => KeyCode::Tab,
        W::Backspace => KeyCode::Backspace,
        W::Space => KeyCode::Space,
        W::Insert => KeyCode::Insert,
        W::Delete => KeyCode::Delete,
        W::Home => KeyCode::Home,
        W::End => KeyCode::End,
        W::PageUp => KeyCode::PageUp,
        W::PageDown => KeyCode::PageDown,
        W::ArrowLeft => KeyCode::ArrowLeft,
        W::ArrowRight => KeyCode::ArrowRight,
        W::ArrowUp => KeyCode::ArrowUp,
        W::ArrowDown => KeyCode::ArrowDown,
        W::ControlLeft => KeyCode::ControlLeft,
        W::ControlRight => KeyCode::ControlRight,
        W::ShiftLeft => KeyCode::ShiftLeft,
        W::ShiftRight => KeyCode::ShiftRight,
        W::AltLeft => KeyCode::AltLeft,
        W::AltRight => KeyCode::AltRight,
        W::F1 => KeyCode::F1,
        W::F2 => KeyCode::F2,
        W::F3 => KeyCode::F3,
        W::F4 => KeyCode::F4,
        W::F5 => KeyCode::F5,
        W::F6 => KeyCode::F6,
        W::F7 => KeyCode::F7,
        W::F8 => KeyCode::F8,
        W::F9 => KeyCode::F9,
        W::F10 => KeyCode::F10,
        W::F11 => KeyCode::F11,
        W::F12 => KeyCode::F12,
        W::Numpad0 => KeyCode::Numpad0,
        W::Numpad1 => KeyCode::Numpad1,
        W::Numpad2 => KeyCode::Numpad2,
        W::Numpad3 => KeyCode::Numpad3,
        W::Numpad4 => KeyCode::Numpad4,
        W::Numpad5 => KeyCode::Numpad5,
        W::Numpad6 => KeyCode::Numpad6,
        W::Numpad7 => KeyCode::Numpad7,
        W::Numpad8 => KeyCode::Numpad8,
        W::Numpad9 => KeyCode::Numpad9,
        W::NumpadEnter => KeyCode::NumpadEnter,
        W::NumpadAdd => KeyCode::NumpadAdd,
        W::NumpadSubtract => KeyCode::NumpadSubtract,
        W::NumpadMultiply => KeyCode::NumpadMultiply,
        W::NumpadDivide => KeyCode::NumpadDivide,
        W::Minus => KeyCode::Minus,
        W::Equal => KeyCode::Equals,
        W::Comma => KeyCode::Comma,
        W::Period => KeyCode::Period,
        W::Slash => KeyCode::Slash,
        W::Semicolon => KeyCode::Semicolon,
        W::Quote => KeyCode::Apostrophe,
        W::BracketLeft => KeyCode::BracketLeft,
        W::BracketRight => KeyCode::BracketRight,
        W::Backslash => KeyCode::Backslash,
        W::Backquote => KeyCode::Grave,
        _ => return None,
    })
}

/// The binding row Slint is capturing for; per-game when the game settings page owns it.
struct CaptureTarget {
    button: slint::SharedString,
    game_id: Option<slint::SharedString>,
}

fn capture_target(window: &ManagerWindow) -> Option<CaptureTarget> {
    let game = window.global::<GameSettings>();
    let button = game.get_capture_button();
    if !button.is_empty() {
        return Some(CaptureTarget {
            button,
            game_id: Some(game.get_game_id()),
        });
    }
    let button = window.global::<Settings>().get_capture_button();
    (!button.is_empty()).then_some(CaptureTarget {
        button,
        game_id: None,
    })
}

/// Mirror Slint's capture row (started, moved or cancelled by the UI) into the controller.
fn sync_capture<C: ManagerController>(
    controller: &RefCell<C>,
    target: Option<&CaptureTarget>,
) -> Result<Option<ManagerViewModel>, String> {
    let current = controller.borrow();
    let same = match (current.key_capture(), target) {
        (None, None) => true,
        (Some(state), Some(target)) => {
            state.button == target.button.as_str() && state.game_id == target.game_id.as_deref()
        }
        _ => false,
    };
    drop(current);
    if same {
        return Ok(None);
    }
    let mut controller = controller.borrow_mut();
    match target {
        Some(target) => controller
            .begin_key_capture(target.button.as_str(), target.game_id.as_deref())
            .map(Some),
        None => controller.cancel_key_capture().map(Some),
    }
}

fn clear_capture(window: &ManagerWindow) {
    let game = window.global::<GameSettings>();
    game.set_capture_button("".into());
    game.set_capture_hint("".into());
    let settings = window.global::<Settings>();
    settings.set_capture_button("".into());
    settings.set_capture_hint("".into());
}
