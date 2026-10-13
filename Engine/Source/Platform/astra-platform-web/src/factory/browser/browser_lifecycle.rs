use super::*;

impl BrowserLifecycle {
    pub(super) fn bind(
        emitter: astra_platform::PlatformEventEmitter,
    ) -> Result<Self, PlatformError> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| web_error("lifecycle.bind"))?;
        let target: EventTarget = document.clone().into();
        let visibility_emitter = emitter.clone();
        let visibility = Closure::wrap(Box::new(move |_| {
            let event = if document.hidden() {
                PlatformEventKind::Suspended
            } else {
                PlatformEventKind::Resumed
            };
            let _ = visibility_emitter.emit(event);
        }) as Box<dyn FnMut(Event)>);
        target
            .add_event_listener_with_callback(
                "visibilitychange",
                visibility.as_ref().unchecked_ref(),
            )
            .map_err(|_| web_error("lifecycle.bind"))?;
        let gamepads = gilrs::Gilrs::new().map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "input.gamepad.open",
                "browser Gamepad API initialization failed",
            )
        })?;
        let mapper = astra_platform_common::GamepadMapper::new(0.2)?;
        let state = Rc::new(RefCell::new((gamepads, mapper)));
        let gamepad_emitter = emitter.clone();
        let callback_slot: AnimationFrameCallback = Rc::new(RefCell::new(None));
        let callback_slot_for_callback = callback_slot.clone();
        let gamepad_window = web_sys::window().ok_or_else(|| web_error("input.gamepad.bind"))?;
        let gamepad_window_for_callback = gamepad_window.clone();
        let gamepad_callback = Closure::wrap(Box::new(move |_timestamp: f64| {
            let mut state = state.borrow_mut();
            while let Some(event) = state.0.next_event() {
                let Some(event) = raw_gamepad_event(event) else {
                    continue;
                };
                match state.1.apply_checked(event) {
                    Ok(events) => {
                        for event in events {
                            let _ = gamepad_emitter.emit(event);
                        }
                    }
                    Err(error) => tracing::warn!(
                        event = "platform.web.gamepad.invalid_event",
                        diagnostic_code = ?error.code,
                        operation = %error.operation,
                        "browser Gamepad event was rejected"
                    ),
                }
            }
            if let Some(callback) = callback_slot_for_callback.borrow().as_ref() {
                if gamepad_window_for_callback
                    .request_animation_frame(callback.as_ref().unchecked_ref())
                    .is_err()
                {
                    tracing::warn!(
                        event = "platform.web.gamepad.raf_failed",
                        diagnostic_code = "ASTRA_PLATFORM_GAMEPAD_RAF_FAILED",
                        "browser requestAnimationFrame could not schedule the gamepad source"
                    );
                }
            }
        }) as Box<dyn FnMut(f64)>);
        *callback_slot.borrow_mut() = Some(gamepad_callback);
        let gamepad_frame = {
            let callback_slot_ref = callback_slot.borrow();
            let callback = callback_slot_ref
                .as_ref()
                .ok_or_else(|| web_error("input.gamepad.bind"))?;
            gamepad_window
                .request_animation_frame(callback.as_ref().unchecked_ref())
                .map_err(|_| web_error("input.gamepad.bind"))?
        };
        Ok(Self {
            target,
            visibility,
            gamepad_frame,
            _gamepad_callback: callback_slot,
        })
    }
}

impl Drop for BrowserLifecycle {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(
            "visibilitychange",
            self.visibility.as_ref().unchecked_ref(),
        );
        if let Some(window) = web_sys::window() {
            let _ = window.cancel_animation_frame(self.gamepad_frame);
        }
    }
}

pub(super) fn raw_gamepad_event(
    event: gilrs::Event,
) -> Option<astra_platform_common::RawGamepadEvent> {
    use astra_platform::GamepadControl;
    use astra_platform_common::RawGamepadEvent;
    use gilrs::{Axis, Button, EventType};

    let raw_device_id = u32::try_from(usize::from(event.id)).ok()?;
    let map_button = |button| match button {
        Button::South => Some(GamepadControl::South),
        Button::East => Some(GamepadControl::East),
        Button::West => Some(GamepadControl::West),
        Button::North => Some(GamepadControl::North),
        Button::DPadUp => Some(GamepadControl::DpadUp),
        Button::DPadDown => Some(GamepadControl::DpadDown),
        Button::DPadLeft => Some(GamepadControl::DpadLeft),
        Button::DPadRight => Some(GamepadControl::DpadRight),
        Button::LeftTrigger => Some(GamepadControl::LeftShoulder),
        Button::RightTrigger => Some(GamepadControl::RightShoulder),
        Button::LeftTrigger2 => Some(GamepadControl::LeftTrigger),
        Button::RightTrigger2 => Some(GamepadControl::RightTrigger),
        Button::LeftThumb => Some(GamepadControl::LeftStickButton),
        Button::RightThumb => Some(GamepadControl::RightStickButton),
        Button::Start => Some(GamepadControl::Start),
        Button::Select => Some(GamepadControl::Select),
        _ => None,
    };
    let map_axis = |axis| match axis {
        Axis::LeftStickX => Some(GamepadControl::LeftStickX),
        Axis::LeftStickY => Some(GamepadControl::LeftStickY),
        Axis::RightStickX => Some(GamepadControl::RightStickX),
        Axis::RightStickY => Some(GamepadControl::RightStickY),
        _ => None,
    };
    match event.event {
        EventType::Connected => Some(RawGamepadEvent::Connected { raw_device_id }),
        EventType::Disconnected => Some(RawGamepadEvent::Disconnected { raw_device_id }),
        EventType::ButtonPressed(button, _) | EventType::ButtonRepeated(button, _) => {
            map_button(button).map(|control| RawGamepadEvent::Button {
                raw_device_id,
                control,
                pressed: true,
            })
        }
        EventType::ButtonReleased(button, _) => {
            map_button(button).map(|control| RawGamepadEvent::Button {
                raw_device_id,
                control,
                pressed: false,
            })
        }
        EventType::ButtonChanged(button, value, _) => map_button(button).map(|control| {
            if matches!(
                control,
                GamepadControl::LeftTrigger | GamepadControl::RightTrigger
            ) {
                RawGamepadEvent::Axis {
                    raw_device_id,
                    control,
                    value,
                }
            } else {
                RawGamepadEvent::Button {
                    raw_device_id,
                    control,
                    pressed: value >= 0.5,
                }
            }
        }),
        EventType::AxisChanged(axis, value, _) => {
            map_axis(axis).map(|control| RawGamepadEvent::Axis {
                raw_device_id,
                control,
                value,
            })
        }
        EventType::Dropped | EventType::ForceFeedbackEffectCompleted => None,
        _ => None,
    }
}

pub(super) type GlobalListener = (EventTarget, &'static str, Closure<dyn FnMut(Event)>);

pub(super) struct AriaListener {
    pub(super) target: EventTarget,
    pub(super) event: &'static str,
    pub(super) callback: Closure<dyn FnMut(Event)>,
}

impl Drop for AriaListener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(
            self.event,
            self.callback.as_ref().unchecked_ref(),
        );
    }
}

pub(super) struct CanvasResource {
    pub(super) canvas: HtmlCanvasElement,
    pub(super) aria_root: HtmlElement,
    pub(super) aria_listeners: Vec<AriaListener>,
    pub(super) aria_programmatic_focus: Rc<Cell<bool>>,
    pub(super) listeners: BTreeMap<&'static str, Closure<dyn FnMut(Event)>>,
    pub(super) global_listeners: Vec<GlobalListener>,
}
