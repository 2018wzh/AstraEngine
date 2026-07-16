use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use astra_core::{Hash256, SchemaVersion};
use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAwaitResult, LegacyEffect, LegacyInputEdge, LegacyProbeRequest,
    LegacyRenderFrameV1, LegacyRuntimeHostCtx, LegacyStepBudget, LegacyVideoCommandV1,
    LegacyWaitRequest,
};
use astra_emu_manager_core::{
    AstraEmuRuntimeProvider, CancellationToken, CaseRecord, DesktopGrantedSource,
    DesktopVfsRegistry, EmuCaseProfile, EmuStepPayload, Library, LibraryScanner, ScanLimits,
    SourceGrant,
};
use astra_headless_protocol::{
    ArtifactManifest, ButtonState, GamepadControl, InputMessage, ObservationPredicate,
    PhysicalInput, PointerButton, TouchPhase,
};
use astra_media::{DecodedVideoStream, DECODED_VIDEO_STREAM_SCHEMA};
use astra_platform::{
    AudioOutputHandle, AudioOutputRequest, AudioPacket, DecodeKind, DecodeOutput,
    GamepadControl as PlatformGamepadControl, HeadlessArtifactRetention, HeadlessHostProfile,
    HostLaunchProfile, InputState, PlatformDecodeRequest, PlatformEventKind, PlatformHostClient,
    PlatformHostFactory, PointerButton as PlatformPointerButton, RgbaFrame, SurfaceHandle,
    SurfaceRequest, TouchPhase as PlatformTouchPhase, WindowHandle, WindowRequest,
};
use astra_platform_headless::HeadlessPlatformFactory;
use astra_plugin::ProductRuntimeProvider;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProviderInstanceId, RuntimeOpenRequest, RuntimeOutputDomain,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSectionCodec, RuntimeSectionPayload,
    RuntimeStepInput, RuntimeStepMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    family_host::CliFamilyHostConfig, input::read_input_sequence, rasterizer::CpuStageRasterizer,
};

pub const HEADLESS_RUN_REPORT_SCHEMA: &str = "astra.emu.headless_run_report.v1";
const FIXED_DELTA_NS: u64 = 16_666_667;

#[derive(Debug, Clone)]
pub struct HeadlessLaunch {
    pub game_dir: PathBuf,
    pub entry: Option<String>,
    pub input_path: PathBuf,
    pub artifact_root: PathBuf,
    pub family_manifest: Option<PathBuf>,
    pub family_library: Option<PathBuf>,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub video_provider: String,
    pub verify_snapshot: bool,
}

#[derive(Debug, Clone)]
pub struct NativeLaunch {
    pub game_dir: PathBuf,
    pub entry: Option<String>,
    pub family_manifest: Option<PathBuf>,
    pub family_library: Option<PathBuf>,
    pub enable_audio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessCheckpointEvidenceV1 {
    pub checkpoint_id: String,
    pub fixed_step: u64,
    pub frame_hash: Hash256,
    pub observation_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessRunReportV1 {
    pub schema: String,
    pub status: String,
    pub engine_id: String,
    pub runtime_provider_id: String,
    pub family_provider_id: String,
    pub host_kind: String,
    pub build_identity_hash: Hash256,
    pub profile_hash: Hash256,
    pub game_identity_hash: Hash256,
    pub entry_identity_hash: Hash256,
    pub session_id_hash: Hash256,
    pub input_sequence_hash: Hash256,
    pub consumed_input_trace_hash: Hash256,
    pub visual_trace_hash: Hash256,
    pub audio_meter_hash: Hash256,
    pub runtime_state_trace_hash: Hash256,
    pub artifact_manifest_hash: Hash256,
    pub fixed_steps: u64,
    pub presented_frames: u64,
    pub consumed_input_messages: u64,
    pub snapshot_round_trip_verified: bool,
    pub terminal_reached: bool,
    pub checkpoints: Vec<HeadlessCheckpointEvidenceV1>,
    pub lifecycle_steps: Vec<String>,
    pub diagnostic_codes: Vec<String>,
}

pub async fn run_native(launch: NativeLaunch) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        run_native_windows(launch).await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = launch;
        Err("PLATFORM_NOT_IMPLEMENTED:astra-emu-cli native host".into())
    }
}

#[cfg(target_os = "windows")]
async fn run_native_windows(launch: NativeLaunch) -> Result<(), String> {
    let game_root = fs::canonicalize(&launch.game_dir)
        .map_err(|_| "ASTRA_EMU_CLI_GAME_DIR_INVALID".to_owned())?;
    if !game_root.is_dir() {
        return Err("ASTRA_EMU_CLI_GAME_DIR_INVALID".into());
    }
    let case = scan_case(&game_root, launch.entry.as_deref())?;
    let game_identity_hash: Hash256 = case
        .content_hash
        .parse()
        .map_err(|_| "ASTRA_EMU_CASE_FINGERPRINT_INVALID".to_owned())?;
    let executable = std::env::current_exe().map_err(|_| "ASTRA_EMU_EXECUTABLE_PATH".to_owned())?;
    let vfs = Arc::new(DesktopVfsRegistry::default());
    let mount_set_id = format!("native-{}", &game_identity_hash.to_string()[7..39]);
    vfs.bind(&mount_set_id, &game_root.to_string_lossy())?;
    let family_config = match (&launch.family_manifest, &launch.family_library) {
        (Some(manifest), Some(library)) => {
            CliFamilyHostConfig::with_paths(manifest.clone(), library.clone())
        }
        (None, None) => CliFamilyHostConfig::installed_for_executable(&executable)?,
        _ => return Err("ASTRA_EMU_CLI_FAMILY_PATH_PAIR_REQUIRED".into()),
    };
    let family = family_config.create_provider(vfs.clone())?;
    let mut runtime = AstraEmuRuntimeProvider::new(family)?;
    runtime.create_instance(ProviderInstanceId("astra.emu.cli.native.instance".into()))?;
    let profile = probe_profile(
        &runtime,
        &case,
        &mount_set_id,
        game_identity_hash,
        "windows",
        "astra.platform.windows.media",
        "astra.emu.cli.native.report",
    )?;
    let stage_width = profile
        .family_options
        .get("fvp.stage_width")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_FVP_PROBE_STAGE_INVALID".to_owned())?;
    let stage_height = profile
        .family_options
        .get("fvp.stage_height")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_FVP_PROBE_STAGE_INVALID".to_owned())?;
    let section = case_profile_section(&case, &profile, &mount_set_id, game_identity_hash)?;
    let seed = u64::from_le_bytes(game_identity_hash.as_bytes()[..8].try_into().unwrap());
    let open = runtime.open(RuntimeOpenRequest {
        target_id: "astra-emu-native-case".into(),
        profile: "fvp-v1".into(),
        locale: "und".into(),
        seed,
        package_hash: case.content_hash.clone(),
        sections: vec![section],
    })?;

    let mut host_profile = astra_platform::PlatformHostProfile::windows_release(
        "astra-emu-cli",
        "dev.astraengine.astraemu-cli",
    );
    host_profile.id = "astra-emu-cli-native".into();
    host_profile.limits.max_frame_bytes = usize::try_from(stage_width)
        .ok()
        .and_then(|width| {
            usize::try_from(stage_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "ASTRA_EMU_NATIVE_FRAME_BOUNDS".to_owned())?;
    let mut host = astra_platform_windows::factory()
        .start(HostLaunchProfile::platform(host_profile))
        .await
        .map_err(|error| error.to_string())?;
    let window = host
        .client
        .create_window(WindowRequest {
            title: "AstraEMU FVP".into(),
            width: stage_width,
            height: stage_height,
            visible: true,
        })
        .await
        .map_err(|error| error.to_string())?;
    let surface = host
        .client
        .create_surface(SurfaceRequest {
            window,
            width: stage_width,
            height: stage_height,
        })
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!(
        event = "astra_emu_cli_native_session_opened",
        family = "fvp",
        stage_width,
        stage_height,
        audio_enabled = launch.enable_audio
    );

    let mut driver = RuntimeDriver::new(
        &mut runtime,
        open.session_id.clone(),
        seed,
        profile.fixed_delta_ns,
        &host.client,
        surface,
        launch.enable_audio,
    );
    let mut viewport = NativeViewport {
        window_width: stage_width,
        window_height: stage_height,
        stage_width,
        stage_height,
    };
    let mut suspended = false;
    let mut ticker = tokio::time::interval(std::time::Duration::from_nanos(profile.fixed_delta_ns));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let run_result = loop {
        tokio::select! {
            _ = ticker.tick(), if !suspended => {
                if let Err(error) = driver.step().await {
                    break Err(error);
                }
                if driver.terminal {
                    break Ok(());
                }
            }
            event = host.events.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(error) => break Err(error.to_string()),
                };
                match route_native_event(&mut driver, window, &mut viewport, event.kind) {
                    Ok(NativeEventAction::Continue) => {}
                    Ok(NativeEventAction::Suspend(value)) => suspended = value,
                    Ok(NativeEventAction::Close) => break Ok(()),
                    Err(error) => break Err(error),
                }
            }
        }
    };
    let fixed_step = driver.fixed_step;
    let audio_cleanup = std::mem::take(&mut driver.audio)
        .shutdown(&host.client)
        .await
        .map(|_| ());
    drop(driver);
    let runtime_cleanup = runtime.shutdown(open.session_id.clone()).map(|_| ());
    let surface_cleanup = host
        .client
        .destroy_surface(surface)
        .await
        .map_err(|error| error.to_string());
    let window_cleanup = host
        .client
        .destroy_window(window)
        .await
        .map_err(|error| error.to_string());
    let host_cleanup = host
        .client
        .shutdown()
        .await
        .map_err(|error| error.to_string());
    vfs.unbind(&mount_set_id);
    let cleanup_errors = [
        ("audio", audio_cleanup),
        ("runtime", runtime_cleanup),
        ("surface", surface_cleanup),
        ("window", window_cleanup),
        ("host", host_cleanup),
    ]
    .into_iter()
    .filter_map(|(stage, result)| result.err().map(|error| format!("{stage}={error}")))
    .collect::<Vec<_>>();
    match (run_result, cleanup_errors.is_empty()) {
        (Err(error), true) => return Err(error),
        (Ok(()), false) => {
            return Err(format!(
                "ASTRA_EMU_NATIVE_CLEANUP_FAILED:{}",
                cleanup_errors.join(";")
            ));
        }
        (Err(error), false) => {
            return Err(format!(
                "ASTRA_EMU_NATIVE_RUN_AND_CLEANUP_FAILED:{error};{}",
                cleanup_errors.join(";")
            ));
        }
        (Ok(()), true) => {}
    }
    tracing::info!(
        event = "astra_emu_cli_native_session_closed",
        fixed_step,
        family = "fvp"
    );
    Ok(())
}

pub async fn run_headless(launch: HeadlessLaunch) -> Result<HeadlessRunReportV1, String> {
    validate_launch(&launch)?;
    let input = read_input_sequence(&launch.input_path)?;
    let game_root = fs::canonicalize(&launch.game_dir)
        .map_err(|_| "ASTRA_EMU_HEADLESS_GAME_DIR_INVALID".to_owned())?;
    if !game_root.is_dir() {
        return Err("ASTRA_EMU_HEADLESS_GAME_DIR_INVALID".into());
    }
    let case = scan_case(&game_root, launch.entry.as_deref())?;
    let game_identity_hash: Hash256 = case
        .content_hash
        .parse()
        .map_err(|_| "ASTRA_EMU_CASE_FINGERPRINT_INVALID".to_owned())?;
    let executable = std::env::current_exe().map_err(|_| "ASTRA_EMU_EXECUTABLE_PATH".to_owned())?;
    let executable_bytes =
        fs::read(&executable).map_err(|_| "ASTRA_EMU_EXECUTABLE_READ".to_owned())?;
    let build_identity_hash = Hash256::from_sha256(&executable_bytes);
    let vfs = Arc::new(DesktopVfsRegistry::default());
    let mount_set_id = format!("headless-{}", &game_identity_hash.to_string()[7..39]);
    vfs.bind(&mount_set_id, &game_root.to_string_lossy())?;
    let family_config = match (&launch.family_manifest, &launch.family_library) {
        (Some(manifest), Some(library)) => {
            CliFamilyHostConfig::with_paths(manifest.clone(), library.clone())
        }
        (None, None) => CliFamilyHostConfig::installed_for_executable(&executable)?,
        _ => return Err("ASTRA_EMU_HEADLESS_FAMILY_PATH_PAIR".into()),
    };
    let family = family_config.create_provider(vfs.clone())?;
    let family_provider_id = family.descriptor().provider_id.clone();
    let mut runtime = AstraEmuRuntimeProvider::new(family)?;
    runtime.create_instance(ProviderInstanceId("astra.emu.cli.headless.instance".into()))?;
    let profile = probe_profile(
        &runtime,
        &case,
        &mount_set_id,
        game_identity_hash,
        "headless-test",
        "astra.platform.headless.media",
        "astra.emu.cli.headless.report",
    )?;
    let stage_width = profile
        .family_options
        .get("fvp.stage_width")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_FVP_PROBE_STAGE_INVALID".to_owned())?;
    let stage_height = profile
        .family_options
        .get("fvp.stage_height")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_FVP_PROBE_STAGE_INVALID".to_owned())?;
    let section = case_profile_section(&case, &profile, &mount_set_id, game_identity_hash)?;
    let seed = u64::from_le_bytes(game_identity_hash.as_bytes()[..8].try_into().unwrap());
    let open = runtime.open(RuntimeOpenRequest {
        target_id: "astra-emu-headless-case".into(),
        profile: "fvp-v1".into(),
        locale: "und".into(),
        seed,
        package_hash: case.content_hash.clone(),
        sections: vec![section],
    })?;
    let session_id_hash = Hash256::from_sha256(open.session_id.0.as_bytes());
    let mut host_profile = HeadlessHostProfile::reference(
        "headless-test",
        "astra.emu.quick_case",
        build_identity_hash.to_string(),
        game_identity_hash.to_string(),
    );
    host_profile.id = "astra-emu-cli-headless".into();
    host_profile.product_profile = "fvp-v1".into();
    host_profile.viewport_width = launch.viewport_width;
    host_profile.viewport_height = launch.viewport_height;
    host_profile.tick_duration_ns = profile.fixed_delta_ns;
    host_profile.providers.product_adapter = "astra.emu".into();
    host_profile.providers.video_decode = launch.video_provider.clone();
    host_profile.artifacts.namespace = input.session.clone();
    host_profile.artifacts.retention = HeadlessArtifactRetention::All;
    host_profile.artifacts.max_frames = input.final_tick.saturating_add(100).max(1);
    host_profile.artifacts.max_duration_ns = input
        .final_tick
        .saturating_add(100)
        .saturating_mul(profile.fixed_delta_ns);
    host_profile.input.max_messages = input.messages.len() as u64;
    host_profile.input.max_tick = input.final_tick;
    let profile_hash: Hash256 = host_profile
        .hash()
        .map_err(|error| error.to_string())?
        .parse()
        .map_err(|_| "ASTRA_EMU_HEADLESS_PROFILE_HASH".to_owned())?;
    let host = HeadlessPlatformFactory::new(&launch.artifact_root, &game_root)
        .with_input_sequence_hash(input.hash.to_string())
        .start(host_profile.into())
        .await
        .map_err(|error| error.to_string())?;
    let window = host
        .client
        .create_window(WindowRequest {
            title: "AstraEMU Headless".into(),
            width: stage_width,
            height: stage_height,
            visible: false,
        })
        .await
        .map_err(|error| error.to_string())?;
    let surface = host
        .client
        .create_surface(SurfaceRequest {
            window,
            width: stage_width,
            height: stage_height,
        })
        .await
        .map_err(|error| error.to_string())?;
    let result = execute_sequence(
        &mut runtime,
        open.session_id.clone(),
        &host.client,
        surface,
        &input.messages,
        ExecutionConfig {
            seed,
            delta_ns: profile.fixed_delta_ns,
            verify_snapshot: launch.verify_snapshot,
        },
    )
    .await;
    let cleanup = async {
        host.client
            .destroy_surface(surface)
            .await
            .map_err(|error| error.to_string())?;
        host.client
            .destroy_window(window)
            .await
            .map_err(|error| error.to_string())?;
        runtime.shutdown(open.session_id.clone())?;
        host.client
            .shutdown()
            .await
            .map_err(|error| error.to_string())
    }
    .await;
    vfs.unbind(&mount_set_id);
    let execution = match (result, cleanup) {
        (Ok(execution), Ok(())) => execution,
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(cleanup)) => return Err(cleanup),
        (Err(error), Err(cleanup)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_RUN_AND_CLEANUP_FAILED:{error};{cleanup}"
            ))
        }
    };
    let manifest_path = launch.artifact_root.join("artifact-manifest.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|_| "ASTRA_EMU_HEADLESS_ARTIFACT_MANIFEST_READ".to_owned())?;
    let manifest: ArtifactManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "ASTRA_EMU_HEADLESS_ARTIFACT_MANIFEST_PARSE".to_owned())?;
    manifest
        .validate()
        .map_err(|_| "ASTRA_EMU_HEADLESS_ARTIFACT_MANIFEST_INVALID".to_owned())?;
    if manifest.package_hash != game_identity_hash.to_string()
        || manifest.input_sequence_hash != input.hash.to_string()
    {
        return Err("ASTRA_EMU_HEADLESS_ARTIFACT_IDENTITY".into());
    }
    let report = HeadlessRunReportV1 {
        schema: HEADLESS_RUN_REPORT_SCHEMA.into(),
        status: "passed".into(),
        engine_id: "fvp".into(),
        runtime_provider_id: "astra.emu.runtime_provider".into(),
        family_provider_id,
        host_kind: "headless".into(),
        build_identity_hash,
        profile_hash,
        game_identity_hash,
        entry_identity_hash: Hash256::from_sha256(case.relative_path.as_bytes()),
        session_id_hash,
        input_sequence_hash: input.hash,
        consumed_input_trace_hash: Hash256::from_sha256(&execution.input_trace),
        visual_trace_hash: Hash256::from_sha256(&execution.visual_trace),
        audio_meter_hash: Hash256::from_sha256(&execution.audio_trace),
        runtime_state_trace_hash: Hash256::from_sha256(&execution.state_trace),
        artifact_manifest_hash: Hash256::from_sha256(&manifest_bytes),
        fixed_steps: execution.fixed_step,
        presented_frames: execution.present_sequence,
        consumed_input_messages: input.messages.len() as u64,
        snapshot_round_trip_verified: execution.snapshot_verified,
        terminal_reached: execution.terminal,
        checkpoints: execution.checkpoints,
        lifecycle_steps: {
            let mut steps = vec![
                "provider.create".into(),
                "family.probe".into(),
                "session.open".into(),
                "session.step".into(),
            ];
            if execution.snapshot_verified {
                steps.push("session.save_restore".into());
            }
            steps.extend(["session.shutdown".into(), "host.shutdown".into()]);
            steps
        },
        diagnostic_codes: execution.diagnostics.into_iter().collect(),
    };
    let report_path = launch.artifact_root.join("astra-emu-headless-run.json");
    write_atomic_json(&report_path, &report)?;
    Ok(report)
}

fn validate_launch(launch: &HeadlessLaunch) -> Result<(), String> {
    if !(320..=8192).contains(&launch.viewport_width)
        || !(240..=8192).contains(&launch.viewport_height)
        || !matches!(launch.video_provider.as_str(), "disabled" | "ffmpeg-vcpkg")
    {
        return Err("ASTRA_EMU_HEADLESS_PROFILE_INVALID".into());
    }
    if launch.artifact_root.exists() {
        return Err("ASTRA_EMU_HEADLESS_ARTIFACT_ROOT_EXISTS".into());
    }
    Ok(())
}

fn scan_case(root: &Path, entry: Option<&str>) -> Result<CaseRecord, String> {
    let source =
        Arc::new(DesktopGrantedSource::new(&root.to_string_lossy()).map_err(|e| e.to_string())?);
    let mut library = Library::in_memory().map_err(|error| error.to_string())?;
    let source_id = "headless-source";
    library
        .upsert_grant(&SourceGrant {
            source_id: source_id.into(),
            alias: "Headless source".into(),
            platform_token: root.to_string_lossy().into_owned(),
            token_kind: "desktop-directory-v1".into(),
            active: true,
        })
        .map_err(|error| error.to_string())?;
    LibraryScanner::new(ScanLimits::default())
        .map_err(|error| error.to_string())?
        .scan(
            &mut library,
            source_id,
            source,
            &CancellationToken::default(),
        )
        .map_err(|error| error.to_string())?;
    let normalized_entry = entry.map(|entry| entry.replace('\\', "/"));
    if normalized_entry.as_ref().is_some_and(|entry| {
        entry.is_empty()
            || entry.starts_with('/')
            || entry
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
    }) {
        return Err("ASTRA_EMU_HEADLESS_ENTRY_INVALID".into());
    }
    let mut cases = library
        .list_cases()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|case| {
            normalized_entry
                .as_ref()
                .is_none_or(|entry| case.relative_path.replace('\\', "/") == *entry)
        })
        .collect::<Vec<_>>();
    if cases.is_empty() {
        return Err("ASTRA_EMU_HEADLESS_CASE_NOT_FOUND".into());
    }
    if cases.len() != 1 {
        return Err("ASTRA_EMU_HEADLESS_CASE_AMBIGUOUS".into());
    }
    Ok(cases.remove(0))
}

fn probe_profile(
    runtime: &AstraEmuRuntimeProvider,
    case: &CaseRecord,
    mount_set_id: &str,
    package_hash: Hash256,
    target: &str,
    media_service_id: &str,
    report_sink_id: &str,
) -> Result<astra_emu_manager_core::CaseRuntimeProfileRecord, String> {
    let report = runtime.probe_family(
        &LegacyRuntimeHostCtx {
            case_id: case.case_identity.clone(),
            package_id: "astra-emu-headless-case".into(),
            package_hash,
            mount_set_id: mount_set_id.into(),
            media_service_ids: vec![media_service_id.into()],
            permission_policy_id: "astra.emu.cli.explicit_directory.v1".into(),
            report_sink_id: report_sink_id.into(),
            target: target.into(),
            profile: "fvp-v1".into(),
        },
        LegacyProbeRequest {
            root_mount_id: mount_set_id.into(),
            candidate_uris: vec![case.relative_path.clone()],
            marker_hashes: vec![package_hash],
            max_entries: 1,
            max_metadata_bytes: 512 * 1024 * 1024,
        },
    )?;
    if report.family_id.0 != "fvp"
        || report.confidence_permyriad != 10_000
        || !report.blockers.is_empty()
    {
        return Err("ASTRA_EMU_FVP_PROBE_BLOCKED".into());
    }
    let marker = |prefix: &str| -> Result<String, String> {
        let values = report
            .markers
            .iter()
            .filter_map(|value| value.strip_prefix(prefix))
            .collect::<Vec<_>>();
        if values.len() != 1 {
            return Err("ASTRA_EMU_FVP_PROBE_MARKER_AMBIGUOUS".into());
        }
        Ok(values[0].to_owned())
    };
    let nls = marker("fvp.nls.")?;
    if !matches!(nls.as_str(), "shift_jis" | "gbk" | "utf8") {
        return Err("ASTRA_EMU_FVP_PROBE_NLS_AMBIGUOUS".into());
    }
    let width = marker("fvp.stage_width.")?;
    let height = marker("fvp.stage_height.")?;
    width
        .parse::<u32>()
        .map_err(|_| "ASTRA_EMU_FVP_PROBE_STAGE_INVALID")?;
    height
        .parse::<u32>()
        .map_err(|_| "ASTRA_EMU_FVP_PROBE_STAGE_INVALID")?;
    Ok(astra_emu_manager_core::CaseRuntimeProfileRecord {
        case_identity: case.case_identity.clone(),
        family_id: "fvp".into(),
        fixed_delta_ns: FIXED_DELTA_NS,
        compatibility_profile: "rfvp-v1".into(),
        family_options: [
            ("fvp.nls".into(), nls),
            ("fvp.stage_width".into(), width),
            ("fvp.stage_height".into(), height),
            ("patch.mode".into(), "no_patch".into()),
        ]
        .into_iter()
        .collect(),
    })
}

fn case_profile_section(
    case: &CaseRecord,
    profile: &astra_emu_manager_core::CaseRuntimeProfileRecord,
    mount_set_id: &str,
    case_fingerprint: Hash256,
) -> Result<RuntimeSectionPayload, String> {
    let value = EmuCaseProfile {
        schema: "astra.emu.case_profile.v1".into(),
        family_id: "fvp".into(),
        case_fingerprint,
        script_uri: case.relative_path.clone(),
        fixed_delta_ns: profile.fixed_delta_ns,
        compatibility_profile: profile.compatibility_profile.clone(),
        mount_set_id: mount_set_id.into(),
        permission_policy_id: "astra.emu.cli.explicit_directory.v1".into(),
        family_options: profile.family_options.clone(),
    };
    let bytes = postcard::to_allocvec(&value).map_err(|error| error.to_string())?;
    Ok(RuntimeSectionPayload {
        section_id: "emu.case_profile".into(),
        schema: "astra.emu.case_profile.v1".into(),
        version: SchemaVersion::new(1, 0, 0),
        codec: RuntimeSectionCodec::Postcard,
        hash: Hash256::from_sha256(&bytes),
        bytes,
    })
}

struct ExecutionEvidence {
    input_trace: Vec<u8>,
    visual_trace: Vec<u8>,
    audio_trace: Vec<u8>,
    state_trace: Vec<u8>,
    checkpoints: Vec<HeadlessCheckpointEvidenceV1>,
    diagnostics: BTreeSet<String>,
    fixed_step: u64,
    present_sequence: u64,
    snapshot_verified: bool,
    terminal: bool,
}

#[derive(Clone)]
enum PendingWait {
    DueStep(u64),
    Input(u64),
    Presentation,
    Media(String),
    Unsupported,
}

struct ActiveVideo {
    playback_id: String,
    stage_width: u32,
    stage_height: u32,
    started_step: u64,
    stream: DecodedVideoStream,
}

struct RuntimeDriver<'a> {
    runtime: &'a mut AstraEmuRuntimeProvider,
    session_id: GameRuntimeSessionId,
    seed: u64,
    delta_ns: u64,
    platform: &'a PlatformHostClient,
    surface: SurfaceHandle,
    fixed_step: u64,
    next_step_mode: RuntimeStepMode,
    input_sequence: u64,
    await_sequence: u64,
    pending_inputs: Vec<LegacyInputEdge>,
    pending_waits: BTreeMap<String, PendingWait>,
    rasterizer: CpuStageRasterizer,
    base_frame: Option<(u32, u32, Vec<u8>)>,
    latest_frame: Option<(u32, u32, Vec<u8>)>,
    present_sequence: u64,
    state_hash: Hash256,
    terminal: bool,
    audio: HeadlessAudioExecutor,
    video: Option<ActiveVideo>,
    completed_media: Vec<String>,
    input_trace: Vec<u8>,
    visual_trace: Vec<u8>,
    state_trace: Vec<u8>,
    diagnostics: BTreeSet<String>,
    active_touch: Option<u64>,
    audio_enabled: bool,
}

struct ExecutionConfig {
    seed: u64,
    delta_ns: u64,
    verify_snapshot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeEventAction {
    Continue,
    Suspend(bool),
    Close,
}

#[derive(Debug, Clone, Copy)]
struct NativeViewport {
    window_width: u32,
    window_height: u32,
    stage_width: u32,
    stage_height: u32,
}

fn route_native_event(
    driver: &mut RuntimeDriver<'_>,
    window: WindowHandle,
    viewport: &mut NativeViewport,
    event: PlatformEventKind,
) -> Result<NativeEventAction, String> {
    match event {
        PlatformEventKind::Resumed => Ok(NativeEventAction::Suspend(false)),
        PlatformEventKind::Suspended => Ok(NativeEventAction::Suspend(true)),
        PlatformEventKind::WindowClosed {
            window: event_window,
        } if event_window == window => Ok(NativeEventAction::Close),
        PlatformEventKind::WindowResized {
            window: event_window,
            width,
            height,
            ..
        } if event_window == window => {
            if width == 0 || height == 0 {
                return Ok(NativeEventAction::Suspend(true));
            }
            viewport.window_width = width;
            viewport.window_height = height;
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::WindowFocused { .. } => Ok(NativeEventAction::Continue),
        PlatformEventKind::Keyboard {
            window: event_window,
            physical_key,
            logical_key,
            state,
            repeat,
        } if event_window == window => {
            if repeat && state == InputState::Released {
                return Err("ASTRA_EMU_NATIVE_KEY_REPEAT_INVALID".into());
            }
            if let Some(control) = native_key_control(logical_key.as_deref(), &physical_key) {
                let pressed = state == InputState::Pressed;
                driver.queue_input(control, pressed, if pressed { 1.0 } else { 0.0 })?;
            }
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::PointerMoved {
            window: event_window,
            x,
            y,
        } if event_window == window => {
            if let Some([stage_x, stage_y]) = viewport.map_pointer(x, y) {
                driver.queue_input("pointer.x", false, stage_x)?;
                driver.queue_input("pointer.y", false, stage_y)?;
            }
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::PointerButton {
            window: event_window,
            button,
            state,
        } if event_window == window => {
            let control = match button {
                PlatformPointerButton::Primary => Some("pointer.primary"),
                PlatformPointerButton::Secondary => Some("pointer.secondary"),
                _ => None,
            };
            if let Some(control) = control {
                let pressed = state == InputState::Pressed;
                driver.queue_input(control, pressed, if pressed { 1.0 } else { 0.0 })?;
            }
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::MouseWheel {
            window: event_window,
            delta_y,
            ..
        } if event_window == window => {
            driver.queue_input("wheel", false, delta_y)?;
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::Touch {
            window: event_window,
            id,
            x,
            y,
            phase,
        } if event_window == window => {
            let Some([stage_x, stage_y]) = viewport.map_pointer(x, y) else {
                return Ok(NativeEventAction::Continue);
            };
            match phase {
                PlatformTouchPhase::Started => {
                    if driver.active_touch.replace(id).is_some() {
                        return Err("ASTRA_EMU_NATIVE_MULTI_TOUCH_UNSUPPORTED".into());
                    }
                    driver.queue_input("pointer.x", false, stage_x)?;
                    driver.queue_input("pointer.y", false, stage_y)?;
                    driver.queue_input("pointer.primary", true, 1.0)?;
                }
                PlatformTouchPhase::Moved if driver.active_touch == Some(id) => {
                    driver.queue_input("pointer.x", false, stage_x)?;
                    driver.queue_input("pointer.y", false, stage_y)?;
                }
                PlatformTouchPhase::Ended | PlatformTouchPhase::Cancelled
                    if driver.active_touch == Some(id) =>
                {
                    driver.active_touch = None;
                    driver.queue_input("pointer.primary", false, 0.0)?;
                }
                _ => return Err("ASTRA_EMU_NATIVE_TOUCH_SEQUENCE".into()),
            }
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::GamepadInput { control, value, .. } => {
            let mapped = match control {
                PlatformGamepadControl::South => Some("confirm"),
                PlatformGamepadControl::East => Some("cancel"),
                PlatformGamepadControl::DpadUp => Some("up"),
                PlatformGamepadControl::DpadDown => Some("down"),
                PlatformGamepadControl::DpadLeft => Some("left"),
                PlatformGamepadControl::DpadRight => Some("right"),
                _ => None,
            };
            if let Some(control) = mapped {
                driver.queue_input(control, value != 0.0, value)?;
            }
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::GamepadConnected { .. }
        | PlatformEventKind::GamepadDisconnected { .. }
        | PlatformEventKind::DeviceRestored { .. }
        | PlatformEventKind::ContextRestored { .. } => Ok(NativeEventAction::Continue),
        PlatformEventKind::DeviceLost { provider }
        | PlatformEventKind::ContextLost { provider } => {
            Err(format!("ASTRA_EMU_NATIVE_DEVICE_LOST:{provider}"))
        }
        PlatformEventKind::ImePreedit { .. } | PlatformEventKind::ImeCommit { .. } => {
            Err("ASTRA_EMU_NATIVE_IME_UNSUPPORTED".into())
        }
        _ => Ok(NativeEventAction::Continue),
    }
}

fn native_key_control(logical_key: Option<&str>, physical_key: &str) -> Option<&'static str> {
    let key = logical_key.unwrap_or(physical_key).to_ascii_lowercase();
    match key.as_str() {
        "enter" | "return" | "numpadenter" => Some("confirm"),
        "escape" | "esc" => Some("cancel"),
        "arrowup" | "up" => Some("up"),
        "arrowdown" | "down" => Some("down"),
        "arrowleft" | "left" => Some("left"),
        "arrowright" | "right" => Some("right"),
        " " | "space" | "spacebar" => Some("space"),
        _ => None,
    }
}

impl NativeViewport {
    fn map_pointer(&self, x: f64, y: f64) -> Option<[f32; 2]> {
        if self.window_width == 0
            || self.window_height == 0
            || self.stage_width == 0
            || self.stage_height == 0
            || !x.is_finite()
            || !y.is_finite()
        {
            return None;
        }
        let scale = (f64::from(self.window_width) / f64::from(self.stage_width))
            .min(f64::from(self.window_height) / f64::from(self.stage_height));
        let display_width = f64::from(self.stage_width) * scale;
        let display_height = f64::from(self.stage_height) * scale;
        let left = (f64::from(self.window_width) - display_width) * 0.5;
        let top = (f64::from(self.window_height) - display_height) * 0.5;
        if x < left || y < top || x >= left + display_width || y >= top + display_height {
            return None;
        }
        Some([((x - left) / scale) as f32, ((y - top) / scale) as f32])
    }
}

async fn execute_sequence(
    runtime: &mut AstraEmuRuntimeProvider,
    session_id: GameRuntimeSessionId,
    platform: &PlatformHostClient,
    surface: SurfaceHandle,
    messages: &[InputMessage],
    config: ExecutionConfig,
) -> Result<ExecutionEvidence, String> {
    let mut driver = RuntimeDriver::new(
        runtime,
        session_id,
        config.seed,
        config.delta_ns,
        platform,
        surface,
        true,
    );
    let mut checkpoints = Vec::new();
    let mut snapshot_verified = false;
    for message in messages {
        while driver.fixed_step < message.tick && !driver.terminal {
            driver.step().await?;
        }
        match &message.event {
            PhysicalInput::Shutdown => break,
            PhysicalInput::AdvanceTicks { count } => {
                for _ in 0..*count {
                    if driver.terminal {
                        break;
                    }
                    driver.step().await?;
                }
            }
            PhysicalInput::Checkpoint { id } => {
                let (width, height, rgba8) = driver
                    .latest_frame
                    .as_ref()
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_CHECKPOINT_FRAME_MISSING".to_owned())?;
                let captured = platform
                    .capture_surface(surface)
                    .await
                    .map_err(|error| error.to_string())?;
                if captured.width != *width
                    || captured.height != *height
                    || captured.rgba8 != *rgba8
                {
                    return Err("ASTRA_EMU_HEADLESS_CHECKPOINT_CAPTURE_MISMATCH".into());
                }
                if config.verify_snapshot && !snapshot_verified {
                    let saved = driver.runtime.save(RuntimeSaveRequest {
                        session_id: driver.session_id.clone(),
                        slot: "automation-round-trip".into(),
                    })?;
                    let restored = driver.runtime.restore(RuntimeRestoreRequest {
                        session_id: driver.session_id.clone(),
                        sections: saved.sections,
                    })?;
                    if restored.restored_fixed_step != driver.fixed_step
                        || restored.session_seed != driver.seed
                    {
                        return Err("ASTRA_EMU_HEADLESS_SNAPSHOT_IDENTITY".into());
                    }
                    driver.audio.reset_for_restore(driver.platform).await?;
                    driver.video = None;
                    driver.completed_media.clear();
                    driver.next_step_mode = RuntimeStepMode::RestoreContinuation;
                    snapshot_verified = true;
                }
                checkpoints.push(HeadlessCheckpointEvidenceV1 {
                    checkpoint_id: id.clone(),
                    fixed_step: driver.fixed_step,
                    frame_hash: Hash256::from_sha256(rgba8),
                    observation_hash: driver.observation_hash()?,
                });
            }
            PhysicalInput::Await {
                observation,
                timeout_ticks,
            } => {
                let mut matched = driver.observation_matches(observation);
                for _ in 0..*timeout_ticks {
                    if matched || driver.terminal {
                        break;
                    }
                    driver.step().await?;
                    matched = driver.observation_matches(observation);
                }
                if !matched {
                    return Err("ASTRA_EMU_HEADLESS_AWAIT_TIMEOUT".into());
                }
            }
            input => driver.consume_physical_input(input)?,
        }
        driver.input_trace.extend_from_slice(
            &serde_json::to_vec(message)
                .map_err(|_| "ASTRA_EMU_HEADLESS_INPUT_TRACE".to_owned())?,
        );
        driver.input_trace.push(b'\n');
    }
    if config.verify_snapshot && !snapshot_verified {
        return Err("ASTRA_EMU_HEADLESS_SNAPSHOT_CHECKPOINT_REQUIRED".into());
    }
    let audio_trace = driver.audio.shutdown(platform).await?;
    Ok(ExecutionEvidence {
        input_trace: driver.input_trace,
        visual_trace: driver.visual_trace,
        audio_trace,
        state_trace: driver.state_trace,
        checkpoints,
        diagnostics: driver.diagnostics,
        fixed_step: driver.fixed_step,
        present_sequence: driver.present_sequence,
        snapshot_verified,
        terminal: driver.terminal,
    })
}

impl<'a> RuntimeDriver<'a> {
    fn new(
        runtime: &'a mut AstraEmuRuntimeProvider,
        session_id: GameRuntimeSessionId,
        seed: u64,
        delta_ns: u64,
        platform: &'a PlatformHostClient,
        surface: SurfaceHandle,
        audio_enabled: bool,
    ) -> RuntimeDriver<'a> {
        RuntimeDriver {
            runtime,
            session_id,
            seed,
            delta_ns,
            platform,
            surface,
            fixed_step: 0,
            next_step_mode: RuntimeStepMode::Live,
            input_sequence: 0,
            await_sequence: 0,
            pending_inputs: Vec::new(),
            pending_waits: BTreeMap::new(),
            rasterizer: CpuStageRasterizer::default(),
            base_frame: None,
            latest_frame: None,
            present_sequence: 0,
            state_hash: Hash256::from_sha256(&[]),
            terminal: false,
            audio: HeadlessAudioExecutor::default(),
            video: None,
            completed_media: Vec::new(),
            input_trace: Vec::new(),
            visual_trace: Vec::new(),
            state_trace: Vec::new(),
            diagnostics: BTreeSet::new(),
            active_touch: None,
            audio_enabled,
        }
    }

    fn queue_input(&mut self, control: &str, pressed: bool, value: f32) -> Result<(), String> {
        if self.pending_inputs.len() >= 4096 || !value.is_finite() {
            return Err("ASTRA_EMU_HEADLESS_INPUT_QUEUE_BOUNDS".into());
        }
        self.input_sequence = self
            .input_sequence
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_INPUT_SEQUENCE_OVERFLOW".to_owned())?;
        self.pending_inputs.push(LegacyInputEdge {
            control: control.into(),
            pressed,
            value,
            sequence: self.input_sequence,
        });
        Ok(())
    }

    fn consume_physical_input(&mut self, input: &PhysicalInput) -> Result<(), String> {
        match input {
            PhysicalInput::Resume
            | PhysicalInput::Focus { .. }
            | PhysicalInput::GamepadConnection { .. } => Ok(()),
            PhysicalInput::Keyboard {
                physical_key,
                logical_key,
                state,
                repeat,
            } => {
                if *repeat && *state == ButtonState::Released {
                    return Err("ASTRA_EMU_HEADLESS_KEY_REPEAT_INVALID".into());
                }
                let key = logical_key
                    .as_deref()
                    .unwrap_or(physical_key)
                    .to_ascii_lowercase();
                let control = match key.as_str() {
                    "enter" | "return" | "numpadenter" => "confirm",
                    "escape" | "esc" => "cancel",
                    "arrowup" | "up" => "up",
                    "arrowdown" | "down" => "down",
                    "arrowleft" | "left" => "left",
                    "arrowright" | "right" => "right",
                    " " | "space" | "spacebar" => "space",
                    _ => return Err("ASTRA_EMU_HEADLESS_KEY_UNSUPPORTED".into()),
                };
                self.queue_input(
                    control,
                    *state == ButtonState::Pressed,
                    if *state == ButtonState::Pressed {
                        1.0
                    } else {
                        0.0
                    },
                )
            }
            PhysicalInput::PointerMove { x, y } => {
                self.queue_input("pointer.x", false, f32::from(*x))?;
                self.queue_input("pointer.y", false, f32::from(*y))
            }
            PhysicalInput::PointerButton { button, state } => {
                let control = match button {
                    PointerButton::Primary => "pointer.primary",
                    PointerButton::Secondary => "pointer.secondary",
                    _ => return Err("ASTRA_EMU_HEADLESS_POINTER_BUTTON_UNSUPPORTED".into()),
                };
                self.queue_input(
                    control,
                    *state == ButtonState::Pressed,
                    if *state == ButtonState::Pressed {
                        1.0
                    } else {
                        0.0
                    },
                )
            }
            PhysicalInput::Wheel { delta_y, .. } => {
                self.queue_input("wheel", false, *delta_y as f32)
            }
            PhysicalInput::Touch { id, x, y, phase } => match phase {
                TouchPhase::Started => {
                    if self.active_touch.replace(*id).is_some() {
                        return Err("ASTRA_EMU_HEADLESS_MULTI_TOUCH_UNSUPPORTED".into());
                    }
                    self.queue_input("pointer.x", false, f32::from(*x))?;
                    self.queue_input("pointer.y", false, f32::from(*y))?;
                    self.queue_input("pointer.primary", true, 1.0)
                }
                TouchPhase::Moved if self.active_touch == Some(*id) => {
                    self.queue_input("pointer.x", false, f32::from(*x))?;
                    self.queue_input("pointer.y", false, f32::from(*y))
                }
                TouchPhase::Ended | TouchPhase::Cancelled if self.active_touch == Some(*id) => {
                    self.active_touch = None;
                    self.queue_input("pointer.primary", false, 0.0)
                }
                _ => Err("ASTRA_EMU_HEADLESS_TOUCH_SEQUENCE".into()),
            },
            PhysicalInput::GamepadInput { control, value, .. } => {
                let mapped = match control {
                    GamepadControl::South => "confirm",
                    GamepadControl::East => "cancel",
                    GamepadControl::DpadUp => "up",
                    GamepadControl::DpadDown => "down",
                    GamepadControl::DpadLeft => "left",
                    GamepadControl::DpadRight => "right",
                    _ => return Err("ASTRA_EMU_HEADLESS_GAMEPAD_CONTROL_UNSUPPORTED".into()),
                };
                self.queue_input(mapped, *value != 0, f32::from(*value) / f32::from(i16::MAX))
            }
            PhysicalInput::ImePreedit { .. } | PhysicalInput::ImeCommit { .. } => {
                Err("ASTRA_EMU_HEADLESS_IME_UNSUPPORTED".into())
            }
            PhysicalInput::AdvanceTicks { .. }
            | PhysicalInput::Await { .. }
            | PhysicalInput::Checkpoint { .. }
            | PhysicalInput::Shutdown => Err("ASTRA_EMU_HEADLESS_INPUT_ROUTING".into()),
        }
    }

    async fn step(&mut self) -> Result<(), String> {
        let next_step = self
            .fixed_step
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TICK_OVERFLOW".to_owned())?;
        for media_id in self.completed_media.drain(..) {
            let mut matched = false;
            for wait in self.pending_waits.values_mut() {
                if matches!(wait, PendingWait::Media(expected) if *expected == media_id) {
                    *wait = PendingWait::DueStep(next_step);
                    matched = true;
                }
            }
            if !matched {
                return Err("ASTRA_EMU_HEADLESS_VIDEO_COMPLETION_UNSOLICITED".into());
            }
        }
        if self
            .pending_waits
            .values()
            .any(|wait| matches!(wait, PendingWait::Unsupported))
        {
            return Err("ASTRA_EMU_HEADLESS_WAIT_UNSUPPORTED".into());
        }
        let input_mask = self
            .pending_inputs
            .iter()
            .fold(0_u64, |mask, edge| mask | input_control_mask(&edge.control));
        let ready = self
            .pending_waits
            .iter()
            .filter(|(_, wait)| match wait {
                PendingWait::DueStep(due) => *due <= next_step,
                PendingWait::Input(mask) => input_mask & *mask != 0,
                _ => false,
            })
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        let mut await_results = Vec::new();
        for token_id in ready {
            self.pending_waits.remove(&token_id);
            self.await_sequence = self
                .await_sequence
                .checked_add(1)
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_AWAIT_SEQUENCE_OVERFLOW".to_owned())?;
            await_results.push(LegacyAwaitResult {
                token_id,
                status: "completed".into(),
                payload_hash: Hash256::from_sha256(&[]),
                sequence: self.await_sequence,
            });
        }
        let output = self.runtime.step(RuntimeStepInput {
            session_id: self.session_id.clone(),
            fixed_step: next_step,
            delta_ns: self.delta_ns,
            session_seed: self.seed,
            mode: self.next_step_mode,
            action: "emu.step".into(),
            payload: serde_json::to_value(EmuStepPayload {
                input_edges: std::mem::take(&mut self.pending_inputs),
                await_results,
                provider_results: Vec::new(),
                budget: LegacyStepBudget {
                    max_instructions: 100_000,
                    max_effects: 4096,
                    max_trace_entries: 65_536,
                },
            })
            .map_err(|_| "ASTRA_EMU_HEADLESS_STEP_PAYLOAD".to_owned())?,
        })?;
        self.next_step_mode = RuntimeStepMode::Live;
        self.fixed_step = next_step;
        let mut rendered = false;
        for envelope in &output.outputs {
            if envelope.domain != RuntimeOutputDomain::Effect
                || envelope.schema != "astra.emu.legacy_step_output.v1"
            {
                continue;
            }
            let family = envelope
                .decode_postcard::<astra_emu_family_api::LegacyStepOutput>(
                    RuntimeOutputDomain::Effect,
                    "astra.emu.legacy_step_output.v1",
                    SchemaVersion::new(1, 0, 0),
                )
                .map_err(|error| error.to_string())?;
            self.state_hash = family.state_hash;
            self.state_trace
                .extend_from_slice(self.state_hash.to_string().as_bytes());
            self.state_trace.push(b'\n');
            for diagnostic in family.diagnostics {
                self.diagnostics.insert(diagnostic.code);
            }
            for effect in family.effects {
                match effect {
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } if command == "astra.emu.render_frame.v1" => {
                        let frame: LegacyRenderFrameV1 = postcard::from_bytes(&payload)
                            .map_err(|_| "ASTRA_EMU_HEADLESS_RENDER_FRAME_DECODE".to_owned())?;
                        let width = frame.width;
                        let height = frame.height;
                        let rgba8 = self.rasterizer.render(frame)?;
                        self.base_frame = Some((width, height, rgba8));
                        rendered = true;
                    }
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } if command == "astra.emu.video_command.v1" => {
                        let command: LegacyVideoCommandV1 = postcard::from_bytes(&payload)
                            .map_err(|_| "ASTRA_EMU_HEADLESS_VIDEO_COMMAND_DECODE".to_owned())?;
                        self.execute_video(command).await?;
                    }
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } => {
                        // FVP publishes observable Graph/Prim/Motion syscall effects in
                        // addition to the host-neutral final render packet. RuntimeWorld
                        // already applies their ordered deterministic intent; the host
                        // retains only a redacted identity trace and renders the explicit
                        // astra.emu.render_frame.v1 packet.
                        self.state_trace.extend_from_slice(
                            Hash256::from_sha256(
                                &[command.as_bytes(), payload.as_slice()].concat(),
                            )
                            .to_string()
                            .as_bytes(),
                        );
                        self.state_trace.push(b'\n');
                    }
                    LegacyEffect::Audio {
                        command, payload, ..
                    } if command == "astra.emu.audio_command.v1" => {
                        let command: LegacyAudioCommandV1 = postcard::from_bytes(&payload)
                            .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_COMMAND_DECODE".to_owned())?;
                        if !self.audio_enabled {
                            command.validate().map_err(|error| error.to_string())?;
                            self.state_trace.extend_from_slice(
                                Hash256::from_sha256(&payload).to_string().as_bytes(),
                            );
                            self.state_trace.push(b'\n');
                            continue;
                        }
                        let resource = match &command {
                            LegacyAudioCommandV1::LoadResource { resource_uri, .. } => {
                                Some(self.runtime.read_session_resource(
                                    &self.session_id,
                                    resource_uri,
                                    512 * 1024 * 1024,
                                )?)
                            }
                            _ => None,
                        };
                        self.audio.execute(command, resource, self.platform).await?;
                    }
                    LegacyEffect::Audio { .. } => {
                        return Err("ASTRA_EMU_HEADLESS_AUDIO_UNSUPPORTED".into())
                    }
                    LegacyEffect::TextCapture {
                        lease_id,
                        text_hash,
                        byte_len,
                        speaker_hash,
                        ..
                    } => {
                        let text = self
                            .runtime
                            .take_ephemeral_text(&self.session_id, &lease_id)?
                            .ok_or_else(|| "ASTRA_EMU_HEADLESS_TEXT_LEASE_MISSING".to_owned())?;
                        if text.text.len() != byte_len as usize
                            || Hash256::from_sha256(text.text.as_bytes()) != text_hash
                            || text
                                .speaker
                                .as_ref()
                                .map(|value| Hash256::from_sha256(value.as_bytes()))
                                != speaker_hash
                        {
                            return Err("ASTRA_EMU_HEADLESS_TEXT_LEASE_IDENTITY".into());
                        }
                    }
                    _ => {}
                }
            }
            for wait in family.waits {
                let (token, condition) = wait_condition(&wait, next_step, self.delta_ns);
                if self.pending_waits.insert(token, condition).is_some() {
                    return Err("ASTRA_EMU_HEADLESS_WAIT_DUPLICATE".into());
                }
            }
        }
        if self.audio_enabled {
            self.audio.pump(self.platform).await?;
        }
        let video_changed = self.advance_video()?;
        if rendered || video_changed {
            self.present().await?;
            for wait in self.pending_waits.values_mut() {
                if matches!(wait, PendingWait::Presentation) {
                    *wait = PendingWait::DueStep(next_step.saturating_add(1));
                }
            }
        }
        self.terminal = output.status == "terminal";
        Ok(())
    }

    async fn present(&mut self) -> Result<(), String> {
        let (width, height, mut rgba8) = self
            .base_frame
            .clone()
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_BASE_FRAME_MISSING".to_owned())?;
        if let Some(video) = &self.video {
            let elapsed_us = self
                .fixed_step
                .saturating_sub(video.started_step)
                .saturating_mul(self.delta_ns)
                / 1_000;
            if let Some(frame) = video
                .stream
                .frames
                .iter()
                .rev()
                .find(|frame| frame.pts_us <= elapsed_us)
            {
                composite_bgra(&mut rgba8, width, height, frame)?;
            }
        }
        self.present_sequence = self
            .present_sequence
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_PRESENT_SEQUENCE_OVERFLOW".to_owned())?;
        self.platform
            .present_rgba(
                self.surface,
                RgbaFrame {
                    sequence: self.present_sequence,
                    width,
                    height,
                    rgba8: rgba8.clone(),
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        let hash = Hash256::from_sha256(&rgba8);
        self.visual_trace
            .extend_from_slice(hash.to_string().as_bytes());
        self.visual_trace.push(b'\n');
        self.latest_frame = Some((width, height, rgba8));
        Ok(())
    }

    async fn execute_video(&mut self, command: LegacyVideoCommandV1) -> Result<(), String> {
        command.validate().map_err(|error| error.to_string())?;
        match command {
            LegacyVideoCommandV1::Play {
                playback_id,
                resource_uri,
                mode: _,
                stage_width,
                stage_height,
            } => {
                if self.video.is_some() {
                    return Err("ASTRA_EMU_HEADLESS_VIDEO_ALREADY_ACTIVE".into());
                }
                let bytes = self.runtime.read_session_resource(
                    &self.session_id,
                    &resource_uri,
                    512 * 1024 * 1024,
                )?;
                let extension = resource_uri
                    .rsplit('.')
                    .next()
                    .unwrap_or("unknown")
                    .to_ascii_lowercase();
                let decode = self
                    .platform
                    .open_decode(DecodeKind::Video)
                    .await
                    .map_err(|e| e.to_string())?;
                let result = self
                    .platform
                    .decode(
                        decode,
                        PlatformDecodeRequest {
                            sequence: 1,
                            kind: DecodeKind::Video,
                            codec: extension,
                            description: Vec::new(),
                            sample_rate: None,
                            channels: None,
                            coded_width: None,
                            coded_height: None,
                            keyframe: true,
                            bytes,
                        },
                    )
                    .await
                    .map_err(|error| error.to_string());
                let close = self
                    .platform
                    .close_decode(decode)
                    .await
                    .map_err(|e| e.to_string());
                let output = match (result, close) {
                    (Ok(output), Ok(())) => output,
                    (Err(error), Ok(())) => return Err(error),
                    (_, Err(error)) => return Err(error),
                };
                let DecodeOutput::CpuBuffer { format, bytes, .. } = output else {
                    return Err("ASTRA_EMU_HEADLESS_VIDEO_OUTPUT_KIND".into());
                };
                if format != format!("postcard:{DECODED_VIDEO_STREAM_SCHEMA}") {
                    return Err("ASTRA_EMU_HEADLESS_VIDEO_OUTPUT_FORMAT".into());
                }
                let stream = DecodedVideoStream::decode(&bytes, 18_000, 512 * 1024 * 1024)
                    .map_err(|error| error.to_string())?;
                self.video = Some(ActiveVideo {
                    playback_id,
                    stage_width,
                    stage_height,
                    started_step: self.fixed_step,
                    stream,
                });
                Ok(())
            }
            LegacyVideoCommandV1::Stop { playback_id } => {
                let active = self
                    .video
                    .take()
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_NOT_ACTIVE".to_owned())?;
                if active.playback_id != playback_id {
                    return Err("ASTRA_EMU_HEADLESS_VIDEO_IDENTITY".into());
                }
                self.completed_media.push(playback_id);
                Ok(())
            }
        }
    }

    fn advance_video(&mut self) -> Result<bool, String> {
        let Some(video) = &self.video else {
            return Ok(false);
        };
        if video.stage_width
            != self
                .base_frame
                .as_ref()
                .map(|frame| frame.0)
                .unwrap_or(video.stage_width)
            || video.stage_height
                != self
                    .base_frame
                    .as_ref()
                    .map(|frame| frame.1)
                    .unwrap_or(video.stage_height)
        {
            return Err("ASTRA_EMU_HEADLESS_VIDEO_STAGE_DIMENSIONS".into());
        }
        let elapsed_us = self
            .fixed_step
            .saturating_sub(video.started_step)
            .saturating_mul(self.delta_ns)
            / 1_000;
        if elapsed_us >= video.stream.duration_us {
            let completed = self.video.take().unwrap();
            self.completed_media.push(completed.playback_id);
            return Ok(true);
        }
        Ok(true)
    }

    fn observations(&self) -> BTreeMap<&'static str, Hash256> {
        let frame_hash = self
            .latest_frame
            .as_ref()
            .map(|(_, _, bytes)| Hash256::from_sha256(bytes))
            .unwrap_or_else(|| Hash256::from_sha256(&[]));
        BTreeMap::from([
            ("runtime.state_hash", self.state_hash),
            ("frame.hash", frame_hash),
            (
                "runtime.terminal",
                Hash256::from_sha256(if self.terminal { b"true" } else { b"false" }),
            ),
            (
                "runtime.tick",
                Hash256::from_sha256(self.fixed_step.to_string().as_bytes()),
            ),
        ])
    }

    fn observation_hash(&self) -> Result<Hash256, String> {
        let value = self
            .observations()
            .into_iter()
            .map(|(key, hash)| (key, hash.to_string()))
            .collect::<BTreeMap<_, _>>();
        serde_json::to_vec(&value)
            .map(|bytes| Hash256::from_sha256(&bytes))
            .map_err(|_| "ASTRA_EMU_HEADLESS_OBSERVATION_ENCODE".to_owned())
    }

    fn observation_matches(&self, predicate: &ObservationPredicate) -> bool {
        match predicate {
            ObservationPredicate::Exists { key } => self.observations().contains_key(key.as_str()),
            ObservationPredicate::Equals { key, value_hash } => self
                .observations()
                .get(key.as_str())
                .is_some_and(|value| value.to_string() == *value_hash),
        }
    }
}

#[derive(Default)]
struct AudioStream {
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
    cursor: usize,
    playing: bool,
    paused: bool,
    repeat: bool,
    volume: f32,
    pan: f32,
    output: Option<AudioOutputHandle>,
    packet_sequence: u64,
}

struct HeadlessAudioExecutor {
    streams: BTreeMap<u32, AudioStream>,
    master_volume: f32,
    meter_trace: Vec<u8>,
}

impl Default for HeadlessAudioExecutor {
    fn default() -> Self {
        Self {
            streams: BTreeMap::new(),
            master_volume: 1.0,
            meter_trace: Vec::new(),
        }
    }
}

impl HeadlessAudioExecutor {
    async fn reset_for_restore(&mut self, platform: &PlatformHostClient) -> Result<(), String> {
        let ids = self
            .streams
            .iter()
            .filter(|(_, stream)| stream.output.is_some())
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in ids {
            self.close_stream(id, platform).await?;
        }
        self.streams.clear();
        Ok(())
    }

    async fn execute(
        &mut self,
        command: LegacyAudioCommandV1,
        resolved_resource: Option<Vec<u8>>,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        command.validate().map_err(|error| error.to_string())?;
        let (operation, stream_id) = audio_command_identity(&command);
        tracing::debug!(
            event = "astra_emu_headless_audio_command",
            operation,
            stream_id
        );
        match command {
            LegacyAudioCommandV1::LoadResource {
                stream_id,
                resource_uri,
                ..
            } => {
                if self.streams.contains_key(&stream_id) {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_STREAM_DUPLICATE".into());
                }
                let encoded = resolved_resource
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_RESOURCE_MISSING".to_owned())?;
                let codec = resource_uri
                    .rsplit('.')
                    .next()
                    .unwrap_or("unknown")
                    .to_ascii_lowercase();
                let decode = platform
                    .open_decode(DecodeKind::Audio)
                    .await
                    .map_err(|e| e.to_string())?;
                let output = platform
                    .decode(
                        decode,
                        PlatformDecodeRequest {
                            sequence: 1,
                            kind: DecodeKind::Audio,
                            codec,
                            description: Vec::new(),
                            sample_rate: None,
                            channels: None,
                            coded_width: None,
                            coded_height: None,
                            keyframe: true,
                            bytes: encoded,
                        },
                    )
                    .await
                    .map_err(|e| e.to_string());
                let close = platform
                    .close_decode(decode)
                    .await
                    .map_err(|e| e.to_string());
                let output = match (output, close) {
                    (Ok(output), Ok(())) => output,
                    (Err(error), Ok(())) => return Err(error),
                    (_, Err(error)) => return Err(error),
                };
                let DecodeOutput::CpuBuffer { format, bytes, .. } = output else {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_OUTPUT_KIND".into());
                };
                let (sample_rate, channels) = parse_pcm_format(&format)?;
                if !bytes.len().is_multiple_of(2 * usize::from(channels)) {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_PCM_LENGTH".into());
                }
                let samples = bytes
                    .chunks_exact(2)
                    .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
                    .collect();
                self.streams.insert(
                    stream_id,
                    AudioStream {
                        sample_rate,
                        channels,
                        samples,
                        volume: 1.0,
                        ..AudioStream::default()
                    },
                );
            }
            LegacyAudioCommandV1::CreateStream {
                stream_id,
                sample_rate,
                channels,
                ..
            } => {
                if self
                    .streams
                    .insert(
                        stream_id,
                        AudioStream {
                            sample_rate,
                            channels,
                            volume: 1.0,
                            ..AudioStream::default()
                        },
                    )
                    .is_some()
                {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_STREAM_DUPLICATE".into());
                }
            }
            LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => {
                let stream = stream_mut(&mut self.streams, stream_id)
                    .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_SUBMIT_STREAM_MISSING".to_owned())?;
                stream.samples.extend(
                    samples
                        .into_iter()
                        .map(|sample| f32::from(sample) / 32768.0),
                );
            }
            LegacyAudioCommandV1::SubmitF32 { stream_id, samples } => {
                if samples.iter().any(|sample| !sample.is_finite()) {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_SAMPLE_INVALID".into());
                }
                stream_mut(&mut self.streams, stream_id)
                    .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_SUBMIT_STREAM_MISSING".to_owned())?
                    .samples
                    .extend(samples);
            }
            LegacyAudioCommandV1::Play {
                stream_id,
                volume,
                pan,
                repeat,
                ..
            } => {
                let stream = stream_mut(&mut self.streams, stream_id)
                    .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_PLAY_STREAM_MISSING".to_owned())?;
                if stream.samples.is_empty() || stream.output.is_some() {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_PLAY_STATE".into());
                }
                stream.output = Some(
                    platform
                        .open_audio_output(AudioOutputRequest {
                            sample_rate: stream.sample_rate,
                            channels: stream.channels,
                            max_buffered_frames: (stream.sample_rate as usize * 4).max(1),
                        })
                        .await
                        .map_err(|e| e.to_string())?,
                );
                stream.cursor = 0;
                stream.playing = true;
                stream.paused = false;
                stream.repeat = repeat;
                stream.volume = volume;
                stream.pan = pan;
            }
            LegacyAudioCommandV1::Stop { stream_id, .. } => {
                if self
                    .streams
                    .get(&stream_id)
                    .is_some_and(|stream| stream.output.is_some())
                {
                    self.close_stream(stream_id, platform).await?;
                } else if let Some(stream) = self.streams.get_mut(&stream_id) {
                    stream.playing = false;
                }
            }
            LegacyAudioCommandV1::Pause { stream_id } => {
                if let Some(stream) = self.streams.get_mut(&stream_id) {
                    if let Some(output) = stream.output {
                        platform
                            .pause_audio(output)
                            .await
                            .map_err(|e| e.to_string())?;
                        stream.paused = true;
                    }
                }
            }
            LegacyAudioCommandV1::Resume { stream_id } => {
                if let Some(stream) = self.streams.get_mut(&stream_id) {
                    if let Some(output) = stream.output {
                        platform
                            .resume_audio(output)
                            .await
                            .map_err(|e| e.to_string())?;
                        stream.paused = false;
                    }
                }
            }
            LegacyAudioCommandV1::SetParams {
                stream_id,
                volume,
                pan,
                repeat,
            } => {
                if let Some(stream) = self
                    .streams
                    .get_mut(&stream_id)
                    .filter(|stream| stream.output.is_some())
                {
                    stream.volume = volume;
                    stream.pan = pan;
                    stream.repeat = repeat;
                }
            }
            LegacyAudioCommandV1::DestroyStream { stream_id } => {
                if self
                    .streams
                    .get(&stream_id)
                    .is_some_and(|stream| stream.output.is_some())
                {
                    self.close_stream(stream_id, platform).await?;
                }
                self.streams.remove(&stream_id);
            }
            LegacyAudioCommandV1::MasterVolume { volume } => self.master_volume = volume,
        }
        Ok(())
    }

    async fn pump(&mut self, platform: &PlatformHostClient) -> Result<(), String> {
        for stream in self
            .streams
            .values_mut()
            .filter(|stream| stream.playing && !stream.paused)
        {
            let output = stream
                .output
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_OUTPUT_MISSING".to_owned())?;
            let frames = usize::try_from(
                u64::from(stream.sample_rate).saturating_mul(FIXED_DELTA_NS) / 1_000_000_000,
            )
            .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_TICK_BOUNDS".to_owned())?
            .max(1);
            let sample_count = frames.saturating_mul(usize::from(stream.channels));
            let mut samples = Vec::with_capacity(sample_count);
            while samples.len() < sample_count && stream.playing {
                if stream.cursor >= stream.samples.len() {
                    if stream.repeat {
                        stream.cursor = 0;
                    } else {
                        stream.playing = false;
                        break;
                    }
                }
                let available =
                    (stream.samples.len() - stream.cursor).min(sample_count - samples.len());
                samples
                    .extend_from_slice(&stream.samples[stream.cursor..stream.cursor + available]);
                stream.cursor += available;
            }
            if samples.is_empty() {
                continue;
            }
            apply_gain_pan(
                &mut samples,
                stream.channels,
                stream.volume * self.master_volume,
                stream.pan,
            )?;
            stream.packet_sequence = stream
                .packet_sequence
                .checked_add(1)
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_SEQUENCE_OVERFLOW".to_owned())?;
            platform
                .submit_audio(
                    output,
                    AudioPacket {
                        sequence: stream.packet_sequence,
                        channels: stream.channels,
                        samples,
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
            let status = platform
                .query_audio_output(output)
                .await
                .map_err(|e| e.to_string())?;
            self.meter_trace.extend_from_slice(
                format!(
                    "{}:{}:{}:{}\n",
                    status.submitted_frames,
                    status.played_frames,
                    status.meter.sample_count,
                    status.meter.peak_dbfs.to_bits()
                )
                .as_bytes(),
            );
        }
        Ok(())
    }

    async fn close_stream(
        &mut self,
        stream_id: u32,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        let stream = stream_mut(&mut self.streams, stream_id)?;
        let output = stream
            .output
            .take()
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_OUTPUT_MISSING".to_owned())?;
        let meter = platform
            .drain_audio(output)
            .await
            .map_err(|e| e.to_string())?;
        self.meter_trace.extend_from_slice(
            format!(
                "{}:{}:{}\n",
                meter.sample_count,
                meter.peak_dbfs.to_bits(),
                meter.rms_dbfs.to_bits()
            )
            .as_bytes(),
        );
        platform
            .close_audio(output)
            .await
            .map_err(|e| e.to_string())?;
        stream.playing = false;
        Ok(())
    }

    async fn shutdown(mut self, platform: &PlatformHostClient) -> Result<Vec<u8>, String> {
        let ids = self
            .streams
            .iter()
            .filter(|(_, stream)| stream.output.is_some())
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in ids {
            self.close_stream(id, platform).await?;
        }
        Ok(self.meter_trace)
    }
}

fn audio_command_identity(command: &LegacyAudioCommandV1) -> (&'static str, u32) {
    match command {
        LegacyAudioCommandV1::LoadResource { stream_id, .. } => ("load_resource", *stream_id),
        LegacyAudioCommandV1::CreateStream { stream_id, .. } => ("create_stream", *stream_id),
        LegacyAudioCommandV1::SubmitI16 { stream_id, .. } => ("submit_i16", *stream_id),
        LegacyAudioCommandV1::SubmitF32 { stream_id, .. } => ("submit_f32", *stream_id),
        LegacyAudioCommandV1::Play { stream_id, .. } => ("play", *stream_id),
        LegacyAudioCommandV1::Stop { stream_id, .. } => ("stop", *stream_id),
        LegacyAudioCommandV1::Pause { stream_id } => ("pause", *stream_id),
        LegacyAudioCommandV1::Resume { stream_id } => ("resume", *stream_id),
        LegacyAudioCommandV1::SetParams { stream_id, .. } => ("set_params", *stream_id),
        LegacyAudioCommandV1::DestroyStream { stream_id } => ("destroy_stream", *stream_id),
        LegacyAudioCommandV1::MasterVolume { .. } => ("master_volume", 0),
    }
}

fn stream_mut(
    streams: &mut BTreeMap<u32, AudioStream>,
    id: u32,
) -> Result<&mut AudioStream, String> {
    streams
        .get_mut(&id)
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_STREAM_MISSING".to_owned())
}

fn parse_pcm_format(format: &str) -> Result<(u32, u16), String> {
    let mut parts = format.split(':');
    if parts.next() != Some("pcm_s16le") {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_PCM_FORMAT".into());
    }
    let sample_rate = parts
        .next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_PCM_FORMAT".to_owned())?;
    let channels = parts
        .next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_PCM_FORMAT".to_owned())?;
    if parts.next().is_some()
        || !(8_000..=384_000).contains(&sample_rate)
        || !(1..=8).contains(&channels)
    {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_PCM_FORMAT".into());
    }
    Ok((sample_rate, channels))
}

fn apply_gain_pan(samples: &mut [f32], channels: u16, gain: f32, pan: f32) -> Result<(), String> {
    if !gain.is_finite()
        || !pan.is_finite()
        || !(0.0..=4.0).contains(&gain)
        || !(-1.0..=1.0).contains(&pan)
    {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_PARAMS".into());
    }
    for frame in samples.chunks_exact_mut(usize::from(channels)) {
        for sample in frame.iter_mut() {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
        if channels >= 2 {
            frame[0] *= (1.0 - pan.max(0.0)).sqrt();
            frame[1] *= (1.0 + pan.min(0.0)).sqrt();
        }
    }
    Ok(())
}

fn wait_condition(wait: &LegacyWaitRequest, step: u64, delta_ns: u64) -> (String, PendingWait) {
    match wait {
        LegacyWaitRequest::Time {
            token_id,
            milliseconds,
        } => {
            let ticks = u64::from(*milliseconds)
                .saturating_mul(1_000_000)
                .div_ceil(delta_ns)
                .max(1);
            (
                token_id.clone(),
                PendingWait::DueStep(step.saturating_add(ticks)),
            )
        }
        LegacyWaitRequest::Frame { token_id, frames } => (
            token_id.clone(),
            PendingWait::DueStep(step.saturating_add(u64::from(*frames).max(1))),
        ),
        LegacyWaitRequest::Input { token_id, mask } => {
            (token_id.clone(), PendingWait::Input(*mask))
        }
        LegacyWaitRequest::MediaFence { token_id, media_id } => {
            (token_id.clone(), PendingWait::Media(media_id.clone()))
        }
        LegacyWaitRequest::PresentationFence { token_id, .. } => {
            (token_id.clone(), PendingWait::Presentation)
        }
        LegacyWaitRequest::ProviderCompletion { token_id, .. }
        | LegacyWaitRequest::FamilyOpaque { token_id, .. } => {
            (token_id.clone(), PendingWait::Unsupported)
        }
    }
}

fn input_control_mask(control: &str) -> u64 {
    match control {
        "confirm" => 1 << 0,
        "cancel" => 1 << 1,
        "up" => 1 << 2,
        "down" => 1 << 3,
        "left" => 1 << 4,
        "right" => 1 << 5,
        "space" => 1 << 6,
        "pointer.primary" => 1 << 7,
        "pointer.secondary" => 1 << 8,
        "wheel" => 1 << 9,
        _ => 0,
    }
}

fn composite_bgra(
    target: &mut [u8],
    target_width: u32,
    target_height: u32,
    frame: &astra_media::DecodedVideoFrame,
) -> Result<(), String> {
    let expected = usize::try_from(target_width)
        .ok()
        .and_then(|width| {
            usize::try_from(target_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_FRAME_BOUNDS".to_owned())?;
    if target.len() != expected {
        return Err("ASTRA_EMU_HEADLESS_VIDEO_TARGET_LENGTH".into());
    }
    for y in 0..target_height {
        let source_y = (u64::from(y) * u64::from(frame.height) / u64::from(target_height)) as u32;
        for x in 0..target_width {
            let source_x = (u64::from(x) * u64::from(frame.width) / u64::from(target_width)) as u32;
            let source = ((source_y as usize * frame.width as usize) + source_x as usize) * 4;
            let destination = ((y as usize * target_width as usize) + x as usize) * 4;
            target[destination] = frame.bgra8[source + 2];
            target[destination + 1] = frame.bgra8[source + 1];
            target[destination + 2] = frame.bgra8[source];
            target[destination + 3] = frame.bgra8[source + 3];
        }
    }
    Ok(())
}

fn write_atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let partial = path.with_extension("partial");
    fs::write(
        &partial,
        serde_json::to_vec_pretty(value)
            .map_err(|_| "ASTRA_EMU_HEADLESS_REPORT_ENCODE".to_owned())?,
    )
    .map_err(|_| "ASTRA_EMU_HEADLESS_REPORT_WRITE".to_owned())?;
    fs::rename(partial, path).map_err(|_| "ASTRA_EMU_HEADLESS_REPORT_COMMIT".to_owned())
}

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn native_key_mapping_is_explicit_and_does_not_capture_unbound_keys() {
        assert_eq!(
            native_key_control(Some("Enter"), "Unidentified"),
            Some("confirm")
        );
        assert_eq!(
            native_key_control(Some("ArrowLeft"), "Unidentified"),
            Some("left")
        );
        assert_eq!(native_key_control(None, "Space"), Some("space"));
        assert_eq!(native_key_control(Some("F12"), "F12"), None);
    }

    #[test]
    fn native_pointer_mapping_preserves_stage_aspect_and_rejects_letterbox() {
        let landscape = NativeViewport {
            window_width: 1_920,
            window_height: 1_080,
            stage_width: 1_280,
            stage_height: 720,
        };
        assert_eq!(landscape.map_pointer(960.0, 540.0), Some([640.0, 360.0]));

        let letterboxed = NativeViewport {
            window_width: 1_600,
            window_height: 1_200,
            stage_width: 1_280,
            stage_height: 720,
        };
        assert_eq!(letterboxed.map_pointer(800.0, 100.0), None);
        assert_eq!(letterboxed.map_pointer(800.0, 600.0), Some([640.0, 360.0]));
    }
}
