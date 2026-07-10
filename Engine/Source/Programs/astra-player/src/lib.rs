use astra_core::Hash256;
pub use astra_player_core::{
    PlatformCommandSink, PlayerAudioMeterEvidence, PlayerAutomationReport, PlayerAutomationScript,
    PlayerAutomationStatus, PlayerAutomationStep, PlayerAutomationValidator,
    PlayerHostCommandExecutor, PlayerHostResourceId, PlayerInputConsumptionEvidence,
    PlayerInputEvent, PlayerInputTranscript, PlayerPlatform, PlayerPlatformEvidenceIdentity,
    PlayerVisualComparisonEvidence, PlayerVisualRegionEvidence,
};
use std::{collections::BTreeSet, fs, path::PathBuf};

pub use astra_player_vn::*;

pub const WINDOWS_SENDINPUT_MOUSE: &str = "sendinput.mouse";
pub const WINDOWS_SENDINPUT_KEYBOARD: &str = "sendinput.keyboard";
pub const WEB_CDP_MOUSE: &str = "cdp.mouse";
pub const WEB_CDP_KEYBOARD: &str = "cdp.keyboard";

pub type PlayerAutomationError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone)]
pub struct WindowsLiveAutomationRequest {
    pub bundle_dir: PathBuf,
    pub visual_comparison_report: PathBuf,
    pub host_conformance_report: PathBuf,
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
        let (audio_meter, platform_identity) =
            host_audio_meter(&request.host_conformance_report, &package_hash)?;
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
        let report = PlayerAutomationValidator.validate_with_platform_identity(
            &script,
            &transcript,
            &platform_identity,
        );
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
    #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
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
            #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
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
    #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
    {
        windows_live_input_impl(bundle, timeout_ms, trace_log, expected_routes)
    }
    #[cfg(not(all(target_os = "windows", feature = "platform-test-driver")))]
    {
        let _ = (bundle, timeout_ms, trace_log, expected_routes);
        Err("windows live automation requires Windows".into())
    }
}

fn host_audio_meter(
    report_path: &PathBuf,
    package_hash: &str,
) -> Result<(PlayerAudioMeterEvidence, PlayerPlatformEvidenceIdentity), PlayerAutomationError> {
    let report: astra_platform::PlatformHostConformanceReport =
        serde_json::from_slice(&fs::read(report_path)?)?;
    if report.schema != astra_platform::PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA
        || report.status != astra_platform::ConformanceStatus::Pass
        || report.package_hash != package_hash
    {
        return Err("host conformance report does not match the bundle package".into());
    }
    let check = report
        .checks
        .iter()
        .find(|check| {
            check.id == "audio.output_meter"
                && check.status == astra_platform::ConformanceStatus::Pass
        })
        .ok_or("host conformance report is missing passing audio.output_meter evidence")?;
    let value = |key: &str| {
        check
            .evidence
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
            .ok_or_else(|| format!("audio.output_meter is missing {key} evidence"))
    };
    Ok((
        PlayerAudioMeterEvidence {
            sample_count: value("sample_count")?.parse()?,
            peak_dbfs: value("peak_dbfs")?.parse()?,
            rms_dbfs: value("rms_dbfs")?.parse()?,
        },
        PlayerPlatformEvidenceIdentity {
            profile_hash: report.profile_hash,
            build_fingerprint: report.build_fingerprint,
            session_id: report.session_id,
        },
    ))
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

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
fn sha256_bytes(bytes: &[u8]) -> String {
    Hash256::from_sha256(bytes).to_string()
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

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
fn windows_live_input_impl(
    bundle: &BundleContext,
    timeout_ms: u64,
    trace_log: Option<&PathBuf>,
    expected_routes: &[String],
) -> Result<LiveInputRun, PlayerAutomationError> {
    windows_live::run(bundle, timeout_ms, trace_log, expected_routes)
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
mod windows_live {
    use super::{
        sha256_bytes, BundleContext, LiveInputRun, PlayerAutomationError,
        PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerVisualRegionEvidence,
        WINDOWS_SENDINPUT_KEYBOARD, WINDOWS_SENDINPUT_MOUSE,
    };
    use astra_platform_windows::WindowsTestDriver;
    use std::{
        fs,
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::Duration,
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
            let window = WindowsTestDriver::wait_for_process_window(
                child.id(),
                Duration::from_millis(timeout_ms.max(1)),
            )?;
            trace_line(
                &mut trace_lines,
                "level=TRACE event=astra.player.window.found title=astra_player".to_string(),
            );
            window.focus()?;
            trace_line(
                &mut trace_lines,
                "level=TRACE event=astra.player.window.focus requested=true".to_string(),
            );
            thread::sleep(Duration::from_millis(150));
            let before = window.capture_rgba()?;
            let before_hash = sha256_bytes(&before.rgba8);
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
                window.send_key(0x20)?;
                thread::sleep(Duration::from_millis(160));
                let after = window.capture_rgba()?;
                let previous_hash = sha256_bytes(&previous.rgba8);
                let after_hash = sha256_bytes(&after.rgba8);
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
}
