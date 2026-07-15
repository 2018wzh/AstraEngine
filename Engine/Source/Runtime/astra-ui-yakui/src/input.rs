use std::collections::BTreeMap;

use astra_ui_core::{
    UiButtonState, UiInputDisposition, UiInputDispositionKind, UiInputEvent, UiInputEventKind,
    UiNavigationAction, UiPoint, UiPointerButton, UiTouchPhase, UiValidationError, ValidateUi,
};
use yakui_core::event::Event;
use yakui_core::geometry::{Rect, Vec2};
use yakui_core::input::{KeyCode, Modifiers, MouseButton};
use yakui_core::Yakui;

pub const MODIFIER_ALT: u32 = 1 << 0;
pub const MODIFIER_CONTROL: u32 = 1 << 1;
pub const MODIFIER_SHIFT: u32 = 1 << 2;
pub const MODIFIER_META: u32 = 1 << 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImeComposition {
    pub text: String,
    pub cursor_start: Option<u32>,
    pub cursor_end: Option<u32>,
}

#[derive(Debug, Default)]
pub struct AstraYakuiInputRouter {
    ime: Option<ImeComposition>,
    touches: BTreeMap<(u64, u64), UiPoint>,
    primary_touch: Option<(u64, u64)>,
    last_pointer: Option<UiPoint>,
    focused: bool,
}

impl AstraYakuiInputRouter {
    pub fn ime(&self) -> Option<&ImeComposition> {
        self.ime.as_ref()
    }

    pub fn touches(&self) -> &BTreeMap<(u64, u64), UiPoint> {
        &self.touches
    }

    pub fn route(
        &mut self,
        yakui: &mut Yakui,
        event: &UiInputEvent,
    ) -> Result<UiInputDisposition, UiValidationError> {
        event.validate()?;
        let consumed = match &event.kind {
            UiInputEventKind::FixedTime { .. } => false,
            UiInputEventKind::Focus { focused } => {
                self.focused = *focused;
                if !focused {
                    self.ime = None;
                    self.touches.clear();
                    self.primary_touch = None;
                    self.last_pointer = None;
                    yakui.handle_event(Event::CursorMoved(None));
                }
                false
            }
            UiInputEventKind::Resize { viewport } => {
                yakui.set_surface_size(Vec2::new(
                    viewport.physical_width as f32,
                    viewport.physical_height as f32,
                ));
                yakui.set_scale_factor(viewport.scale_factor * viewport.font_scale);
                yakui.handle_event(Event::ViewportChanged(Rect::from_pos_size(
                    Vec2::ZERO,
                    Vec2::new(
                        viewport.physical_width as f32,
                        viewport.physical_height as f32,
                    ),
                )))
            }
            UiInputEventKind::PointerMove { position } => {
                self.last_pointer = Some(*position);
                yakui.handle_event(Event::CursorMoved(Some(point(*position))))
            }
            UiInputEventKind::PointerButton {
                position,
                button,
                state,
            } => {
                self.last_pointer = Some(*position);
                let moved = yakui.handle_event(Event::CursorMoved(Some(point(*position))));
                let pressed = matches!(state, UiButtonState::Pressed);
                let button = match button {
                    UiPointerButton::Primary => Some(MouseButton::One),
                    UiPointerButton::Secondary => Some(MouseButton::Two),
                    UiPointerButton::Middle => Some(MouseButton::Three),
                    UiPointerButton::Back
                    | UiPointerButton::Forward
                    | UiPointerButton::Other(_) => None,
                };
                moved
                    || button.is_some_and(|button| {
                        yakui.handle_event(Event::MouseButtonChanged {
                            button,
                            down: pressed,
                        })
                    })
            }
            UiInputEventKind::Wheel { delta_points } => yakui.handle_event(Event::MouseScroll {
                delta: point(*delta_points),
            }),
            UiInputEventKind::Keyboard {
                physical_key,
                state,
                modifiers,
                ..
            } => map_key(physical_key).is_some_and(|key| {
                yakui.handle_event(Event::KeyChanged {
                    key,
                    down: matches!(state, UiButtonState::Pressed),
                    modifiers: Some(map_modifiers(*modifiers)),
                })
            }),
            UiInputEventKind::ImePreedit {
                text,
                cursor_start,
                cursor_end,
            } => {
                if yakui.text_input_enabled() {
                    self.ime = Some(ImeComposition {
                        text: text.clone(),
                        cursor_start: *cursor_start,
                        cursor_end: *cursor_end,
                    });
                    true
                } else {
                    false
                }
            }
            UiInputEventKind::ImeCommit { text } => {
                if yakui.text_input_enabled() {
                    self.ime = None;
                    let mut consumed = false;
                    for character in text.chars() {
                        consumed |= yakui.handle_event(Event::TextInput(character));
                    }
                    consumed
                } else {
                    false
                }
            }
            UiInputEventKind::Touch {
                device_id,
                contact_id,
                position,
                phase,
            } => self.route_touch(yakui, *device_id, *contact_id, *position, *phase),
            UiInputEventKind::Navigation { action } => {
                let (key, down) = match action {
                    UiNavigationAction::Next => (KeyCode::Tab, true),
                    UiNavigationAction::Previous => (KeyCode::Tab, true),
                    UiNavigationAction::Up => (KeyCode::ArrowUp, true),
                    UiNavigationAction::Down => (KeyCode::ArrowDown, true),
                    UiNavigationAction::Left => (KeyCode::ArrowLeft, true),
                    UiNavigationAction::Right => (KeyCode::ArrowRight, true),
                    UiNavigationAction::Activate => (KeyCode::Enter, true),
                    UiNavigationAction::Cancel => (KeyCode::Escape, true),
                    UiNavigationAction::PagePrevious => (KeyCode::PageUp, true),
                    UiNavigationAction::PageNext => (KeyCode::PageDown, true),
                };
                let modifiers = if matches!(action, UiNavigationAction::Previous) {
                    Modifiers::SHIFT
                } else {
                    Modifiers::empty()
                };
                let consumed = yakui.handle_event(Event::KeyChanged {
                    key,
                    down,
                    modifiers: Some(modifiers),
                });
                let _ = yakui.handle_event(Event::KeyChanged {
                    key,
                    down: false,
                    modifiers: Some(modifiers),
                });
                consumed
            }
            UiInputEventKind::AccessibilityAction { .. } => false,
        };
        Ok(UiInputDisposition {
            sequence: event.sequence,
            disposition: if consumed {
                UiInputDispositionKind::Consumed
            } else {
                UiInputDispositionKind::Bubble
            },
            semantic_target_id: None,
        })
    }

    fn route_touch(
        &mut self,
        yakui: &mut Yakui,
        device_id: u64,
        contact_id: u64,
        position: UiPoint,
        phase: UiTouchPhase,
    ) -> bool {
        let key = (device_id, contact_id);
        match phase {
            UiTouchPhase::Started => {
                self.touches.insert(key, position);
                if self.primary_touch.is_none() {
                    self.primary_touch = Some(key);
                    self.last_pointer = Some(position);
                    let moved = yakui.handle_event(Event::CursorMoved(Some(point(position))));
                    moved
                        || yakui.handle_event(Event::MouseButtonChanged {
                            button: MouseButton::One,
                            down: true,
                        })
                } else {
                    false
                }
            }
            UiTouchPhase::Moved => {
                if let Some(stored) = self.touches.get_mut(&key) {
                    *stored = position;
                }
                if self.primary_touch == Some(key) {
                    self.last_pointer = Some(position);
                    yakui.handle_event(Event::CursorMoved(Some(point(position))))
                } else {
                    false
                }
            }
            UiTouchPhase::Ended | UiTouchPhase::Cancelled => {
                self.touches.remove(&key);
                if self.primary_touch == Some(key) {
                    let moved = yakui.handle_event(Event::CursorMoved(Some(point(position))));
                    let released = yakui.handle_event(Event::MouseButtonChanged {
                        button: MouseButton::One,
                        down: false,
                    });
                    self.primary_touch = None;
                    moved || released
                } else {
                    false
                }
            }
        }
    }
}

fn point(value: UiPoint) -> Vec2 {
    Vec2::new(value.x, value.y)
}

fn map_modifiers(bits: u32) -> Modifiers {
    let mut modifiers = Modifiers::empty();
    if bits & MODIFIER_ALT != 0 {
        modifiers |= Modifiers::ALT;
    }
    if bits & MODIFIER_CONTROL != 0 {
        modifiers |= Modifiers::CONTROL;
    }
    if bits & MODIFIER_SHIFT != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if bits & MODIFIER_META != 0 {
        modifiers |= Modifiers::META;
    }
    modifiers
}

fn map_key(value: &str) -> Option<KeyCode> {
    Some(match value {
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "Enter" | "NumpadEnter" => KeyCode::Enter,
        "Escape" => KeyCode::Escape,
        "Space" => KeyCode::Space,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "KeyA" => KeyCode::KeyA,
        "KeyC" => KeyCode::KeyC,
        "KeyV" => KeyCode::KeyV,
        "KeyX" => KeyCode::KeyX,
        "KeyY" => KeyCode::KeyY,
        "KeyZ" => KeyCode::KeyZ,
        _ => return None,
    })
}
