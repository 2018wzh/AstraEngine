#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GameInput {
    pub(crate) control: &'static str,
    pub(crate) pressed: bool,
    pub(crate) value: f32,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(crate) struct GameInputPump {
    backend: Option<gilrs::Gilrs>,
    left_x: DirectionalAxis,
    left_y: DirectionalAxis,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
impl GameInputPump {
    pub(crate) fn new() -> Self {
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
        Self {
            backend,
            left_x: DirectionalAxis::default(),
            left_y: DirectionalAxis::default(),
        }
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<GameInput>, String> {
        let mut output = Vec::new();
        let Some(backend) = self.backend.as_mut() else {
            return Ok(output);
        };
        while let Some(event) = backend.next_event() {
            use gilrs::{Axis, EventType};
            match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(control) = map_button(button) {
                        output.push(GameInput {
                            control,
                            pressed: true,
                            value: 1.0,
                        });
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(control) = map_button(button) {
                        output.push(GameInput {
                            control,
                            pressed: false,
                            value: 0.0,
                        });
                    }
                }
                EventType::AxisChanged(Axis::LeftStickX, value, _) => {
                    self.left_x.update(value, "left", "right", &mut output);
                }
                EventType::AxisChanged(Axis::LeftStickY, value, _) => {
                    self.left_y.update(value, "down", "up", &mut output);
                }
                _ => {}
            }
        }
        Ok(output)
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn map_button(button: gilrs::Button) -> Option<&'static str> {
    use gilrs::Button;
    match button {
        Button::South | Button::Start => Some("confirm"),
        Button::East | Button::Select => Some("cancel"),
        Button::DPadUp => Some("up"),
        Button::DPadDown => Some("down"),
        Button::DPadLeft => Some("left"),
        Button::DPadRight => Some("right"),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct DirectionalAxis {
    negative_pressed: bool,
    positive_pressed: bool,
}

impl DirectionalAxis {
    const PRESS_THRESHOLD: f32 = 0.55;
    const RELEASE_THRESHOLD: f32 = 0.35;

    fn update(
        &mut self,
        value: f32,
        negative_control: &'static str,
        positive_control: &'static str,
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
            value <= -Self::RELEASE_THRESHOLD
        } else {
            value <= -Self::PRESS_THRESHOLD
        };
        let positive = if self.positive_pressed {
            value >= Self::RELEASE_THRESHOLD
        } else {
            value >= Self::PRESS_THRESHOLD
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
    control: &'static str,
    output: &mut Vec<GameInput>,
) {
    if *previous == next {
        return;
    }
    *previous = next;
    output.push(GameInput {
        control,
        pressed: next,
        value: if next { 1.0 } else { 0.0 },
    });
}

#[cfg(target_os = "android")]
pub(crate) struct GameInputPump;

#[cfg(target_os = "android")]
impl GameInputPump {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<GameInput>, String> {
        crate::android_platform::take_pending_gamepad_inputs().map(|events| {
            events
                .into_iter()
                .map(|event| GameInput {
                    control: event.control,
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
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<GameInput>, String> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_axis_uses_hysteresis_and_ordered_edges() {
        let mut axis = DirectionalAxis::default();
        let mut output = Vec::new();
        axis.update(-0.7, "left", "right", &mut output);
        axis.update(-0.4, "left", "right", &mut output);
        axis.update(0.0, "left", "right", &mut output);
        axis.update(0.8, "left", "right", &mut output);
        axis.update(-0.8, "left", "right", &mut output);
        assert_eq!(
            output,
            vec![
                GameInput {
                    control: "left",
                    pressed: true,
                    value: 1.0
                },
                GameInput {
                    control: "left",
                    pressed: false,
                    value: 0.0
                },
                GameInput {
                    control: "right",
                    pressed: true,
                    value: 1.0
                },
                GameInput {
                    control: "left",
                    pressed: true,
                    value: 1.0
                },
                GameInput {
                    control: "right",
                    pressed: false,
                    value: 0.0
                },
            ]
        );
    }

    #[test]
    fn directional_axis_rejects_non_finite_values_without_state_change() {
        let mut axis = DirectionalAxis::default();
        let mut output = Vec::new();
        axis.update(f32::NAN, "left", "right", &mut output);
        assert!(output.is_empty());
        assert!(!axis.negative_pressed);
        assert!(!axis.positive_pressed);
    }
}
