use astra_core::Hash256;

pub use astra_player_core::{
    PlatformCommandSink, PlayerAudioMeterEvidence, PlayerAutomationReport, PlayerAutomationScript,
    PlayerAutomationStatus, PlayerAutomationStep, PlayerAutomationValidator,
    PlayerHostCommandExecutor, PlayerHostCommandResult, PlayerHostResourceId,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerInputTranscript, PlayerPlatform,
    PlayerPlatformEvidenceIdentity, PlayerRuntimeRouteEvidence, PlayerVisualComparisonEvidence,
    PlayerVisualRegionEvidence,
};

use std::{fs, path::PathBuf};

mod web_cdp;

pub use web_cdp::*;

mod bundled_observability;

pub use bundled_observability::*;

mod save_timestamp;

pub use save_timestamp::current_save_timestamp;

mod save_catalog;

pub use save_catalog::hydrate_save_catalog;

mod native_policy_loading;

mod native_session;

pub use native_policy_loading::*;

mod native_session_clock;

pub use native_session::*;

pub use astra_player_vn::*;

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
mod windows_live {
    use super::validate_web_same_run_audio::{ParsedPlayerHostTrace, ScenarioHostInput};
    use super::{
        parse_player_host_traces, sha256_bytes, BundleContext, LiveInputRun, PlayerAutomationError,
        PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerRuntimeRouteEvidence,
        PlayerVisualRegionEvidence, WINDOWS_SENDINPUT_KEYBOARD, WINDOWS_SENDINPUT_MOUSE,
    };
    use astra_platform_windows::WindowsTestDriver;
    use std::{
        fs,
        io::{Read, Write},
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::Duration,
    };

    const MAX_HOST_STDERR_BYTES: usize = 16 * 1024 * 1024;

    struct StderrCapture {
        bytes: Vec<u8>,
        total_bytes: usize,
    }

    pub fn run(
        bundle: &BundleContext,
        timeout_ms: u64,
        trace_log: Option<&PathBuf>,
        inputs: &[ScenarioHostInput],
    ) -> Result<LiveInputRun, PlayerAutomationError> {
        let mut trace_lines = Vec::new();
        trace_line(
            &mut trace_lines,
            "level=TRACE event=astra.player.automation.start platform=windows".to_string(),
        );
        let entrypoint = bundle.bundle_dir.join(&bundle.entrypoint);
        let mut child = Command::new(entrypoint)
            .current_dir(&bundle.bundle_dir)
            .env("ASTRA_LOG", "trace")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child
            .stderr
            .take()
            .ok_or("ASTRA_PLAYER_E3_STDERR_UNAVAILABLE: child stderr was not piped")?;
        let stderr_reader = thread::spawn(move || drain_stderr(stderr, MAX_HOST_STDERR_BYTES));
        trace_line(
            &mut trace_lines,
            "level=TRACE event=astra.player.bundle.launch role=bundle_entrypoint".to_string(),
        );
        let result = (|| {
            let window = WindowsTestDriver::wait_for_window(
                child.id(),
                &bundle.target,
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
            let mut previous = before;
            for (index, input) in inputs.iter().enumerate() {
                let input_sequence = (index + 2) as u64;
                trace_line(
                    &mut trace_lines,
                    format!(
                        "level=TRACE event=astra.player.input.sent source=sendinput.keyboard kind=key input_sequence={input_sequence}"
                    ),
                );
                window.send_key(input.virtual_key)?;
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
                    step_id: format!("input.{}.{}", input.kind, index + 1),
                    source: WINDOWS_SENDINPUT_KEYBOARD.to_string(),
                    kind: "key".to_string(),
                    sequence: input_sequence,
                    route_id: None,
                });
                visual_regions.push(PlayerVisualRegionEvidence {
                    region_id: format!("client_full_frame.input.{}", index + 1),
                    before_hash: previous_hash,
                    after_hash,
                    width: previous.width.min(after.width),
                    height: previous.height.min(after.height),
                });
                previous = after;
            }
            Ok(LiveInputRun {
                events,
                input_consumption: Vec::new(),
                visual_regions,
                runtime_routes: Vec::new(),
                route_coverage: Vec::new(),
            })
        })();
        let _ = child.kill();
        child.wait()?;
        let stderr_capture = stderr_reader
            .join()
            .map_err(|_| "ASTRA_PLAYER_E3_STDERR_READER_PANICKED")??;
        if stderr_capture.total_bytes > MAX_HOST_STDERR_BYTES {
            return Err(format!(
                "ASTRA_PLAYER_E3_STDERR_LIMIT_EXCEEDED: captured {} bytes, limit is {} bytes",
                stderr_capture.total_bytes, MAX_HOST_STDERR_BYTES
            )
            .into());
        }
        let host_stderr = String::from_utf8(stderr_capture.bytes)
            .map_err(|_| "ASTRA_PLAYER_E3_STDERR_NOT_UTF8")?;
        let host_traces = parse_player_host_traces(&host_stderr);
        trace_line(
            &mut trace_lines,
            format!(
                "level=TRACE event=astra.player.host.trace_captured consumed_count={} trace_hash={}",
                host_traces.len(),
                sha256_bytes(host_stderr.as_bytes())
            ),
        );
        let mut result = result;
        if let Ok(run) = result.as_mut() {
            correlate_host_traces(run, &host_traces);
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

    fn drain_stderr(
        mut stderr: impl Read,
        capture_limit: usize,
    ) -> Result<StderrCapture, std::io::Error> {
        let mut bytes = Vec::with_capacity(capture_limit.min(64 * 1024));
        let mut total_bytes = 0usize;
        let mut chunk = [0u8; 8192];
        loop {
            let read = stderr.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            total_bytes = total_bytes.saturating_add(read);
            let remaining = capture_limit.saturating_sub(bytes.len());
            bytes.write_all(&chunk[..read.min(remaining)])?;
        }
        Ok(StderrCapture { bytes, total_bytes })
    }

    fn trace_line(lines: &mut Vec<String>, line: String) {
        eprintln!("{line}");
        lines.push(line);
    }

    fn correlate_host_traces(run: &mut LiveInputRun, traces: &[ParsedPlayerHostTrace]) {
        let input_events = run
            .events
            .iter_mut()
            .filter(|event| {
                matches!(
                    event.source.as_str(),
                    WINDOWS_SENDINPUT_KEYBOARD | WINDOWS_SENDINPUT_MOUSE
                )
            })
            .zip(traces.iter());
        let mut route_coverage = std::collections::BTreeSet::new();
        let mut consumption = Vec::new();
        let mut runtime_routes = Vec::new();
        for (event, trace) in input_events {
            let route_id = trace
                .current_state_id
                .clone()
                .or_else(|| trace.coverage_reached.last().cloned());
            event.route_id = route_id.clone();
            route_coverage.extend(trace.coverage_reached.iter().cloned());
            consumption.push(PlayerInputConsumptionEvidence {
                input_sequence: event.sequence,
                player_sequence: trace.player_sequence,
                source: "player_host.trace".to_string(),
                kind: trace.kind.clone(),
                trace_event: "astra.player.input.consumed".to_string(),
                trace_hash: trace.trace_hash.clone(),
                route_id,
            });
            runtime_routes.push(PlayerRuntimeRouteEvidence {
                input_sequence: event.sequence,
                player_sequence: trace.player_sequence,
                fixed_step: trace.fixed_step,
                coverage_reached: trace.coverage_reached.clone(),
                current_state_id: trace.current_state_id.clone(),
                pending_choice_ids: trace.pending_choice_ids.clone(),
                terminal_route_ids: trace.terminal_route_ids.clone(),
                session_id: trace.session_id.clone(),
                trace_hash: trace.trace_hash.clone(),
            });
        }
        run.input_consumption = consumption;
        run.runtime_routes = runtime_routes;
        run.route_coverage = route_coverage.into_iter().collect();
    }

    #[cfg(test)]
    mod tests {
        use super::drain_stderr;

        #[test]
        fn stderr_drain_keeps_reading_after_capture_limit() {
            let source = vec![b'x'; 256 * 1024];
            let capture = drain_stderr(source.as_slice(), 1024).unwrap();
            assert_eq!(capture.bytes.len(), 1024);
            assert_eq!(capture.total_bytes, source.len());
        }
    }
}

#[cfg(test)]
mod host_trace_tests {
    use super::{parse_player_host_traces, parse_scenario_input_plan, ScenarioInputAction};

    #[test]
    fn scenario_input_plan_uses_declared_actions_instead_of_expected_route_count() {
        let yaml = br#"
schema: astra.scenario.v1
generated_route_id: route.library
actions:
  - launch: {}
  - player_input: { kind: advance, ticks: 2 }
  - player_input: { kind: choose, value: choice.library }
  - player_input: { kind: open_system, value: backlog }
"#;

        let plan = parse_scenario_input_plan(yaml).unwrap();

        assert_eq!(
            plan,
            vec![
                ScenarioInputAction::Advance,
                ScenarioInputAction::Advance,
                ScenarioInputAction::Choose {
                    option_id: "choice.library".to_string()
                },
                ScenarioInputAction::OpenSystem {
                    page: "backlog".to_string()
                },
            ]
        );
    }

    #[test]
    fn scenario_input_plan_blocks_media_completion_bypass() {
        let yaml = br#"
schema: astra.scenario.v1
actions:
  - player_input: { kind: complete_wait, value: voice.end }
"#;

        let error = parse_scenario_input_plan(yaml).unwrap_err();

        assert!(error.contains("ASTRA_PLAYER_AUTOMATION_MEDIA_COMPLETION_REQUIRED"));
    }

    #[test]
    fn route_coverage_is_parsed_from_runtime_evidence_not_expected_labels() {
        let stderr = concat!(
            "event=astra.player.input.consumed player_sequence=17 kind=keyboard session_id=session.one\n",
            "event=astra.player.vn.step player_sequence=17 fixed_step=3 coverage=state.library,state.good ",
            "session_id=session.one current_state_id=state.library ",
            "pending_choice_ids=choice.left,choice.right terminal_route_ids=ending.good\n",
        );

        let traces = parse_player_host_traces(stderr);

        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].player_sequence, 17);
        assert_eq!(traces[0].fixed_step, 3);
        assert_eq!(traces[0].kind, "keyboard");
        assert_eq!(
            traces[0].coverage_reached,
            vec!["state.library", "state.good"]
        );
        assert_eq!(traces[0].current_state_id.as_deref(), Some("state.library"));
        assert_eq!(traces[0].session_id, "session.one");
        assert!(parse_player_host_traces(&stderr.replacen(
            "session_id=session.one",
            "session_id=other",
            1
        ))
        .is_empty());
        assert!(parse_player_host_traces(&stderr.replace("session_id=session.one", "")).is_empty());
        assert_eq!(
            traces[0].pending_choice_ids,
            vec!["choice.left", "choice.right"]
        );
        assert_eq!(traces[0].terminal_route_ids, vec!["ending.good"]);
        assert!(traces[0].trace_hash.starts_with("sha256:"));
    }

    #[test]
    fn visual_change_without_runtime_route_trace_produces_no_coverage() {
        let stderr = "event=astra.player.input.consumed player_sequence=2 kind=keyboard\n";

        let traces = parse_player_host_traces(stderr);

        assert!(traces.is_empty());
    }
}

mod windows_input;
use windows_input::parse_scenario_input_plan;
pub use windows_input::AndroidInputHost;
pub use windows_input::LinuxUinputHost;
pub use windows_input::MacosCgEventHost;
pub use windows_input::PlayerAutomationError;
use windows_input::ScenarioInputAction;
pub use windows_input::WebCdpInputHost;
pub use windows_input::WebLiveAutomationRequest;
pub use windows_input::WebLiveAutomationRun;
pub use windows_input::WindowsLiveAutomationRequest;
pub use windows_input::WindowsLiveAutomationRun;
pub use windows_input::WindowsSendInputHost;
pub use windows_input::ANDROID_ACCESSIBILITY;
pub use windows_input::ANDROID_GAMEPAD;
pub use windows_input::ANDROID_KEYBOARD;
pub use windows_input::ANDROID_TOUCH;
pub use windows_input::LINUX_UINPUT_GAMEPAD;
pub use windows_input::LINUX_UINPUT_KEYBOARD;
pub use windows_input::LINUX_UINPUT_MOUSE;
pub use windows_input::LINUX_UINPUT_TOUCH;
pub use windows_input::MACOS_CGEVENT_KEYBOARD;
pub use windows_input::MACOS_CGEVENT_MOUSE;
pub use windows_input::WEB_CDP_KEYBOARD;
pub use windows_input::WEB_CDP_MOUSE;
pub use windows_input::WINDOWS_SENDINPUT_KEYBOARD;
pub use windows_input::WINDOWS_SENDINPUT_MOUSE;
mod validate_web_same_run_audio;
use validate_web_same_run_audio::host_audio_meter;
#[cfg(any(test, all(target_os = "windows", feature = "platform-test-driver")))]
use validate_web_same_run_audio::parse_player_host_traces;
use validate_web_same_run_audio::sha256_bytes;
use validate_web_same_run_audio::sha256_file;
use validate_web_same_run_audio::validate_web_same_run_audio;
use validate_web_same_run_audio::visual_comparison_evidence;
use validate_web_same_run_audio::windows_live_input;
use validate_web_same_run_audio::BundleContext;
use validate_web_same_run_audio::LiveInputRun;
