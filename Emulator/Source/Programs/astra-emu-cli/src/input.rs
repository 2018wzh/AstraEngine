use std::{fs, path::Path};

use astra_core::Hash256;
use astra_headless_protocol::{InputMessage, PhysicalInput};

pub const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_INPUT_MESSAGES: usize = 100_000;

pub struct ValidatedInputSequence {
    pub session: String,
    pub hash: Hash256,
    pub messages: Vec<InputMessage>,
    pub final_tick: u64,
    pub max_execution_tick: u64,
}

pub fn read_input_sequence(path: &Path) -> Result<ValidatedInputSequence, String> {
    let metadata = fs::metadata(path).map_err(|_| "ASTRA_EMU_HEADLESS_INPUT_READ".to_owned())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_INPUT_BYTES {
        return Err("ASTRA_EMU_HEADLESS_INPUT_BOUNDS".into());
    }
    let bytes = fs::read(path).map_err(|_| "ASTRA_EMU_HEADLESS_INPUT_READ".to_owned())?;
    let mut messages = Vec::new();
    let mut expected_session = None::<String>;
    let mut previous_sequence = 0_u64;
    let mut previous_tick = 0_u64;
    for (line_index, raw) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if messages.len() >= MAX_INPUT_MESSAGES {
            return Err("ASTRA_EMU_HEADLESS_INPUT_MESSAGE_BOUNDS".into());
        }
        let message: InputMessage = serde_json::from_slice(line).map_err(|error| {
            format!(
                "ASTRA_EMU_HEADLESS_INPUT_PARSE:line={};category={:?}",
                line_index + 1,
                error.classify()
            )
        })?;
        message
            .validate()
            .map_err(|_| "ASTRA_EMU_HEADLESS_INPUT_INVALID".to_owned())?;
        if expected_session.get_or_insert_with(|| message.session.clone()) != &message.session {
            return Err("ASTRA_EMU_HEADLESS_INPUT_SESSION_DRIFT".into());
        }
        if message.sequence <= previous_sequence || message.tick < previous_tick {
            return Err("ASTRA_EMU_HEADLESS_INPUT_ORDER".into());
        }
        previous_sequence = message.sequence;
        previous_tick = message.tick;
        messages.push(message);
    }
    if messages.is_empty() {
        return Err("ASTRA_EMU_HEADLESS_INPUT_EMPTY".into());
    }
    if !matches!(
        messages.last().map(|message| &message.event),
        Some(PhysicalInput::Shutdown)
    ) {
        return Err("ASTRA_EMU_HEADLESS_INPUT_SHUTDOWN_REQUIRED".into());
    }
    if messages[..messages.len() - 1]
        .iter()
        .any(|message| matches!(message.event, PhysicalInput::Shutdown))
    {
        return Err("ASTRA_EMU_HEADLESS_INPUT_SHUTDOWN_ORDER".into());
    }
    let canonical = messages
        .iter()
        .map(serde_json::to_vec)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "ASTRA_EMU_HEADLESS_INPUT_CANONICALIZE".to_owned())?;
    let mut joined = Vec::new();
    for line in canonical {
        joined.extend_from_slice(&line);
        joined.push(b'\n');
    }
    let max_execution_tick = maximum_execution_tick(&messages)?;
    Ok(ValidatedInputSequence {
        session: expected_session.ok_or_else(|| "ASTRA_EMU_HEADLESS_INPUT_EMPTY".to_owned())?,
        hash: Hash256::from_sha256(&joined),
        messages,
        final_tick: previous_tick,
        max_execution_tick,
    })
}

fn maximum_execution_tick(messages: &[InputMessage]) -> Result<u64, String> {
    let mut execution_tick = 0_u64;
    for message in messages {
        execution_tick = execution_tick.max(message.tick);
        let additional_ticks = match &message.event {
            PhysicalInput::AdvanceTicks { count } => u64::from(*count),
            PhysicalInput::Await { timeout_ticks, .. } => u64::from(*timeout_ticks),
            _ => 0,
        };
        execution_tick = execution_tick
            .checked_add(additional_ticks)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_INPUT_EXECUTION_BUDGET_OVERFLOW".to_owned())?;
    }
    Ok(execution_tick)
}

#[cfg(test)]
mod tests {
    use astra_headless_protocol::{ButtonState, USER_INPUT_SEQUENCE_SCHEMA};

    use super::*;

    #[test]
    fn validates_physical_input_order_and_terminal_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input.jsonl");
        let messages = [
            InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: "run-1".into(),
                sequence: 1,
                tick: 0,
                event: PhysicalInput::Keyboard {
                    physical_key: "Enter".into(),
                    logical_key: Some("Enter".into()),
                    state: ButtonState::Pressed,
                    repeat: false,
                },
            },
            InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: "run-1".into(),
                sequence: 2,
                tick: 1,
                event: PhysicalInput::Shutdown,
            },
        ];
        let bytes = messages
            .iter()
            .map(|message| serde_json::to_string(message).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&input, bytes).unwrap();
        let loaded = read_input_sequence(&input).unwrap();
        assert_eq!(loaded.session, "run-1");
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.max_execution_tick, 1);
    }

    #[test]
    fn execution_budget_counts_await_and_advance_without_double_counting_reserved_ticks() {
        let messages = [
            InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: "run-budget".into(),
                sequence: 1,
                tick: 5,
                event: PhysicalInput::Await {
                    observation: astra_headless_protocol::ObservationPredicate::Exists {
                        key: "runtime.tick".into(),
                    },
                    timeout_ticks: 10,
                    continue_at_match: true,
                },
            },
            InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: "run-budget".into(),
                sequence: 2,
                tick: 15,
                event: PhysicalInput::AdvanceTicks { count: 3 },
            },
            InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: "run-budget".into(),
                sequence: 3,
                tick: 16,
                event: PhysicalInput::Await {
                    observation: astra_headless_protocol::ObservationPredicate::Exists {
                        key: "runtime.terminal".into(),
                    },
                    timeout_ticks: 20,
                    continue_at_match: false,
                },
            },
            InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: "run-budget".into(),
                sequence: 4,
                tick: 17,
                event: PhysicalInput::Shutdown,
            },
        ];

        assert_eq!(maximum_execution_tick(&messages).unwrap(), 38);
    }
}
