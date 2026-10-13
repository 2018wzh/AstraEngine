use super::*;

pub(super) type PlayerCliError = Box<dyn std::error::Error + Send + Sync>;

pub(super) fn main() -> Result<(), PlayerCliError> {
    let mut log_filter = env::var("ASTRA_LOG").ok();
    let mut log_format = None;
    let mut log_dir = None;
    let mut log_max_file_bytes = astra_observability::DEFAULT_MAX_FILE_BYTES;
    let mut log_max_archives = astra_observability::DEFAULT_MAX_ARCHIVES;
    let mut show_help = false;
    let mut test_null_audio = false;
    let mut preview_control = false;
    let mut script = None;
    let mut transcript = None;
    let mut windows_bundle = None;
    let mut web_bundle = None;
    let mut browser_executable = None;
    let mut web_headless = false;
    let mut visual_comparison_report = None;
    let mut host_conformance_report = None;
    let mut output_report = None;
    let mut output_script = None;
    let mut output_transcript = None;
    let mut output_trace_log = None;
    let mut timeout_ms = 30_000u64;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--log-filter" => {
                log_filter = Some(
                    args.next()
                        .ok_or("missing --log-filter value")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--log-format" => {
                log_format = Some(
                    match args
                        .next()
                        .ok_or("missing --log-format value")?
                        .to_string_lossy()
                        .as_ref()
                    {
                        "compact" => ConsoleFormat::Compact,
                        "json" => ConsoleFormat::Json,
                        _ => return Err("invalid --log-format value".into()),
                    },
                )
            }
            "--log-dir" => log_dir = args.next().map(PathBuf::from),
            "--log-max-file-bytes" => {
                log_max_file_bytes = parse_usize_arg(&mut args, "--log-max-file-bytes")?
            }
            "--log-max-archives" => {
                log_max_archives = parse_usize_arg(&mut args, "--log-max-archives")?
            }
            "--script" => script = args.next().map(PathBuf::from),
            "--transcript" => transcript = args.next().map(PathBuf::from),
            "--windows-bundle" => windows_bundle = args.next().map(PathBuf::from),
            "--web-bundle" => web_bundle = args.next().map(PathBuf::from),
            "--browser-executable" => browser_executable = args.next().map(PathBuf::from),
            "--web-headless" => web_headless = true,
            "--test-null-audio" => test_null_audio = true,
            "--preview-control" => preview_control = true,
            "--visual-comparison-report" => {
                visual_comparison_report = args.next().map(PathBuf::from)
            }
            "--host-conformance-report" => host_conformance_report = args.next().map(PathBuf::from),
            "--output-report" => output_report = args.next().map(PathBuf::from),
            "--output-script" => output_script = args.next().map(PathBuf::from),
            "--output-transcript" => output_transcript = args.next().map(PathBuf::from),
            "--output-trace-log" => output_trace_log = args.next().map(PathBuf::from),
            "--timeout-ms" => {
                let raw = args.next().ok_or("missing --timeout-ms value")?;
                timeout_ms = raw
                    .to_string_lossy()
                    .parse::<u64>()
                    .map_err(|_| "invalid --timeout-ms value")?;
            }
            "--help" | "-h" => {
                show_help = true;
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    let bundled_mode = script.is_none()
        && transcript.is_none()
        && windows_bundle.is_none()
        && web_bundle.is_none()
        && !show_help;
    if test_null_audio && (!cfg!(target_os = "windows") || (!bundled_mode && !show_help)) {
        return Err("--test-null-audio requires a bundled Windows Player".into());
    }
    let observability = if bundled_mode {
        load_bundled_observability(
            &bundled_player_resource_root().map_err(|error| -> PlayerCliError { error.into() })?,
            PlayerObservabilityOverrides {
                filter: log_filter,
                console_format: log_format,
                log_dir,
                max_file_bytes: Some(log_max_file_bytes),
                max_archives: Some(log_max_archives),
            },
        )
        .map_err(|error| -> PlayerCliError { error.into() })?
    } else {
        let mut config =
            HostObservabilityConfig::for_cli(log_filter.unwrap_or_else(|| "info".to_string()));
        config.role = HostRole::Player;
        config.console_format = log_format.unwrap_or(ConsoleFormat::Compact);
        config.log_dir = log_dir;
        config.max_file_bytes = log_max_file_bytes;
        config.max_archives = log_max_archives;
        config
    };
    let _observability = init_host(observability)?;
    tracing::info!(event = "player.host.start", "AstraPlayer host started");
    if show_help {
        println!(
            "Usage:\n  astra-player [--test-null-audio] [--preview-control] (bundled desktop game; preview uses bounded JSONL stdin/stdout)\n  astra-player --script <automation.json> --transcript <transcript.json>\n  astra-player --windows-bundle <dir> --visual-comparison-report <report.json> --host-conformance-report <report.json> [--output-report <report.json>] [--output-script <script.json>] [--output-transcript <transcript.json>] [--output-trace-log <trace.log>] [--timeout-ms <ms>]\n  astra-player --web-bundle <dir> --browser-executable <chromium> --visual-comparison-report <report.json> --host-conformance-report <report.json> [--web-headless] [--output-report <report.json>] [--output-script <script.json>] [--output-transcript <transcript.json>] [--timeout-ms <ms>] [--log-filter <filter>] [--log-format compact|json] [--log-dir <dir>]"
        );
        return Ok(());
    }
    if preview_control
        && (windows_bundle.is_some()
            || web_bundle.is_some()
            || script.is_some()
            || transcript.is_some())
    {
        return Err("--preview-control requires an ordinary bundled Player session".into());
    }
    if windows_bundle.is_some() && web_bundle.is_some() {
        return Err("--windows-bundle and --web-bundle are mutually exclusive".into());
    }
    if let Some(bundle_dir) = windows_bundle {
        let comparison = visual_comparison_report.ok_or("missing --visual-comparison-report")?;
        let conformance = host_conformance_report.ok_or("missing --host-conformance-report")?;
        let run = WindowsSendInputHost.run_live_bundle(WindowsLiveAutomationRequest {
            bundle_dir,
            visual_comparison_report: comparison,
            host_conformance_report: conformance,
            timeout_ms,
            trace_log: output_trace_log,
        })?;
        if let Some(path) = output_script {
            write_json(path, &run.script)?;
        }
        if let Some(path) = output_transcript {
            write_json(path, &run.transcript)?;
        }
        let report_json = serde_json::to_string_pretty(&run.report)?;
        if let Some(path) = output_report {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, report_json.as_bytes())?;
        }
        println!("{report_json}");
        return Ok(());
    }
    if let Some(bundle_dir) = web_bundle {
        let comparison = visual_comparison_report.ok_or("missing --visual-comparison-report")?;
        let conformance = host_conformance_report.ok_or("missing --host-conformance-report")?;
        let browser_executable =
            browser_executable.ok_or("missing --browser-executable for Web automation")?;
        let run = WebCdpInputHost.run_live_bundle(WebLiveAutomationRequest {
            bundle_dir,
            browser_executable,
            visual_comparison_report: comparison,
            host_conformance_report: conformance,
            headless: web_headless,
            timeout_ms,
        })?;
        if let Some(path) = output_script {
            write_json(path, &run.script)?;
        }
        if let Some(path) = output_transcript {
            write_json(path, &run.transcript)?;
        }
        let report_json = serde_json::to_string_pretty(&run.report)?;
        if let Some(path) = output_report {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, report_json.as_bytes())?;
        }
        println!("{report_json}");
        return Ok(());
    }

    if script.is_none() && transcript.is_none() {
        let result = run_bundled_game(test_null_audio, preview_control);
        if let Err(error) = &result {
            if let Some(platform) = error.downcast_ref::<astra_platform::PlatformError>() {
                tracing::error!(
                    event = "player.host.failed",
                    diagnostic = ?platform.code,
                    operation = %platform.operation,
                    reason = %platform.message,
                    "Bundled Player terminated"
                );
            } else {
                tracing::error!(
                    event = "player.host.failed",
                    diagnostic = "ASTRA_PLAYER_BUNDLE_FAILED",
                    "Bundled Player terminated before completing its platform session"
                );
            }
        }
        return result;
    }

    let script_path = script.ok_or("missing --script")?;
    let transcript_path = transcript.ok_or("missing --transcript")?;
    let script: PlayerAutomationScript = serde_json::from_slice(&fs::read(script_path)?)?;
    let transcript: PlayerInputTranscript = serde_json::from_slice(&fs::read(transcript_path)?)?;
    let report = match script.platform {
        PlayerPlatform::Windows => WindowsSendInputHost.build_report(&script, &transcript),
        PlayerPlatform::Linux => LinuxUinputHost.build_report(&script, &transcript),
        PlayerPlatform::Macos => MacosCgEventHost.build_report(&script, &transcript),
        PlayerPlatform::Web => WebCdpInputHost.build_report(&script, &transcript),
        PlayerPlatform::Android => AndroidInputHost.build_report(&script, &transcript),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
