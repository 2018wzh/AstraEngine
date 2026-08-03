use astra_emu_manager_core::{GamepadInput, InputMapping};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GameInput {
    pub(crate) control: String,
    pub(crate) pressed: bool,
    pub(crate) value: f32,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(crate) struct GameInputPump {
    backend: Option<gilrs::Gilrs>,
    mapping: InputMapping,
    left_x: DirectionalAxis,
    left_y: DirectionalAxis,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
impl GameInputPump {
    pub(crate) fn new(mapping: InputMapping) -> Self {
        let backend = match gilrs::Gilrs::new() {
            Ok(backend) => Some(backend),
            Err(error) => {
                tracing::warn!(
                    event = "astra.emu.input.gamepad_backend_unavailable",
                    diagnostic_code = "ASTRA_EMU_GAMEPAD_BACKEND_UNAVAILABLE",
                    error_kind = %error
                );
                None
            }
        };
        let (press, release) = mapping.deadzone.thresholds();
        Self {
            backend,
            left_x: DirectionalAxis::new(press, release),
            left_y: DirectionalAxis::new(press, release),
            mapping,
        }
    }

    /// Replace the active mapping, re-tuning stick hysteresis.
    pub(crate) fn set_mapping(&mut self, mapping: InputMapping) {
        let (press, release) = mapping.deadzone.thresholds();
        self.left_x.set_thresholds(press, release);
        self.left_y.set_thresholds(press, release);
        self.mapping = mapping;
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<GameInput>, String> {
        let mut output = Vec::new();
        let Some(backend) = self.backend.as_mut() else {
            return Ok(output);
        };
        if !self.mapping.gamepad_enabled {
            // Drain the event queue so stale events do not burst when the
            // gamepad is re-enabled, but emit nothing.
            while backend.next_event().is_some() {}
            return Ok(output);
        }
        while let Some(event) = backend.next_event() {
            use gilrs::{Axis, EventType};
            match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(control) = map_button(&self.mapping, button) {
                        output.push(GameInput {
                            control,
                            pressed: true,
                            value: 1.0,
                        });
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(control) = map_button(&self.mapping, button) {
                        output.push(GameInput {
                            control,
                            pressed: false,
                            value: 0.0,
                        });
                    }
                }
                EventType::AxisChanged(Axis::LeftStickX, value, _) => {
                    let negative = self
                        .mapping
                        .gamepad
                        .get(&GamepadInput::LeftStickLeft)
                        .cloned();
                    let positive = self
                        .mapping
                        .gamepad
                        .get(&GamepadInput::LeftStickRight)
                        .cloned();
                    self.left_x.update(value, negative, positive, &mut output);
                }
                EventType::AxisChanged(Axis::LeftStickY, value, _) => {
                    let negative = self
                        .mapping
                        .gamepad
                        .get(&GamepadInput::LeftStickDown)
                        .cloned();
                    let positive = self
                        .mapping
                        .gamepad
                        .get(&GamepadInput::LeftStickUp)
                        .cloned();
                    self.left_y.update(value, negative, positive, &mut output);
                }
                _ => {}
            }
        }
        Ok(output)
    }
}

fn map_button(mapping: &InputMapping, button: gilrs::Button) -> Option<String> {
    use gilrs::Button;
    let input = match button {
        Button::South => GamepadInput::South,
        Button::East => GamepadInput::East,
        Button::North => GamepadInput::North,
        Button::West => GamepadInput::West,
        Button::Start => GamepadInput::Start,
        Button::Select => GamepadInput::Select,
        Button::DPadUp => GamepadInput::DpadUp,
        Button::DPadDown => GamepadInput::DpadDown,
        Button::DPadLeft => GamepadInput::DpadLeft,
        Button::DPadRight => GamepadInput::DpadRight,
        Button::LeftTrigger => GamepadInput::LeftShoulder,
        Button::RightTrigger => GamepadInput::RightShoulder,
        Button::LeftTrigger2 => GamepadInput::LeftTrigger,
        Button::RightTrigger2 => GamepadInput::RightTrigger,
        Button::LeftThumb => GamepadInput::LeftThumb,
        Button::RightThumb => GamepadInput::RightThumb,
        _ => return None,
    };
    mapping.gamepad.get(&input).cloned()
}

#[derive(Debug)]
struct DirectionalAxis {
    press_threshold: f32,
    release_threshold: f32,
    negative_pressed: bool,
    positive_pressed: bool,
}

impl DirectionalAxis {
    fn new(press_threshold: f32, release_threshold: f32) -> Self {
        Self {
            press_threshold,
            release_threshold,
            negative_pressed: false,
            positive_pressed: false,
        }
    }

    fn set_thresholds(&mut self, press_threshold: f32, release_threshold: f32) {
        self.press_threshold = press_threshold;
        self.release_threshold = release_threshold;
    }

    fn update(
        &mut self,
        value: f32,
        negative_control: Option<String>,
        positive_control: Option<String>,
        output: &mut Vec<GameInput>,
    ) {
        if !value.is_finite() {
            tracing::warn!(
                event = "astra.emu.input.gamepad_axis_rejected",
                diagnostic_code = "ASTRA_EMU_GAMEPAD_AXIS_INVALID"
            );
            return;
        }
        let negative = if self.negative_pressed {
            value <= -self.release_threshold
        } else {
            value <= -self.press_threshold
        };
        let positive = if self.positive_pressed {
            value >= self.release_threshold
        } else {
            value >= self.press_threshold
        };
        update_button(
            &mut self.negative_pressed,
            negative,
            negative_control,
            output,
        );
        update_button(
            &mut self.positive_pressed,
            positive,
            positive_control,
            output,
        );
    }
}

fn update_button(
    previous: &mut bool,
    next: bool,
    control: Option<String>,
    output: &mut Vec<GameInput>,
) {
    if *previous == next {
        return;
    }
    *previous = next;
    if let Some(control) = control {
        output.push(GameInput {
            control,
            pressed: next,
            value: if next { 1.0 } else { 0.0 },
        });
    }
}

#[cfg(target_os = "android")]
pub(crate) struct GameInputPump;

#[cfg(target_os = "android")]
impl GameInputPump {
    pub(crate) fn new(_mapping: InputMapping) -> Self {
        Self
    }

    pub(crate) fn set_mapping(&mut self, _mapping: InputMapping) {}

    pub(crate) fn poll(&mut self) -> Result<Vec<GameInput>, String> {
        crate::android_platform::take_pending_gamepad_inputs().map(|events| {
            events
                .into_iter()
                .map(|event| GameInput {
                    control: event.control.to_owned(),
                    pressed: event.pressed,
                    value: event.value,
                })
                .collect()
        })
    }
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
pub(crate) struct GameInputPump;

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
impl GameInputPump {
    pub(crate) fn new(_mapping: InputMapping) -> Self {
        Self
    }

    pub(crate) fn set_mapping(&mut self, _mapping: InputMapping) {}

    pub(crate) fn poll(&mut self) -> Result<Vec<GameInput>, String> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_axis_uses_hysteresis_and_ordered_edges() {
        let mut axis = DirectionalAxis::new(0.55, 0.35);
        let mut output = Vec::new();
        axis.update(
            -0.7,
            Some("arrow_left".into()),
            Some("arrow_right".into()),
            &mut output,
        );
        axis.update(
            -0.4,
            Some("arrow_left".into()),
            Some("arrow_right".into()),
            &mut output,
        );
        axis.update(
            0.0,
            Some("arrow_left".into()),
            Some("arrow_right".into()),
            &mut output,
        );
        axis.update(
            0.8,
            Some("arrow_left".into()),
            Some("arrow_right".into()),
            &mut output,
        );
        axis.update(
            -0.8,
            Some("arrow_left".into()),
            Some("arrow_right".into()),
            &mut output,
        );
        assert_eq!(
            output,
            vec![
                GameInput {
                    control: "arrow_left".into(),
                    pressed: true,
                    value: 1.0
                },
                GameInput {
                    control: "arrow_left".into(),
                    pressed: false,
                    value: 0.0
                },
                GameInput {
                    control: "arrow_right".into(),
                    pressed: true,
                    value: 1.0
                },
                GameInput {
                    control: "arrow_left".into(),
                    pressed: true,
                    value: 1.0
                },
                GameInput {
                    control: "arrow_right".into(),
                    pressed: false,
                    value: 0.0
                },
            ]
        );
    }

    #[test]
    fn directional_axis_rejects_non_finite_values_without_state_change() {
        let mut axis = DirectionalAxis::new(0.55, 0.35);
        let mut output = Vec::new();
        axis.update(
            f32::NAN,
            Some("arrow_left".into()),
            Some("arrow_right".into()),
            &mut output,
        );
        assert!(output.is_empty());
        assert!(!axis.negative_pressed);
        assert!(!axis.positive_pressed);
    }

    #[test]
    fn unmapped_direction_emits_nothing_but_tracks_state() {
        let mut axis = DirectionalAxis::new(0.55, 0.35);
        let mut output = Vec::new();
        axis.update(-0.7, None, Some("arrow_right".into()), &mut output);
        assert!(output.is_empty());
        assert!(axis.negative_pressed);
        // Releasing the unmapped direction also emits nothing.
        axis.update(0.0, None, Some("arrow_right".into()), &mut output);
        assert!(output.is_empty());
        assert!(!axis.negative_pressed);
    }
}
