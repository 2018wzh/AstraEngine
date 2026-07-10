use astra_observability::{init_host, ConsoleFormat, HostObservabilityConfig, HostRole};
use astra_player::{WebCdpInputHost, WindowsLiveAutomationRequest, WindowsSendInputHost};
use astra_player_core::{
    PlayerActionMap, PlayerAutomationScript, PlayerInputTranscript, PlayerPlatform,
};
use std::{env, fs, path::PathBuf};

type PlayerCliError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), PlayerCliError> {
    let mut log_filter = env::var("ASTRA_LOG").unwrap_or_else(|_| "info".to_string());
    let mut log_format = ConsoleFormat::Compact;
    let mut log_dir = None;
    let mut log_max_file_bytes = astra_observability::DEFAULT_MAX_FILE_BYTES;
    let mut log_max_archives = astra_observability::DEFAULT_MAX_ARCHIVES;
    let mut show_help = false;
    let mut script = None;
    let mut transcript = None;
    let mut windows_bundle = None;
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
                log_filter = args
                    .next()
                    .ok_or("missing --log-filter value")?
                    .to_string_lossy()
                    .into_owned()
            }
            "--log-format" => {
                log_format = match args
                    .next()
                    .ok_or("missing --log-format value")?
                    .to_string_lossy()
                    .as_ref()
                {
                    "compact" => ConsoleFormat::Compact,
                    "json" => ConsoleFormat::Json,
                    _ => return Err("invalid --log-format value".into()),
                }
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
    let mut observability = HostObservabilityConfig::for_cli(log_filter);
    observability.role = HostRole::Player;
    observability.console_format = log_format;
    observability.log_dir = log_dir;
    observability.max_file_bytes = log_max_file_bytes;
    observability.max_archives = log_max_archives;
    let _observability = init_host(observability)?;
    tracing::info!(event = "player.host.start", "AstraPlayer host started");
    if show_help {
        println!(
            "Usage:\n  astra-player --script <automation.json> --transcript <transcript.json>\n  astra-player --windows-bundle <dir> --visual-comparison-report <report.json> --host-conformance-report <report.json> [--output-report <report.json>] [--output-script <script.json>] [--output-transcript <transcript.json>] [--output-trace-log <trace.log>] [--timeout-ms <ms>] [--log-filter <filter>] [--log-format compact|json] [--log-dir <dir>]"
        );
        return Ok(());
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

    if script.is_none() && transcript.is_none() {
        return run_bundled_game();
    }

    let script_path = script.ok_or("missing --script")?;
    let transcript_path = transcript.ok_or("missing --transcript")?;
    let script: PlayerAutomationScript = serde_json::from_slice(&fs::read(script_path)?)?;
    let transcript: PlayerInputTranscript = serde_json::from_slice(&fs::read(transcript_path)?)?;
    let report = match script.platform {
        PlayerPlatform::Windows => WindowsSendInputHost.build_report(&script, &transcript),
        PlayerPlatform::Web => WebCdpInputHost.build_report(&script, &transcript),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_bundled_game() -> Result<(), PlayerCliError> {
    use astra_core::Hash256;
    use astra_package::{PackageManifest, PackageReader};
    use astra_platform::{
        InputState, PackageSourceRequest, PlatformEventKind, PlatformHostFactory, PlatformId,
        PointerButton, SurfaceRequest, WindowRequest,
    };
    use astra_player::{
        NativeVnHostCommandSource, PlatformCommandSink, PlayerHostCommandExecutor,
        PlayerHostResourceId,
    };
    use astra_vn_core::VnRunConfig;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Config {
        schema: String,
        target: String,
        profile: String,
        platform: String,
        package: String,
    }
    #[derive(Deserialize)]
    struct Profiles {
        schema: String,
        profiles: Vec<serde_json::Value>,
    }

    let config: Config = serde_json::from_slice(&fs::read("AstraPlayer.config.json")?)?;
    if config.schema != "astra.player_config.v2" || config.platform != "windows" {
        return Err("invalid Windows Player config".into());
    }
    let package_bytes = fs::read(&config.package)?;
    let package_hash = Hash256::from_sha256(&package_bytes).to_string();
    let package = PackageReader::open(&package_bytes)?;
    let manifest: PackageManifest = package.container().decode_postcard("package.manifest")?;
    if manifest.profile != config.profile {
        return Err("Player config/package profile mismatch".into());
    }
    let profiles: Profiles =
        serde_json::from_slice(&package.container().read_section("platform.profiles")?)?;
    if !matches!(
        profiles.schema.as_str(),
        "astra.platform_profiles.v1" | "astra.platform_profiles.v2"
    ) {
        return Err("unsupported platform profile section".into());
    }
    let profile = profiles
        .profiles
        .into_iter()
        .map(astra_platform::migrate_host_profile_json)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|profile| {
            profile.platform == PlatformId::Windows
                && profile.target == config.target
                && profile.package_id == manifest.package_id
        })
        .ok_or("package does not contain a matching Windows profile")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let mut session = astra_platform_windows::factory().start(profile).await?;
        let source = session
            .client
            .open_package(PackageSourceRequest::Bundled {
                relative_path: config.package.clone(),
                expected_hash: package_hash.clone(),
            })
            .await?;
        let _container_header = session.client.read_package_range(source, 0, 16).await?;
        session.client.close_package(source).await?;

        let width = 1280;
        let height = 720;
        let window = session
            .client
            .create_window(WindowRequest {
                title: manifest.package_id,
                width,
                height,
                visible: true,
            })
            .await?;
        let surface = session
            .client
            .create_surface(SurfaceRequest {
                window,
                width,
                height,
            })
            .await?;
        let logical_surface = PlayerHostResourceId(1);
        let mut sink = PlatformCommandSink::new(session.client.clone());
        sink.bind_surface(logical_surface, surface)?;
        let mut executor = PlayerHostCommandExecutor::new(sink);
        let mut vn = NativeVnHostCommandSource::from_package(
            &package,
            VnRunConfig {
                profile: config.profile,
                locale: "zh-Hans".to_string(),
            },
            width,
            height,
            logical_surface,
        )
        .map_err(|error| {
            astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "player.runtime.open",
                error.to_string(),
            )
        })?;
        executor
            .execute_batch(vn.launch().map_err(|error| {
                astra_platform::PlatformError::new(
                    astra_platform::PlatformErrorCode::InvalidState,
                    "player.runtime.launch",
                    error.to_string(),
                )
            })?)
            .await
            .map_err(|error| {
                astra_platform::PlatformError::new(
                    astra_platform::PlatformErrorCode::InvalidState,
                    "player.host.execute",
                    error.to_string(),
                )
            })?;
        let action_map = PlayerActionMap::standard();
        let mut pointer = (0.0_f64, 0.0_f64);
        loop {
            let event = session.events.recv().await?;
            match event.kind {
                PlatformEventKind::WindowClosed { window: closed } if closed == window => break,
                PlatformEventKind::Keyboard {
                    window: input_window,
                    physical_key,
                    state: InputState::Pressed,
                    ..
                } if input_window == window => {
                    let Some(action) = action_map.keyboard(&physical_key) else {
                        continue;
                    };
                    let batch = vn.dispatch_action(action).map_err(|error| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.runtime.input",
                            error.to_string(),
                        )
                    })?;
                    executor.execute_batch(batch).await.map_err(|error| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.host.execute",
                            error.to_string(),
                        )
                    })?;
                }
                PlatformEventKind::PointerMoved {
                    window: input_window,
                    x,
                    y,
                } if input_window == window => pointer = (x, y),
                PlatformEventKind::PointerButton {
                    window: input_window,
                    button: PointerButton::Primary,
                    state: InputState::Pressed,
                } if input_window == window => {
                    let batch = vn.dispatch_pointer(pointer.0, pointer.1).map_err(|error| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.runtime.hit_test",
                            error.to_string(),
                        )
                    })?;
                    executor.execute_batch(batch).await.map_err(|error| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.host.execute",
                            error.to_string(),
                        )
                    })?;
                }
                _ => {}
            }
        }
        session.client.destroy_surface(surface).await?;
        session.client.destroy_window(window).await?;
        session.client.shutdown().await?;
        Ok::<(), astra_platform::PlatformError>(())
    })?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_bundled_game() -> Result<(), PlayerCliError> {
    Err("native AstraPlayer bundle host is only implemented for Windows in Migration 8".into())
}

fn parse_usize_arg(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    option: &str,
) -> Result<usize, PlayerCliError> {
    let raw = args
        .next()
        .ok_or_else(|| format!("missing {option} value"))?;
    let value = raw
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| format!("invalid {option} value"))?;
    if value == 0 {
        return Err(format!("{option} must be non-zero").into());
    }
    Ok(value)
}

fn write_json<T: serde::Serialize>(path: PathBuf, value: &T) -> Result<(), PlayerCliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
