use astra_core::Hash256;
use astra_package::PackageReader;
pub use astra_player_core::{
    PlayerAudioMeterEvidence, PlayerAutomationReport, PlayerAutomationScript,
    PlayerAutomationStatus, PlayerAutomationStep, PlayerAutomationValidator,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerInputTranscript, PlayerPlatform,
    PlayerVisualComparisonEvidence, PlayerVisualRegionEvidence,
};
use std::{collections::BTreeSet, fs, path::PathBuf};

pub const WINDOWS_SENDINPUT_MOUSE: &str = "sendinput.mouse";
pub const WINDOWS_SENDINPUT_KEYBOARD: &str = "sendinput.keyboard";
pub const WEB_CDP_MOUSE: &str = "cdp.mouse";
pub const WEB_CDP_KEYBOARD: &str = "cdp.keyboard";

pub type PlayerAutomationError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone)]
pub struct WindowsLiveAutomationRequest {
    pub bundle_dir: PathBuf,
    pub visual_comparison_report: PathBuf,
    pub timeout_ms: u64,
    pub trace_log: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WindowsLiveAutomationRun {
    pub script: PlayerAutomationScript,
    pub transcript: PlayerInputTranscript,
    pub report: PlayerAutomationReport,
}

#[derive(Debug, Clone, Default)]
pub struct WindowsSendInputHost;

impl WindowsSendInputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Windows
    }

    pub fn run_live_bundle(
        &self,
        request: WindowsLiveAutomationRequest,
    ) -> Result<WindowsLiveAutomationRun, PlayerAutomationError> {
        let bundle = BundleContext::read(request.bundle_dir)?;
        if bundle.platform != "windows" {
            return Err("windows live automation requires a windows bundle".into());
        }
        let package_hash = sha256_file(&bundle.package_path)?;
        if package_hash != bundle.package_hash {
            return Err("bundle package hash does not match bundle manifest".into());
        }

        let expected_routes = bundle.expected_route_ids()?;
        let scenario_ref = bundle
            .scenario_refs
            .iter()
            .find(|item| item.contains(".windows."))
            .or_else(|| bundle.scenario_refs.first())
            .cloned()
            .unwrap_or_else(|| "scenario.refs/windows-live.json".to_string());
        let mut script = PlayerAutomationScript::new(
            &bundle.target,
            &bundle.profile,
            PlayerPlatform::Windows,
            package_hash.clone(),
            scenario_ref,
        );
        script.expected_routes = expected_routes.clone();
        script.steps = vec![PlayerAutomationStep {
            id: "focus.window".to_string(),
            action: "focus_window".to_string(),
            expected_route_id: None,
        }];
        script
            .steps
            .extend(expected_routes.iter().enumerate().map(|(index, route)| {
                PlayerAutomationStep {
                    id: format!("input.advance.{}", index + 1),
                    action: "sendinput.advance".to_string(),
                    expected_route_id: Some(route.clone()),
                }
            }));

        let live = windows_live_input(
            &bundle,
            request.timeout_ms,
            request.trace_log.as_ref(),
            &expected_routes,
        )?;
        let audio_meter = package_audio_meter(&bundle.package_path)?;
        let comparison = visual_comparison_evidence(&request.visual_comparison_report)?;
        let transcript = PlayerInputTranscript {
            schema: "astra.player_input_transcript.v1".to_string(),
            target: bundle.target,
            profile: bundle.profile,
            platform: PlayerPlatform::Windows,
            package_hash,
            events: live.events,
            input_consumption: live.input_consumption,
            visual_regions: live.visual_regions,
            audio_meter,
            visual_comparison: Some(comparison),
            route_coverage: live.route_coverage,
        };
        let report = self.build_report(&script, &transcript);
        Ok(WindowsLiveAutomationRun {
            script,
            transcript,
            report,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebCdpInputHost;

impl WebCdpInputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Web
    }
}

#[derive(Debug, Clone)]
struct BundleContext {
    bundle_dir: PathBuf,
    entrypoint: String,
    target: String,
    profile: String,
    platform: String,
    package_hash: String,
    package_path: PathBuf,
    scenario_refs: Vec<String>,
}

impl BundleContext {
    fn read(bundle_dir: PathBuf) -> Result<Self, PlayerAutomationError> {
        let manifest_path = bundle_dir.join("bundle_manifest.json");
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
        let entrypoint = required_string(&manifest, "entrypoint")?;
        let target = required_string(&manifest, "target")?;
        let profile = required_string(&manifest, "profile")?;
        let platform = required_string(&manifest, "platform")?;
        let package_rel = required_string(&manifest, "package")?;
        if !is_safe_relative_ref(&entrypoint) || !is_safe_relative_ref(&package_rel) {
            return Err("bundle manifest contains an unsafe relative path".into());
        }
        let scenario_refs = manifest
            .get("scenario_refs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter(|value| is_safe_relative_ref(value))
            .map(str::to_string)
            .collect::<Vec<_>>();
        Ok(Self {
            bundle_dir: bundle_dir.clone(),
            entrypoint,
            target,
            profile,
            platform,
            package_hash: required_string(&manifest, "package_hash")?,
            package_path: bundle_dir.join(package_rel),
            scenario_refs,
        })
    }

    fn expected_route_ids(&self) -> Result<Vec<String>, PlayerAutomationError> {
        let mut routes = BTreeSet::new();
        for scenario_ref in &self.scenario_refs {
            if !scenario_ref.contains(".windows.") {
                continue;
            }
            let scenario_path = self.bundle_dir.join(scenario_ref);
            let scenario: serde_json::Value = serde_json::from_slice(&fs::read(scenario_path)?)?;
            if let Some(route) = scenario
                .get("generated_route_id")
                .and_then(serde_json::Value::as_str)
            {
                routes.insert(route.to_string());
            }
        }
        Ok(routes.into_iter().collect())
    }
}

#[derive(Debug, Clone)]
struct LiveInputRun {
    events: Vec<PlayerInputEvent>,
    input_consumption: Vec<PlayerInputConsumptionEvidence>,
    visual_regions: Vec<PlayerVisualRegionEvidence>,
    route_coverage: Vec<String>,
}

fn windows_live_input(
    bundle: &BundleContext,
    timeout_ms: u64,
    trace_log: Option<&PathBuf>,
    expected_routes: &[String],
) -> Result<LiveInputRun, PlayerAutomationError> {
    #[cfg(target_os = "windows")]
    {
        windows_live_input_impl(bundle, timeout_ms, trace_log, expected_routes)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (bundle, timeout_ms, trace_log, expected_routes);
        Err("windows live automation requires Windows".into())
    }
}

fn package_audio_meter(
    package_path: &PathBuf,
) -> Result<PlayerAudioMeterEvidence, PlayerAutomationError> {
    let package_bytes = fs::read(package_path)?;
    let reader = PackageReader::open(&package_bytes)?;
    let vfs_manifest: serde_json::Value =
        serde_json::from_slice(&reader.container().read_section("asset.vfs_manifest")?)?;
    let entries = vfs_manifest
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or("asset.vfs_manifest entries must be an array")?;
    for entry in entries {
        let Some(vfs_uri) = entry.get("vfs_uri").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !vfs_uri.to_ascii_lowercase().ends_with(".wav") {
            continue;
        }
        let Some(source) = entry.get("source").and_then(serde_json::Value::as_object) else {
            continue;
        };
        if source.get("kind").and_then(serde_json::Value::as_str) != Some("package_section") {
            continue;
        }
        let Some(section_id) = source.get("section_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let section = reader.container().read_section(section_id)?;
        if let Some(expected_hash) = entry.get("hash").and_then(serde_json::Value::as_str) {
            if Hash256::from_sha256(&section).to_string() != expected_hash {
                return Err("audio asset hash mismatch in package VFS manifest".into());
            }
        }
        if let Some(expected_size) = entry.get("size").and_then(serde_json::Value::as_u64) {
            if section.len() as u64 != expected_size {
                return Err("audio asset byte size mismatch in package VFS manifest".into());
            }
        }
        if let Some(meter) = audio_meter_from_wav_bytes(&section) {
            return Ok(meter);
        }
    }
    Ok(PlayerAudioMeterEvidence {
        sample_count: 0,
        peak_dbfs: -120.0,
        rms_dbfs: -120.0,
    })
}

pub fn audio_meter_from_wav_bytes(bytes: &[u8]) -> Option<PlayerAudioMeterEvidence> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12usize;
    let mut audio_format = 0u16;
    let mut channels = 0u16;
    let mut bits_per_sample = 0u16;
    let mut data = None;
    while offset.checked_add(8)? <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let start = offset + 8;
        let end = start.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        if id == b"fmt " && size >= 16 {
            audio_format = u16::from_le_bytes(bytes[start..start + 2].try_into().ok()?);
            channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().ok()?);
            bits_per_sample = u16::from_le_bytes(bytes[start + 14..start + 16].try_into().ok()?);
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
        }
        offset = end + (size % 2);
    }
    let data = data?;
    if audio_format != 1 || channels == 0 || !matches!(bits_per_sample, 8 | 16) {
        return None;
    }
    let bytes_per_sample = (bits_per_sample / 8) as usize;
    if bytes_per_sample == 0 || data.len() < bytes_per_sample {
        return None;
    }
    let mut peak = 0.0f64;
    let mut square_sum = 0.0f64;
    let mut samples = 0u64;
    for sample in data.chunks_exact(bytes_per_sample) {
        let normalized = if bits_per_sample == 8 {
            (sample[0] as f64 - 128.0) / 128.0
        } else {
            i16::from_le_bytes([sample[0], sample[1]]) as f64 / 32768.0
        };
        let absolute = normalized.abs();
        peak = peak.max(absolute);
        square_sum += normalized * normalized;
        samples += 1;
    }
    if samples == 0 {
        return None;
    }
    let rms = (square_sum / samples as f64).sqrt();
    Some(PlayerAudioMeterEvidence {
        sample_count: samples,
        peak_dbfs: dbfs(peak),
        rms_dbfs: dbfs(rms),
    })
}

fn visual_comparison_evidence(
    path: &PathBuf,
) -> Result<PlayerVisualComparisonEvidence, PlayerAutomationError> {
    let bytes = fs::read(path)?;
    let report: serde_json::Value = serde_json::from_slice(&bytes)?;
    let checkpoint_count = report
        .get("checkpoints")
        .and_then(serde_json::Value::as_array)
        .map(|items| items.len() as u32)
        .unwrap_or_default();
    let status = if report.get("schema").and_then(serde_json::Value::as_str)
        == Some("tsuinosora.visual_comparison_report.v1")
        && report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && checkpoint_count > 0
    {
        PlayerAutomationStatus::Pass
    } else {
        PlayerAutomationStatus::Blocked
    };
    Ok(PlayerVisualComparisonEvidence {
        report_hash: Hash256::from_sha256(&bytes).to_string(),
        checkpoint_count,
        status,
    })
}

fn required_string(value: &serde_json::Value, key: &str) -> Result<String, PlayerAutomationError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("bundle manifest missing string field: {key}").into())
}

fn sha256_file(path: &PathBuf) -> Result<String, PlayerAutomationError> {
    Ok(Hash256::from_sha256(&fs::read(path)?).to_string())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Hash256::from_sha256(bytes).to_string()
}

fn dbfs(value: f64) -> f32 {
    (20.0 * value.max(0.000_000_001).log10()) as f32
}

fn is_safe_relative_ref(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains("://")
        && !value.contains('\\')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(':'))
}

#[cfg(target_os = "windows")]
fn windows_live_input_impl(
    bundle: &BundleContext,
    timeout_ms: u64,
    trace_log: Option<&PathBuf>,
    expected_routes: &[String],
) -> Result<LiveInputRun, PlayerAutomationError> {
    windows_live::run(bundle, timeout_ms, trace_log, expected_routes)
}

#[cfg(target_os = "windows")]
mod windows_live {
    use super::{
        sha256_bytes, BundleContext, LiveInputRun, PlayerAutomationError,
        PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerVisualRegionEvidence,
        WINDOWS_SENDINPUT_KEYBOARD, WINDOWS_SENDINPUT_MOUSE,
    };
    use std::{
        ffi::c_void,
        fs,
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    use windows::{
        core::BOOL,
        Win32::{
            Foundation::{HWND, LPARAM, RECT},
            Graphics::Gdi::{
                BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
                GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
                DIB_RGB_COLORS, SRCCOPY,
            },
            UI::{
                Input::KeyboardAndMouse::{
                    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
                    KEYEVENTF_KEYUP, VIRTUAL_KEY,
                },
                WindowsAndMessaging::{
                    EnumWindows, GetClientRect, GetWindowTextLengthW, GetWindowTextW,
                    GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
                    SW_RESTORE,
                },
            },
        },
    };

    pub fn run(
        bundle: &BundleContext,
        timeout_ms: u64,
        trace_log: Option<&PathBuf>,
        expected_routes: &[String],
    ) -> Result<LiveInputRun, PlayerAutomationError> {
        let mut trace_lines = Vec::new();
        trace_line(
            &mut trace_lines,
            "level=TRACE event=astra.player.automation.start platform=windows".to_string(),
        );
        let entrypoint = bundle.bundle_dir.join(&bundle.entrypoint);
        let mut child = Command::new(entrypoint)
            .current_dir(&bundle.bundle_dir)
            .env("ASTRA_PLAYER_TRACE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        trace_line(
            &mut trace_lines,
            "level=TRACE event=astra.player.bundle.launch role=bundle_entrypoint".to_string(),
        );
        let result = (|| {
            let hwnd = wait_for_window(child.id(), timeout_ms)?;
            trace_line(
                &mut trace_lines,
                "level=TRACE event=astra.player.window.found title=astra_player".to_string(),
            );
            focus_window(hwnd)?;
            trace_line(
                &mut trace_lines,
                "level=TRACE event=astra.player.window.focus requested=true".to_string(),
            );
            thread::sleep(Duration::from_millis(150));
            let before = capture_client_rgba(hwnd)?;
            let before_hash = sha256_bytes(&before.rgba);
            trace_line(
                &mut trace_lines,
                format!(
                    "level=TRACE event=astra.player.capture.before width={} height={} hash={}",
                    before.width, before.height, before_hash
                ),
            );
            let mut events = vec![PlayerInputEvent {
                step_id: "focus.window".to_string(),
                source: "window.focus".to_string(),
                kind: "focus".to_string(),
                sequence: 1,
                route_id: None,
            }];
            let mut visual_regions = Vec::new();
            let mut route_coverage = Vec::new();
            let mut previous = before;
            let routes = if expected_routes.is_empty() {
                vec![None]
            } else {
                expected_routes
                    .iter()
                    .map(|route| Some(route.clone()))
                    .collect()
            };
            for (index, route_id) in routes.iter().enumerate() {
                let input_sequence = (index + 2) as u64;
                trace_line(
                    &mut trace_lines,
                    format!(
                        "level=TRACE event=astra.player.input.sent source=sendinput.keyboard kind=key input_sequence={input_sequence}"
                    ),
                );
                send_key(0x20)?;
                thread::sleep(Duration::from_millis(160));
                let after = capture_client_rgba(hwnd)?;
                let previous_hash = sha256_bytes(&previous.rgba);
                let after_hash = sha256_bytes(&after.rgba);
                let changed = previous_hash != after_hash;
                trace_line(
                    &mut trace_lines,
                    format!(
                        "level=TRACE event=astra.player.capture.after input_sequence={input_sequence} width={} height={} hash={} changed={}",
                        after.width, after.height, after_hash, changed
                    ),
                );
                events.push(PlayerInputEvent {
                    step_id: format!("input.advance.{}", index + 1),
                    source: WINDOWS_SENDINPUT_KEYBOARD.to_string(),
                    kind: "key".to_string(),
                    sequence: input_sequence,
                    route_id: route_id.clone(),
                });
                visual_regions.push(PlayerVisualRegionEvidence {
                    region_id: format!("client_full_frame.input.{}", index + 1),
                    before_hash: previous_hash,
                    after_hash,
                    width: previous.width.min(after.width),
                    height: previous.height.min(after.height),
                });
                if changed {
                    if let Some(route_id) = route_id {
                        route_coverage.push(route_id.clone());
                    }
                }
                previous = after;
            }
            Ok(LiveInputRun {
                events,
                input_consumption: Vec::new(),
                visual_regions,
                route_coverage,
            })
        })();
        let _ = child.kill();
        let output = child.wait_with_output()?;
        let host_stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let consumed = parse_consumed_input_traces(&host_stderr);
        trace_line(
            &mut trace_lines,
            format!(
                "level=TRACE event=astra.player.host.trace_captured consumed_count={} trace_hash={}",
                consumed.len(),
                sha256_bytes(host_stderr.as_bytes())
            ),
        );
        let mut result = result;
        if let Ok(run) = result.as_mut() {
            run.input_consumption = correlate_consumed_traces(&run.events, &consumed);
            let consumed_inputs = run
                .input_consumption
                .iter()
                .map(|item| item.input_sequence)
                .collect::<std::collections::BTreeSet<_>>();
            run.route_coverage.retain(|route| {
                run.events.iter().any(|event| {
                    event.route_id.as_ref() == Some(route)
                        && consumed_inputs.contains(&event.sequence)
                })
            });
            trace_line(
                &mut trace_lines,
                format!(
                    "level=TRACE event=astra.player.input.correlation consumed_input_count={} route_coverage_count={}",
                    run.input_consumption.len(),
                    run.route_coverage.len()
                ),
            );
        }
        if let Some(path) = trace_log {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut combined = trace_lines.join("\n");
            if !host_stderr.trim().is_empty() {
                combined.push('\n');
                combined.push_str(host_stderr.trim_end());
            }
            combined.push('\n');
            fs::write(path, combined)?;
        }
        result
    }

    #[derive(Debug, Clone)]
    struct CapturedFrame {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    }

    #[derive(Debug, Clone)]
    struct ConsumedInputTrace {
        player_sequence: u64,
        kind: String,
        trace_hash: String,
    }

    fn trace_line(lines: &mut Vec<String>, line: String) {
        eprintln!("{line}");
        lines.push(line);
    }

    fn parse_consumed_input_traces(stderr: &str) -> Vec<ConsumedInputTrace> {
        stderr
            .lines()
            .filter(|line| line.contains("event=astra.player.input.consumed"))
            .filter_map(|line| {
                let player_sequence = token_value(line, "player_sequence")?.parse().ok()?;
                let kind = token_value(line, "kind")?.to_string();
                Some(ConsumedInputTrace {
                    player_sequence,
                    kind,
                    trace_hash: sha256_bytes(line.as_bytes()),
                })
            })
            .collect()
    }

    fn correlate_consumed_traces(
        events: &[PlayerInputEvent],
        traces: &[ConsumedInputTrace],
    ) -> Vec<PlayerInputConsumptionEvidence> {
        events
            .iter()
            .filter(|event| {
                matches!(
                    event.source.as_str(),
                    WINDOWS_SENDINPUT_KEYBOARD | WINDOWS_SENDINPUT_MOUSE
                )
            })
            .zip(traces.iter())
            .map(|(event, trace)| PlayerInputConsumptionEvidence {
                input_sequence: event.sequence,
                player_sequence: trace.player_sequence,
                source: "player_host.trace".to_string(),
                kind: trace.kind.clone(),
                trace_event: "astra.player.input.consumed".to_string(),
                trace_hash: trace.trace_hash.clone(),
                route_id: event.route_id.clone(),
            })
            .collect()
    }

    fn token_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
        let prefix = format!("{key}=");
        line.split_whitespace()
            .find_map(|token| token.strip_prefix(&prefix))
    }

    fn wait_for_window(pid: u32, timeout_ms: u64) -> Result<HWND, PlayerAutomationError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(1));
        while Instant::now() < deadline {
            if let Some(hwnd) = find_window_for_pid(pid) {
                return Ok(hwnd);
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err("windows live automation could not find the player window".into())
    }

    fn find_window_for_pid(pid: u32) -> Option<HWND> {
        struct Search {
            pid: u32,
            hwnd: HWND,
        }
        unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let search = &mut *(lparam.0 as *mut Search);
            if !search.hwnd.0.is_null() {
                return BOOL(0);
            }
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut window_pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
            if window_pid != search.pid {
                return BOOL(1);
            }
            let len = GetWindowTextLengthW(hwnd);
            let mut buffer = vec![0u16; len as usize + 1];
            let read = GetWindowTextW(hwnd, &mut buffer);
            let title = String::from_utf16_lossy(&buffer[..read as usize]);
            if !title.contains("AstraPlayer") {
                return BOOL(1);
            }
            search.hwnd = hwnd;
            BOOL(0)
        }
        let mut search = Search {
            pid,
            hwnd: HWND::default(),
        };
        unsafe {
            let _ = EnumWindows(Some(callback), LPARAM(&mut search as *mut Search as isize));
        }
        (!search.hwnd.0.is_null()).then_some(search.hwnd)
    }

    fn focus_window(hwnd: HWND) -> Result<(), PlayerAutomationError> {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    fn send_key(vk: u16) -> Result<(), PlayerAutomationError> {
        send_keyboard(vk, KEYBD_EVENT_FLAGS::default())?;
        send_keyboard(vk, KEYEVENTF_KEYUP)?;
        Ok(())
    }

    fn send_keyboard(vk: u16, flags: KEYBD_EVENT_FLAGS) -> Result<(), PlayerAutomationError> {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: Default::default(),
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent != 1 {
            return Err("windows live automation could not send keyboard input".into());
        }
        Ok(())
    }

    fn capture_client_rgba(hwnd: HWND) -> Result<CapturedFrame, PlayerAutomationError> {
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut rect).map_err(|err| {
                format!("windows live automation could not query client rect: {err:?}")
            })?;
            let width = (rect.right - rect.left).max(0);
            let height = (rect.bottom - rect.top).max(0);
            if width <= 0 || height <= 0 {
                return Err("windows live automation window has an empty client area".into());
            }
            let window_dc = GetDC(Some(hwnd));
            if window_dc.0.is_null() {
                return Err("windows live automation could not acquire window DC".into());
            }
            let memory_dc = CreateCompatibleDC(Some(window_dc));
            let bitmap = CreateCompatibleBitmap(window_dc, width, height);
            let old_object = SelectObject(memory_dc, bitmap.into());
            let result = (|| {
                if BitBlt(
                    memory_dc,
                    0,
                    0,
                    width,
                    height,
                    Some(window_dc),
                    0,
                    0,
                    SRCCOPY,
                )
                .is_err()
                {
                    return Err("windows live automation could not capture window pixels".into());
                }
                let mut info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bgra = vec![0u8; width as usize * height as usize * 4];
                let lines = GetDIBits(
                    memory_dc,
                    bitmap,
                    0,
                    height as u32,
                    Some(bgra.as_mut_ptr() as *mut c_void),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                if lines == 0 {
                    return Err("windows live automation could not read captured pixels".into());
                }
                let mut rgba = Vec::with_capacity(bgra.len());
                for pixel in bgra.chunks_exact(4) {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
                Ok(CapturedFrame {
                    width: width as u32,
                    height: height as u32,
                    rgba,
                })
            })();
            if !old_object.0.is_null() {
                let _ = SelectObject(memory_dc, old_object);
            }
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(Some(hwnd), window_dc);
            result
        }
    }
}
