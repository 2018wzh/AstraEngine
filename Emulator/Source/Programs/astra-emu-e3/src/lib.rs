#![cfg_attr(not(windows), allow(dead_code))]

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use astra_core::Hash256;
use astra_headless_protocol::{ButtonState, InputMessage, PhysicalInput, PointerButton};
use serde::{Deserialize, Serialize};

const MANIFEST_SCHEMA: &str = "astra.emu.manager_e3_manifest.v1";
const REPORT_SCHEMA: &str = "astra.emu.manager_e3_report.v1";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INPUT_MESSAGES: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerE3Manifest {
    pub schema: String,
    pub manager_executable: PathBuf,
    pub authorized_source_directory: PathBuf,
    pub entry: String,
    pub input: PathBuf,
    pub output_directory: PathBuf,
    pub timeout_ms: u64,
    pub stage_width: u32,
    pub stage_height: u32,
    pub expected_terminal_hash: String,
    #[serde(default)]
    pub expected_coverage: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerE3Report {
    pub schema: String,
    pub status: String,
    pub build_identity_hash: String,
    pub profile_hash: String,
    pub package_hash: String,
    pub input_sequence_hash: String,
    pub input_count: u64,
    pub visual_trace_hash: String,
    pub consumed_input_trace_hash: Option<String>,
    pub audio_meter_hash: Option<String>,
    pub route_terminal_hash: Option<String>,
    pub lifecycle_steps: Vec<String>,
    pub diagnostic_codes: Vec<String>,
}

pub fn run_from_args(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let _program = args.next();
    let manifest = args.next().ok_or_else(|| "ASTRA_EMU_E3_USAGE".to_owned())?;
    if args.next().is_some() {
        return Err("ASTRA_EMU_E3_USAGE".into());
    }
    let manifest = load_manifest(Path::new(&manifest))?;
    #[cfg(windows)]
    return run_windows(manifest);
    #[cfg(not(windows))]
    {
        let _ = manifest;
        Err("ASTRA_EMU_E3_WINDOWS_ONLY".into())
    }
}

pub fn load_manifest(path: &Path) -> Result<ManagerE3Manifest, String> {
    let metadata = fs::metadata(path).map_err(|_| "ASTRA_EMU_E3_MANIFEST_READ".to_owned())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
        return Err("ASTRA_EMU_E3_MANIFEST_BOUNDS".into());
    }
    let manifest: ManagerE3Manifest = serde_json::from_slice(
        &fs::read(path).map_err(|_| "ASTRA_EMU_E3_MANIFEST_READ".to_owned())?,
    )
    .map_err(|_| "ASTRA_EMU_E3_MANIFEST_PARSE".to_owned())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn validate_manifest(value: &ManagerE3Manifest) -> Result<(), String> {
    if value.schema != MANIFEST_SCHEMA
        || !value.manager_executable.is_absolute()
        || !value.authorized_source_directory.is_absolute()
        || !value.input.is_absolute()
        || !value.output_directory.is_absolute()
        || !value.manager_executable.is_file()
        || !value.authorized_source_directory.is_dir()
        || !safe_relative_path(&value.entry)
        || value.timeout_ms == 0
        || value.timeout_ms > 30 * 60 * 1000
        || value.stage_width == 0
        || value.stage_height == 0
        || !valid_hash(&value.expected_terminal_hash)
        || value.expected_coverage.is_empty()
        || value.expected_coverage.iter().any(|id| !safe_symbol(id))
    {
        return Err("ASTRA_EMU_E3_MANIFEST_INVALID".into());
    }
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f'))
}

pub fn load_input(path: &Path) -> Result<Vec<InputMessage>, String> {
    let metadata = fs::metadata(path).map_err(|_| "ASTRA_EMU_E3_INPUT_READ".to_owned())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_INPUT_BYTES {
        return Err("ASTRA_EMU_E3_INPUT_BOUNDS".into());
    }
    let bytes = fs::read(path).map_err(|_| "ASTRA_EMU_E3_INPUT_READ".to_owned())?;
    let mut messages = Vec::new();
    let mut session = None;
    let mut previous_sequence = 0;
    let mut previous_tick = 0;
    for raw in bytes.split(|byte| *byte == b'\n') {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if messages.len() >= MAX_INPUT_MESSAGES {
            return Err("ASTRA_EMU_E3_INPUT_BOUNDS".into());
        }
        let message: InputMessage =
            serde_json::from_slice(line).map_err(|_| "ASTRA_EMU_E3_INPUT_PARSE".to_owned())?;
        message
            .validate()
            .map_err(|_| "ASTRA_EMU_E3_INPUT_INVALID".to_owned())?;
        if session.get_or_insert_with(|| message.session.clone()) != &message.session
            || message.sequence <= previous_sequence
            || message.tick < previous_tick
        {
            return Err("ASTRA_EMU_E3_INPUT_ORDER".into());
        }
        previous_sequence = message.sequence;
        previous_tick = message.tick;
        messages.push(message);
    }
    if messages.is_empty()
        || !matches!(
            messages.last().map(|message| &message.event),
            Some(PhysicalInput::Shutdown)
        )
    {
        return Err("ASTRA_EMU_E3_INPUT_SHUTDOWN_REQUIRED".into());
    }
    if messages[..messages.len() - 1]
        .iter()
        .any(|message| matches!(message.event, PhysicalInput::Shutdown))
    {
        return Err("ASTRA_EMU_E3_INPUT_ORDER".into());
    }
    Ok(messages)
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && value
            .split(['/', '\\'])
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(windows)]
fn run_windows(manifest: ManagerE3Manifest) -> Result<(), String> {
    use astra_platform_windows::WindowsTestDriver;
    use std::{
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    let messages = load_input(&manifest.input)?;
    let input_hash = hash_input(&messages)?;
    if manifest.output_directory.exists() {
        return Err("ASTRA_EMU_E3_OUTPUT_NOT_EMPTY".into());
    }
    fs::create_dir_all(&manifest.output_directory)
        .map_err(|_| "ASTRA_EMU_E3_OUTPUT_CREATE".to_owned())?;
    let diagnostics_root = manifest.output_directory.join("harness-diagnostics");
    fs::create_dir_all(&diagnostics_root).map_err(|_| "ASTRA_EMU_E3_OUTPUT_CREATE".to_owned())?;
    let mut observability = astra_observability::HostObservabilityConfig::for_cli("info");
    observability.role = astra_observability::HostRole::Test;
    observability.console = false;
    observability.log_dir = Some(diagnostics_root);
    let _observability = astra_observability::init_host(observability)
        .map_err(|_| "ASTRA_EMU_E3_OBSERVABILITY_INIT".to_owned())?;
    tracing::info!(
        event = "astra.emu.e3.started",
        input_count = messages.len(),
        input_hash = %input_hash
    );
    let data_root = manifest.output_directory.join("manager-state");
    let mut child = Command::new(&manifest.manager_executable)
        .current_dir(
            manifest
                .manager_executable
                .parent()
                .ok_or_else(|| "ASTRA_EMU_E3_MANAGER_PATH".to_owned())?,
        )
        .env("ASTRA_EMU_QUICK_ENGINE", "fvp")
        .env(
            "ASTRA_EMU_QUICK_GAME_DIR",
            &manifest.authorized_source_directory,
        )
        .env("ASTRA_EMU_QUICK_ENTRY", &manifest.entry)
        .env("ASTRA_EMU_DATA_DIR", &data_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "ASTRA_EMU_E3_MANAGER_START".to_owned())?;
    let mut diagnostics = Vec::new();
    let mut visual = Vec::new();
    let mut lifecycle_steps = vec!["manager_started".into()];
    let replay_result = (|| -> Result<(), String> {
        let window = WindowsTestDriver::wait_for_process_window(
            child.id(),
            Duration::from_millis(manifest.timeout_ms),
        )
        .map_err(|error| error.to_string())?;
        lifecycle_steps.push("window_created".into());
        window.focus().map_err(|error| error.to_string())?;
        lifecycle_steps.push("window_focused".into());
        let start = Instant::now();
        let baseline = window.capture_rgba().map_err(|error| error.to_string())?;
        let client_width = baseline.width;
        let client_height = baseline.height;
        let baseline_hash = Hash256::from_sha256(&baseline.rgba8);
        visual.extend_from_slice(baseline_hash.as_bytes());
        let mut changed = false;
        for message in &messages {
            let target = Duration::from_nanos(
                message
                    .time_ns()
                    .map_err(|_| "ASTRA_EMU_E3_INPUT_INVALID".to_owned())?,
            );
            if target > start.elapsed() {
                thread::sleep(target - start.elapsed());
            }
            if start.elapsed() > Duration::from_millis(manifest.timeout_ms) {
                diagnostics.push("ASTRA_EMU_E3_TIMEOUT".into());
                return Ok(());
            }
            dispatch_input(
                &window,
                &message.event,
                manifest.stage_width,
                manifest.stage_height,
                client_width,
                client_height,
            )?;
            tracing::debug!(
                event = "astra.emu.e3.input_dispatched",
                sequence = message.sequence,
                tick = message.tick
            );
            let frame = window.capture_rgba().map_err(|error| error.to_string())?;
            if frame.width != client_width || frame.height != client_height {
                return Err("ASTRA_EMU_E3_WINDOW_RESIZED".into());
            }
            let frame_hash = Hash256::from_sha256(&frame.rgba8);
            changed |= frame_hash != baseline_hash;
            visual.extend_from_slice(frame_hash.as_bytes());
        }
        if !changed {
            diagnostics.push("ASTRA_EMU_E3_VISUAL_UNCHANGED".into());
        }
        lifecycle_steps.push("input_replay_completed".into());
        Ok(())
    })();
    let _ = child.kill();
    let _ = child.wait();
    if let Err(error) = replay_result {
        diagnostics.push(error);
    }
    // The Manager currently has no public save/restore, terminal, coverage, or audio-meter
    // report surface. Keep the harness fail-closed until those observations are implemented.
    diagnostics.push("ASTRA_EMU_E3_MANAGER_OBSERVABILITY_INCOMPLETE".into());
    tracing::warn!(
        event = "astra.emu.e3.blocked",
        diagnostic_code = "ASTRA_EMU_E3_MANAGER_OBSERVABILITY_INCOMPLETE"
    );
    let report = ManagerE3Report {
        schema: REPORT_SCHEMA.into(),
        status: "blocked".into(),
        build_identity_hash: Hash256::from_sha256(
            &fs::read(&manifest.manager_executable)
                .map_err(|_| "ASTRA_EMU_E3_MANAGER_READ".to_owned())?,
        )
        .to_string(),
        profile_hash: Hash256::from_sha256(
            &serde_json::to_vec(&manifest)
                .map_err(|_| "ASTRA_EMU_E3_MANIFEST_SERIALIZE".to_owned())?,
        )
        .to_string(),
        package_hash: Hash256::from_sha256(manifest.entry.as_bytes()).to_string(),
        input_sequence_hash: input_hash.to_string(),
        input_count: messages.len() as u64,
        visual_trace_hash: Hash256::from_sha256(&visual).to_string(),
        consumed_input_trace_hash: None,
        audio_meter_hash: None,
        route_terminal_hash: None,
        lifecycle_steps,
        diagnostic_codes: diagnostics,
    };
    fs::write(
        manifest.output_directory.join("manager-e3-report.json"),
        serde_json::to_vec_pretty(&report)
            .map_err(|_| "ASTRA_EMU_E3_REPORT_SERIALIZE".to_owned())?,
    )
    .map_err(|_| "ASTRA_EMU_E3_REPORT_WRITE".to_owned())?;
    Err("ASTRA_EMU_E3_BLOCKED".into())
}

#[cfg(windows)]
fn dispatch_input(
    window: &astra_platform_windows::TestWindow,
    input: &PhysicalInput,
    stage_width: u32,
    stage_height: u32,
    client_width: u32,
    client_height: u32,
) -> Result<(), String> {
    match input {
        PhysicalInput::Keyboard {
            physical_key,
            state,
            ..
        } => {
            let key = virtual_key(physical_key)?;
            window
                .send_key_state(key, matches!(state, ButtonState::Pressed))
                .map_err(|error| error.to_string())?;
        }
        PhysicalInput::PointerMove { x, y } => window
            .move_pointer(
                scale(*x, stage_width, client_width)?,
                scale(*y, stage_height, client_height)?,
            )
            .map_err(|error| error.to_string())?,
        PhysicalInput::PointerButton { button, state } => match button {
            PointerButton::Primary => window
                .send_primary_button(matches!(state, ButtonState::Pressed))
                .map_err(|error| error.to_string())?,
            PointerButton::Secondary => window
                .send_secondary_button(matches!(state, ButtonState::Pressed))
                .map_err(|error| error.to_string())?,
            _ => return Err("ASTRA_EMU_E3_POINTER_BUTTON_UNSUPPORTED".into()),
        },
        PhysicalInput::Wheel { delta_x, delta_y } if *delta_x == 0 && *delta_y != 0 => window
            .send_wheel(*delta_y)
            .map_err(|error| error.to_string())?,
        PhysicalInput::Wheel { .. } => return Err("ASTRA_EMU_E3_WHEEL_UNSUPPORTED".into()),
        PhysicalInput::Resume
        | PhysicalInput::Focus { .. }
        | PhysicalInput::AdvanceTicks { .. }
        | PhysicalInput::Await { .. }
        | PhysicalInput::Checkpoint { .. }
        | PhysicalInput::Shutdown => {}
        _ => return Err("ASTRA_EMU_E3_INPUT_UNSUPPORTED".into()),
    }
    Ok(())
}

#[cfg(windows)]
fn virtual_key(key: &str) -> Result<u16, String> {
    match key {
        "Enter" | "Return" => Ok(0x0D),
        "Escape" => Ok(0x1B),
        "ArrowUp" | "Up" => Ok(0x26),
        "ArrowDown" | "Down" => Ok(0x28),
        "ArrowLeft" | "Left" => Ok(0x25),
        "ArrowRight" | "Right" => Ok(0x27),
        "Space" => Ok(0x20),
        _ => Err("ASTRA_EMU_E3_KEY_UNSUPPORTED".into()),
    }
}

#[cfg(windows)]
fn scale(value: u16, stage_extent: u32, client_extent: u32) -> Result<u32, String> {
    if stage_extent == 0 || client_extent == 0 || u32::from(value) >= stage_extent {
        return Err("ASTRA_EMU_E3_POINTER_BOUNDS".into());
    }
    Ok(u32::from(value) * client_extent / stage_extent)
}

fn hash_input(messages: &[InputMessage]) -> Result<Hash256, String> {
    let mut bytes = Vec::new();
    for message in messages {
        bytes.extend_from_slice(
            &serde_json::to_vec(message).map_err(|_| "ASTRA_EMU_E3_INPUT_SERIALIZE".to_owned())?,
        );
        bytes.push(b'\n');
    }
    Ok(Hash256::from_sha256(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_headless_protocol::USER_INPUT_SEQUENCE_SCHEMA;

    #[test]
    fn rejects_unsafe_entry_and_empty_coverage() {
        let temp = tempfile::tempdir().unwrap();
        let manager = temp.path().join("manager.exe");
        let input = temp.path().join("input.jsonl");
        fs::write(&manager, b"manager").unwrap();
        fs::write(&input, b"input").unwrap();
        let manifest = ManagerE3Manifest {
            schema: MANIFEST_SCHEMA.into(),
            manager_executable: manager,
            authorized_source_directory: temp.path().into(),
            entry: "../unsafe".into(),
            input,
            output_directory: temp.path().join("out"),
            timeout_ms: 1,
            stage_width: 1,
            stage_height: 1,
            expected_terminal_hash: format!("sha256:{}", "0".repeat(64)),
            expected_coverage: vec![],
        };
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn input_requires_ordered_terminal_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("input.jsonl");
        let message = InputMessage {
            schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
            session: "e3".into(),
            sequence: 1,
            tick: 0,
            event: PhysicalInput::Shutdown,
        };
        fs::write(&path, serde_json::to_vec(&message).unwrap()).unwrap();
        assert_eq!(load_input(&path).unwrap().len(), 1);
    }

    #[test]
    fn terminal_hash_must_be_lowercase_sha256() {
        assert!(valid_hash(&format!("sha256:{}", "a".repeat(64))));
        assert!(!valid_hash(&format!("sha256:{}", "A".repeat(64))));
    }
}
