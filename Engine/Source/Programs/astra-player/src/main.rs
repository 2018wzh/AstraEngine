use astra_observability::{init_host, ConsoleFormat, HostObservabilityConfig, HostRole};
use astra_player::{
    LinuxUinputHost, MacosCgEventHost, WebCdpInputHost, WebLiveAutomationRequest,
    WindowsLiveAutomationRequest, WindowsSendInputHost,
};
use astra_player_core::{
    PlayerAutomationScript, PlayerHostCommandResult, PlayerInputTranscript, PlayerPlatform,
};
use std::{env, fs, path::PathBuf};

type PlayerCliError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(target_os = "windows")]
#[derive(serde::Deserialize)]
struct UiComponentsConfig {
    schema: String,
    host: Option<String>,
    allowlist: String,
    deadline_ms: u64,
    components: Vec<UiComponentConfig>,
}

#[cfg(target_os = "windows")]
#[derive(serde::Deserialize)]
struct UiComponentConfig {
    id: String,
    manifest: String,
    artifact: String,
}

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
            "--web-bundle" => web_bundle = args.next().map(PathBuf::from),
            "--browser-executable" => browser_executable = args.next().map(PathBuf::from),
            "--web-headless" => web_headless = true,
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
            "Usage:\n  astra-player --script <automation.json> --transcript <transcript.json>\n  astra-player --windows-bundle <dir> --visual-comparison-report <report.json> --host-conformance-report <report.json> [--output-report <report.json>] [--output-script <script.json>] [--output-transcript <transcript.json>] [--output-trace-log <trace.log>] [--timeout-ms <ms>]\n  astra-player --web-bundle <dir> --browser-executable <chromium> --visual-comparison-report <report.json> --host-conformance-report <report.json> [--web-headless] [--output-report <report.json>] [--output-script <script.json>] [--output-transcript <transcript.json>] [--timeout-ms <ms>] [--log-filter <filter>] [--log-format compact|json] [--log-dir <dir>]"
        );
        return Ok(());
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
        return run_bundled_game();
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
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn run_bundled_game() -> Result<(), PlayerCliError> {
    use astra_core::Hash256;
    use astra_package::{PackageManifest, PackageReader};
    use astra_platform::{
        HostLaunchProfile, InputState, PackageSourceRequest, PlatformEventKind,
        PlatformHostFactory, PlatformId, SurfaceRequest, WindowRequest,
    };
    use astra_player::{
        NativeVnHostCommandSource, PlatformCommandSink, PlayerHostCommandExecutor,
        PlayerHostResourceId,
    };
    use astra_ui_core::{UiInputEventKind, UiInsets, UiPoint, UiTouchPhase, UiViewport};
    use astra_vn_core::VnRunConfig;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Config {
        schema: String,
        target: String,
        profile: String,
        platform: String,
        locale: String,
        package: String,
        #[cfg(target_os = "windows")]
        #[serde(default)]
        ui_components: Option<UiComponentsConfig>,
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        #[serde(default)]
        ui_components: Option<serde_json::Value>,
    }
    #[derive(Deserialize)]
    struct Profiles {
        schema: String,
        profiles: Vec<serde_json::Value>,
    }

    #[cfg(target_os = "macos")]
    let resource_root = std::env::current_exe()?
        .parent()
        .and_then(std::path::Path::parent)
        .map(|contents| contents.join("Resources"))
        .ok_or("macOS app resource directory is unavailable")?;
    #[cfg(not(target_os = "macos"))]
    let resource_root = PathBuf::from(".");
    let config: Config =
        serde_json::from_slice(&fs::read(resource_root.join("AstraPlayer.config.json"))?)?;
    let expected_platform = match () {
        _ if cfg!(target_os = "windows") => "windows",
        _ if cfg!(target_os = "macos") => "macos",
        _ => "linux",
    };
    if config.schema != "astra.player_config.v2" || config.platform != expected_platform {
        return Err("Player config does not match the native platform".into());
    }
    #[cfg(target_os = "windows")]
    let mut ui_component_processes = open_ui_component_processes(config.ui_components.as_ref())?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if config.ui_components.is_some() {
        return Err("native Player UI components are not implemented on this platform".into());
    }
    let package_bytes = fs::read(resource_root.join(&config.package))?;
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
            profile.platform
                == match () {
                    _ if cfg!(target_os = "windows") => PlatformId::Windows,
                    _ if cfg!(target_os = "macos") => PlatformId::Macos,
                    _ => PlatformId::Linux,
                }
                && profile.target == config.target
                && profile.package_id == manifest.package_id
        })
        .ok_or("package does not contain a matching native platform profile")?;
    #[cfg(target_os = "macos")]
    let (mut runner, factory) = astra_platform_macos::main_thread_host()?;
    let player = async move {
        #[cfg(target_os = "windows")]
        let factory = astra_platform_windows::factory();
        #[cfg(target_os = "linux")]
        let factory = astra_platform_linux::factory();
        let mut session = factory.start(HostLaunchProfile::platform(profile)).await?;
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
                locale: config.locale,
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
        let mut pointer = (0.0_f64, 0.0_f64);
        let mut save_transaction_id = 1000_u64;
        let timeline_clock = std::time::Instant::now();
        let mut media = astra_player::NativeVnProductMediaHost::default();
        let player_result: Result<(), astra_platform::PlatformError> = async {
            media
                .process(
                    &mut vn,
                    &mut executor,
                    timeline_clock.elapsed().as_millis() as u64,
                    Vec::new(),
                )
                .await?;
            let mut timeline_tick = tokio::time::interval(std::time::Duration::from_millis(8));
            timeline_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let event = tokio::select! {
                    event = session.events.recv() => event?,
                    _ = timeline_tick.tick(), if media.is_active() => {
                        let now_ms = timeline_clock.elapsed().as_millis() as u64;
                        media.poll_and_process(&mut vn, &mut executor, now_ms).await?;
                        continue;
                    }
                };
                let player_sequence = event.sequence;
                let ui_input = match event.kind {
                    PlatformEventKind::WindowClosed { window: closed } if closed == window => break,
                    PlatformEventKind::WindowResized {
                        window: resized,
                        width,
                        height,
                        scale_factor,
                    } if resized == window && width > 0 && height > 0 => {
                        Some(UiInputEventKind::Resize {
                            viewport: UiViewport {
                                physical_width: width,
                                physical_height: height,
                                scale_factor: scale_factor as f32,
                                font_scale: 1.0,
                                safe_area_points: UiInsets {
                                    left: 0.0,
                                    top: 0.0,
                                    right: 0.0,
                                    bottom: 0.0,
                                },
                            },
                        })
                    }
                    PlatformEventKind::Keyboard {
                        window: input_window,
                        physical_key,
                        logical_key,
                        state,
                        repeat,
                    } if input_window == window => {
                        if state == InputState::Pressed && physical_key == "F5" {
                            save_transaction_id =
                                save_transaction_id.checked_add(1).ok_or_else(|| {
                                    astra_platform::PlatformError::new(
                                        astra_platform::PlatformErrorCode::InvalidState,
                                        "player.save.transaction",
                                        "ASTRA_PLAYER_SAVE_TRANSACTION_OVERFLOW",
                                    )
                                })?;
                            execute_platform_save(
                                &mut vn,
                                &mut executor,
                                "slot.quick",
                                PlayerHostResourceId(save_transaction_id),
                            )
                            .await?;
                            tracing::info!(
                                event = "astra.player.save.committed",
                                player_sequence,
                                slot = "slot.quick",
                                "Player committed platform save transaction"
                            );
                            continue;
                        }
                        if state == InputState::Pressed && physical_key == "F9" {
                            execute_platform_load(&mut vn, &mut executor, "slot.quick").await?;
                            tracing::info!(
                                event = "astra.player.save.restored",
                                player_sequence,
                                slot = "slot.quick",
                                "Player restored platform save transaction"
                            );
                            continue;
                        }
                        Some(UiInputEventKind::Keyboard {
                            logical_key: logical_key.unwrap_or_else(|| physical_key.clone()),
                            physical_key,
                            state: ui_button_state(state),
                            repeat,
                            modifiers: 0,
                        })
                    }
                    PlatformEventKind::PointerMoved {
                        window: input_window,
                        x,
                        y,
                    } if input_window == window => {
                        pointer = (x, y);
                        Some(UiInputEventKind::PointerMove {
                            position: UiPoint {
                                x: x as f32,
                                y: y as f32,
                            },
                        })
                    }
                    PlatformEventKind::PointerButton {
                        window: input_window,
                        button,
                        state,
                    } if input_window == window => Some(UiInputEventKind::PointerButton {
                        position: UiPoint {
                            x: pointer.0 as f32,
                            y: pointer.1 as f32,
                        },
                        button: ui_pointer_button(button),
                        state: ui_button_state(state),
                    }),
                    PlatformEventKind::MouseWheel {
                        window: input_window,
                        delta_x,
                        delta_y,
                    } if input_window == window => Some(UiInputEventKind::Wheel {
                        delta_points: UiPoint {
                            x: delta_x,
                            y: delta_y,
                        },
                    }),
                    PlatformEventKind::ImePreedit {
                        window: input_window,
                        text,
                        cursor,
                    } if input_window == window => Some(UiInputEventKind::ImePreedit {
                        text,
                        cursor_start: cursor.map(|value| value.0 as u32),
                        cursor_end: cursor.map(|value| value.1 as u32),
                    }),
                    PlatformEventKind::ImeCommit {
                        window: input_window,
                        text,
                    } if input_window == window => Some(UiInputEventKind::ImeCommit { text }),
                    PlatformEventKind::Touch {
                        window: input_window,
                        id,
                        x,
                        y,
                        phase,
                    } if input_window == window => Some(UiInputEventKind::Touch {
                        device_id: 0,
                        contact_id: id,
                        position: UiPoint {
                            x: x as f32,
                            y: y as f32,
                        },
                        phase: match phase {
                            astra_platform::TouchPhase::Started => UiTouchPhase::Started,
                            astra_platform::TouchPhase::Moved => UiTouchPhase::Moved,
                            astra_platform::TouchPhase::Ended => UiTouchPhase::Ended,
                            astra_platform::TouchPhase::Cancelled => UiTouchPhase::Cancelled,
                        },
                    }),
                    PlatformEventKind::GamepadInput { control, value, .. } if value > 0.5 => {
                        gamepad_navigation(control)
                            .map(|action| UiInputEventKind::Navigation { action })
                    }
                    PlatformEventKind::AccessibilityAction {
                        window: input_window,
                        semantic_id,
                        action,
                        value,
                    } if input_window == window => Some(UiInputEventKind::AccessibilityAction {
                        semantic_id,
                        action,
                        value,
                    }),
                    _ => None,
                };
                if let Some(kind) = ui_input {
                    let batch = vn.dispatch_ui_event(kind).map_err(|error| {
                        astra_platform::PlatformError::new(
                            astra_platform::PlatformErrorCode::InvalidState,
                            "player.runtime.ui_input",
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
                    if let Some(request) = vn.take_ui_host_request() {
                        match request {
                            astra_player::VnUiHostRequest::Save { slot_id } => {
                                save_transaction_id =
                                    save_transaction_id.checked_add(1).ok_or_else(|| {
                                        astra_platform::PlatformError::new(
                                            astra_platform::PlatformErrorCode::InvalidState,
                                            "player.save.transaction",
                                            "ASTRA_PLAYER_SAVE_TRANSACTION_OVERFLOW",
                                        )
                                    })?;
                                execute_platform_save(
                                    &mut vn,
                                    &mut executor,
                                    &slot_id,
                                    PlayerHostResourceId(save_transaction_id),
                                )
                                .await?;
                                vn.mark_save_committed(&slot_id).map_err(|error| {
                                    player_platform_error("player.save.commit_state", error)
                                })?;
                            }
                            astra_player::VnUiHostRequest::Load { slot_id } => {
                                execute_platform_load(&mut vn, &mut executor, &slot_id).await?;
                            }
                            astra_player::VnUiHostRequest::Delete { slot_id } => {
                                executor
                                    .execute_batch(vn.delete_save(&slot_id).map_err(|error| {
                                        player_platform_error("player.save.delete.prepare", error)
                                    })?)
                                    .await
                                    .map_err(|error| {
                                        player_platform_error("player.save.delete", error)
                                    })?;
                                vn.mark_save_deleted(&slot_id).map_err(|error| {
                                    player_platform_error("player.save.delete_state", error)
                                })?;
                            }
                        }
                    }
                    media
                        .process(
                            &mut vn,
                            &mut executor,
                            timeline_clock.elapsed().as_millis() as u64,
                            Vec::new(),
                        )
                        .await?;
                    log_consumed_vn_step(player_sequence, "physical_ui", &vn)?;
                }
            }
            Ok(())
        }
        .await;
        let audio_cleanup = media.shutdown(&mut vn, &mut executor).await;
        match (player_result, audio_cleanup) {
            (Err(error), Err(cleanup)) => {
                return Err(player_platform_error(
                    "player.session",
                    format!("{error}; audio cleanup failed: {cleanup}"),
                ));
            }
            (Err(error), Ok(())) => return Err(error),
            (Ok(()), Err(cleanup)) => return Err(cleanup),
            (Ok(()), Ok(())) => {}
        }
        let shutdown_batch = vn.release_resources().map_err(|error| {
            astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "player.runtime.release_resources",
                error.to_string(),
            )
        })?;
        executor
            .execute_batch(shutdown_batch)
            .await
            .map_err(|error| {
                astra_platform::PlatformError::new(
                    astra_platform::PlatformErrorCode::InvalidState,
                    "player.host.release_resources",
                    error.to_string(),
                )
            })?;
        vn.shutdown().map_err(|error| {
            astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "player.runtime.shutdown",
                error.to_string(),
            )
        })?;
        session.client.destroy_surface(surface).await?;
        session.client.destroy_window(window).await?;
        session.client.shutdown().await?;
        #[cfg(target_os = "windows")]
        for process in &mut ui_component_processes {
            process
                .invoke(astra_ui_plugin_abi::UiComponentRequest::Shutdown)
                .map_err(|error| player_platform_error("player.ui_component.shutdown", error))?;
        }
        Ok::<(), astra_platform::PlatformError>(())
    };
    #[cfg(target_os = "macos")]
    runner.run(player)??;
    #[cfg(not(target_os = "macos"))]
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(player)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_ui_component_processes(
    config: Option<&UiComponentsConfig>,
) -> Result<Vec<astra_ui_component_host::UiComponentProcess>, PlayerCliError> {
    use astra_ui_component_host::{UiComponentProcess, UiComponentProcessConfig};
    use astra_ui_plugin_abi::UiComponentRequest;
    use std::collections::BTreeSet;
    use std::time::Duration;

    let Some(config) = config else {
        return Ok(Vec::new());
    };
    if config.schema != "astra.player_ui_components.v1"
        || config.components.is_empty()
        || !(1..=60_000).contains(&config.deadline_ms)
    {
        return Err("ASTRA_UI_COMPONENT_CONFIG: invalid component session config".into());
    }
    let host = config
        .host
        .as_deref()
        .ok_or("ASTRA_UI_COMPONENT_HOST_MISSING: Windows config requires a host")?;
    let host = resolve_bundled_component_path(host)?;
    let allowlist = resolve_bundled_component_path(&config.allowlist)?;
    let mut ids = BTreeSet::new();
    let mut processes = Vec::with_capacity(config.components.len());
    for component in &config.components {
        if component.id.is_empty() || !ids.insert(component.id.clone()) {
            return Err("ASTRA_UI_COMPONENT_CONFIG_ID: component IDs must be unique".into());
        }
        let mut process = UiComponentProcess::spawn(UiComponentProcessConfig {
            host_binary: host.clone(),
            manifest: resolve_bundled_component_path(&component.manifest)?,
            artifact: resolve_bundled_component_path(&component.artifact)?,
            allowlist: allowlist.clone(),
            deadline: Duration::from_millis(config.deadline_ms),
        })?;
        let session_id = format!("vn.ui.component.{}", component.id);
        process.invoke(UiComponentRequest::Open {
            session_id,
            component_id: component.id.clone(),
            initial_state: Vec::new(),
        })?;
        processes.push(process);
    }
    Ok(processes)
}

#[cfg(target_os = "windows")]
fn resolve_bundled_component_path(relative: &str) -> Result<PathBuf, PlayerCliError> {
    use std::path::Component;
    let path = std::path::Path::new(relative);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("ASTRA_UI_COMPONENT_PATH: component path must be bundle-relative".into());
    }
    let root = std::env::current_dir()?.canonicalize()?;
    let resolved = root.join(path).canonicalize()?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err("ASTRA_UI_COMPONENT_PATH: component path escapes or is missing".into());
    }
    Ok(resolved)
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn ui_button_state(state: astra_platform::InputState) -> astra_ui_core::UiButtonState {
    match state {
        astra_platform::InputState::Pressed => astra_ui_core::UiButtonState::Pressed,
        astra_platform::InputState::Released => astra_ui_core::UiButtonState::Released,
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn ui_pointer_button(button: astra_platform::PointerButton) -> astra_ui_core::UiPointerButton {
    match button {
        astra_platform::PointerButton::Primary => astra_ui_core::UiPointerButton::Primary,
        astra_platform::PointerButton::Secondary => astra_ui_core::UiPointerButton::Secondary,
        astra_platform::PointerButton::Middle => astra_ui_core::UiPointerButton::Middle,
        astra_platform::PointerButton::Back => astra_ui_core::UiPointerButton::Back,
        astra_platform::PointerButton::Forward => astra_ui_core::UiPointerButton::Forward,
        astra_platform::PointerButton::Other(value) => astra_ui_core::UiPointerButton::Other(value),
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn gamepad_navigation(
    control: astra_platform::GamepadControl,
) -> Option<astra_ui_core::UiNavigationAction> {
    use astra_platform::GamepadControl;
    use astra_ui_core::UiNavigationAction;
    match control {
        GamepadControl::DpadUp => Some(UiNavigationAction::Up),
        GamepadControl::DpadDown => Some(UiNavigationAction::Down),
        GamepadControl::DpadLeft => Some(UiNavigationAction::Left),
        GamepadControl::DpadRight => Some(UiNavigationAction::Right),
        GamepadControl::South => Some(UiNavigationAction::Activate),
        GamepadControl::East => Some(UiNavigationAction::Cancel),
        GamepadControl::LeftShoulder => Some(UiNavigationAction::PagePrevious),
        GamepadControl::RightShoulder => Some(UiNavigationAction::PageNext),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
async fn execute_platform_save(
    source: &mut astra_player::NativeVnHostCommandSource,
    executor: &mut astra_player::PlayerHostCommandExecutor<astra_player::PlatformCommandSink>,
    slot: &str,
    transaction: astra_player::PlayerHostResourceId,
) -> Result<(), astra_platform::PlatformError> {
    let plan = source
        .prepare_save_transaction(slot, transaction)
        .map_err(|error| player_platform_error("player.save.prepare", error))?;
    executor
        .execute_save_transaction(plan)
        .await
        .map_err(|error| player_platform_error("player.save.transaction", error))?;
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
async fn execute_platform_load(
    source: &mut astra_player::NativeVnHostCommandSource,
    executor: &mut astra_player::PlayerHostCommandExecutor<astra_player::PlatformCommandSink>,
    slot: &str,
) -> Result<(), astra_platform::PlatformError> {
    let results = executor
        .execute_batch(
            source
                .read_save(slot)
                .map_err(|error| player_platform_error("player.save.read.prepare", error))?,
        )
        .await
        .map_err(|error| player_platform_error("player.save.read", error))?;
    let bytes = match results.as_slice() {
        [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
        _ => {
            return Err(astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "player.save.read",
                "ASTRA_PLAYER_SAVE_RESULT_INVALID: platform returned an unexpected result",
            ));
        }
    };
    let present = source
        .restore(bytes)
        .map_err(|error| player_platform_error("player.save.restore", error))?;
    executor
        .execute_batch(present)
        .await
        .map_err(|error| player_platform_error("player.save.present", error))?;
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn player_platform_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> astra_platform::PlatformError {
    astra_platform::PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn log_consumed_vn_step(
    player_sequence: u64,
    kind: &str,
    source: &astra_player_vn::NativeVnHostCommandSource,
) -> Result<(), astra_platform::PlatformError> {
    let evidence = source.last_step_evidence().ok_or_else(|| {
        astra_platform::PlatformError::new(
            astra_platform::PlatformErrorCode::InvalidState,
            "player.runtime.evidence",
            "ASTRA_PLAYER_VN_EVIDENCE_MISSING: consumed input has no runtime evidence",
        )
    })?;
    let coverage = if evidence.coverage_reached.is_empty() {
        "-".to_string()
    } else {
        evidence
            .coverage_reached
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
    };
    let current_state_id = evidence.current_state_id.as_deref().unwrap_or("-");
    let pending_choice_ids = if evidence.pending_choice_ids.is_empty() {
        "-".to_string()
    } else {
        evidence.pending_choice_ids.join(",")
    };
    let terminal_route_ids = if evidence.terminal_route_ids.is_empty() {
        "-".to_string()
    } else {
        evidence
            .terminal_route_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
    };
    tracing::info!(
        event = "astra.player.input.consumed",
        player_sequence,
        kind,
        "Player host consumed platform input"
    );
    tracing::info!(
        event = "astra.player.vn.step",
        player_sequence,
        fixed_step = evidence.fixed_step,
        coverage = %coverage,
        runtime_state_hash = %evidence.runtime_state_hash,
        runtime_event_hash = %evidence.runtime_event_hash,
        runtime_presentation_hash = %evidence.runtime_presentation_hash,
        current_state_id,
        pending_choice_ids = %pending_choice_ids,
        terminal_route_ids = %terminal_route_ids,
        "Player host committed RuntimeWorld VN step"
    );
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn run_bundled_game() -> Result<(), PlayerCliError> {
    Err("native AstraPlayer bundle host is unavailable on this platform".into())
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
