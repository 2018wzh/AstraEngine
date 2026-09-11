use std::{cell::RefCell, rc::Rc};

use astra_emu_family_api::{FamilyEvent, KeyCode, KeyModifiers, KeyState, WindowState};
use astra_emu_manager_ui_slint::SlintManagerAdapter;
use slint::{
    winit_030::{EventResult, WinitWindowAccessor},
    ComponentHandle,
};
use winit::event::{ElementState, WindowEvent};

use super::ManagerController;

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
                focused: window.has_focus(),
                visible: window
                    .is_visible()
                    .ok_or("ASTRA_EMU_WINDOW_VISIBILITY_UNAVAILABLE")?,
            })
        })
        .ok_or_else(|| "ASTRA_EMU_WINDOW_UNAVAILABLE".to_owned())?
}

pub(super) fn install<C: ManagerController>(
    adapter: &Rc<SlintManagerAdapter>,
    controller: Rc<RefCell<C>>,
) {
    let weak = adapter.window().as_weak();
    let weak_adapter = Rc::downgrade(adapter);
    let mut modifiers = KeyModifiers {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    };
    adapter
        .window()
        .window()
        .on_winit_window_event(move |native_window, event| {
            let Some(window) = weak.upgrade() else {
                return EventResult::Propagate;
            };
            let input_active = window.get_game_active()
                && !window.get_translation_overlay_active()
                && !window.get_diagnostics_overlay_active()
                && !window.get_filters_overlay_active();
            let mut consumed = false;
            let result = match event {
                WindowEvent::ThemeChanged(theme) => {
                    if window.get_theme_mode() == "system" {
                        let dark = *theme == winit::window::Theme::Dark;
                        controller.borrow_mut().set_system_theme(dark);
                        window.set_theme_dark(dark);
                    }
                    Ok(())
                }
                WindowEvent::Focused(focused) => {
                    current_window_state(native_window).and_then(|mut state| {
                        state.focused = *focused;
                        controller.borrow_mut().set_window_state(state)
                    })
                }
                WindowEvent::Resized(_) | WindowEvent::Occluded(_) => {
                    current_window_state(native_window).and_then(|mut state| {
                        if let WindowEvent::Occluded(occluded) = event {
                            state.visible = !occluded;
                        }
                        controller.borrow_mut().set_window_state(state)
                    })
                }
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
                WindowEvent::CloseRequested => controller
                    .borrow_mut()
                    .physical_event(FamilyEvent::WindowCloseRequested),
                _ => Ok(()),
            };
            if let Err(error) = result {
                if controller.borrow().is_game_active() {
                    super::terminate_game(&controller, &weak_adapter, &window, error);
                } else {
                    window.set_global_diagnostic(error.into());
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
