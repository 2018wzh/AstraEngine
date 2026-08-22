#![cfg_attr(not(windows), allow(dead_code))]

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use astra_core::Hash256;
use astra_emu_manager_core::EmuPlatformRunEvidenceV1;
#[cfg(windows)]
use astra_headless_protocol::{ButtonState, PointerButton};
use astra_headless_protocol::{InputMessage, PhysicalInput};
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
    pub expected_package_hash: String,
    #[serde(default)]
    pub expected_profile_hash: Option<String>,
    pub require_terminal: bool,
    #[serde(default)]
    pub expected_coverage: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerE3Report {
    pub schema: String,
    pub status: String,
    pub build_identity_hash: String,
    pub profile_hash: Option<String>,
    pub package_hash: Option<String>,
    pub input_count: u64,
    pub presented_frame_count: u64,
    pub visual_changed: bool,
    pub audio_non_silent: bool,
    pub terminal_observed: bool,
    pub coverage_ids: Vec<String>,
    pub lifecycle_steps: Vec<String>,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Default)]
struct ManagerObservations {
    profile_hash: Option<String>,
    package_hash: Option<String>,
    input_count: u64,
    audio_non_silent: bool,
    terminal_observed: bool,
    shutdown_completed: bool,
    coverage_ids: Vec<String>,
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
    let mut expected_coverage = value.expected_coverage.clone();
    expected_coverage.sort();
    expected_coverage.dedup();
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
        || !valid_hash(&value.expected_package_hash)
        || value
            .expected_profile_hash
            .as_deref()
            .is_some_and(|hash| !valid_hash(hash))
        || !value.require_terminal
        || value.expected_coverage.is_empty()
        || value.expected_coverage.iter().any(|id| !safe_symbol(id))
        || value.expected_coverage != expected_coverage
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
    tracing::info!(event = "astra.emu.e3.started", input_count = messages.len());
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
        .env("ASTRA_EMU_QUICK_EVIDENCE", "1")
        .env("ASTRA_EMU_DATA_DIR", &data_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "ASTRA_EMU_E3_MANAGER_START".to_owned())?;
    let mut diagnostics = Vec::new();
    let mut visual_changed = false;
    let mut presented_frame_count = 0_u64;
    let mut lifecycle_steps = vec!["manager_started".into()];
    let replay_result = (|| -> Result<(), String> {
        let window_deadline = Instant::now() + Duration::from_millis(manifest.timeout_ms);
        let window = loop {
            if let Some(window) = WindowsTestDriver::find_process_window(child.id()) {
                break window;
            }
            if child
                .try_wait()
                .map_err(|_| "ASTRA_EMU_E3_MANAGER_WAIT".to_owned())?
                .is_some()
            {
                return Err("ASTRA_EMU_E3_MANAGER_EXITED_BEFORE_WINDOW".into());
            }
            if Instant::now() >= window_deadline {
                return Err("ASTRA_EMU_E3_WINDOW_TIMEOUT".into());
            }
            thread::sleep(Duration::from_millis(20));
        };
        lifecycle_steps.push("window_created".into());
        window.focus().map_err(|error| error.to_string())?;
        lifecycle_steps.push("window_focused".into());
        let start = Instant::now();
        let baseline = window.capture_rgba().map_err(|error| error.to_string())?;
        let client_width = baseline.width;
        let client_height = baseline.height;
        let baseline_pixels = baseline.rgba8;
        presented_frame_count = 1;
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
            if matches!(message.event, PhysicalInput::Shutdown) {
                window.request_close().map_err(|error| error.to_string())?;
                lifecycle_steps.push("close_requested".into());
                break;
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
            changed |= frame.rgba8 != baseline_pixels;
            presented_frame_count = presented_frame_count
                .checked_add(1)
                .ok_or_else(|| "ASTRA_EMU_E3_FRAME_COUNT_OVERFLOW".to_owned())?;
        }
        if !changed {
            diagnostics.push("ASTRA_EMU_E3_VISUAL_UNCHANGED".into());
        }
        visual_changed = changed;
        lifecycle_steps.push("input_replay_completed".into());
        Ok(())
    })();
    let shutdown_deadline = Instant::now() + Duration::from_millis(manifest.timeout_ms);
    let exited_normally = loop {
        if child
            .try_wait()
            .map_err(|_| "ASTRA_EMU_E3_MANAGER_WAIT".to_owned())?
            .is_some()
        {
            break true;
        }
        if Instant::now() >= shutdown_deadline {
            let _ = child.kill();
            let _ = child.wait();
            diagnostics.push("ASTRA_EMU_E3_SHUTDOWN_TIMEOUT".into());
            break false;
        }
        thread::sleep(Duration::from_millis(20));
    };
    if exited_normally {
        lifecycle_steps.push("manager_shutdown".into());
    }
    if let Err(error) = replay_result {
        diagnostics.push(error);
    }
    let observations =
        read_manager_observations(&data_root.join("diagnostics").join("astra.jsonl"))
            .unwrap_or_else(|diagnostic| {
                diagnostics.push(diagnostic);
                ManagerObservations::default()
            });
    validate_observations(&manifest, &observations, &mut diagnostics);
    if observations.package_hash.is_some() && observations.profile_hash.is_some() {
        lifecycle_steps.push("open".into());
    }
    if observations.input_count > 0 {
        lifecycle_steps.push("step".into());
    }
    lifecycle_steps.push("create".into());
    lifecycle_steps.sort();
    lifecycle_steps.dedup();
    let evidence_complete = diagnostics.is_empty();
    let build_identity_hash = Hash256::from_sha256(
        &fs::read(&manifest.manager_executable)
            .map_err(|_| "ASTRA_EMU_E3_MANAGER_READ".to_owned())?,
    )
    .to_string();
    let status = if evidence_complete { "pass" } else { "blocked" };
    let report = ManagerE3Report {
        schema: REPORT_SCHEMA.into(),
        status: status.into(),
        build_identity_hash: build_identity_hash.clone(),
        profile_hash: observations.profile_hash,
        package_hash: observations.package_hash,
        input_count: messages.len() as u64,
        presented_frame_count,
        visual_changed,
        audio_non_silent: observations.audio_non_silent,
        terminal_observed: observations.terminal_observed,
        coverage_ids: observations.coverage_ids,
        lifecycle_steps,
        diagnostic_codes: diagnostics,
    };
    fs::write(
        manifest.output_directory.join("manager-e3-report.json"),
        serde_json::to_vec_pretty(&report)
            .map_err(|_| "ASTRA_EMU_E3_REPORT_SERIALIZE".to_owned())?,
    )
    .map_err(|_| "ASTRA_EMU_E3_REPORT_WRITE".to_owned())?;
    if !evidence_complete {
        tracing::warn!(
            event = "astra.emu.e3.blocked",
            diagnostic_code = "ASTRA_EMU_E3_EVIDENCE_INCOMPLETE"
        );
        return Err("ASTRA_EMU_E3_BLOCKED".into());
    }
    let platform_evidence = platform_evidence(&report)?;
    fs::write(
        manifest.output_directory.join("platform-run-evidence.json"),
        serde_json::to_vec_pretty(&platform_evidence)
            .map_err(|_| "ASTRA_EMU_E3_REPORT_SERIALIZE".to_owned())?,
    )
    .map_err(|_| "ASTRA_EMU_E3_REPORT_WRITE".to_owned())?;
    tracing::info!(event = "astra.emu.e3.completed", evidence_level = "E3");
    Ok(())
}

fn platform_evidence(report: &ManagerE3Report) -> Result<EmuPlatformRunEvidenceV1, String> {
    let parse = |value: Option<&String>| {
        value
            .ok_or_else(|| "ASTRA_EMU_E3_MANAGER_IDENTITY_INVALID".to_owned())
            .and_then(|value| {
                Hash256::from_str(value)
                    .map_err(|_| "ASTRA_EMU_E3_MANAGER_IDENTITY_INVALID".to_owned())
            })
    };
    Ok(EmuPlatformRunEvidenceV1 {
        schema: "astra.emu.platform_run_evidence.v1".into(),
        platform: "windows".into(),
        architecture: "x86_64".into(),
        host_kind: "native".into(),
        build_identity_hash: Hash256::from_str(&report.build_identity_hash)
            .map_err(|_| "ASTRA_EMU_E3_MANAGER_IDENTITY_INVALID".to_owned())?,
        profile_hash: parse(report.profile_hash.as_ref())?,
        package_hash: parse(report.package_hash.as_ref())?,
        input_count: report.input_count,
        presented_frame_count: report.presented_frame_count,
        visual_changed: report.visual_changed,
        audio_non_silent: report.audio_non_silent,
        terminal_observed: report.terminal_observed,
        coverage_ids: report.coverage_ids.clone(),
        lifecycle_steps: report.lifecycle_steps.clone(),
        evidence_level: "E3".into(),
        status: "pass".into(),
        diagnostic_codes: Vec::new(),
    })
}

fn read_manager_observations(path: &Path) -> Result<ManagerObservations, String> {
    let bytes = fs::read(path).map_err(|_| "ASTRA_EMU_E3_MANAGER_LOG_MISSING".to_owned())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("ASTRA_EMU_E3_MANAGER_LOG_BOUNDS".into());
    }
    let mut observations = ManagerObservations::default();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let event: astra_observability::LogEventV1 = serde_json::from_slice(line)
            .map_err(|_| "ASTRA_EMU_E3_MANAGER_LOG_PARSE".to_owned())?;
        match event.event.as_str() {
            "astra.emu.manager.session_opened" => {
                observations.package_hash = log_hash(&event, "package_hash")?;
                observations.profile_hash = log_hash(&event, "profile_hash")?;
            }
            "astra.emu.manager.input_consumed" => {
                let count = event
                    .fields
                    .get("input_count")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "ASTRA_EMU_E3_CONSUMED_INPUT_INVALID".to_owned())?;
                observations.input_count = observations
                    .input_count
                    .checked_add(count)
                    .ok_or_else(|| "ASTRA_EMU_E3_INPUT_COUNT_OVERFLOW".to_owned())?;
            }
            "astra.emu.manager.audio_meter_observed" => {
                observations.audio_non_silent = event
                    .fields
                    .get("audio_non_silent")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
            }
            "astra.emu.manager.terminal_observed" => {
                observations.terminal_observed = true;
            }
            "astra.emu.manager.coverage_observed" => {
                let ids = event
                    .fields
                    .get("coverage_ids")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "ASTRA_EMU_E3_COVERAGE_EVIDENCE_INVALID".to_owned())?;
                observations.coverage_ids = if ids.is_empty() {
                    Vec::new()
                } else {
                    ids.split(',').map(str::to_owned).collect()
                };
            }
            "astra.emu.manager.shutdown_completed" => {
                observations.shutdown_completed = true;
            }
            _ => {}
        }
    }
    Ok(observations)
}

fn log_hash(
    event: &astra_observability::LogEventV1,
    field: &str,
) -> Result<Option<String>, String> {
    event
        .fields
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|hash| valid_hash(hash))
        .map(str::to_owned)
        .ok_or_else(|| "ASTRA_EMU_E3_MANAGER_IDENTITY_INVALID".to_owned())
        .map(Some)
}

fn validate_observations(
    manifest: &ManagerE3Manifest,
    observations: &ManagerObservations,
    diagnostics: &mut Vec<String>,
) {
    if observations.input_count == 0 {
        diagnostics.push("ASTRA_EMU_E3_INPUT_NOT_CONSUMED".into());
    }
    if observations.profile_hash.is_none() || observations.package_hash.is_none() {
        diagnostics.push("ASTRA_EMU_E3_IDENTITY_MISSING".into());
    }
    if observations.package_hash.as_deref() != Some(manifest.expected_package_hash.as_str())
        || manifest
            .expected_profile_hash
            .as_deref()
            .is_some_and(|expected| observations.profile_hash.as_deref() != Some(expected))
    {
        diagnostics.push("ASTRA_EMU_E3_PACKAGE_OR_PROFILE_IDENTITY_MISMATCH".into());
    }
    if !observations.audio_non_silent {
        diagnostics.push("ASTRA_EMU_E3_AUDIO_METER_MISSING".into());
    }
    if manifest.require_terminal && !observations.terminal_observed {
        diagnostics.push("ASTRA_EMU_E3_TERMINAL_IDENTITY_MISMATCH".into());
    }
    if !observations.shutdown_completed {
        diagnostics.push("ASTRA_EMU_E3_SHUTDOWN_EVIDENCE_MISSING".into());
    }
    if observations.coverage_ids != manifest.expected_coverage {
        diagnostics.push("ASTRA_EMU_E3_COVERAGE_EVIDENCE_MISSING".into());
    }
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
        "ControlLeft" | "ControlRight" | "Control" => Ok(0x11),
        "F5" => Ok(0x74),
        "F9" => Ok(0x78),
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

#[cfg(test)]
mod tests {
    use super::*;
    use astra_headless_protocol::USER_INPUT_SEQUENCE_SCHEMA;
    use std::collections::BTreeMap;

    #[cfg(windows)]
    #[test]
    fn physical_control_keys_map_to_windows_control() {
        assert_eq!(virtual_key("ControlLeft"), Ok(0x11));
        assert_eq!(virtual_key("ControlRight"), Ok(0x11));
    }

    fn log_event(
        event: &str,
        fields: &[(&str, serde_json::Value)],
    ) -> astra_observability::LogEventV1 {
        astra_observability::LogEventV1 {
            schema: astra_observability::LOG_EVENT_SCHEMA.into(),
            timestamp: "2026-08-01T00:00:00Z".into(),
            level: "info".into(),
            target: "astra_emu_manager".into(),
            event: event.into(),
            session_id: "test".into(),
            process_role: "manager".into(),
            thread_label: "main".into(),
            span_stack: Vec::new(),
            fields: fields
                .iter()
                .map(|(key, value)| ((*key).into(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

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
            expected_package_hash: format!("sha256:{}", "0".repeat(64)),
            expected_profile_hash: None,
            require_terminal: true,
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
    fn manager_observations_collect_runtime_and_shutdown_identity() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("astra.jsonl");
        let hash = format!("sha256:{}", "1".repeat(64));
        let events = [
            log_event(
                "astra.emu.manager.session_opened",
                &[
                    ("package_hash", hash.clone().into()),
                    ("profile_hash", hash.clone().into()),
                ],
            ),
            log_event(
                "astra.emu.manager.input_consumed",
                &[("input_count", 1_u64.into())],
            ),
            log_event(
                "astra.emu.manager.audio_meter_observed",
                &[("audio_non_silent", true.into())],
            ),
            log_event("astra.emu.manager.terminal_observed", &[]),
            log_event(
                "astra.emu.manager.coverage_observed",
                &[("coverage_ids", "syscall.one".into())],
            ),
            log_event("astra.emu.manager.shutdown_completed", &[]),
        ];
        let mut bytes = Vec::new();
        for event in events {
            bytes.extend_from_slice(&serde_json::to_vec(&event).unwrap());
            bytes.push(b'\n');
        }
        fs::write(&path, bytes).unwrap();
        let observations = read_manager_observations(&path).unwrap();
        assert_eq!(observations.profile_hash.as_deref(), Some(hash.as_str()));
        assert_eq!(observations.input_count, 1);
        assert!(observations.shutdown_completed);
    }

    #[test]
    fn platform_evidence_is_a_windows_e3_record() {
        let hash = format!("sha256:{}", "2".repeat(64));
        let report = ManagerE3Report {
            schema: REPORT_SCHEMA.into(),
            status: "pass".into(),
            build_identity_hash: hash.clone(),
            profile_hash: Some(hash.clone()),
            package_hash: Some(hash.clone()),
            input_count: 3,
            presented_frame_count: 4,
            visual_changed: true,
            audio_non_silent: true,
            terminal_observed: true,
            coverage_ids: vec!["syscall.one".into()],
            lifecycle_steps: vec![
                "create".into(),
                "open".into(),
                "step".into(),
                "shutdown".into(),
            ],
            diagnostic_codes: Vec::new(),
        };
        let evidence = platform_evidence(&report).unwrap();
        astra_emu_manager_core::validate_platform_evidence(&evidence, "windows").unwrap();
    }
}
