use std::{fs, io::BufReader, path::Path};

use astra_headless_protocol::{
    CheckpointConfig, JsonlReader, ObservationPredicate, PhysicalInput, ProductSession,
};
use astra_platform::HeadlessHostProfile;
use sha2::{Digest, Sha256};

use super::*;

pub(crate) fn physical_input_kind(input: &PhysicalInput) -> &'static str {
    match input {
        PhysicalInput::Resume => "resume",
        PhysicalInput::Focus { .. } => "focus",
        PhysicalInput::Keyboard { .. } => "keyboard",
        PhysicalInput::ImePreedit { .. } => "ime_preedit",
        PhysicalInput::ImeCommit { .. } => "ime_commit",
        PhysicalInput::PointerMove { .. } => "pointer_move",
        PhysicalInput::PointerButton { .. } => "pointer_button",
        PhysicalInput::Wheel { .. } => "wheel",
        PhysicalInput::Touch { .. } => "touch",
        PhysicalInput::GamepadConnection { .. } => "gamepad_connection",
        PhysicalInput::GamepadInput { .. } => "gamepad_input",
        PhysicalInput::AdvanceTicks { .. } => "advance_ticks",
        PhysicalInput::Await { .. } => "await",
        PhysicalInput::Checkpoint { .. } => "checkpoint",
        PhysicalInput::Shutdown => "shutdown",
    }
}

pub(crate) fn blocked_profile_field(
    path: &Path,
    select: impl FnOnce(HeadlessHostProfile) -> String,
) -> Option<String> {
    serde_json::from_slice::<HeadlessHostProfile>(&fs::read(path).ok()?)
        .ok()
        .map(select)
}

pub(crate) fn diagnostic_code(error: &str) -> String {
    error
        .split(|character: char| character == ':' || character.is_whitespace())
        .next()
        .filter(|code| {
            code.starts_with("ASTRA_")
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .unwrap_or("ASTRA_HEADLESS_RUN_BLOCKED")
        .to_string()
}

pub(crate) fn read_identity_hash(path: &Path) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("ASTRA_BUILD_IDENTITY_READ_FAILED: {e}"))?,
    )
    .map_err(|e| format!("ASTRA_BUILD_IDENTITY_INVALID: {e}"))?;
    if value.get("schema").and_then(|v| v.as_str()) != Some("astra.build_identity.v1") {
        return Err("ASTRA_BUILD_IDENTITY_SCHEMA_INVALID".into());
    }
    let hash = value
        .get("identity_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "ASTRA_BUILD_IDENTITY_HASH_MISSING".to_string())?;
    if !is_hash(hash) {
        return Err("ASTRA_BUILD_IDENTITY_HASH_INVALID".into());
    }
    Ok(hash.to_owned())
}

pub(crate) struct PerformanceBuildIdentity {
    pub(crate) checkout_id: String,
    pub(crate) dirty: bool,
}

pub(crate) fn read_performance_build_identity(
    path: &Path,
) -> Result<PerformanceBuildIdentity, String> {
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("ASTRA_BUILD_IDENTITY_READ_FAILED: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_BUILD_IDENTITY_INVALID: {error}"))?;
    let checkout_id = value
        .get("checkout_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("ASTRA_BUILD_IDENTITY_CHECKOUT_INVALID")?;
    let dirty = value
        .get("dirty")
        .and_then(serde_json::Value::as_bool)
        .ok_or("ASTRA_BUILD_IDENTITY_DIRTY_STATE_MISSING")?;
    if dirty {
        return Err("ASTRA_PERFORMANCE_DIRTY_CHECKOUT_BLOCKED".into());
    }
    Ok(PerformanceBuildIdentity {
        checkout_id: checkout_id.into(),
        dirty,
    })
}

pub(crate) fn update_input_digest(
    digest: &mut Sha256,
    message: &astra_headless_protocol::InputMessage,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(message)
        .map_err(|error| format!("ASTRA_HEADLESS_INPUT_CANONICALIZE_FAILED: {error}"))?;
    digest.update(bytes);
    digest.update(b"\n");
    Ok(())
}

pub(crate) fn hash_input_messages(
    messages: &[astra_headless_protocol::InputMessage],
) -> Result<String, String> {
    let mut digest = Sha256::new();
    for message in messages {
        update_input_digest(&mut digest, message)?;
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

pub(crate) fn canonical_or_raw_input_hash(bytes: &[u8]) -> String {
    let canonical = (|| {
        let mut reader = JsonlReader::new(BufReader::new(bytes), 1024 * 1024).ok()?;
        let mut messages = Vec::new();
        while let Some(message) = reader
            .read::<astra_headless_protocol::InputMessage>()
            .ok()?
        {
            messages.push(message);
        }
        hash_input_messages(&messages).ok()
    })();
    canonical.unwrap_or_else(|| astra_core::Hash256::from_sha256(bytes).to_string())
}

pub(crate) fn read_checkpoint_config(path: &Path) -> Result<(CheckpointConfig, String), String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_READ_FAILED: {error}"))?;
    let config: CheckpointConfig = serde_json::from_slice(&bytes)
        .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_INVALID: {error}"))?;
    config.validate().map_err(|error| error.to_string())?;
    let config_hash = astra_core::Hash256::from_sha256(&bytes).to_string();
    Ok((config, config_hash))
}

pub(crate) fn observation_matches(
    predicate: &ObservationPredicate,
    observations: &[astra_headless_protocol::Observation],
) -> bool {
    match predicate {
        ObservationPredicate::Equals { key, value_hash } => observations
            .iter()
            .any(|observation| observation.key == *key && observation.value_hash == *value_hash),
        ObservationPredicate::Exists { key } => observations
            .iter()
            .any(|observation| observation.key == *key),
    }
}

// Await first consumes its request tick, then reports only additional polling ticks.
// Shutdown is immediate control input and never reaches this accounting path.
pub(crate) fn consumed_input_tick(
    tick: u64,
    input: &PhysicalInput,
    await_advanced_ticks: u32,
) -> Result<u64, String> {
    let additional_ticks = match input {
        PhysicalInput::AdvanceTicks { count } => count
            .checked_sub(1)
            .ok_or_else(|| "ASTRA_HEADLESS_ADVANCE_TICKS_ZERO".to_string())?,
        PhysicalInput::Await { .. } => await_advanced_ticks,
        _ => 0,
    };
    tick.checked_add(u64::from(additional_ticks))
        .ok_or_else(|| "ASTRA_HEADLESS_CONSUMED_TICK_OVERFLOW".to_string())
}

pub(crate) async fn consume_input(
    product: &mut dyn ProductSession,
    tick: u64,
    input: &PhysicalInput,
) -> Result<(Vec<astra_headless_protocol::Observation>, u32), String> {
    let mut observations = product
        .consume(tick, input)
        .await
        .map_err(|error| error.to_string())?;
    if let PhysicalInput::Await {
        observation,
        timeout_ticks,
        ..
    } = input
    {
        let mut advanced_ticks = 0;
        for offset in 0..*timeout_ticks {
            if observation_matches(observation, &observations) {
                break;
            }
            let next_tick = tick
                .checked_add(u64::from(offset) + 1)
                .ok_or_else(|| "ASTRA_HEADLESS_AWAIT_TICK_OVERFLOW".to_string())?;
            observations = product
                .consume(next_tick, &PhysicalInput::AdvanceTicks { count: 1 })
                .await
                .map_err(|error| error.to_string())?;
            advanced_ticks = offset + 1;
        }
        if !observation_matches(observation, &observations) {
            let (key, expected) = match observation {
                ObservationPredicate::Equals { key, value_hash } => {
                    (key.as_str(), value_hash.as_str())
                }
                ObservationPredicate::Exists { key } => (key.as_str(), "exists"),
            };
            let actual = observations
                .iter()
                .filter(|candidate| candidate.key == key)
                .map(|candidate| candidate.value_hash.as_str())
                .collect::<Vec<_>>();
            return Err(format!(
                "ASTRA_HEADLESS_AWAIT_TIMEOUT: key={key} expected={expected} actual_hashes={actual:?}"
            ));
        }
        return Ok((observations, advanced_ticks));
    }
    Ok((observations, 0))
}
