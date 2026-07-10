use std::collections::BTreeMap;

use astra_platform::{GamepadControl, PlatformError, PlatformErrorCode, PlatformEventKind};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawGamepadEvent {
    Connected {
        raw_device_id: u32,
    },
    Disconnected {
        raw_device_id: u32,
    },
    Button {
        raw_device_id: u32,
        control: GamepadControl,
        pressed: bool,
    },
    Axis {
        raw_device_id: u32,
        control: GamepadControl,
        value: f32,
    },
}

pub struct GamepadMapper {
    deadzone: f32,
    devices: BTreeMap<u32, u32>,
    next_device_id: u32,
}

impl GamepadMapper {
    pub fn new(deadzone: f32) -> Result<Self, PlatformError> {
        if !deadzone.is_finite() || !(0.0..1.0).contains(&deadzone) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "input.gamepad.configure",
                "gamepad deadzone must be finite and within [0, 1)",
            ));
        }
        Ok(Self {
            deadzone,
            devices: BTreeMap::new(),
            next_device_id: 0,
        })
    }

    pub fn apply(&mut self, event: RawGamepadEvent) -> Vec<PlatformEventKind> {
        self.apply_checked(event).unwrap_or_default()
    }

    pub fn apply_checked(
        &mut self,
        event: RawGamepadEvent,
    ) -> Result<Vec<PlatformEventKind>, PlatformError> {
        match event {
            RawGamepadEvent::Connected { raw_device_id } => {
                if self.devices.contains_key(&raw_device_id) {
                    return Ok(Vec::new());
                }
                let device_id = self.next_device_id;
                self.next_device_id = self.next_device_id.checked_add(1).ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "input.gamepad.connect",
                        "gamepad session id space is exhausted",
                    )
                })?;
                self.devices.insert(raw_device_id, device_id);
                Ok(vec![PlatformEventKind::GamepadConnected { device_id }])
            }
            RawGamepadEvent::Disconnected { raw_device_id } => Ok(self
                .devices
                .remove(&raw_device_id)
                .map(|device_id| vec![PlatformEventKind::GamepadDisconnected { device_id }])
                .unwrap_or_default()),
            RawGamepadEvent::Button {
                raw_device_id,
                control,
                pressed,
            } => Ok(self
                .device(raw_device_id)
                .map(|device_id| {
                    vec![PlatformEventKind::GamepadInput {
                        device_id,
                        control,
                        value: f32::from(pressed),
                    }]
                })
                .unwrap_or_default()),
            RawGamepadEvent::Axis {
                raw_device_id,
                control,
                value,
            } => {
                if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "input.gamepad.axis",
                        "gamepad axis value is invalid",
                    ));
                }
                let normalized = normalize_axis(value, self.deadzone);
                Ok(self
                    .device(raw_device_id)
                    .filter(|_| normalized != 0.0)
                    .map(|device_id| {
                        vec![PlatformEventKind::GamepadInput {
                            device_id,
                            control,
                            value: normalized,
                        }]
                    })
                    .unwrap_or_default())
            }
        }
    }

    fn device(&self, raw_device_id: u32) -> Option<u32> {
        self.devices.get(&raw_device_id).copied()
    }
}

fn normalize_axis(value: f32, deadzone: f32) -> f32 {
    if value.abs() <= deadzone {
        0.0
    } else {
        value.signum() * ((value.abs() - deadzone) / (1.0 - deadzone))
    }
}
