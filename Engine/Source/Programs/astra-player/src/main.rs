use astra_observability::{init_host, ConsoleFormat, HostObservabilityConfig, HostRole};
use astra_player::{WebCdpInputHost, WindowsLiveAutomationRequest, WindowsSendInputHost};
use astra_player_core::{
    PlayerActionMap, PlayerAutomationScript, PlayerHostCommandResult, PlayerInputTranscript,
    PlayerPlatform,
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
        let mut save_transaction_id = 1000_u64;
        let timeline_clock = std::time::Instant::now();
        let mut timeline = astra_player_core::PlayerTimelineScheduler::new(256);
        let mut completed_media_signals = std::collections::BTreeSet::new();
        let mut persistent_audio = astra_player::NativeVnProductAudioHost::default();
        let player_result: Result<(), astra_platform::PlatformError> = async {
            process_timeline_updates(
            &mut vn,
            &mut executor,
            &mut timeline,
            timeline_clock.elapsed().as_millis() as u64,
            Vec::new(),
            &mut completed_media_signals,
            &mut persistent_audio,
        )
            .await?;
            let mut timeline_tick = tokio::time::interval(std::time::Duration::from_millis(8));
            timeline_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
            let event = tokio::select! {
                event = session.events.recv() => event?,
                _ = timeline_tick.tick(), if timeline.active_count() > 0 || persistent_audio.is_active() => {
                    let now_ms = timeline_clock.elapsed().as_millis() as u64;
                    let completed = timeline
                        .poll(now_ms)
                        .map_err(|error| player_platform_error("player.timeline.poll", error))?;
                    process_timeline_updates(
                        &mut vn,
                        &mut executor,
                        &mut timeline,
                        now_ms,
                        completed,
                        &mut completed_media_signals,
                        &mut persistent_audio,
                    )
                    .await?;
                    continue;
                }
            };
            let player_sequence = event.sequence;
            match event.kind {
                PlatformEventKind::WindowClosed { window: closed } if closed == window => break,
                PlatformEventKind::Keyboard {
                    window: input_window,
                    physical_key,
                    state: InputState::Pressed,
                    ..
                } if input_window == window => {
                    if physical_key == "F5" {
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
                    if physical_key == "F9" {
                        execute_platform_load(&mut vn, &mut executor, "slot.quick").await?;
                        tracing::info!(
                            event = "astra.player.save.restored",
                            player_sequence,
                            slot = "slot.quick",
                            "Player restored platform save transaction"
                        );
                        continue;
                    }
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
                    process_timeline_updates(
                        &mut vn,
                        &mut executor,
                        &mut timeline,
                        timeline_clock.elapsed().as_millis() as u64,
                        Vec::new(),
                        &mut completed_media_signals,
                        &mut persistent_audio,
                    )
                    .await?;
                    log_consumed_vn_step(player_sequence, "keyboard", &vn)?;
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
                    process_timeline_updates(
                        &mut vn,
                        &mut executor,
                        &mut timeline,
                        timeline_clock.elapsed().as_millis() as u64,
                        Vec::new(),
                        &mut completed_media_signals,
                        &mut persistent_audio,
                    )
                    .await?;
                    log_consumed_vn_step(player_sequence, "pointer", &vn)?;
                }
                _ => {}
            }
            }
            Ok(())
        }
        .await;
        let audio_cleanup = persistent_audio.shutdown(&mut vn, &mut executor).await;
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
        session.client.destroy_surface(surface).await?;
        session.client.destroy_window(window).await?;
        session.client.shutdown().await?;
        Ok::<(), astra_platform::PlatformError>(())
    })?;
    Ok(())
}

#[cfg(target_os = "windows")]
async fn process_timeline_updates(
    source: &mut astra_player::NativeVnHostCommandSource,
    executor: &mut astra_player::PlayerHostCommandExecutor<astra_player::PlatformCommandSink>,
    scheduler: &mut astra_player_core::PlayerTimelineScheduler,
    now_ms: u64,
    mut completed: Vec<astra_player_core::PlayerTimelineCompletion>,
    completed_media_signals: &mut std::collections::BTreeSet<String>,
    persistent_audio: &mut astra_player::NativeVnProductAudioHost,
) -> Result<(), astra_platform::PlatformError> {
    const MAX_DECODED_AUDIO_SAMPLES: usize = 10_000_000;
    for _ in 0..1024 {
        let tasks = source.take_timeline_tasks();
        if !tasks.is_empty() {
            let mut candidate = scheduler.clone();
            let mut scheduled_completions = Vec::new();
            for task in tasks {
                scheduled_completions.extend(
                    candidate.schedule(task, now_ms).map_err(|error| {
                        player_platform_error("player.timeline.schedule", error)
                    })?,
                );
            }
            *scheduler = candidate;
            completed.extend(scheduled_completions);
        }
        let current = std::mem::take(&mut completed);
        for completion in current {
            tracing::info!(
                event = "astra.player.timeline.completed",
                task_id = %completion.task_id,
                target = %completion.target,
                completion = ?completion.kind,
                completed_at_ms = completion.completed_at_ms,
                "Player timeline task reached a host completion boundary"
            );
            if let Some(fence) = completion.fence {
                completed_media_signals.insert(fence);
            }
        }

        let audio_requests = source.take_audio_requests();
        for output in audio_requests {
            let request = match output {
                astra_player::NativeVnAudioOutput::Control(request) => {
                    persistent_audio.control(&request, completed_media_signals)?;
                    continue;
                }
                astra_player::NativeVnAudioOutput::Start(request) => request,
            };
            let decode = source
                .prepare_audio_decode(&request)
                .map_err(|error| player_platform_error("player.audio.decode.prepare", error))?;
            let decoded = executor
                .execute_decode_lifecycle(decode)
                .await
                .map_err(|error| player_platform_error("player.audio.decode", error))?;
            let audio = astra_player_core::PlayerDecodedAudio::parse(
                &decoded.format,
                &decoded.bytes,
                MAX_DECODED_AUDIO_SAMPLES,
            )
            .map_err(|error| player_platform_error("player.audio.contract", error))?;
            persistent_audio
                .start(source, executor, &request, audio)
                .await?;
            tracing::info!(
                event = "astra.player.audio.started",
                command_id = %request.command_id,
                command = %request.command,
                asset_id = %request.asset_id,
                encoded_hash = %request.encoded_hash,
                decoded_hash = %decoded.hash,
                "Player started a packaged audio voice in the persistent mixer"
            );
        }

        persistent_audio
            .pump(source, executor, completed_media_signals)
            .await?;

        let pending_fence = source.pending_wait().map(|wait| wait.fence.clone());
        if let Some(fence) = pending_fence {
            if completed_media_signals.remove(&fence) {
                let batch = source
                    .complete_wait(fence)
                    .map_err(|error| player_platform_error("player.media.complete_wait", error))?;
                executor
                    .execute_batch(batch)
                    .await
                    .map_err(|error| player_platform_error("player.media.present", error))?;
                continue;
            }
        }
        if completed.is_empty() {
            return Ok(());
        }
    }
    Err(astra_platform::PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        "player.timeline.schedule",
        "ASTRA_PLAYER_TIMELINE_COMPLETION_LOOP: completion chain exceeded its bound",
    ))
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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
