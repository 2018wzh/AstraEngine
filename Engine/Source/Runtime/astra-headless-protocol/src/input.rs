use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ProtocolError, TICK_DURATION_NS, USER_INPUT_SEQUENCE_SCHEMA};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputMessage {
    pub schema: String,
    pub session: String,
    pub sequence: u64,
    pub tick: u64,
    pub event: PhysicalInput,
}

impl InputMessage {
    pub fn time_ns(&self) -> Result<u64, ProtocolError> {
        self.tick
            .checked_mul(TICK_DURATION_NS)
            .ok_or_else(|| ProtocolError::invalid("input.tick", "tick overflows canonical time"))
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != USER_INPUT_SEQUENCE_SCHEMA {
            return Err(ProtocolError::invalid(
                "input.schema",
                "unsupported input schema",
            ));
        }
        validate_symbol("input.session", &self.session)?;
        if self.sequence == 0 {
            return Err(ProtocolError::invalid(
                "input.sequence",
                "sequence must be non-zero",
            ));
        }
        self.time_ns()?;
        self.event.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhysicalInput {
    Resume,
    Focus {
        focused: bool,
    },
    Keyboard {
        physical_key: String,
        logical_key: Option<String>,
        state: ButtonState,
        repeat: bool,
    },
    ImePreedit {
        text: String,
        cursor_start: Option<u32>,
        cursor_end: Option<u32>,
    },
    ImeCommit {
        text: String,
    },
    PointerMove {
        x: u16,
        y: u16,
    },
    PointerButton {
        button: PointerButton,
        state: ButtonState,
    },
    Wheel {
        delta_x: i32,
        delta_y: i32,
    },
    Touch {
        id: u64,
        x: u16,
        y: u16,
        phase: TouchPhase,
    },
    GamepadConnection {
        device_id: u32,
        connected: bool,
    },
    GamepadInput {
        device_id: u32,
        control: GamepadControl,
        value: i16,
    },
    AdvanceTicks {
        count: u32,
    },
    Await {
        observation: ObservationPredicate,
        timeout_ticks: u32,
    },
    Checkpoint {
        id: String,
    },
    Shutdown,
}

impl PhysicalInput {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Keyboard {
                physical_key,
                logical_key,
                ..
            } => {
                validate_text("input.keyboard.physical_key", physical_key, 128)?;
                if let Some(key) = logical_key {
                    validate_text("input.keyboard.logical_key", key, 128)?;
                }
            }
            Self::ImePreedit {
                text,
                cursor_start,
                cursor_end,
            } => {
                validate_text("input.ime.text", text, 4096)?;
                if cursor_start
                    .zip(*cursor_end)
                    .is_some_and(|(start, end)| start > end)
                {
                    return Err(ProtocolError::invalid(
                        "input.ime.cursor",
                        "IME cursor range is reversed",
                    ));
                }
            }
            Self::ImeCommit { text } => validate_text("input.ime.text", text, 4096)?,
            Self::AdvanceTicks { count } if *count == 0 => {
                return Err(ProtocolError::invalid(
                    "input.advance_ticks",
                    "tick count must be non-zero",
                ))
            }
            Self::Await {
                observation,
                timeout_ticks,
            } => {
                if *timeout_ticks == 0 {
                    return Err(ProtocolError::invalid(
                        "input.await",
                        "await timeout must be non-zero",
                    ));
                }
                observation.validate()?;
            }
            Self::Checkpoint { id } => validate_symbol("input.checkpoint", id)?,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GamepadControl {
    South,
    East,
    West,
    North,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftStickButton,
    RightStickButton,
    Start,
    Select,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationPredicate {
    Equals { key: String, value_hash: String },
    Exists { key: String },
}

impl ObservationPredicate {
    fn validate(&self) -> Result<(), ProtocolError> {
        let (key, hash) = match self {
            Self::Equals { key, value_hash } => (key, Some(value_hash)),
            Self::Exists { key } => (key, None),
        };
        validate_symbol("input.await.key", key)?;
        if hash.is_some_and(|value| !is_sha256(value)) {
            return Err(ProtocolError::invalid(
                "input.await.value_hash",
                "observation hash must be sha256",
            ));
        }
        Ok(())
    }
}

pub(crate) fn validate_symbol(operation: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(ProtocolError::invalid(
            operation,
            "value is not a safe symbol",
        ));
    }
    Ok(())
}

pub(crate) fn validate_text(
    operation: &'static str,
    value: &str,
    max: usize,
) -> Result<(), ProtocolError> {
    if value.len() > max || value.chars().any(|c| c == '\0') {
        return Err(ProtocolError::invalid(
            operation,
            "text exceeds bounds or contains NUL",
        ));
    }
    Ok(())
}

pub(crate) fn is_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}
