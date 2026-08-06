use astra_emu_manager_core::{GamepadInput, InputMapping};

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GameInput {
    pub(crate) control: String,
    pub(crate) pressed: bool,
    pub(crate) value: f32,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(crate) type GameInputWake = std::sync::Arc<dyn Fn() + Send + Sync + 'static>;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub(crate) struct GameInputPump {
    batches: Option<Receiver<Result<Vec<GameInput>, String>>>,
    mapping: Arc<Mutex<InputMapping>>,
    wake: Arc<Mutex<Option<GameInputWake>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
impl GameInputPump {
    pub(crate) fn new(mapping: InputMapping) -> Self {
        let mapping = Arc::new(Mutex::new(mapping));
        let worker_mapping = Arc::clone(&mapping);
        let wake = Arc::new(Mutex::new(None));
        let worker_wake = Arc::clone(&wake);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (batch_tx, batches) = mpsc::sync_channel(32);
        let worker = match thread::Builder::new()
            .name("astra-manager-gamepad".to_string())
            .spawn(move || {
                gamepad_worker(worker_mapping, worker_wake, worker_stop, batch_tx.clone())
            }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                tracing::error!(
                    event = "astra.emu.input.gamepad_worker_create_failed",
                    diagnostic_code = "ASTRA_EMU_GAMEPAD_WORKER_CREATE",
                    error_kind = %error
                );
                None
            }
        };
        Self {
            batches: Some(batches),
            mapping,
            wake,
            stop,
            worker,
        }
    }

    /// Replace the active mapping, re-tuning stick hysteresis.
    pub(crate) fn set_mapping(&mut self, mapping: InputMapping) {
        if let Ok(mut current) = self.mapping.lock() {
            *current = mapping;
        }
    }

    /// Installs the host event-loop wake used by the worker.  The worker never
    /// touches UI state; it only signals that a bounded batch is ready so the
    /// host can drain it on its own thread.
    pub(crate) fn set_wake(&mut self, wake: GameInputWake) {
        if let Ok(mut current) = self.wake.lock() {
            *current = Some(wake.clone());
        }
        // The worker can receive a device event during startup before the
        // host installs its callback. One explicit wake closes that race; the
        // UI still drains only the bounded channel and never starts polling.
        wake();
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<GameInput>, String> {
        let mut output = Vec::new();
        let Some(batches) = self.batches.as_ref() else {
            return Ok(output);
        };
        while let Ok(batch) = batches.try_recv() {
            output.extend(batch?);
        }
        Ok(output)
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
impl Drop for GameInputPump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Dropping the receiver releases a worker blocked by a full bounded
        // queue, so shutdown never relies on a spin/yield polling loop.
        self.batches = None;
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                tracing::error!(
                    event = "astra.emu.input.gamepad_worker_panic",
                    diagnostic_code = "ASTRA_EMU_GAMEPAD_WORKER_PANIC"
                );
            }
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn gamepad_worker(
    mapping: Arc<Mutex<InputMapping>>,
    wake: Arc<Mutex<Option<GameInputWake>>>,
    stop: Arc<AtomicBool>,
    batches: SyncSender<Result<Vec<GameInput>, String>>,
) {
    let mut backend = match gilrs::Gilrs::new() {
        Ok(backend) => backend,
        Err(error) => {
            send_gamepad_batch(
                &batches,
                &wake,
                &stop,
                Err(format!("ASTRA_EMU_GAMEPAD_BACKEND_UNAVAILABLE:{error}")),
            );
            return;
        }
    };
    let initial = match mapping.lock() {
        Ok(value) => value.clone(),
        Err(_) => {
            send_gamepad_batch(
                &batches,
                &wake,
                &stop,
                Err("ASTRA_EMU_GAMEPAD_MAPPING_POISONED".to_owned()),
            );
            return;
        }
    };
    let (press, release) = initial.deadzone.thresholds();
    let mut left_x = DirectionalAxis::new(press, release);
    let mut left_y = DirectionalAxis::new(press, release);
    while !stop.load(Ordering::Acquire) {
        let Some(event) = backend.next_event_blocking(Some(Duration::from_millis(250))) else {
            continue;
        };
        let current_mapping = match mapping.lock() {
            Ok(value) => value.clone(),
            Err(_) => {
                send_gamepad_batch(
                    &batches,
                    &wake,
                    &stop,
                    Err("ASTRA_EMU_GAMEPAD_MAPPING_POISONED".to_owned()),
                );
                return;
            }
        };
        if !current_mapping.gamepad_enabled {
            continue;
        }
        let mut output = Vec::new();
        process_gamepad_event(
            &current_mapping,
            &mut left_x,
            &mut left_y,
            event,
            &mut output,
        );
        if !output.is_empty() {
            send_gamepad_batch(&batches, &wake, &stop, Ok(output));
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn send_gamepad_batch(
    batches: &SyncSender<Result<Vec<GameInput>, String>>,
    wake: &Arc<Mutex<Option<GameInputWake>>>,
    stop: &AtomicBool,
    batch: Result<Vec<GameInput>, String>,
) {
    if stop.load(Ordering::Acquire) || batches.send(batch).is_err() {
        return;
    }
    let callback = wake.lock().ok().and_then(|guard| guard.clone());
    if let Some(callback) = callback {
        callback();
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn process_gamepad_event(
    mapping: &InputMapping,
    left_x: &mut DirectionalAxis,
    left_y: &mut DirectionalAxis,
    event: gilrs::Event,
    output: &mut Vec<GameInput>,
) {
    use gilrs::{Axis, EventType};
    match event.event {
        EventType::ButtonPressed(button, _) => {
            if let Some(control) = map_button(mapping, button) {
                output.push(GameInput {
                    control,
                    pressed: true,
                    value: 1.0,
                });
            }
        }
        EventType::ButtonReleased(button, _) => {
            if let Some(control) = map_button(mapping, button) {
                output.push(GameInput {
                    control,
                    pressed: false,
                    value: 0.0,
                });
            }
        }
        EventType::AxisChanged(Axis::LeftStickX, value, _) => {
            let negative = mapping.gamepad.get(&GamepadInput::LeftStickLeft).cloned();
            let positive = mapping.gamepad.get(&GamepadInput::LeftStickRight).cloned();
            left_x.update(value, negative, positive, output);
        }
        EventType::AxisChanged(Axis::LeftStickY, value, _) => {
            let negative = mapping.gamepad.get(&GamepadInput::LeftStickDown).cloned();
            let positive = mapping.gamepad.get(&GamepadInput::LeftStickUp).cloned();
            left_y.update(value, negative, positive, output);
        }
        _ => {}
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

    pub(crate) fn set_wake(&mut self, _wake: std::sync::Arc<dyn Fn() + Send + Sync + 'static>) {}

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

    pub(crate) fn set_wake(&mut self, _wake: std::sync::Arc<dyn Fn() + Send + Sync + 'static>) {}

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
