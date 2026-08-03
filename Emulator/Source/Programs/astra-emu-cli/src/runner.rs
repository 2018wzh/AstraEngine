use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use astra_core::{
    Hash256, PerformanceBudget, PerformanceMetricBudget, PerformanceRecorder,
    PerformanceRunIdentity, PerformanceStatus, PerformanceTraceManifest, PerformanceUnit,
    SchemaVersion, PERFORMANCE_TRACE_MANIFEST_SCHEMA,
};
#[cfg(test)]
use astra_emu_family_api::LegacyProbeReport;
use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioSampleFormat, LegacyAwaitResult,
    LegacyDrawV1, LegacyEffect, LegacyInputEdge, LegacyPreparedSceneCommitV1, LegacyProbeRequest,
    LegacyRenderFrameV1, LegacyRenderResourceFrameV1, LegacyRuntimeHostCtx,
    LegacySceneResourceOperationV1, LegacyStepBudget, LegacyTextPresentationLeaseV1,
    LegacyTextureFormat, LegacyTextureUpdateV1, LegacyVfsReader, LegacyVideoCommandV1,
    LegacyVideoMode, LegacyWaitRequest,
};
use astra_emu_family_support::{
    verify_vfs, LegacyAudioPlaybackService, LegacyMountedVfsReaderAdapter, LegacyVfsFamilyRegistry,
};
use astra_emu_fvp::{decode_fvp_movie, fvp_movie_compatibility, FvpMovieCompatibility};
use astra_emu_manager_core::{
    AstraEmuRuntimeProvider, CancellationToken, CaseRecord, DesktopGrantedSource,
    DesktopVfsRegistry, EmuCaseProfile, EmuStepPayload, Library, LibraryScanner, ScanLimits,
    SourceGrant,
};
use astra_emu_minori::MinoriVfsFamilyFactory;
use astra_headless_protocol::{
    ArtifactEntry, ArtifactManifest, ButtonState, CheckpointResult, Diagnostic, GamepadControl,
    InputMessage, ObservationPredicate, PhysicalInput, PointerButton, RunReport, RunStatus,
    TouchPhase, HEADLESS_RUN_REPORT_SCHEMA as STANDARD_HEADLESS_RUN_REPORT_SCHEMA,
};
use astra_media::{
    open_symphonia_audio_stream, DecodeBindingContext, DecodeOutput as MediaDecodeOutput,
    DecodeProviderRegistry, DecodeRequest, DecodedVideoFrame, DecodedVideoStream,
    ImageDecodeProvider, MediaError, SymphoniaAudioStreamDecoder, DECODED_VIDEO_STREAM_SCHEMA,
};
use astra_media_core::{
    BlendMode, MeshMaterial2D, MeshVertex2D, RectI, SceneCommand, TextureFrame,
};
use astra_observability::{
    sample_process_memory, PerfettoTraceConfig, PerfettoTraceSummary, PerfettoTraceWriter,
};
use astra_platform::{
    AudioOutputHandle, AudioOutputRequest, AudioPacket, DecodeKind, DecodeOutput,
    GamepadControl as PlatformGamepadControl, GpuAdapterPolicy, GpuBackendPolicy,
    GpuDeviceTypePolicy, HeadlessArtifactPolicy, HeadlessArtifactRetention, HeadlessHostProfile,
    HeadlessReadbackPolicy, HeadlessRenderPolicy, HostLaunchProfile, InputState,
    PlatformDecodeRequest, PlatformEventKind, PlatformHostClient, PlatformHostFactory,
    PointerButton as PlatformPointerButton, RgbaFrame, SceneFrame, ScenePresentReceipt,
    SurfaceHandle, SurfaceRequest, TouchPhase as PlatformTouchPhase, WindowHandle, WindowRequest,
};
use astra_platform_headless::{
    HeadlessGpuFrameSample, HeadlessPerformanceObserver, HeadlessPlatformFactory,
};
use astra_plugin::ProductRuntimeProvider;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProviderInstanceId, RuntimeOpenRequest, RuntimeOutputDomain,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionCodec,
    RuntimeSectionPayload, RuntimeStepInput, RuntimeStepMode, RuntimeTickIntegrityMode,
};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    family_host::CliFamilyHostConfig,
    input::{read_input_sequence, ValidatedInputSequence},
    rasterizer::CpuStageRasterizer,
    text_presentation::BoundTextPresenter,
};

pub const HEADLESS_RUN_REPORT_SCHEMA: &str = "astra.emu.headless_run_report.v3";
const FIXED_DELTA_NS: u64 = 16_666_667;
const MAX_STREAM_DECODED_AUDIO_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MOVIE_FRAMES: usize = 18_000;
const MAX_MOVIE_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAX_MOVIE_AUDIO_SAMPLES: usize = 64 * 1024 * 1024;
const MOVIE_AUDIO_STREAM_BASE: u32 = 0xF000_0000;
const HEADLESS_RESUME_SNAPSHOT_SCHEMA: &str = "astra.emu.headless_resume_snapshot.v1";
const MAX_RESUME_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
/// Matches the shared WGPU scene resource budget.  Native semantic uploads
/// are not RGBA frame payloads, so the platform command bound must admit a
/// bounded texture delta without weakening the renderer's own resource cap.
const MAX_NATIVE_SCENE_UPLOAD_BYTES: usize = 64 * 1024 * 1024;
// Hosted RFVP requires `default.ttf` during core boot.  Some original FVP
// installations rely on the platform renderer's system-font fallback and do
// not ship that file.  The CLI supplies this public OFL font through the
// already-bounded, mount-scoped VFS overlay port only when the installation
// lacks its own default font; it never mutates or shadows game content.
const FVP_HOSTED_FALLBACK_FONT: &[u8] =
    include_bytes!("../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf");

#[derive(Debug, Clone)]
pub struct HeadlessLaunch {
    pub family_id: String,
    pub game_dir: PathBuf,
    pub mount_profile: PathBuf,
    pub entry: Option<String>,
    pub input_path: PathBuf,
    pub artifact_root: PathBuf,
    pub family_manifest: Option<PathBuf>,
    pub family_library: Option<PathBuf>,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub video_provider: String,
    pub verify_snapshot: bool,
    pub artifact_retention: String,
    pub frame_sample_interval: u64,
    /// Presentation cadence. Runtime simulation remains fixed at 60 Hz; 120 Hz
    /// is two GPU presentations per simulation tick.
    pub presentation_rate_hz: u32,
    pub perfetto_trace: Option<PathBuf>,
    pub audit_all_resources: bool,
    pub resume_snapshot: Option<PathBuf>,
    pub snapshot_output: Option<PathBuf>,
    pub performance: Option<HeadlessPerformanceArtifacts>,
}

/// Local-private outputs required to turn a Headless run into performance
/// evidence.  The generic budget/report/manifest schemas live in `astra-core`;
/// this wrapper intentionally contains paths only and is not serialized.
#[derive(Debug, Clone)]
pub struct HeadlessPerformanceArtifacts {
    pub budget_path: PathBuf,
    pub report_path: PathBuf,
    pub trace_manifest_path: PathBuf,
    pub warmup_presentations: u64,
}

#[derive(Debug, Clone)]
pub struct NativeLaunch {
    pub family_id: String,
    pub game_dir: PathBuf,
    pub mount_profile: PathBuf,
    pub entry: Option<String>,
    pub family_manifest: Option<PathBuf>,
    pub family_library: Option<PathBuf>,
    pub enable_audio: bool,
    pub perfetto_trace: Option<PathBuf>,
    pub input_path: Option<PathBuf>,
    pub max_fixed_steps: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessCheckpointEvidenceV1 {
    pub checkpoint_id: String,
    pub fixed_step: u64,
    pub frame_hash: Hash256,
    pub observation_hash: Hash256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessVfsAccessEvidenceV1 {
    pub resource_count: u64,
    pub unique_range_count: u64,
    pub read_count: u64,
    pub bytes_read: u64,
    pub max_range_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessResourceAuditEvidenceV1 {
    pub resource_count: u64,
    pub range_count: u64,
    pub bytes_read: u64,
    pub max_range_bytes: u64,
    pub manifest_hash: Hash256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessDurationDistributionV1 {
    pub sample_count: u64,
    pub total_ns: u64,
    pub median_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessPhaseTimingEvidenceV1 {
    pub step_total: HeadlessDurationDistributionV1,
    pub runtime_step: HeadlessDurationDistributionV1,
    pub effect_dispatch: HeadlessDurationDistributionV1,
    pub raster: HeadlessDurationDistributionV1,
    pub media: HeadlessDurationDistributionV1,
    pub present: HeadlessDurationDistributionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessRunReportV3 {
    pub schema: String,
    pub status: String,
    pub family_id: String,
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
    pub frame_sample_interval: u64,
    pub consumed_input_messages: u64,
    pub snapshot_round_trip_verified: bool,
    pub resumed_from_fixed_step: Option<u64>,
    pub resume_snapshot_exported: bool,
    pub terminal_reached: bool,
    pub vfs_access: HeadlessVfsAccessEvidenceV1,
    pub resource_audit: Option<HeadlessResourceAuditEvidenceV1>,
    pub phase_timings: HeadlessPhaseTimingEvidenceV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance_report_hash: Option<Hash256>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance_trace_manifest_hash: Option<Hash256>,
    pub checkpoints: Vec<HeadlessCheckpointEvidenceV1>,
    pub lifecycle_steps: Vec<String>,
    pub diagnostic_codes: Vec<String>,
}

struct PreparedFamilyCase {
    family_id: String,
    case_identity: String,
    package_hash: Hash256,
    entry_uri: String,
    fvp_pack_paths: Option<Vec<String>>,
    reader: Arc<dyn LegacyVfsReader>,
    evidence: VfsEvidenceBackend,
}

enum VfsEvidenceBackend {
    Desktop {
        registry: Arc<DesktopVfsRegistry>,
        mount_set_id: String,
    },
    Mounted(Arc<LegacyMountedVfsReaderAdapter>),
}

impl VfsEvidenceBackend {
    fn access_metrics(&self) -> Result<HeadlessVfsAccessEvidenceV1, String> {
        match self {
            Self::Desktop {
                registry,
                mount_set_id,
            } => {
                let access = registry.access_metrics(mount_set_id)?;
                Ok(HeadlessVfsAccessEvidenceV1 {
                    resource_count: access.resource_count,
                    unique_range_count: access.unique_range_count,
                    read_count: access.read_count,
                    bytes_read: access.bytes_read,
                    max_range_bytes: access.max_range_bytes,
                })
            }
            Self::Mounted(adapter) => {
                let access = adapter
                    .access_metrics()
                    .map_err(|error| error.to_string())?;
                Ok(HeadlessVfsAccessEvidenceV1 {
                    resource_count: access.resource_count,
                    unique_range_count: access.unique_range_count,
                    read_count: access.read_count,
                    bytes_read: access.bytes_read,
                    max_range_bytes: access.max_range_bytes,
                })
            }
        }
    }

    fn audit(&self) -> Result<HeadlessResourceAuditEvidenceV1, String> {
        match self {
            Self::Desktop {
                registry,
                mount_set_id,
            } => {
                let audit = registry.audit_mount(mount_set_id)?;
                Ok(HeadlessResourceAuditEvidenceV1 {
                    resource_count: audit.resource_count,
                    range_count: audit.range_count,
                    bytes_read: audit.bytes_read,
                    max_range_bytes: audit.max_range_bytes,
                    manifest_hash: audit.manifest_hash,
                })
            }
            Self::Mounted(adapter) => {
                let report = verify_vfs(adapter.mounted_vfs().as_ref())
                    .map_err(|error| error.to_string())?;
                let max_range_bytes = adapter
                    .mounted_vfs()
                    .manifest()
                    .entries
                    .iter()
                    .map(|entry| entry.decoded_size.min(4 * 1024 * 1024))
                    .max()
                    .unwrap_or(0);
                Ok(HeadlessResourceAuditEvidenceV1 {
                    resource_count: report.entry_count,
                    range_count: report.range_count,
                    bytes_read: report.byte_count,
                    max_range_bytes,
                    manifest_hash: report.aggregate_hash,
                })
            }
        }
    }

    fn cleanup(&self) {
        if let Self::Desktop {
            registry,
            mount_set_id,
        } = self
        {
            registry.unbind(mount_set_id);
        }
    }
}

fn prepare_family_case(
    family_id: &str,
    game_root: &Path,
    mount_profile: &Path,
    entry: Option<&str>,
    mount_set_id: &str,
) -> Result<PreparedFamilyCase, String> {
    match family_id {
        "fvp" => prepare_fvp_case(game_root, mount_profile, entry, mount_set_id),
        "minori" => prepare_minori_case(game_root, mount_profile, entry, mount_set_id),
        _ => Err("ASTRA_EMU_CLI_FAMILY_UNSUPPORTED".into()),
    }
}

fn prepare_fvp_case(
    game_root: &Path,
    mount_profile: &Path,
    entry: Option<&str>,
    mount_set_id: &str,
) -> Result<PreparedFamilyCase, String> {
    let loaded = astra_emu_family_support::load_mount_profile(mount_profile)
        .map_err(|error| error.to_string())?;
    if loaded.profile.family_id != "fvp" {
        return Err("ASTRA_EMU_VFS_FAMILY_MISMATCH".into());
    }
    let options: astra_emu_fvp::FvpVfsFamilyOptions =
        serde_json::from_slice(&loaded.family_config.payload)
            .map_err(|_| "ASTRA_EMU_FVP_MOUNT_OPTIONS".to_owned())?;
    if options.archives.is_empty() || options.archives.len() > 4096 {
        return Err("ASTRA_EMU_FVP_MOUNT_OPTIONS".into());
    }
    let mut pack_paths = BTreeSet::new();
    for archive in options.archives {
        let archive = normalize_fvp_pack_path(&archive)?;
        if !pack_paths.insert(archive) {
            return Err("ASTRA_EMU_FVP_ARCHIVE_DUPLICATE".into());
        }
    }
    let case = scan_case(game_root, entry)?;
    let package_hash: Hash256 = case
        .content_hash
        .parse()
        .map_err(|_| "ASTRA_EMU_CASE_FINGERPRINT_INVALID".to_owned())?;
    let registry = Arc::new(DesktopVfsRegistry::default());
    registry.bind(mount_set_id, &game_root.to_string_lossy())?;
    install_fvp_hosted_font_overlay(registry.as_ref(), mount_set_id)?;
    Ok(PreparedFamilyCase {
        family_id: "fvp".into(),
        case_identity: case.case_identity,
        package_hash,
        entry_uri: case.relative_path,
        fvp_pack_paths: Some(pack_paths.into_iter().collect()),
        reader: registry.clone(),
        evidence: VfsEvidenceBackend::Desktop {
            registry,
            mount_set_id: mount_set_id.into(),
        },
    })
}

fn normalize_fvp_pack_path(path: &str) -> Result<String, String> {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if normalized.is_empty()
        || !normalized.ends_with(".bin")
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err("ASTRA_EMU_FVP_ARCHIVE_PATH".into());
    }
    Ok(normalized)
}

fn install_fvp_hosted_font_overlay(
    registry: &DesktopVfsRegistry,
    mount_set_id: &str,
) -> Result<(), String> {
    if registry
        .list_resources(mount_set_id)?
        .iter()
        .any(|resource| resource.path.eq_ignore_ascii_case("default.ttf"))
    {
        return Ok(());
    }
    registry.install_overlays(
        mount_set_id,
        [("default.ttf".into(), FVP_HOSTED_FALLBACK_FONT.to_vec())]
            .into_iter()
            .collect(),
    )
}

fn prepare_minori_case(
    game_root: &Path,
    mount_profile: &Path,
    entry: Option<&str>,
    mount_set_id: &str,
) -> Result<PreparedFamilyCase, String> {
    let mut registry = LegacyVfsFamilyRegistry::default();
    registry
        .register(Arc::new(MinoriVfsFamilyFactory))
        .map_err(|error| error.to_string())?;
    let loaded = registry
        .load_profile(mount_profile)
        .map_err(|error| error.to_string())?;
    let mounted = registry
        .mount("minori", game_root, &loaded)
        .map_err(|error| error.to_string())?;
    let entry_uri = match entry {
        Some(uri)
            if mounted
                .manifest()
                .entries
                .iter()
                .any(|candidate| candidate.uri == uri && candidate.media_kind == "script") =>
        {
            uri.to_owned()
        }
        Some(_) => return Err("ASTRA_EMU_MINORI_ENTRY_INVALID".into()),
        None => {
            let scripts = mounted
                .manifest()
                .entries
                .iter()
                .filter(|candidate| candidate.media_kind == "script")
                .map(|candidate| candidate.uri.clone())
                .collect::<Vec<_>>();
            if scripts.len() != 1 {
                return Err("ASTRA_EMU_MINORI_ENTRY_REQUIRED".into());
            }
            scripts[0].clone()
        }
    };
    let manifest_bytes = postcard::to_allocvec(mounted.manifest())
        .map_err(|_| "ASTRA_EMU_VFS_MANIFEST_HASH".to_owned())?;
    let package_hash = Hash256::from_sha256(&manifest_bytes);
    let adapter = Arc::new(
        LegacyMountedVfsReaderAdapter::new(mount_set_id, mounted)
            .map_err(|error| error.to_string())?,
    );
    Ok(PreparedFamilyCase {
        family_id: "minori".into(),
        case_identity: format!("minori-{}", &package_hash.to_string()[7..23]),
        package_hash,
        entry_uri,
        fvp_pack_paths: None,
        reader: adapter.clone(),
        evidence: VfsEvidenceBackend::Mounted(adapter),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HeadlessResumeSnapshotV1 {
    schema: String,
    build_identity_hash: Hash256,
    family_provider_id: String,
    family_binary_hash: Hash256,
    game_identity_hash: Hash256,
    entry_identity_hash: Hash256,
    fixed_delta_ns: u64,
    stage_width: u32,
    stage_height: u32,
    fixed_step: u64,
    session_seed: u64,
    runtime_sections: Vec<RuntimeSectionPayload>,
    driver: HeadlessDriverResumeV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HeadlessDriverResumeV1 {
    fixed_step: u64,
    input_sequence: u64,
    await_sequence: u64,
    pending_inputs: Vec<LegacyInputEdge>,
    pending_waits: BTreeMap<String, PendingWait>,
    completed_media: Vec<String>,
    active_video: Option<HeadlessVideoResumeV1>,
    state_hash: Hash256,
    active_touch: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HeadlessVideoResumeV1 {
    playback_id: String,
    resource_uri: String,
    mode: LegacyVideoMode,
    stage_width: u32,
    stage_height: u32,
    started_step: u64,
}

struct HeadlessResumeIdentity<'a> {
    build_identity_hash: Hash256,
    family_provider_id: &'a str,
    family_binary_hash: Hash256,
    game_identity_hash: Hash256,
    entry_identity_hash: Hash256,
    fixed_delta_ns: u64,
    stage_width: u32,
    stage_height: u32,
    session_seed: u64,
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
    if launch.max_fixed_steps == Some(0) {
        return Err("ASTRA_EMU_NATIVE_MAX_FIXED_STEPS_INVALID".into());
    }
    let native_input = launch
        .input_path
        .as_deref()
        .map(read_input_sequence)
        .transpose()?;
    let game_root = fs::canonicalize(&launch.game_dir)
        .map_err(|_| "ASTRA_EMU_CLI_GAME_DIR_INVALID".to_owned())?;
    if !game_root.is_dir() {
        return Err("ASTRA_EMU_CLI_GAME_DIR_INVALID".into());
    }
    let executable = std::env::current_exe().map_err(|_| "ASTRA_EMU_EXECUTABLE_PATH".to_owned())?;
    let mount_seed = Hash256::from_sha256(
        format!("{}\0{}", launch.family_id, game_root.to_string_lossy()).as_bytes(),
    );
    let mount_set_id = format!("native-{}", &mount_seed.to_string()[7..39]);
    let prepared = prepare_family_case(
        &launch.family_id,
        &game_root,
        &launch.mount_profile,
        launch.entry.as_deref(),
        &mount_set_id,
    )?;
    let game_identity_hash = prepared.package_hash;
    let family_config = match (&launch.family_manifest, &launch.family_library) {
        (Some(manifest), Some(library)) => {
            CliFamilyHostConfig::with_paths(&launch.family_id, manifest.clone(), library.clone())?
        }
        (None, None) => {
            CliFamilyHostConfig::installed_for_executable(&executable, &launch.family_id)?
        }
        _ => return Err("ASTRA_EMU_CLI_FAMILY_PATH_PAIR_REQUIRED".into()),
    };
    let family = family_config.create_provider(prepared.reader.clone())?;
    let mut runtime = AstraEmuRuntimeProvider::new(family)?;
    runtime.create_instance(ProviderInstanceId("astra.emu.cli.native.instance".into()))?;
    let probe = probe_profile(
        &runtime,
        &prepared,
        ProbeProfileRequest {
            mount_set_id: &mount_set_id,
            package_hash: game_identity_hash,
            target: "windows",
            media_service_id: "astra.platform.windows.media",
            report_sink_id: "astra.emu.cli.native.report",
            stage_size: (1280, 720),
        },
    )?;
    let stage_width = probe
        .runtime
        .family_options
        .get("astra.stage_width")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_PROBE_STAGE_INVALID".to_owned())?;
    let stage_height = probe
        .runtime
        .family_options
        .get("astra.stage_height")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_PROBE_STAGE_INVALID".to_owned())?;
    let section = case_profile_section(
        &prepared,
        &probe.runtime,
        &mount_set_id,
        probe.content_identity,
    )?;
    let seed = u64::from_le_bytes(game_identity_hash.as_bytes()[..8].try_into().unwrap());
    let open = runtime.open(RuntimeOpenRequest {
        target_id: "astra-emu-native-case".into(),
        profile: format!("{}-v1", launch.family_id),
        locale: "und".into(),
        seed,
        integrity_mode: RuntimeTickIntegrityMode::Evidence,
        executor: astra_plugin_abi::RuntimeExecutorConfig::serial(),
        package_hash: game_identity_hash.to_string(),
        sections: vec![section],
    })?;
    let mut host_profile = astra_platform::PlatformHostProfile::windows_release(
        "astra-emu-cli",
        "dev.astraengine.astraemu-cli",
    );
    host_profile.id = "astra-emu-cli-native".into();
    let native_rgba_frame_bytes = usize::try_from(stage_width)
        .ok()
        .and_then(|width| {
            usize::try_from(stage_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "ASTRA_EMU_NATIVE_FRAME_BOUNDS".to_owned())?;
    host_profile.limits.max_frame_bytes =
        native_rgba_frame_bytes.max(MAX_NATIVE_SCENE_UPLOAD_BYTES);
    let mut host = astra_platform_windows::factory()
        .start(HostLaunchProfile::platform(host_profile))
        .await
        .map_err(|error| error.to_string())?;
    let window = host
        .client
        .create_window(WindowRequest {
            title: format!("AstraEMU {}", launch.family_id),
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
        family = launch.family_id.as_str(),
        stage_width,
        stage_height,
        audio_enabled = launch.enable_audio
    );

    let mut driver = RuntimeDriver::new(
        &mut runtime,
        open.session_id.clone(),
        &host.client,
        surface,
        RuntimeDriverConfig {
            seed,
            delta_ns: probe.runtime.fixed_delta_ns,
            audio_enabled: launch.enable_audio,
            text: TextProviderBinding {
                provider_id: "cosmic_text_cpu",
                target: "windows",
                profile: &format!("{}-v1", launch.family_id),
            },
            resume: None,
            frame_sample_interval: 1,
            perfetto_trace: launch.perfetto_trace.clone(),
            perfetto_rfvp_core: launch.family_id.as_str() == "fvp",
            capture_performance_samples: false,
            presentation: PresentationPath::NativeGpu,
            presentation_substeps: 1,
            synchronous_gpu_presents: false,
            background_audio: true,
            audio_pump: AudioPumpPolicy::Realtime {
                target_latency_ms: 180,
                refill_low_water_ms: 120,
                poll_interval_ticks: 4,
            },
        },
    )?;
    let mut viewport = NativeViewport {
        window_width: stage_width,
        window_height: stage_height,
        stage_width,
        stage_height,
    };
    let mut suspended = false;
    let mut native_input_cursor = 0usize;
    let mut native_shutdown_requested = false;
    if let Some(input) = native_input.as_ref() {
        native_shutdown_requested =
            consume_native_inputs_due(&mut driver, &input.messages, &mut native_input_cursor)?;
    }
    let mut ticker = tokio::time::interval(std::time::Duration::from_nanos(
        probe.runtime.fixed_delta_ns,
    ));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let run_result = loop {
        if native_shutdown_requested {
            break Ok(());
        }
        tokio::select! {
            _ = ticker.tick(), if !suspended => {
                if let Err(error) = driver.step().await {
                    break Err(error);
                }
                if driver.terminal {
                    break Ok(());
                }
                if let Some(input) = native_input.as_ref() {
                    native_shutdown_requested = consume_native_inputs_due(
                        &mut driver,
                        &input.messages,
                        &mut native_input_cursor,
                    )?;
                }
                if launch.max_fixed_steps.is_some_and(|limit| driver.fixed_step >= limit) {
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
                    Ok(NativeEventAction::Suspend(value)) => {
                        driver.audio.set_suspended(value)?;
                        suspended = value;
                    }
                    Ok(NativeEventAction::Close) => break Ok(()),
                    Err(error) => break Err(error),
                }
            }
        }
    };
    let fixed_step = driver.fixed_step;
    let perfetto_cleanup = driver.finish_perfetto().map(|_| ());
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
    prepared.evidence.cleanup();
    let cleanup_errors = [
        ("perfetto", perfetto_cleanup),
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
        family = launch.family_id.as_str()
    );
    Ok(())
}

pub async fn run_headless(launch: HeadlessLaunch) -> Result<HeadlessRunReportV3, String> {
    validate_launch(&launch)?;
    let input = read_input_sequence(&launch.input_path)?;
    let game_root = fs::canonicalize(&launch.game_dir)
        .map_err(|_| "ASTRA_EMU_HEADLESS_GAME_DIR_INVALID".to_owned())?;
    if !game_root.is_dir() {
        return Err("ASTRA_EMU_HEADLESS_GAME_DIR_INVALID".into());
    }
    let executable = std::env::current_exe().map_err(|_| "ASTRA_EMU_EXECUTABLE_PATH".to_owned())?;
    let executable_bytes =
        fs::read(&executable).map_err(|_| "ASTRA_EMU_EXECUTABLE_READ".to_owned())?;
    let build_identity_hash = Hash256::from_sha256(&executable_bytes);
    let mount_seed = Hash256::from_sha256(
        format!("{}\0{}", launch.family_id, game_root.to_string_lossy()).as_bytes(),
    );
    let mount_set_id = format!("headless-{}", &mount_seed.to_string()[7..39]);
    let prepared = prepare_family_case(
        &launch.family_id,
        &game_root,
        &launch.mount_profile,
        launch.entry.as_deref(),
        &mount_set_id,
    )?;
    let game_identity_hash = prepared.package_hash;
    let family_config = match (&launch.family_manifest, &launch.family_library) {
        (Some(manifest), Some(library)) => {
            CliFamilyHostConfig::with_paths(&launch.family_id, manifest.clone(), library.clone())?
        }
        (None, None) => {
            CliFamilyHostConfig::installed_for_executable(&executable, &launch.family_id)?
        }
        _ => return Err("ASTRA_EMU_HEADLESS_FAMILY_PATH_PAIR".into()),
    };
    let (family, family_binary_hash) =
        family_config.create_provider_with_identity(prepared.reader.clone())?;
    let family_provider_id = family.descriptor().provider_id.clone();
    let mut runtime = AstraEmuRuntimeProvider::new(family)?;
    runtime.create_instance(ProviderInstanceId("astra.emu.cli.headless.instance".into()))?;
    let probe = probe_profile(
        &runtime,
        &prepared,
        ProbeProfileRequest {
            mount_set_id: &mount_set_id,
            package_hash: game_identity_hash,
            target: "headless-test",
            media_service_id: "astra.platform.headless.media",
            report_sink_id: "astra.emu.cli.headless.report",
            stage_size: (launch.viewport_width, launch.viewport_height),
        },
    )?;
    let stage_width = probe
        .runtime
        .family_options
        .get("astra.stage_width")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_PROBE_STAGE_INVALID".to_owned())?;
    let stage_height = probe
        .runtime
        .family_options
        .get("astra.stage_height")
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "ASTRA_EMU_PROBE_STAGE_INVALID".to_owned())?;
    let entry_identity_hash = Hash256::from_sha256(prepared.entry_uri.as_bytes());
    let section = case_profile_section(
        &prepared,
        &probe.runtime,
        &mount_set_id,
        probe.content_identity,
    )?;
    let seed = u64::from_le_bytes(game_identity_hash.as_bytes()[..8].try_into().unwrap());
    let open = runtime.open(RuntimeOpenRequest {
        target_id: "astra-emu-headless-case".into(),
        profile: format!("{}-v1", launch.family_id),
        locale: "und".into(),
        seed,
        integrity_mode: RuntimeTickIntegrityMode::Evidence,
        executor: astra_plugin_abi::RuntimeExecutorConfig::serial(),
        package_hash: game_identity_hash.to_string(),
        sections: vec![section],
    })?;
    let resume_identity = HeadlessResumeIdentity {
        build_identity_hash,
        family_provider_id: &family_provider_id,
        family_binary_hash,
        game_identity_hash,
        entry_identity_hash,
        fixed_delta_ns: probe.runtime.fixed_delta_ns,
        stage_width,
        stage_height,
        session_seed: seed,
    };
    let resume = launch
        .resume_snapshot
        .as_deref()
        .map(read_resume_snapshot)
        .transpose()?;
    let resumed_from_fixed_step = if let Some(snapshot) = &resume {
        validate_resume_snapshot(snapshot, &resume_identity)?;
        validate_resume_input_ticks(&input.messages, snapshot.fixed_step)?;
        let restored = runtime.restore(RuntimeRestoreRequest {
            session_id: open.session_id.clone(),
            sections: snapshot.runtime_sections.clone(),
        })?;
        if restored.restored_fixed_step != snapshot.fixed_step
            || restored.session_seed != snapshot.session_seed
            || restored.status != "restored"
            || !restored.diagnostics.is_empty()
        {
            return Err("ASTRA_EMU_HEADLESS_RESUME_RESTORE_IDENTITY".into());
        }
        Some(snapshot.fixed_step)
    } else {
        None
    };
    let session_id_hash = Hash256::from_sha256(open.session_id.0.as_bytes());
    let mut host_profile = HeadlessHostProfile::reference(
        "headless-test",
        "astra.emu.quick_case",
        build_identity_hash.to_string(),
        game_identity_hash.to_string(),
    );
    host_profile.id = "astra-emu-cli-headless".into();
    host_profile.product_profile = format!("{}-v1", launch.family_id);
    host_profile.viewport_width = launch.viewport_width;
    host_profile.viewport_height = launch.viewport_height;
    host_profile.tick_duration_ns = probe.runtime.fixed_delta_ns;
    host_profile.presentation_rate_hz = launch.presentation_rate_hz;
    host_profile.providers.product_adapter = "astra.emu".into();
    host_profile.providers.video_decode = launch.video_provider.clone();
    // FVP Headless executes the same retained semantic GPU scene path as the
    // native host. CPU rasterization remains available only to oracle tests.
    host_profile.providers.renderer = "wgpu_offscreen".into();
    host_profile.gpu_adapter = Some(GpuAdapterPolicy {
        backend: GpuBackendPolicy::Dx12,
        device_type: GpuDeviceTypePolicy::Integrated,
        require_timestamp_query: true,
        adapter_identity_hash: None,
    });
    host_profile.render_policy = HeadlessRenderPolicy::All;
    host_profile.readback_policy = HeadlessReadbackPolicy::CheckpointsOnly;
    host_profile.artifacts.namespace = input.session.clone();
    host_profile.artifacts.retention = parse_artifact_retention(&launch.artifact_retention)?;
    host_profile.artifacts.required_checkpoints = input
        .messages
        .iter()
        .filter_map(|message| match &message.event {
            PhysicalInput::Checkpoint { id } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let frame_budget = input.final_tick.saturating_add(100).max(1);
    let presentation_substeps = u64::from(launch.presentation_rate_hz / 60);
    host_profile.artifacts.max_submitted_frames =
        frame_budget
            .checked_mul(presentation_substeps)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_PRESENTATION_FRAME_BUDGET_OVERFLOW".to_owned())?;
    // Checkpoint readback remains tied to Runtime ticks, not presentation
    // substeps, so it keeps the original bounded storage budget.
    host_profile.artifacts.max_rasterized_frames = frame_budget;
    host_profile.artifacts.max_duration_ns = input
        .final_tick
        .saturating_add(100)
        .saturating_mul(probe.runtime.fixed_delta_ns);
    host_profile.input.max_messages = input.messages.len() as u64;
    host_profile.input.max_tick = input.final_tick;
    let artifact_policy = host_profile.artifacts.clone();
    let profile_hash: Hash256 = host_profile
        .hash()
        .map_err(|error| error.to_string())?
        .parse()
        .map_err(|_| "ASTRA_EMU_HEADLESS_PROFILE_HASH".to_owned())?;
    let performance_profile_hash: Hash256 = host_profile
        .performance_policy_hash()
        .map_err(|error| error.to_string())?
        .parse()
        .map_err(|_| "ASTRA_EMU_HEADLESS_PERFORMANCE_PROFILE_HASH".to_owned())?;
    let performance_memory_baseline = launch
        .performance
        .as_ref()
        .map(|_| sample_process_memory().map_err(|error| error.to_string()))
        .transpose()?;
    let gpu_observer = launch.performance.as_ref().map(|_| {
        Arc::new(EmuHeadlessGpuObserver::new(
            PERFORMANCE_WARMUP_PRESENTATIONS + PERFORMANCE_MEASURED_PRESENTATIONS,
        ))
    });
    let mut host_factory = HeadlessPlatformFactory::new(&launch.artifact_root, &game_root)
        .with_input_sequence_hash(input.hash.to_string())
        .with_gpu(true);
    if let Some(observer) = &gpu_observer {
        host_factory = host_factory.with_performance_observer(observer.clone());
    }
    let host = host_factory
        .start(host_profile.clone().into())
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
    let execution_result = execute_sequence(
        &mut runtime,
        open.session_id.clone(),
        &host.client,
        surface,
        &input.messages,
        ExecutionConfig {
            seed,
            delta_ns: probe.runtime.fixed_delta_ns,
            verify_snapshot: launch.verify_snapshot,
            text: TextProviderBinding {
                provider_id: &host_profile.providers.text,
                target: &host_profile.target,
                profile: &host_profile.product_profile,
            },
            resume_driver: resume.as_ref().map(|snapshot| snapshot.driver.clone()),
            export_snapshot: launch.snapshot_output.is_some(),
            frame_sample_interval: launch.frame_sample_interval,
            presentation: PresentationPath::NativeGpu,
            presentation_substeps: (launch.presentation_rate_hz / 60) as u8,
            synchronous_gpu_presents: launch.presentation_rate_hz == 120,
            perfetto_trace: launch.perfetto_trace.clone(),
            capture_performance_samples: launch.performance.is_some(),
        },
    )
    .await;
    let result = execution_result.and_then(|execution| {
        let access = prepared.evidence.access_metrics()?;
        let audit = launch
            .audit_all_resources
            .then(|| prepared.evidence.audit())
            .transpose()?;
        Ok((execution, access, audit))
    });
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
    prepared.evidence.cleanup();
    let (mut execution, vfs_access, resource_audit) = match (result, cleanup) {
        (Ok(evidence), Ok(())) => evidence,
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(cleanup)) => return Err(cleanup),
        (Err(error), Err(cleanup)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_RUN_AND_CLEANUP_FAILED:{error};{cleanup}"
            ))
        }
    };
    if let Some(observer) = gpu_observer {
        execution.gpu_samples = observer.finish()?;
    }
    if let Some(output) = &launch.snapshot_output {
        let exported = execution
            .resume_snapshot
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_RESUME_EXPORT_MISSING".to_owned())?;
        let snapshot = HeadlessResumeSnapshotV1 {
            schema: HEADLESS_RESUME_SNAPSHOT_SCHEMA.into(),
            build_identity_hash,
            family_provider_id: family_provider_id.clone(),
            family_binary_hash,
            game_identity_hash,
            entry_identity_hash,
            fixed_delta_ns: probe.runtime.fixed_delta_ns,
            stage_width,
            stage_height,
            fixed_step: exported.driver.fixed_step,
            session_seed: seed,
            runtime_sections: exported.runtime_sections.clone(),
            driver: exported.driver.clone(),
        };
        validate_resume_snapshot(&snapshot, &resume_identity)?;
        let bytes = postcard::to_allocvec(&snapshot)
            .map_err(|_| "ASTRA_EMU_HEADLESS_RESUME_ENCODE".to_owned())?;
        if bytes.len() as u64 > MAX_RESUME_SNAPSHOT_BYTES {
            return Err("ASTRA_EMU_HEADLESS_RESUME_BOUNDS".into());
        }
        write_atomic_bytes(output, &bytes)?;
    }
    let manifest_path = launch.artifact_root.join("artifact-manifest.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|_| "ASTRA_EMU_HEADLESS_ARTIFACT_MANIFEST_READ".to_owned())?;
    let mut manifest: ArtifactManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "ASTRA_EMU_HEADLESS_ARTIFACT_MANIFEST_PARSE".to_owned())?;
    if artifact_policy.retention != HeadlessArtifactRetention::ManifestOnly {
        persist_checkpoint_frames(
            &launch.artifact_root,
            &execution.checkpoint_frames,
            &mut manifest,
            &artifact_policy,
        )?;
        write_atomic_json(&manifest_path, &manifest)?;
    }
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|_| "ASTRA_EMU_HEADLESS_ARTIFACT_MANIFEST_READ".to_owned())?;
    manifest
        .validate()
        .map_err(|_| "ASTRA_EMU_HEADLESS_ARTIFACT_MANIFEST_INVALID".to_owned())?;
    if manifest.package_hash != game_identity_hash.to_string()
        || manifest.input_sequence_hash != input.hash.to_string()
    {
        return Err("ASTRA_EMU_HEADLESS_ARTIFACT_IDENTITY".into());
    }
    let artifact_manifest_hash = Hash256::from_sha256(&manifest_bytes);
    let standard_report = standard_headless_run_report(
        &host_profile,
        &manifest,
        artifact_manifest_hash,
        &input,
        &execution,
    )?;
    write_atomic_json(
        &launch.artifact_root.join("run-report.json"),
        &standard_report,
    )?;
    let performance = match (&launch.performance, performance_memory_baseline) {
        (Some(artifacts), Some(_)) => Some(finalize_headless_performance(
            HeadlessPerformanceFinalize {
                artifacts,
                launch: &launch,
                host_profile: &host_profile,
                profile_hash: performance_profile_hash,
                build_identity_hash,
                game_identity_hash,
                family_binary_hash,
                session_id: &open.session_id,
                execution: &execution,
                memory_baseline: execution
                    .performance_memory_after_warmup
                    .ok_or("ASTRA_EMU_PERFORMANCE_WARMUP_MEMORY_MISSING")?,
                memory_final: sample_process_memory().map_err(|error| error.to_string())?,
            },
        )?),
        (None, None) => None,
        _ => return Err("ASTRA_EMU_PERFORMANCE_MEMORY_BASELINE_MISMATCH".into()),
    };
    let status = if execution.diagnostics.is_empty() {
        "passed"
    } else {
        "blocked"
    };
    let report = HeadlessRunReportV3 {
        schema: HEADLESS_RUN_REPORT_SCHEMA.into(),
        status: status.into(),
        family_id: launch.family_id.clone(),
        runtime_provider_id: "astra.emu.runtime_provider".into(),
        family_provider_id,
        host_kind: "headless".into(),
        build_identity_hash,
        profile_hash,
        game_identity_hash,
        entry_identity_hash,
        session_id_hash,
        input_sequence_hash: input.hash,
        consumed_input_trace_hash: Hash256::from_sha256(&execution.input_trace),
        visual_trace_hash: Hash256::from_sha256(&execution.visual_trace),
        audio_meter_hash: Hash256::from_sha256(&execution.audio_trace),
        runtime_state_trace_hash: Hash256::from_sha256(&execution.state_trace),
        artifact_manifest_hash,
        fixed_steps: execution.fixed_step,
        presented_frames: execution.present_sequence,
        frame_sample_interval: launch.frame_sample_interval,
        consumed_input_messages: input.messages.len() as u64,
        snapshot_round_trip_verified: execution.snapshot_verified,
        resumed_from_fixed_step,
        resume_snapshot_exported: launch.snapshot_output.is_some(),
        terminal_reached: execution.terminal,
        vfs_access,
        resource_audit,
        phase_timings: execution.phase_timings,
        performance_report_hash: performance.as_ref().map(|evidence| evidence.report_hash),
        performance_trace_manifest_hash: performance
            .as_ref()
            .map(|evidence| evidence.trace_manifest_hash),
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
            if resumed_from_fixed_step.is_some() {
                steps.push("session.resume".into());
            }
            if launch.snapshot_output.is_some() {
                steps.push("session.resume_snapshot_export".into());
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

fn standard_headless_run_report(
    profile: &HeadlessHostProfile,
    manifest: &ArtifactManifest,
    manifest_hash: Hash256,
    input: &ValidatedInputSequence,
    execution: &ExecutionEvidence,
) -> Result<RunReport, String> {
    let diagnostics = execution
        .diagnostics
        .iter()
        .map(|code| Diagnostic {
            code: code.clone(),
            operation: "astra.emu.runtime".into(),
            message: "family runtime emitted a blocking diagnostic".into(),
        })
        .collect::<Vec<_>>();
    let report = RunReport {
        schema: STANDARD_HEADLESS_RUN_REPORT_SCHEMA.into(),
        run_id: manifest.run_id.clone(),
        build_fingerprint: manifest.build_fingerprint.clone(),
        package_hash: manifest.package_hash.clone(),
        input_sequence_hash: manifest.input_sequence_hash.clone(),
        checkpoint_config_hash: Hash256::from_sha256(&[]).to_string(),
        profile_id: profile.id.clone(),
        session_id: input.session.clone(),
        scenario: "default".into(),
        target: profile.target.clone(),
        content_identity: profile.package_id.clone(),
        status: if diagnostics.is_empty() {
            RunStatus::Passed
        } else {
            RunStatus::Blocked
        },
        manifest_hash: manifest_hash.to_string(),
        renderer_identity_hash: manifest.renderer_identity_hash.clone(),
        render_policy: manifest.render_policy.clone(),
        submitted_frame_count: manifest.submitted_frame_count,
        rasterized_frame_count: manifest.rasterized_frame_count,
        submitted_scene_stream_hash: manifest.submitted_scene_stream_hash.clone(),
        rasterized_frame_stream_hash: manifest.rasterized_frame_stream_hash.clone(),
        audio_frame_count: manifest.audio_frame_count,
        duration_ns: input
            .final_tick
            .checked_mul(profile.tick_duration_ns)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_DURATION_OVERFLOW".to_owned())?,
        completed_sequence: input
            .messages
            .last()
            .map(|message| message.sequence)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_INPUT_EMPTY".to_owned())?,
        checkpoint_results: execution
            .checkpoints
            .iter()
            .map(|checkpoint| CheckpointResult {
                id: checkpoint.checkpoint_id.clone(),
                passed: true,
                observation_hash: checkpoint.observation_hash.to_string(),
                image_metrics: None,
                audio_metrics: None,
            })
            .collect(),
        diagnostics,
    };
    report
        .validate()
        .map_err(|_| "ASTRA_EMU_HEADLESS_STANDARD_REPORT_INVALID".to_owned())?;
    Ok(report)
}

fn validate_launch(launch: &HeadlessLaunch) -> Result<(), String> {
    if !(320..=8192).contains(&launch.viewport_width)
        || !(240..=8192).contains(&launch.viewport_height)
        || !matches!(launch.video_provider.as_str(), "disabled" | "ffmpeg-vcpkg")
        || parse_artifact_retention(&launch.artifact_retention).is_err()
        || !(1..=10_000).contains(&launch.frame_sample_interval)
        || !matches!(launch.presentation_rate_hz, 60 | 120)
    {
        return Err("ASTRA_EMU_HEADLESS_PROFILE_INVALID".into());
    }
    if launch.artifact_root.exists() {
        return Err("ASTRA_EMU_HEADLESS_ARTIFACT_ROOT_EXISTS".into());
    }
    if let Some(output) = &launch.snapshot_output {
        if output.exists()
            || output.parent().is_none_or(|parent| !parent.is_dir())
            || launch.resume_snapshot.as_ref() == Some(output)
        {
            return Err("ASTRA_EMU_HEADLESS_RESUME_OUTPUT_INVALID".into());
        }
    }
    if launch.frame_sample_interval != 1
        && (launch.resume_snapshot.is_some() || launch.snapshot_output.is_some())
    {
        return Err("ASTRA_EMU_HEADLESS_SAMPLED_RESUME_UNSUPPORTED".into());
    }
    if let Some(performance) = &launch.performance {
        if launch.frame_sample_interval != 1
            || launch.presentation_rate_hz != 120
            || launch.perfetto_trace.is_none()
            || launch.resume_snapshot.is_some()
            || launch.snapshot_output.is_some()
            || performance.budget_path == performance.report_path
            || performance.budget_path == performance.trace_manifest_path
            || performance.report_path == performance.trace_manifest_path
            || performance.warmup_presentations != PERFORMANCE_WARMUP_PRESENTATIONS as u64
            || performance.report_path.exists()
            || performance.trace_manifest_path.exists()
        {
            return Err("ASTRA_EMU_PERFORMANCE_PROFILE_INVALID".into());
        }
        if cfg!(debug_assertions) || option_env!("ASTRA_EMU_CLI_SOURCE_DIRTY") == Some("1") {
            return Err("ASTRA_EMU_PERFORMANCE_IDENTITY_DIRTY_OR_DEBUG".into());
        }
    }
    Ok(())
}

const PERFORMANCE_WARMUP_PRESENTATIONS: usize = 1_200;
const PERFORMANCE_MEASURED_PRESENTATIONS: usize = 72_000;
const PERFORMANCE_RUNTIME_P99_NS: u64 = 16_666_667;
const PERFORMANCE_PRESENTATION_P99_NS: u64 = 8_333_333;

struct HeadlessPerformanceEvidence {
    report_hash: Hash256,
    trace_manifest_hash: Hash256,
}

struct HeadlessPerformanceFinalize<'a> {
    artifacts: &'a HeadlessPerformanceArtifacts,
    launch: &'a HeadlessLaunch,
    host_profile: &'a HeadlessHostProfile,
    profile_hash: Hash256,
    build_identity_hash: Hash256,
    game_identity_hash: Hash256,
    family_binary_hash: Hash256,
    session_id: &'a GameRuntimeSessionId,
    execution: &'a ExecutionEvidence,
    memory_baseline: astra_observability::ProcessMemorySample,
    memory_final: astra_observability::ProcessMemorySample,
}

fn finalize_headless_performance(
    finalize: HeadlessPerformanceFinalize<'_>,
) -> Result<HeadlessPerformanceEvidence, String> {
    let HeadlessPerformanceFinalize {
        artifacts,
        launch,
        host_profile,
        profile_hash,
        build_identity_hash,
        game_identity_hash,
        family_binary_hash,
        session_id,
        execution,
        memory_baseline,
        memory_final,
    } = finalize;
    if execution.present_sequence
        != (PERFORMANCE_WARMUP_PRESENTATIONS + PERFORMANCE_MEASURED_PRESENTATIONS) as u64
        || execution.runtime_samples_ns.len() != execution.fixed_step as usize
        || execution.presentation_samples_ns.len() != execution.present_sequence as usize
        || execution.gpu_samples.len() != execution.present_sequence as usize
    {
        return Err("ASTRA_EMU_PERFORMANCE_SAMPLE_CADENCE_INVALID".into());
    }
    let expected_runtime_warmup = PERFORMANCE_WARMUP_PRESENTATIONS / 2;
    let runtime_samples = execution
        .runtime_samples_ns
        .get(expected_runtime_warmup..)
        .ok_or("ASTRA_EMU_PERFORMANCE_RUNTIME_WARMUP_INVALID")?;
    let gpu_samples = execution
        .gpu_samples
        .get(PERFORMANCE_WARMUP_PRESENTATIONS..)
        .ok_or("ASTRA_EMU_PERFORMANCE_GPU_WARMUP_INVALID")?;
    if runtime_samples.len() != PERFORMANCE_MEASURED_PRESENTATIONS / 2
        || gpu_samples.len() != PERFORMANCE_MEASURED_PRESENTATIONS
    {
        return Err("ASTRA_EMU_PERFORMANCE_MEASUREMENT_COUNT_INVALID".into());
    }
    let presentation_samples = gpu_samples
        .iter()
        .map(|sample| {
            sample
                .scene_build_ns
                .checked_add(sample.cpu_submit_ns)
                .and_then(|value| value.checked_add(sample.gpu_duration_ns))
                .ok_or("ASTRA_EMU_PERFORMANCE_PRESENTATION_DURATION_OVERFLOW")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let allocation_bytes = gpu_samples
        .iter()
        .map(|sample| sample.heap_allocation_bytes)
        .collect::<Vec<_>>();
    let allocation_count = gpu_samples
        .iter()
        .map(|sample| sample.heap_allocation_count)
        .collect::<Vec<_>>();
    let upload_bytes = gpu_samples
        .iter()
        .map(|sample| sample.upload_bytes)
        .collect::<Vec<_>>();
    let readback_bytes = gpu_samples
        .iter()
        .map(|sample| sample.readback_bytes)
        .collect::<Vec<_>>();
    let budget: PerformanceBudget = serde_json::from_slice(
        &fs::read(&artifacts.budget_path)
            .map_err(|_| "ASTRA_EMU_PERFORMANCE_BUDGET_READ".to_owned())?,
    )
    .map_err(|_| "ASTRA_EMU_PERFORMANCE_BUDGET_PARSE".to_owned())?;
    validate_fvp_performance_budget(&budget, host_profile, profile_hash)?;
    let source_revision = option_env!("ASTRA_EMU_CLI_SOURCE_REVISION")
        .ok_or("ASTRA_EMU_PERFORMANCE_SOURCE_REVISION_MISSING")?;
    let identity = PerformanceRunIdentity {
        source_revision: source_revision.into(),
        dirty: false,
        target: host_profile.target.clone(),
        profile: host_profile.product_profile.clone(),
        profile_hash: profile_hash.to_string(),
        package_hash: game_identity_hash.to_string(),
        build_fingerprint: build_identity_hash.to_string(),
        session_id: format!(
            "headless-{}",
            &Hash256::from_sha256(session_id.0.as_bytes()).to_string()[7..23]
        ),
    };
    let mut recorder = PerformanceRecorder::new(budget).map_err(|error| error.to_string())?;
    record_performance_samples(&mut recorder, "runtime.fixed_tick_ns", runtime_samples)?;
    record_performance_samples(&mut recorder, "presentation.e2e_ns", &presentation_samples)?;
    record_performance_samples(&mut recorder, "gpu.upload_bytes", &upload_bytes)?;
    record_performance_samples(&mut recorder, "heap.allocation_bytes", &allocation_bytes)?;
    record_performance_samples(&mut recorder, "heap.allocation_count", &allocation_count)?;
    record_performance_samples(&mut recorder, "gpu.readback_bytes", &readback_bytes)?;
    let deadline_miss_count = runtime_samples
        .iter()
        .filter(|sample| **sample > PERFORMANCE_RUNTIME_P99_NS)
        .count()
        .checked_add(
            presentation_samples
                .iter()
                .filter(|sample| **sample > PERFORMANCE_PRESENTATION_P99_NS)
                .count(),
        )
        .ok_or("ASTRA_EMU_PERFORMANCE_DEADLINE_OVERFLOW")? as u64;
    record_performance_samples(&mut recorder, "deadline.miss_count", &[deadline_miss_count])?;
    record_performance_samples(
        &mut recorder,
        "audio.underflow_count",
        &[execution.audio_underflow_count],
    )?;
    record_performance_samples(
        &mut recorder,
        "scene.full_resync_count",
        &[execution.scene_full_resync_count],
    )?;
    let trace = execution
        .perfetto_trace
        .as_ref()
        .ok_or("ASTRA_EMU_PERFORMANCE_TRACE_MISSING")?;
    record_performance_samples(
        &mut recorder,
        "trace.dropped_count",
        &[trace.dropped_event_count],
    )?;
    record_performance_samples(
        &mut recorder,
        "memory.working_set_bytes",
        &[memory_final.working_set_bytes],
    )?;
    record_performance_samples(
        &mut recorder,
        "memory.private_bytes",
        &[memory_final.private_bytes],
    )?;
    record_performance_samples(
        &mut recorder,
        "memory.growth_bytes",
        &[memory_final
            .private_bytes
            .saturating_sub(memory_baseline.private_bytes)],
    )?;
    let report = recorder
        .finalize(identity.clone(), 600_000_000)
        .map_err(|error| error.to_string())?;
    write_atomic_json(&artifacts.report_path, &report)?;
    let report_hash = Hash256::from_sha256(
        &fs::read(&artifacts.report_path)
            .map_err(|_| "ASTRA_EMU_PERFORMANCE_REPORT_READBACK".to_owned())?,
    );
    let adapter_identity_hash =
        Hash256::from_sha256(format!("{}\\0{}", launch.family_id, family_binary_hash).as_bytes());
    let driver_identity_hash = Hash256::from_sha256(
        format!(
            "semantic-gpu\\0{}\\0{}\\0{}",
            launch.presentation_rate_hz,
            launch.frame_sample_interval,
            host_profile.readback_policy as u8
        )
        .as_bytes(),
    );
    let manifest = PerformanceTraceManifest {
        schema: PERFORMANCE_TRACE_MANIFEST_SCHEMA.into(),
        identity,
        workload_id: "fvp.real_game.120hz".into(),
        adapter_identity_hash: adapter_identity_hash.to_string(),
        driver_identity_hash: driver_identity_hash.to_string(),
        report_hash: report_hash.to_string(),
        trace_hash: trace.trace_hash.to_string(),
        event_count: trace.event_count,
        dropped_event_count: trace.dropped_event_count,
        byte_length: trace.byte_length,
        truncated: trace.truncated,
        timestamps_monotonic: trace.timestamps_monotonic,
    };
    manifest.validate().map_err(|error| error.to_string())?;
    write_atomic_json(&artifacts.trace_manifest_path, &manifest)?;
    let trace_manifest_hash = Hash256::from_sha256(
        &fs::read(&artifacts.trace_manifest_path)
            .map_err(|_| "ASTRA_EMU_PERFORMANCE_MANIFEST_READBACK".to_owned())?,
    );
    if report.status != PerformanceStatus::Pass {
        return Err("ASTRA_EMU_PERFORMANCE_BUDGET_BLOCKED".into());
    }
    Ok(HeadlessPerformanceEvidence {
        report_hash,
        trace_manifest_hash,
    })
}

fn record_performance_samples(
    recorder: &mut PerformanceRecorder,
    metric_id: &str,
    samples: &[u64],
) -> Result<(), String> {
    for sample in samples {
        recorder
            .record(metric_id, *sample)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn validate_fvp_performance_budget(
    budget: &PerformanceBudget,
    profile: &HeadlessHostProfile,
    profile_hash: Hash256,
) -> Result<(), String> {
    budget.validate().map_err(|error| error.to_string())?;
    if budget.target != profile.target
        || budget.profile != profile.product_profile
        || budget.profile_hash != profile_hash.to_string()
        || budget.min_run_duration_us != 600_000_000
    {
        return Err(format!(
            "ASTRA_EMU_PERFORMANCE_BUDGET_IDENTITY:expected_policy_hash={profile_hash}"
        ));
    }
    let expected = [
        (
            "runtime.fixed_tick_ns",
            PerformanceUnit::Nanoseconds,
            PERFORMANCE_MEASURED_PRESENTATIONS / 2,
        ),
        (
            "presentation.e2e_ns",
            PerformanceUnit::Nanoseconds,
            PERFORMANCE_MEASURED_PRESENTATIONS,
        ),
        (
            "gpu.upload_bytes",
            PerformanceUnit::Bytes,
            PERFORMANCE_MEASURED_PRESENTATIONS,
        ),
        (
            "gpu.readback_bytes",
            PerformanceUnit::Bytes,
            PERFORMANCE_MEASURED_PRESENTATIONS,
        ),
        (
            "heap.allocation_bytes",
            PerformanceUnit::Bytes,
            PERFORMANCE_MEASURED_PRESENTATIONS,
        ),
        (
            "heap.allocation_count",
            PerformanceUnit::Count,
            PERFORMANCE_MEASURED_PRESENTATIONS,
        ),
        ("deadline.miss_count", PerformanceUnit::Count, 1),
        ("audio.underflow_count", PerformanceUnit::Count, 1),
        ("scene.full_resync_count", PerformanceUnit::Count, 1),
        ("trace.dropped_count", PerformanceUnit::Count, 1),
        ("memory.working_set_bytes", PerformanceUnit::Bytes, 1),
        ("memory.private_bytes", PerformanceUnit::Bytes, 1),
        ("memory.growth_bytes", PerformanceUnit::Bytes, 1),
    ];
    if budget.metrics.len() != expected.len() {
        return Err("ASTRA_EMU_PERFORMANCE_BUDGET_METRIC_SET".into());
    }
    for (id, unit, samples) in expected {
        let metric = budget
            .metrics
            .iter()
            .find(|metric| metric.id == id)
            .ok_or("ASTRA_EMU_PERFORMANCE_BUDGET_METRIC_SET")?;
        if metric.unit != unit || metric.min_samples != samples || metric.max_samples != samples {
            return Err("ASTRA_EMU_PERFORMANCE_BUDGET_SAMPLE_SET".into());
        }
    }
    require_max_p99(budget, "runtime.fixed_tick_ns", PERFORMANCE_RUNTIME_P99_NS)?;
    require_max_p99(
        budget,
        "presentation.e2e_ns",
        PERFORMANCE_PRESENTATION_P99_NS,
    )?;
    for id in [
        "deadline.miss_count",
        "audio.underflow_count",
        "scene.full_resync_count",
        "trace.dropped_count",
    ] {
        let metric = find_performance_metric(budget, id)?;
        if metric.thresholds.max != Some(0) {
            return Err("ASTRA_EMU_PERFORMANCE_BUDGET_ZERO_COUNTER".into());
        }
    }
    for id in [
        "gpu.upload_bytes",
        "gpu.readback_bytes",
        "heap.allocation_bytes",
        "heap.allocation_count",
    ] {
        let metric = find_performance_metric(budget, id)?;
        if metric.thresholds.max_p95 != Some(0) {
            return Err("ASTRA_EMU_PERFORMANCE_BUDGET_STABLE_ZERO".into());
        }
    }
    Ok(())
}

fn find_performance_metric<'a>(
    budget: &'a PerformanceBudget,
    id: &str,
) -> Result<&'a PerformanceMetricBudget, String> {
    budget
        .metrics
        .iter()
        .find(|metric| metric.id == id)
        .ok_or_else(|| "ASTRA_EMU_PERFORMANCE_BUDGET_METRIC_SET".to_owned())
}

fn require_max_p99(budget: &PerformanceBudget, id: &str, maximum: u64) -> Result<(), String> {
    if find_performance_metric(budget, id)?.thresholds.max_p99 != Some(maximum) {
        return Err("ASTRA_EMU_PERFORMANCE_BUDGET_P99".into());
    }
    Ok(())
}

fn read_resume_snapshot(path: &Path) -> Result<HeadlessResumeSnapshotV1, String> {
    let metadata = fs::metadata(path).map_err(|_| "ASTRA_EMU_HEADLESS_RESUME_READ")?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_RESUME_SNAPSHOT_BYTES {
        return Err("ASTRA_EMU_HEADLESS_RESUME_BOUNDS".into());
    }
    let bytes = fs::read(path).map_err(|_| "ASTRA_EMU_HEADLESS_RESUME_READ")?;
    postcard::from_bytes(&bytes).map_err(|_| "ASTRA_EMU_HEADLESS_RESUME_DECODE".into())
}

fn validate_resume_snapshot(
    snapshot: &HeadlessResumeSnapshotV1,
    expected: &HeadlessResumeIdentity<'_>,
) -> Result<(), String> {
    if snapshot.schema != HEADLESS_RESUME_SNAPSHOT_SCHEMA
        || snapshot.build_identity_hash != expected.build_identity_hash
        || snapshot.family_provider_id != expected.family_provider_id
        || snapshot.family_binary_hash != expected.family_binary_hash
        || snapshot.game_identity_hash != expected.game_identity_hash
        || snapshot.entry_identity_hash != expected.entry_identity_hash
        || snapshot.fixed_delta_ns != expected.fixed_delta_ns
        || snapshot.stage_width != expected.stage_width
        || snapshot.stage_height != expected.stage_height
        || snapshot.session_seed != expected.session_seed
        || snapshot.fixed_step != snapshot.driver.fixed_step
    {
        return Err("ASTRA_EMU_HEADLESS_RESUME_IDENTITY".into());
    }
    validate_runtime_sections(&snapshot.runtime_sections)?;
    validate_driver_resume(&snapshot.driver)
}

fn validate_runtime_save_sections(saved: &RuntimeSaveSections) -> Result<(), String> {
    if !saved.diagnostics.is_empty() {
        return Err("ASTRA_EMU_HEADLESS_RESUME_SAVE_DIAGNOSTIC".into());
    }
    validate_runtime_sections(&saved.sections)
}

fn validate_runtime_sections(sections: &[RuntimeSectionPayload]) -> Result<(), String> {
    if sections.is_empty() || sections.len() > 64 {
        return Err("ASTRA_EMU_HEADLESS_RESUME_SECTION_SET".into());
    }
    let mut ids = BTreeSet::new();
    let mut total = 0_u64;
    for section in sections {
        if section.section_id.is_empty()
            || section.schema.is_empty()
            || !section.validate_hash()
            || !ids.insert(section.section_id.as_str())
        {
            return Err("ASTRA_EMU_HEADLESS_RESUME_SECTION_INVALID".into());
        }
        total = total
            .checked_add(section.bytes.len() as u64)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_RESUME_BOUNDS".to_owned())?;
        if total > MAX_RESUME_SNAPSHOT_BYTES {
            return Err("ASTRA_EMU_HEADLESS_RESUME_BOUNDS".into());
        }
    }
    Ok(())
}

fn validate_driver_resume(driver: &HeadlessDriverResumeV1) -> Result<(), String> {
    if driver.pending_inputs.len() > 4096
        || driver.pending_waits.len() > 65_536
        || driver.completed_media.len() > 65_536
        || driver
            .pending_inputs
            .iter()
            .any(|edge| !edge.value.is_finite() || edge.sequence > driver.input_sequence)
        || driver.pending_waits.keys().any(|token| {
            token.is_empty()
                || token.len() > 128
                || !token.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')
                })
        })
        || driver
            .completed_media
            .iter()
            .any(|media| media.is_empty() || media.len() > 256)
        || driver.active_video.as_ref().is_some_and(|video| {
            video.playback_id.is_empty()
                || video.playback_id.len() > 256
                || video.resource_uri.is_empty()
                || video.resource_uri.len() > 4096
                || video.resource_uri.starts_with('/')
                || video.resource_uri.contains("..")
                || video.resource_uri.contains('\\')
                || video.resource_uri.contains(':')
                || video.stage_width == 0
                || video.stage_height == 0
                || video.started_step > driver.fixed_step
        })
    {
        return Err("ASTRA_EMU_HEADLESS_RESUME_DRIVER_STATE".into());
    }
    Ok(())
}

fn validate_resume_input_ticks(
    messages: &[InputMessage],
    restored_fixed_step: u64,
) -> Result<(), String> {
    if messages
        .iter()
        .any(|message| message.tick < restored_fixed_step)
    {
        return Err("ASTRA_EMU_HEADLESS_RESUME_INPUT_TICK".into());
    }
    Ok(())
}

fn parse_artifact_retention(value: &str) -> Result<HeadlessArtifactRetention, String> {
    match value {
        "all" => Ok(HeadlessArtifactRetention::All),
        "checkpoints" => Ok(HeadlessArtifactRetention::Checkpoints),
        "final" => Ok(HeadlessArtifactRetention::Final),
        "manifest-only" => Ok(HeadlessArtifactRetention::ManifestOnly),
        _ => Err("ASTRA_EMU_HEADLESS_ARTIFACT_RETENTION_INVALID".into()),
    }
}

fn elapsed_ns(started: Instant) -> Result<u64, String> {
    u64::try_from(started.elapsed().as_nanos())
        .map_err(|_| "ASTRA_EMU_HEADLESS_TIMING_OVERFLOW".to_owned())
}

fn elapsed_ns_since(origin: Instant, timestamp: Instant) -> Result<u64, String> {
    u64::try_from(timestamp.duration_since(origin).as_nanos())
        .map_err(|_| "ASTRA_EMU_NATIVE_PERFETTO_TIMING_OVERFLOW".to_owned())
}

fn duration_distribution(mut samples: Vec<u64>) -> HeadlessDurationDistributionV1 {
    if samples.is_empty() {
        return HeadlessDurationDistributionV1 {
            sample_count: 0,
            total_ns: 0,
            median_ns: 0,
            p95_ns: 0,
            p99_ns: 0,
            max_ns: 0,
        };
    }
    samples.sort_unstable();
    let sample_count = u64::try_from(samples.len()).unwrap_or(u64::MAX);
    let total_ns = samples
        .iter()
        .copied()
        .try_fold(0_u64, u64::checked_add)
        .unwrap_or(u64::MAX);
    let median_ns = samples[samples.len() / 2];
    let p95_index = samples
        .len()
        .saturating_mul(95)
        .div_ceil(100)
        .saturating_sub(1)
        .min(samples.len() - 1);
    let p99_index = samples
        .len()
        .saturating_mul(99)
        .div_ceil(100)
        .saturating_sub(1)
        .min(samples.len() - 1);
    HeadlessDurationDistributionV1 {
        sample_count,
        total_ns,
        median_ns,
        p95_ns: samples[p95_index],
        p99_ns: samples[p99_index],
        max_ns: *samples.last().expect("non-empty samples checked above"),
    }
}

fn persist_checkpoint_frames(
    root: &Path,
    frames: &[CheckpointFrame],
    manifest: &mut ArtifactManifest,
    policy: &HeadlessArtifactPolicy,
) -> Result<(), String> {
    let checkpoint_ids = frames
        .iter()
        .map(|frame| frame.id.as_str())
        .collect::<BTreeSet<_>>();
    if checkpoint_ids.len() != frames.len()
        || policy
            .required_checkpoints
            .iter()
            .any(|required| !checkpoint_ids.contains(required.as_str()))
    {
        return Err("ASTRA_EMU_HEADLESS_CHECKPOINT_SET_MISMATCH".into());
    }
    let mut total_bytes = manifest
        .artifacts
        .iter()
        .try_fold(0_u64, |total, artifact| {
            let byte_size = match artifact {
                ArtifactEntry::Frame { byte_size, .. } | ArtifactEntry::Audio { byte_size, .. } => {
                    *byte_size
                }
            };
            total.checked_add(byte_size)
        })
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_ARTIFACT_BYTES_OVERFLOW".to_owned())?;
    let next_artifact_count = (manifest.artifacts.len() as u64)
        .checked_add(frames.len() as u64)
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_ARTIFACT_COUNT_OVERFLOW".to_owned())?;
    if next_artifact_count > policy.max_artifacts {
        return Err("ASTRA_EMU_HEADLESS_ARTIFACT_COUNT_LIMIT".into());
    }
    let directory = root.join("checkpoints");
    fs::create_dir_all(&directory)
        .map_err(|_| "ASTRA_EMU_HEADLESS_CHECKPOINT_DIRECTORY".to_owned())?;
    for frame in frames {
        if frame.id.is_empty()
            || frame.id.len() > 128
            || !frame
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err("ASTRA_EMU_HEADLESS_CHECKPOINT_ID_INVALID".into());
        }
        let expected = usize::try_from(frame.width)
            .ok()
            .and_then(|width| {
                usize::try_from(frame.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_CHECKPOINT_BOUNDS".to_owned())?;
        if frame.rgba8.len() != expected {
            return Err("ASTRA_EMU_HEADLESS_CHECKPOINT_FRAME_LENGTH".into());
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(
                &frame.rgba8,
                frame.width,
                frame.height,
                ExtendedColorType::Rgba8,
            )
            .map_err(|_| "ASTRA_EMU_HEADLESS_CHECKPOINT_ENCODE".to_owned())?;
        total_bytes = total_bytes
            .checked_add(png.len() as u64)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_ARTIFACT_BYTES_OVERFLOW".to_owned())?;
        if total_bytes > policy.max_total_bytes {
            return Err("ASTRA_EMU_HEADLESS_ARTIFACT_BYTES_LIMIT".into());
        }
        let filename = format!("{}.png", frame.id);
        write_atomic_bytes(&directory.join(&filename), &png)?;
        manifest.artifacts.push(ArtifactEntry::Frame {
            relative_path: format!("checkpoints/{filename}"),
            sha256: Hash256::from_sha256(&png).to_string(),
            byte_size: png.len() as u64,
            width: frame.width,
            height: frame.height,
            color_space: "rgba8_srgb".into(),
            sequence: frame.sequence,
            checkpoint_ids: vec![frame.id.clone()],
        });
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

struct ProbeProfileRequest<'a> {
    mount_set_id: &'a str,
    package_hash: Hash256,
    target: &'a str,
    media_service_id: &'a str,
    report_sink_id: &'a str,
    stage_size: (u32, u32),
}

struct ProbeProfile {
    runtime: astra_emu_manager_core::CaseRuntimeProfileRecord,
    content_identity: Hash256,
}

#[cfg(test)]
fn fvp_probe_request(mount_set_id: &str, script_uri: &str) -> LegacyProbeRequest {
    LegacyProbeRequest {
        root_mount_id: mount_set_id.into(),
        candidate_uris: vec![script_uri.into()],
        marker_hashes: Vec::new(),
        max_entries: 1,
        max_metadata_bytes: 512 * 1024 * 1024,
    }
}

#[cfg(test)]
fn profile_from_probe_report(
    case: &CaseRecord,
    report: LegacyProbeReport,
) -> Result<ProbeProfile, String> {
    if report.family_id.0 != "fvp"
        || report.confidence_permyriad != 10_000
        || !report.blockers.is_empty()
    {
        return Err("ASTRA_EMU_FAMILY_PROBE_BLOCKED".into());
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
    let width = marker("fvp.stage_width.")?;
    let height = marker("fvp.stage_height.")?;
    Ok(ProbeProfile {
        runtime: astra_emu_manager_core::CaseRuntimeProfileRecord {
            case_identity: case.case_identity.clone(),
            family_id: "fvp".into(),
            fixed_delta_ns: FIXED_DELTA_NS,
            compatibility_profile: "rfvp-v1".into(),
            family_options: [
                ("fvp.nls".into(), nls),
                ("fvp.pack_paths".into(), "[]".into()),
                ("fvp.stage_width".into(), width.clone()),
                ("fvp.stage_height".into(), height.clone()),
                ("astra.stage_width".into(), width),
                ("astra.stage_height".into(), height),
                ("patch.mode".into(), "no_patch".into()),
            ]
            .into_iter()
            .collect(),
        },
        content_identity: report.content_identity,
    })
}

fn probe_profile(
    runtime: &AstraEmuRuntimeProvider,
    case: &PreparedFamilyCase,
    request: ProbeProfileRequest<'_>,
) -> Result<ProbeProfile, String> {
    let (requested_stage_width, requested_stage_height) = request.stage_size;
    let report = runtime.probe_family(
        &LegacyRuntimeHostCtx {
            case_id: case.case_identity.clone(),
            package_id: "astra-emu-headless-case".into(),
            package_hash: request.package_hash,
            mount_set_id: request.mount_set_id.into(),
            media_service_ids: vec![request.media_service_id.into()],
            permission_policy_id: "astra.emu.cli.explicit_directory.v1".into(),
            report_sink_id: request.report_sink_id.into(),
            target: request.target.into(),
            profile: format!("{}-v1", case.family_id),
        },
        LegacyProbeRequest {
            root_mount_id: request.mount_set_id.into(),
            candidate_uris: vec![case.entry_uri.clone()],
            // Installation identity belongs to the host. The family returns the
            // bounded entry/script identity used by the runtime profile.
            marker_hashes: Vec::new(),
            max_entries: 1,
            max_metadata_bytes: 512 * 1024 * 1024,
        },
    )?;
    if report.family_id.0 != case.family_id
        || report.confidence_permyriad != 10_000
        || !report.blockers.is_empty()
    {
        return Err("ASTRA_EMU_FAMILY_PROBE_BLOCKED".into());
    }
    if case.family_id == "minori" {
        if requested_stage_width == 0 || requested_stage_height == 0 {
            return Err("ASTRA_EMU_MINORI_PROBE_STAGE_INVALID".into());
        }
        return Ok(ProbeProfile {
            runtime: astra_emu_manager_core::CaseRuntimeProfileRecord {
                case_identity: case.case_identity.clone(),
                family_id: case.family_id.clone(),
                fixed_delta_ns: FIXED_DELTA_NS,
                compatibility_profile: "minori.reference".into(),
                family_options: [
                    (
                        "astra.stage_width".into(),
                        requested_stage_width.to_string(),
                    ),
                    (
                        "astra.stage_height".into(),
                        requested_stage_height.to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            },
            content_identity: report.content_identity,
        });
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
    let pack_paths = case
        .fvp_pack_paths
        .as_ref()
        .ok_or_else(|| "ASTRA_EMU_FVP_PACK_PATHS_MISSING".to_owned())?;
    let pack_paths = serde_json::to_string(pack_paths)
        .map_err(|_| "ASTRA_EMU_FVP_PACK_PATHS_ENCODE".to_owned())?;
    Ok(ProbeProfile {
        runtime: astra_emu_manager_core::CaseRuntimeProfileRecord {
            case_identity: case.case_identity.clone(),
            family_id: "fvp".into(),
            fixed_delta_ns: FIXED_DELTA_NS,
            compatibility_profile: "rfvp-v1".into(),
            family_options: [
                ("fvp.nls".into(), nls),
                ("fvp.pack_paths".into(), pack_paths),
                ("fvp.stage_width".into(), width.clone()),
                ("fvp.stage_height".into(), height.clone()),
                ("astra.stage_width".into(), width),
                ("astra.stage_height".into(), height),
                ("patch.mode".into(), "no_patch".into()),
            ]
            .into_iter()
            .collect(),
        },
        content_identity: report.content_identity,
    })
}

fn case_profile_section(
    case: &PreparedFamilyCase,
    profile: &astra_emu_manager_core::CaseRuntimeProfileRecord,
    mount_set_id: &str,
    case_fingerprint: Hash256,
) -> Result<RuntimeSectionPayload, String> {
    let value = EmuCaseProfile {
        schema: "astra.emu.case_profile.v1".into(),
        family_id: case.family_id.clone(),
        case_fingerprint,
        script_uri: case.entry_uri.clone(),
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
    checkpoint_frames: Vec<CheckpointFrame>,
    diagnostics: BTreeSet<String>,
    fixed_step: u64,
    present_sequence: u64,
    snapshot_verified: bool,
    terminal: bool,
    phase_timings: HeadlessPhaseTimingEvidenceV1,
    runtime_samples_ns: Vec<u64>,
    presentation_samples_ns: Vec<u64>,
    gpu_samples: Vec<HeadlessGpuFrameSample>,
    performance_memory_after_warmup: Option<astra_observability::ProcessMemorySample>,
    scene_full_resync_count: u64,
    audio_underflow_count: u64,
    perfetto_trace: Option<PerfettoTraceSummary>,
    resume_snapshot: Option<HeadlessResumeExport>,
}

/// Evidence-only receiver for the Headless GPU timestamp path.  The renderer
/// remains the owner of GPU timings and allocation counters; this observer
/// only transports its bounded, scalar samples back to the report writer.
/// It deliberately has no Astra runtime state and is never installed for a
/// Shipping run.
#[derive(Debug)]
struct EmuHeadlessGpuObserver {
    expected_samples: usize,
    samples: Mutex<Vec<HeadlessGpuFrameSample>>,
}

impl EmuHeadlessGpuObserver {
    fn new(expected_samples: usize) -> Self {
        Self {
            expected_samples,
            samples: Mutex::new(Vec::with_capacity(expected_samples)),
        }
    }

    fn finish(&self) -> Result<Vec<HeadlessGpuFrameSample>, String> {
        let mut samples = self
            .samples
            .lock()
            .map_err(|_| "ASTRA_EMU_PERFORMANCE_GPU_OBSERVER_POISONED".to_owned())?;
        if samples.len() != self.expected_samples {
            return Err(format!(
                "ASTRA_EMU_PERFORMANCE_GPU_SAMPLE_CADENCE_INVALID:{}/{}",
                samples.len(),
                self.expected_samples
            ));
        }
        Ok(std::mem::take(&mut *samples))
    }
}

impl HeadlessPerformanceObserver for EmuHeadlessGpuObserver {
    fn pace_gpu_frame(&self, _sequence: u64) -> Result<(), astra_platform::PlatformError> {
        // The RuntimeDriver owns the fixed 60 Hz / presentation 120 Hz cadence.
        // Sleeping here would alter the workload being measured.
        Ok(())
    }

    fn bind_gpu_frame(&self, sequence: u64) -> Result<Option<u64>, astra_platform::PlatformError> {
        Ok(Some(sequence))
    }

    fn record_gpu_frame(
        &self,
        sample: HeadlessGpuFrameSample,
    ) -> Result<(), astra_platform::PlatformError> {
        let mut samples = self.samples.lock().map_err(|_| {
            astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "headless.performance.observer",
                "GPU observer lock is poisoned",
            )
        })?;
        if samples.len() >= self.expected_samples {
            return Err(astra_platform::PlatformError::new(
                astra_platform::PlatformErrorCode::InvalidState,
                "headless.performance.observer",
                "GPU sample capacity exceeded",
            ));
        }
        samples.push(sample);
        Ok(())
    }
}

struct HeadlessResumeExport {
    runtime_sections: Vec<RuntimeSectionPayload>,
    driver: HeadlessDriverResumeV1,
}

struct CheckpointFrame {
    id: String,
    sequence: u64,
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum PendingWait {
    DueStep(u64),
    Input(Vec<String>),
    Presentation,
    Media(String),
    Unsupported,
}

struct ActiveVideo {
    playback_id: String,
    resource_uri: String,
    mode: LegacyVideoMode,
    stage_width: u32,
    stage_height: u32,
    started_step: u64,
    stream: DecodedVideoStream,
    audio_stream_id: Option<u32>,
}

/// Native-only bridge from the bounded family scene contract to the shared
/// platform GPU scene.  It retains texture bytes solely to apply validated
/// subresource updates; it never rasterizes a framebuffer on the CPU.
#[derive(Default)]
struct GpuSceneAdapter {
    resources: astra_emu_family_api::LegacySceneResourceStateV1,
    textures: BTreeMap<u32, GpuSceneTexture>,
    generation: u64,
    width: u32,
    height: u32,
    draws: Vec<LegacyDrawV1>,
}

#[derive(Clone)]
struct GpuSceneTexture {
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    resource_id: String,
}

/// Per-transaction semantic resource accounting.  Values are recorded only
/// after validation and local transaction preparation succeeds, which keeps
/// Perfetto counters aligned with state that may be submitted to the platform.
#[derive(Clone, Copy, Default)]
struct GpuScenePrepareMetrics {
    resource_operations: u64,
    create_bytes: u64,
    update_bytes: u64,
    draw_count: u64,
    live_textures: u64,
    generation: u64,
}

impl GpuScenePrepareMetrics {
    fn accumulate(&mut self, next: Self) -> Result<(), String> {
        self.resource_operations = self
            .resource_operations
            .checked_add(next.resource_operations)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_RESOURCE_OPERATION_OVERFLOW".to_owned())?;
        self.create_bytes = self
            .create_bytes
            .checked_add(next.create_bytes)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_UPLOAD_BYTES_OVERFLOW".to_owned())?;
        self.update_bytes = self
            .update_bytes
            .checked_add(next.update_bytes)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_UPLOAD_BYTES_OVERFLOW".to_owned())?;
        self.draw_count = next.draw_count;
        self.live_textures = next.live_textures;
        self.generation = next.generation;
        Ok(())
    }
}

impl GpuSceneAdapter {
    fn prepare(
        &mut self,
        commit: LegacyPreparedSceneCommitV1,
    ) -> Result<(SceneFrame, GpuScenePrepareMetrics), String> {
        let LegacyPreparedSceneCommitV1 {
            packet,
            next_resources,
            reset_resources,
        } = commit;
        let prior_resources = if reset_resources {
            astra_emu_family_api::LegacySceneResourceStateV1::default()
        } else {
            self.resources.clone()
        };
        let verified = prior_resources
            .validate(&packet)
            .map_err(|error| format!("ASTRA_EMU_NATIVE_GPU_SCENE_PREPARE:{}", error.code()))?;
        if verified != next_resources {
            return Err("ASTRA_EMU_NATIVE_GPU_SCENE_COMMIT_MISMATCH".into());
        }

        let mut textures = if reset_resources {
            BTreeMap::new()
        } else {
            self.textures.clone()
        };
        let mut generation = self.generation;
        let mut metrics = GpuScenePrepareMetrics {
            resource_operations: packet.resources.len() as u64,
            draw_count: packet.draws.len() as u64,
            ..GpuScenePrepareMetrics::default()
        };
        let mut commands = Vec::with_capacity(
            packet.resources.len().saturating_mul(2)
                + packet.draws.len().saturating_mul(3)
                + self.textures.len(),
        );
        if reset_resources {
            for texture in self.textures.values() {
                commands.push(SceneCommand::ReleaseResource {
                    resource_id: texture.resource_id.clone(),
                });
            }
        }
        for operation in packet.resources {
            match operation {
                LegacySceneResourceOperationV1::CreateTexture(texture) => {
                    metrics.create_bytes = metrics
                        .create_bytes
                        .checked_add(texture.pixels.len() as u64)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_UPLOAD_BYTES_OVERFLOW".to_owned())?;
                    generation = generation.checked_add(1).ok_or_else(|| {
                        "ASTRA_EMU_NATIVE_GPU_RESOURCE_GENERATION_OVERFLOW".to_owned()
                    })?;
                    let rgba8: Arc<[u8]> = Arc::from(gpu_rgba8_owned(
                        texture.width,
                        texture.height,
                        texture.format,
                        texture.pixels,
                    )?);
                    let resource_id = gpu_resource_id(texture.texture_id, generation);
                    commands.push(SceneCommand::UploadTexture {
                        resource_id: resource_id.clone(),
                        frame: TextureFrame::from_rgba8(
                            texture.width,
                            texture.height,
                            Arc::clone(&rgba8),
                        )
                        .map_err(|error| error.to_string())?,
                    });
                    textures.insert(
                        texture.texture_id,
                        GpuSceneTexture {
                            width: texture.width,
                            height: texture.height,
                            format: texture.format,
                            resource_id,
                        },
                    );
                }
                LegacySceneResourceOperationV1::UpdateTexture(update) => {
                    metrics.update_bytes = metrics
                        .update_bytes
                        .checked_add(update.pixels.len() as u64)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_UPLOAD_BYTES_OVERFLOW".to_owned())?;
                    let old = textures
                        .get(&update.texture_id)
                        .cloned()
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_TEXTURE_MISSING".to_owned())?;
                    if old.format != update.format
                        || update
                            .x
                            .checked_add(update.width)
                            .is_none_or(|right| right > old.width)
                        || update
                            .y
                            .checked_add(update.height)
                            .is_none_or(|bottom| bottom > old.height)
                    {
                        return Err("ASTRA_EMU_NATIVE_GPU_TEXTURE_REGION".into());
                    }
                    let rgba8: Arc<[u8]> = Arc::from(gpu_rgba8_owned(
                        update.width,
                        update.height,
                        update.format,
                        update.pixels,
                    )?);
                    commands.push(SceneCommand::UpdateTextureRegion {
                        resource_id: old.resource_id.clone(),
                        x: update.x,
                        y: update.y,
                        width: update.width,
                        height: update.height,
                        hash: Hash256::from_sha256(&rgba8),
                        rgba8,
                    });
                    textures.insert(
                        update.texture_id,
                        GpuSceneTexture {
                            width: old.width,
                            height: old.height,
                            format: old.format,
                            resource_id: old.resource_id,
                        },
                    );
                }
                LegacySceneResourceOperationV1::DestroyTexture { texture_id } => {
                    let texture = textures
                        .remove(&texture_id)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_TEXTURE_MISSING".to_owned())?;
                    commands.push(SceneCommand::ReleaseResource {
                        resource_id: texture.resource_id,
                    });
                }
            }
        }
        commands.extend(gpu_draw_commands(&textures, &packet.draws)?);
        self.resources = verified;
        self.textures = textures;
        self.generation = generation;
        self.width = packet.width;
        self.height = packet.height;
        self.draws = packet.draws;
        metrics.live_textures = self.textures.len() as u64;
        metrics.generation = self.generation;
        Ok((
            SceneFrame {
                sequence: 0,
                width: self.width,
                height: self.height,
                clear_rgba: [0, 0, 0, 255],
                commands,
                semantics: None,
            },
            metrics,
        ))
    }

    /// Replays the current retained draw state without re-uploading resources.
    /// This is used only for a later presentation substep after the resource
    /// transaction has completed successfully on the platform.
    fn draw_scene(&self) -> Result<SceneFrame, String> {
        if self.width == 0 || self.height == 0 {
            return Err("ASTRA_EMU_NATIVE_GPU_DRAW_BEFORE_SCENE".into());
        }
        Ok(SceneFrame {
            sequence: 0,
            width: self.width,
            height: self.height,
            clear_rgba: [0, 0, 0, 255],
            commands: gpu_draw_commands(&self.textures, &self.draws)?,
            semantics: None,
        })
    }
}

/// Combines two unsent semantic frames without duplicating the retained
/// texture table.  Resource commands remain ordered exactly as emitted by the
/// provider; only superseded draw-state commands are discarded.  The result
/// is submitted after any in-flight receipt, so each retained generation is
/// materialized before a later release or draw can reference it.
fn merge_scene_frames(mut queued: SceneFrame, latest: SceneFrame) -> Result<SceneFrame, String> {
    if latest.sequence != 0 || queued.sequence != 0 {
        return Err("ASTRA_EMU_NATIVE_GPU_SCENE_SEQUENCE_PREASSIGNED".into());
    }
    let mut commands = Vec::with_capacity(queued.commands.len() + latest.commands.len());
    commands.extend(queued.commands.drain(..).filter(is_scene_resource_command));
    commands.extend(latest.commands);
    queued.width = latest.width;
    queued.height = latest.height;
    queued.clear_rgba = latest.clear_rgba;
    queued.commands = commands;
    queued.semantics = latest.semantics;
    Ok(queued)
}

fn is_scene_resource_command(command: &SceneCommand) -> bool {
    matches!(
        command,
        SceneCommand::UploadTexture { .. }
            | SceneCommand::UpdateTextureRegion { .. }
            | SceneCommand::UploadGlyph { .. }
            | SceneCommand::ReleaseResource { .. }
    )
}

fn gpu_draw_commands(
    textures: &BTreeMap<u32, GpuSceneTexture>,
    draws: &[LegacyDrawV1],
) -> Result<Vec<SceneCommand>, String> {
    let mut commands = Vec::with_capacity(draws.len().saturating_mul(3));
    for (draw_index, draw) in draws.iter().enumerate() {
        if let Some(scissor) = draw.scissor {
            if scissor.x < 0 || scissor.y < 0 || scissor.width <= 0 || scissor.height <= 0 {
                return Err("ASTRA_EMU_NATIVE_GPU_SCISSOR_INVALID".into());
            }
            commands.push(SceneCommand::PushClip {
                rect: RectI::new(
                    scissor.x,
                    scissor.y,
                    scissor.width as u32,
                    scissor.height as u32,
                ),
            });
        }
        let (material, texture_id) = if draw.texture_id == u32::MAX {
            (MeshMaterial2D::Solid, None)
        } else {
            let texture = textures
                .get(&draw.texture_id)
                .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_TEXTURE_MISSING".to_owned())?;
            (
                MeshMaterial2D::ColorTexture,
                Some(texture.resource_id.clone()),
            )
        };
        let vertices = draw
            .vertices
            .map(gpu_vertex)
            .into_iter()
            .collect::<Vec<_>>();
        commands.push(SceneCommand::Mesh2D {
            id: format!("rfvp-draw-{draw_index}"),
            vertices: Arc::from(vertices),
            indices: Arc::from(vec![0, 1, 2, 2, 1, 3]),
            material,
            texture_id,
            opacity: 1.0,
            blend: match draw.blend {
                astra_emu_family_api::LegacyBlendMode::Alpha => BlendMode::Alpha,
                astra_emu_family_api::LegacyBlendMode::Add => BlendMode::Add,
                astra_emu_family_api::LegacyBlendMode::Multiply => BlendMode::Multiply,
            },
        });
        if draw.scissor.is_some() {
            commands.push(SceneCommand::PopClip);
        }
    }
    Ok(commands)
}

fn gpu_resource_id(texture_id: u32, generation: u64) -> String {
    format!("rfvp-texture-{texture_id}-{generation}")
}

fn gpu_rgba8_owned(
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    pixels: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let channels = match format {
        LegacyTextureFormat::Rgba8 => 4usize,
        LegacyTextureFormat::LumaAlpha8 => 2usize,
    };
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_TEXTURE_BOUNDS".to_owned())?;
    if pixels.len() != expected {
        return Err("ASTRA_EMU_NATIVE_GPU_TEXTURE_LENGTH".into());
    }
    Ok(match format {
        LegacyTextureFormat::Rgba8 => pixels,
        LegacyTextureFormat::LumaAlpha8 => pixels
            .chunks_exact(2)
            .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
            .collect(),
    })
}

fn gpu_vertex(vertex: astra_emu_family_api::LegacyVertexV1) -> MeshVertex2D {
    let alpha = (vertex.color[3].clamp(0.0, 1.0) * 255.0).round() as u8;
    let channel =
        |index: usize| (vertex.color[index].clamp(0.0, 1.0) * f32::from(alpha)).round() as u8;
    MeshVertex2D {
        position: vertex.position,
        uv: vertex.tex_coord,
        premultiplied_rgba: [channel(0), channel(1), channel(2), alpha],
    }
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
    gpu_scene: Option<GpuSceneAdapter>,
    pending_scene_metrics: Option<GpuScenePrepareMetrics>,
    pending_render_frame: Option<LegacyRenderFrameV1>,
    pending_scene_frame: Option<SceneFrame>,
    pending_scene_present: Option<ScenePresentReceipt>,
    queued_visual_hash: Option<Hash256>,
    visual_dirty: bool,
    image_decoders: DecodeProviderRegistry,
    text_presenter: BoundTextPresenter,
    underlay_frame: Option<(u32, u32, Vec<u8>)>,
    base_frame: Option<(u32, u32, Vec<u8>)>,
    latest_frame: Option<(u32, u32, Vec<u8>)>,
    present_sequence: u64,
    state_hash: Hash256,
    terminal: bool,
    audio: AudioExecutor,
    video: Option<ActiveVideo>,
    pending_video_restore: Option<HeadlessVideoResumeV1>,
    movie_audio_sequence: u32,
    completed_media: Vec<String>,
    input_trace: Vec<u8>,
    visual_trace: Vec<u8>,
    state_trace: Vec<u8>,
    diagnostics: BTreeSet<String>,
    active_touch: Option<u64>,
    audio_enabled: bool,
    audio_pump: AudioPumpPolicy,
    frame_sample_interval: u64,
    presentation_substeps: u8,
    synchronous_gpu_presents: bool,
    step_timings_ns: Vec<u64>,
    runtime_timings_ns: Vec<u64>,
    effect_timings_ns: Vec<u64>,
    raster_timings_ns: Vec<u64>,
    media_timings_ns: Vec<u64>,
    present_timings_ns: Vec<u64>,
    perfetto: Option<NativePerfettoCapture>,
    perfetto_rfvp_core: bool,
    capture_performance_samples: bool,
    performance_memory_after_warmup: Option<astra_observability::ProcessMemorySample>,
    scene_full_resync_count: u64,
}

#[derive(Clone, Copy)]
struct TextProviderBinding<'a> {
    provider_id: &'a str,
    target: &'a str,
    profile: &'a str,
}

struct RuntimeDriverConfig<'a> {
    seed: u64,
    delta_ns: u64,
    audio_enabled: bool,
    text: TextProviderBinding<'a>,
    resume: Option<HeadlessDriverResumeV1>,
    frame_sample_interval: u64,
    perfetto_trace: Option<PathBuf>,
    perfetto_rfvp_core: bool,
    capture_performance_samples: bool,
    presentation: PresentationPath,
    presentation_substeps: u8,
    synchronous_gpu_presents: bool,
    background_audio: bool,
    audio_pump: AudioPumpPolicy,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Kept for the explicit pixel-oracle test path, never performance mode.
enum PresentationPath {
    /// Deterministic CPU renderer used only by Headless evidence/capture.
    CpuReference,
    /// Retained semantic scene submitted to the platform WGPU compositor.
    NativeGpu,
}

#[derive(Clone, Copy)]
enum AudioPumpPolicy {
    FixedTick,
    Realtime {
        target_latency_ms: u32,
        refill_low_water_ms: u32,
        poll_interval_ticks: u8,
    },
}

struct NativePerfettoCapture {
    started: Instant,
    writer: PerfettoTraceWriter,
    recorded: u64,
}

impl NativePerfettoCapture {
    fn new(output_path: PathBuf) -> Result<Self, String> {
        Ok(Self {
            started: Instant::now(),
            writer: PerfettoTraceWriter::create(PerfettoTraceConfig::production(
                output_path,
                "astra-emu-cli-native",
            ))
            .map_err(|error| error.to_string())?,
            recorded: 0,
        })
    }

    fn record(
        &mut self,
        name: &'static str,
        track: u32,
        fixed_step: u64,
        started: Instant,
    ) -> Result<(), String> {
        self.writer
            .complete(
                perfetto_domain(name),
                name,
                track,
                Some(fixed_step),
                elapsed_ns_since(self.started, started)?,
                elapsed_ns(started)?,
            )
            .map_err(|error| error.to_string())?;
        self.recorded = self
            .recorded
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_PERFETTO_EVENT_OVERFLOW".to_owned())?;
        Ok(())
    }

    fn counter(&mut self, name: &'static str, value: u64) -> Result<(), String> {
        self.writer
            .counter(
                perfetto_domain(name),
                name,
                elapsed_ns(self.started)?,
                value,
            )
            .map_err(|error| error.to_string())?;
        self.recorded = self
            .recorded
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_PERFETTO_EVENT_OVERFLOW".to_owned())?;
        Ok(())
    }

    fn begin(
        &mut self,
        name: &'static str,
        track: u32,
        fixed_step: u64,
        started: Instant,
    ) -> Result<(), String> {
        self.writer
            .begin(
                perfetto_domain(name),
                name,
                track,
                Some(fixed_step),
                elapsed_ns_since(self.started, started)?,
            )
            .map_err(|error| error.to_string())?;
        self.recorded = self
            .recorded
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_PERFETTO_EVENT_OVERFLOW".to_owned())?;
        Ok(())
    }

    fn end(&mut self, name: &'static str, track: u32, fixed_step: u64) -> Result<(), String> {
        self.writer
            .end(
                perfetto_domain(name),
                name,
                track,
                Some(fixed_step),
                elapsed_ns(self.started)?,
            )
            .map_err(|error| error.to_string())?;
        self.recorded = self
            .recorded
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_PERFETTO_EVENT_OVERFLOW".to_owned())?;
        Ok(())
    }

    fn finish(self) -> Result<PerfettoTraceSummary, String> {
        if self.recorded == 0 {
            return Err("ASTRA_EMU_NATIVE_PERFETTO_NO_SAMPLES".into());
        }
        self.writer.finish().map_err(|error| error.to_string())
    }
}

fn perfetto_domain(name: &str) -> &'static str {
    if name.starts_with("rfvp.core.") {
        "rfvp.core"
    } else {
        "astra.emu.adapter"
    }
}

struct ExecutionConfig<'a> {
    seed: u64,
    delta_ns: u64,
    verify_snapshot: bool,
    text: TextProviderBinding<'a>,
    resume_driver: Option<HeadlessDriverResumeV1>,
    export_snapshot: bool,
    frame_sample_interval: u64,
    presentation: PresentationPath,
    presentation_substeps: u8,
    synchronous_gpu_presents: bool,
    perfetto_trace: Option<PathBuf>,
    capture_performance_samples: bool,
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
                driver.queue_input("pointer.x", true, stage_x)?;
                driver.queue_input("pointer.y", true, stage_y)?;
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
                    driver.queue_input("pointer.x", true, stage_x)?;
                    driver.queue_input("pointer.y", true, stage_y)?;
                    driver.queue_input("pointer.primary", true, 1.0)?;
                }
                PlatformTouchPhase::Moved if driver.active_touch == Some(id) => {
                    driver.queue_input("pointer.x", true, stage_x)?;
                    driver.queue_input("pointer.y", true, stage_y)?;
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
                PlatformGamepadControl::South => Some("enter"),
                PlatformGamepadControl::East => Some("escape"),
                PlatformGamepadControl::DpadUp => Some("arrow_up"),
                PlatformGamepadControl::DpadDown => Some("arrow_down"),
                PlatformGamepadControl::DpadLeft => Some("arrow_left"),
                PlatformGamepadControl::DpadRight => Some("arrow_right"),
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
        "enter" | "return" | "numpadenter" => Some("enter"),
        "escape" | "esc" => Some("escape"),
        "arrowup" | "up" => Some("arrow_up"),
        "arrowdown" | "down" => Some("arrow_down"),
        "arrowleft" | "left" => Some("arrow_left"),
        "arrowright" | "right" => Some("arrow_right"),
        " " | "space" | "spacebar" => Some("space"),
        "shift" | "shiftleft" | "shiftright" => Some("shift"),
        "control" | "ctrl" | "controlleft" | "controlright" => Some("control"),
        _ => None,
    }
}

/// Applies the portion of a validated physical input sequence that becomes due
/// at the driver's current fixed-step boundary. Native replay deliberately
/// shares the exact `consume_physical_input` mapper used by Headless: the host
/// does not synthesize legacy control names or bypass RuntimeDriver queues.
///
/// A native window cannot make deterministic Headless captures, so checkpoint
/// records remain observable trace markers rather than silently becoming a
/// second capture protocol. `AdvanceTicks` and `Await` are rejected because a
/// real-time native host must not skip simulation or poll internal state.
fn consume_native_inputs_due(
    driver: &mut RuntimeDriver<'_>,
    messages: &[InputMessage],
    cursor: &mut usize,
) -> Result<bool, String> {
    let mut shutdown_requested = false;
    while let Some(message) = messages.get(*cursor) {
        if message.tick > driver.fixed_step {
            break;
        }
        match &message.event {
            PhysicalInput::Shutdown => shutdown_requested = true,
            PhysicalInput::Checkpoint { id } => {
                tracing::debug!(
                    event = "astra_emu_native_input_checkpoint",
                    fixed_step = driver.fixed_step,
                    checkpoint_id = id.as_str(),
                    "native replay reached a declared checkpoint"
                );
            }
            PhysicalInput::AdvanceTicks { .. } | PhysicalInput::Await { .. } => {
                return Err("ASTRA_EMU_NATIVE_INPUT_CONTROL_UNSUPPORTED".into());
            }
            input => driver.consume_physical_input(input)?,
        }
        *cursor = cursor
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_INPUT_CURSOR_OVERFLOW".to_owned())?;
    }
    Ok(shutdown_requested)
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
    config: ExecutionConfig<'_>,
) -> Result<ExecutionEvidence, String> {
    let mut driver = RuntimeDriver::new(
        runtime,
        session_id,
        platform,
        surface,
        RuntimeDriverConfig {
            seed: config.seed,
            delta_ns: config.delta_ns,
            audio_enabled: true,
            text: config.text,
            resume: config.resume_driver,
            frame_sample_interval: config.frame_sample_interval,
            perfetto_trace: config.perfetto_trace,
            perfetto_rfvp_core: false,
            capture_performance_samples: config.capture_performance_samples,
            presentation: config.presentation,
            presentation_substeps: config.presentation_substeps,
            synchronous_gpu_presents: config.synchronous_gpu_presents,
            background_audio: false,
            audio_pump: AudioPumpPolicy::FixedTick,
        },
    )?;
    driver.restore_pending_video().await?;
    let mut checkpoints = Vec::new();
    let mut checkpoint_frames = Vec::new();
    let mut snapshot_verified = false;
    let mut resume_snapshot = None;
    let run_result: Result<(), String> = async {
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
                    let captured = platform
                        .capture_surface(surface)
                        .await
                        .map_err(|error| error.to_string())?;
                    if let Some((width, height, rgba8)) = driver.latest_frame.as_ref() {
                        if captured.width != *width
                            || captured.height != *height
                            || captured.rgba8.as_ref() != rgba8.as_slice()
                        {
                            return Err("ASTRA_EMU_HEADLESS_CHECKPOINT_CAPTURE_MISMATCH".into());
                        }
                    }
                    let width = captured.width;
                    let height = captured.height;
                    let rgba8 = captured.rgba8.to_vec();
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
                        let active_video = driver.capture_active_video();
                        driver.video = None;
                        driver.pending_video_restore = active_video;
                        driver.restore_pending_video().await?;
                        driver.next_step_mode = RuntimeStepMode::RestoreContinuation;
                        snapshot_verified = true;
                    }
                    checkpoints.push(HeadlessCheckpointEvidenceV1 {
                        checkpoint_id: id.clone(),
                        fixed_step: driver.fixed_step,
                        frame_hash: Hash256::from_sha256(&rgba8),
                        observation_hash: driver.observation_hash()?,
                    });
                    checkpoint_frames.push(CheckpointFrame {
                        id: id.clone(),
                        sequence: driver.present_sequence,
                        width,
                        height,
                        rgba8,
                    });
                }
                PhysicalInput::Await {
                    observation,
                    timeout_ticks,
                    continue_at_match,
                } => {
                    if *continue_at_match {
                        return Err("ASTRA_EMU_HEADLESS_AWAIT_CONTINUATION_UNSUPPORTED".into());
                    }
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
        if config.export_snapshot {
            let saved = driver.runtime.save(RuntimeSaveRequest {
                session_id: driver.session_id.clone(),
                slot: "headless-continuation".into(),
            })?;
            validate_runtime_save_sections(&saved)?;
            resume_snapshot = Some(HeadlessResumeExport {
                runtime_sections: saved.sections,
                driver: driver.capture_resume_state(),
            });
        }
        Ok(())
    }
    .await;
    let perfetto_trace = driver.finish_perfetto()?;
    let audio_underflow_count = driver.audio.underflow_count()?;
    let audio_cleanup = driver.audio.shutdown(platform).await;
    let audio_trace = match (run_result, audio_cleanup) {
        (Ok(()), Ok(trace)) => trace,
        (Err(error), Ok(_)) => return Err(error),
        (Ok(()), Err(cleanup)) => {
            return Err(format!("ASTRA_EMU_HEADLESS_AUDIO_CLEANUP_FAILED:{cleanup}"));
        }
        (Err(error), Err(cleanup)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_RUN_AND_AUDIO_CLEANUP_FAILED:{error};audio={cleanup}"
            ));
        }
    };
    let runtime_samples_ns = driver.runtime_timings_ns.clone();
    let presentation_samples_ns = driver.present_timings_ns.clone();
    let phase_timings = HeadlessPhaseTimingEvidenceV1 {
        step_total: duration_distribution(std::mem::take(&mut driver.step_timings_ns)),
        runtime_step: duration_distribution(std::mem::take(&mut driver.runtime_timings_ns)),
        effect_dispatch: duration_distribution(std::mem::take(&mut driver.effect_timings_ns)),
        raster: duration_distribution(std::mem::take(&mut driver.raster_timings_ns)),
        media: duration_distribution(std::mem::take(&mut driver.media_timings_ns)),
        present: duration_distribution(std::mem::take(&mut driver.present_timings_ns)),
    };
    Ok(ExecutionEvidence {
        input_trace: driver.input_trace,
        visual_trace: driver.visual_trace,
        audio_trace,
        state_trace: driver.state_trace,
        checkpoints,
        checkpoint_frames,
        diagnostics: driver.diagnostics,
        fixed_step: driver.fixed_step,
        present_sequence: driver.present_sequence,
        snapshot_verified,
        terminal: driver.terminal,
        phase_timings,
        runtime_samples_ns,
        presentation_samples_ns,
        gpu_samples: Vec::new(),
        performance_memory_after_warmup: driver.performance_memory_after_warmup,
        scene_full_resync_count: driver.scene_full_resync_count,
        audio_underflow_count,
        perfetto_trace,
        resume_snapshot,
    })
}

impl<'a> RuntimeDriver<'a> {
    fn record_perfetto_phase(
        &mut self,
        name: &'static str,
        track: u32,
        started: Instant,
    ) -> Result<(), String> {
        if let Some(perfetto) = self.perfetto.as_mut() {
            perfetto.record(name, track, self.fixed_step, started)?;
        }
        Ok(())
    }

    fn record_perfetto_counter(&mut self, name: &'static str, value: u64) -> Result<(), String> {
        if let Some(perfetto) = self.perfetto.as_mut() {
            perfetto.counter(name, value)?;
        }
        Ok(())
    }

    fn begin_perfetto_phase(
        &mut self,
        name: &'static str,
        track: u32,
        started: Instant,
    ) -> Result<(), String> {
        if let Some(perfetto) = self.perfetto.as_mut() {
            perfetto.begin(name, track, self.fixed_step.saturating_add(1), started)?;
        }
        Ok(())
    }

    fn end_perfetto_phase(&mut self, name: &'static str, track: u32) -> Result<(), String> {
        if let Some(perfetto) = self.perfetto.as_mut() {
            perfetto.end(name, track, self.fixed_step)?;
        }
        Ok(())
    }

    fn record_audio_perfetto(&mut self, telemetry: AudioPumpTelemetry) -> Result<(), String> {
        // These are device observations at the adapter boundary. They do not
        // infer callback starvation or decoder stalls from a missing packet:
        // only the platform-reported queue and underflow counters are emitted.
        self.record_perfetto_counter(
            "astra.emu.adapter.audio_active_streams",
            telemetry.active_streams,
        )?;
        self.record_perfetto_counter(
            "astra.emu.adapter.audio_packets_submitted",
            telemetry.packets_submitted,
        )?;
        self.record_perfetto_counter(
            "astra.emu.adapter.audio_submitted_frames",
            telemetry.submitted_frames,
        )?;
        self.record_perfetto_counter(
            "astra.emu.adapter.audio_consumed_frames",
            telemetry.consumed_frames,
        )?;
        self.record_perfetto_counter(
            "astra.emu.adapter.audio_queued_frames",
            telemetry.queued_frames,
        )?;
        self.record_perfetto_counter(
            "astra.emu.adapter.audio_underflow_count",
            telemetry.underflow_count,
        )?;
        self.record_perfetto_counter(
            "astra.emu.adapter.audio_decoder_refills",
            telemetry.decoder_refills,
        )
    }

    fn finish_perfetto(&mut self) -> Result<Option<PerfettoTraceSummary>, String> {
        self.perfetto
            .take()
            .map(NativePerfettoCapture::finish)
            .transpose()
    }

    fn new(
        runtime: &'a mut AstraEmuRuntimeProvider,
        session_id: GameRuntimeSessionId,
        platform: &'a PlatformHostClient,
        surface: SurfaceHandle,
        config: RuntimeDriverConfig<'_>,
    ) -> Result<RuntimeDriver<'a>, String> {
        if config.presentation_substeps == 0 || config.presentation_substeps > 2 {
            return Err("ASTRA_EMU_PRESENTATION_SUBSTEPS_INVALID".into());
        }
        if config.synchronous_gpu_presents && config.presentation != PresentationPath::NativeGpu {
            return Err("ASTRA_EMU_SYNCHRONOUS_PRESENTATION_REQUIRES_GPU".into());
        }
        if !config.synchronous_gpu_presents && config.presentation_substeps != 1 {
            return Err("ASTRA_EMU_ASYNC_PRESENTATION_SUBSTEPS_INVALID".into());
        }
        let mut image_decoders = DecodeProviderRegistry::default();
        image_decoders
            .register(Box::new(ImageDecodeProvider))
            .map_err(|error| error.to_string())?;
        let mut driver = RuntimeDriver {
            runtime,
            session_id,
            seed: config.seed,
            delta_ns: config.delta_ns,
            platform,
            surface,
            fixed_step: 0,
            next_step_mode: RuntimeStepMode::Live,
            input_sequence: 0,
            await_sequence: 0,
            pending_inputs: Vec::new(),
            pending_waits: BTreeMap::new(),
            rasterizer: CpuStageRasterizer::default(),
            gpu_scene: (config.presentation == PresentationPath::NativeGpu)
                .then(GpuSceneAdapter::default),
            pending_scene_metrics: None,
            pending_render_frame: None,
            pending_scene_frame: None,
            pending_scene_present: None,
            queued_visual_hash: None,
            visual_dirty: false,
            image_decoders,
            text_presenter: BoundTextPresenter::new(
                config.text.provider_id,
                config.text.target,
                config.text.profile,
            )?,
            underlay_frame: None,
            base_frame: None,
            latest_frame: None,
            present_sequence: 0,
            state_hash: Hash256::from_sha256(&[]),
            terminal: false,
            audio: if config.background_audio {
                AudioExecutor::Worker(LegacyAudioPlaybackService::start_with_client(
                    platform.clone(),
                    false,
                )?)
            } else {
                AudioExecutor::Deterministic(HeadlessAudioExecutor::default())
            },
            video: None,
            pending_video_restore: None,
            movie_audio_sequence: 0,
            completed_media: Vec::new(),
            input_trace: Vec::new(),
            visual_trace: Vec::new(),
            state_trace: Vec::new(),
            diagnostics: BTreeSet::new(),
            active_touch: None,
            audio_enabled: config.audio_enabled,
            audio_pump: config.audio_pump,
            frame_sample_interval: config.frame_sample_interval,
            presentation_substeps: config.presentation_substeps,
            synchronous_gpu_presents: config.synchronous_gpu_presents,
            step_timings_ns: Vec::new(),
            runtime_timings_ns: Vec::new(),
            effect_timings_ns: Vec::new(),
            raster_timings_ns: Vec::new(),
            media_timings_ns: Vec::new(),
            present_timings_ns: Vec::new(),
            perfetto: config
                .perfetto_trace
                .map(NativePerfettoCapture::new)
                .transpose()?,
            perfetto_rfvp_core: config.perfetto_rfvp_core,
            capture_performance_samples: config.capture_performance_samples,
            performance_memory_after_warmup: None,
            scene_full_resync_count: 0,
        };
        if let Some(resume) = config.resume {
            driver.fixed_step = resume.fixed_step;
            driver.next_step_mode = RuntimeStepMode::RestoreContinuation;
            driver.input_sequence = resume.input_sequence;
            driver.await_sequence = resume.await_sequence;
            driver.pending_inputs = resume.pending_inputs;
            driver.pending_waits = resume.pending_waits;
            driver.completed_media = resume.completed_media;
            driver.pending_video_restore = resume.active_video;
            driver.state_hash = resume.state_hash;
            driver.active_touch = resume.active_touch;
        }
        Ok(driver)
    }

    fn capture_resume_state(&self) -> HeadlessDriverResumeV1 {
        HeadlessDriverResumeV1 {
            fixed_step: self.fixed_step,
            input_sequence: self.input_sequence,
            await_sequence: self.await_sequence,
            pending_inputs: self.pending_inputs.clone(),
            pending_waits: self.pending_waits.clone(),
            completed_media: self.completed_media.clone(),
            active_video: self.capture_active_video(),
            state_hash: self.state_hash,
            active_touch: self.active_touch,
        }
    }

    fn capture_active_video(&self) -> Option<HeadlessVideoResumeV1> {
        self.video.as_ref().map(|video| HeadlessVideoResumeV1 {
            playback_id: video.playback_id.clone(),
            resource_uri: video.resource_uri.clone(),
            mode: video.mode,
            stage_width: video.stage_width,
            stage_height: video.stage_height,
            started_step: video.started_step,
        })
    }

    async fn restore_pending_video(&mut self) -> Result<(), String> {
        let Some(video) = self.pending_video_restore.take() else {
            return Ok(());
        };
        self.open_video(
            video.playback_id,
            video.resource_uri,
            video.mode,
            video.stage_width,
            video.stage_height,
            video.started_step,
        )
        .await
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
                let control = native_key_control(logical_key.as_deref(), physical_key)
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_KEY_UNSUPPORTED".to_owned())?;
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
                self.queue_input("pointer.x", true, f32::from(*x))?;
                self.queue_input("pointer.y", true, f32::from(*y))
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
                    self.queue_input("pointer.x", true, f32::from(*x))?;
                    self.queue_input("pointer.y", true, f32::from(*y))?;
                    self.queue_input("pointer.primary", true, 1.0)
                }
                TouchPhase::Moved if self.active_touch == Some(*id) => {
                    self.queue_input("pointer.x", true, f32::from(*x))?;
                    self.queue_input("pointer.y", true, f32::from(*y))
                }
                TouchPhase::Ended | TouchPhase::Cancelled if self.active_touch == Some(*id) => {
                    self.active_touch = None;
                    self.queue_input("pointer.primary", false, 0.0)
                }
                _ => Err("ASTRA_EMU_HEADLESS_TOUCH_SEQUENCE".into()),
            },
            PhysicalInput::GamepadInput { control, value, .. } => {
                let mapped = match control {
                    GamepadControl::South => "enter",
                    GamepadControl::East => "escape",
                    GamepadControl::DpadUp => "arrow_up",
                    GamepadControl::DpadDown => "arrow_down",
                    GamepadControl::DpadLeft => "arrow_left",
                    GamepadControl::DpadRight => "arrow_right",
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
        self.poll_native_scene_present()?;
        let step_started = Instant::now();
        self.begin_perfetto_phase("astra.emu.adapter.fixed_tick", 0, step_started)?;
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
        let pressed_keys = pressed_input_keys(&self.pending_inputs);
        let ready = self
            .pending_waits
            .iter()
            .filter_map(|(token, wait)| match wait {
                PendingWait::DueStep(due) if *due <= next_step => {
                    Some((token.clone(), BTreeSet::new()))
                }
                PendingWait::Input(keys) => {
                    let consumed = keys
                        .iter()
                        .filter(|key| pressed_keys.contains(*key))
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    if consumed.is_empty() {
                        None
                    } else {
                        Some((token.clone(), consumed))
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let consumed_input_keys = ready
            .iter()
            .fold(BTreeSet::new(), |mut acc, (_, consumed)| {
                acc.extend(consumed.iter().cloned());
                acc
            });
        let mut await_results = Vec::new();
        for (token_id, _) in ready {
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
        let input_edges = retain_unconsumed_input_edges(
            std::mem::take(&mut self.pending_inputs),
            &consumed_input_keys,
        );
        let runtime_started = Instant::now();
        let output = self.runtime.step(RuntimeStepInput {
            session_id: self.session_id.clone(),
            fixed_step: next_step,
            delta_ns: self.delta_ns,
            session_seed: self.seed,
            mode: self.next_step_mode,
            action: "emu.step".into(),
            payload: serde_json::to_value(EmuStepPayload {
                input_edges,
                await_results,
                provider_results: Vec::new(),
                budget: LegacyStepBudget {
                    max_instructions: 100_000,
                    max_effects: 65_536,
                    max_trace_entries: 100_000,
                },
            })
            .map_err(|_| "ASTRA_EMU_HEADLESS_STEP_PAYLOAD".to_owned())?,
        })?;
        self.runtime_timings_ns.push(elapsed_ns(runtime_started)?);
        if self.perfetto_rfvp_core {
            // This is the measured dynamic hosted-provider call, including its
            // ABI boundary. Fine-grained VM phases are emitted only when the
            // hosted observer supplies them; this outer slice is never used as
            // a substitute for those phase timings.
            self.record_perfetto_phase("rfvp.core.provider_step", 6, runtime_started)?;
        }
        self.record_perfetto_phase(
            "astra.emu.adapter.runtime_world_effects",
            1,
            runtime_started,
        )?;
        self.next_step_mode = RuntimeStepMode::Live;
        self.fixed_step = next_step;
        let mut rendered = false;
        let mut text_presentations = BTreeMap::new();
        let effect_started = Instant::now();
        for envelope in &output.outputs {
            if envelope.domain == RuntimeOutputDomain::Presentation
                && envelope.schema == "astra.emu.render_frame.v1"
            {
                let frame = envelope
                    .decode_bulk_postcard::<LegacyRenderFrameV1>(
                        RuntimeOutputDomain::Presentation,
                        "astra.emu.render_frame.v1",
                        SchemaVersion::new(1, 0, 0),
                    )
                    .map_err(|error| error.to_string())?;
                self.queue_render_frame(frame)?;
                rendered = true;
                continue;
            }
            if envelope.domain == RuntimeOutputDomain::Presentation
                && envelope.schema == "astra.emu.scene_packet.v1"
            {
                let commit = envelope
                    .decode_bulk_postcard::<LegacyPreparedSceneCommitV1>(
                        RuntimeOutputDomain::Presentation,
                        "astra.emu.scene_packet.v1",
                        SchemaVersion::new(1, 0, 0),
                    )
                    .map_err(|error| error.to_string())?;
                self.queue_scene_commit(commit)?;
                rendered = true;
                continue;
            }
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
            let scene_commit_count = family
                .effects
                .iter()
                .filter(|effect| {
                    matches!(
                        effect,
                        LegacyEffect::Presentation { command, .. }
                            if command == "astra.emu.scene_packet.v1"
                    )
                })
                .count();
            let video_command_count = family
                .effects
                .iter()
                .filter(|effect| {
                    matches!(
                        effect,
                        LegacyEffect::Presentation { command, .. }
                            if command == "astra.emu.video_command.v1"
                    )
                })
                .count();
            tracing::debug!(
                event = "astra_emu_headless_family_step",
                fixed_step = next_step,
                status = ?family.status,
                effect_count = family.effects.len(),
                scene_commit_count,
                video_command_count,
                wait_count = family.waits.len(),
                diagnostic_count = family.diagnostics.len(),
                "received validated legacy family step output"
            );
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
                    } if command == "astra.emu.render_resource_frame.v1" => {
                        let resource_frame: LegacyRenderResourceFrameV1 =
                            postcard::from_bytes(&payload).map_err(|_| {
                                "ASTRA_EMU_HEADLESS_RENDER_RESOURCE_FRAME_DECODE".to_owned()
                            })?;
                        let frame = self.materialize_resource_frame(resource_frame)?;
                        self.queue_render_frame(frame)?;
                        rendered = true;
                    }
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } if command == "astra.emu.render_frame.v1" => {
                        let frame: LegacyRenderFrameV1 = postcard::from_bytes(&payload)
                            .map_err(|_| "ASTRA_EMU_HEADLESS_RENDER_FRAME_DECODE".to_owned())?;
                        self.queue_render_frame(frame)?;
                        rendered = true;
                    }
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } if command == "astra.emu.scene_packet.v1" => {
                        let commit: LegacyPreparedSceneCommitV1 = postcard::from_bytes(&payload)
                            .map_err(|_| "ASTRA_EMU_HEADLESS_SCENE_PACKET_DECODE".to_owned())?;
                        self.queue_scene_commit(commit)?;
                        rendered = true;
                    }
                    LegacyEffect::Presentation {
                        command, payload, ..
                    } if command == "astra.emu.text_presentation.v1" => {
                        let binding: LegacyTextPresentationLeaseV1 = postcard::from_bytes(&payload)
                            .map_err(|_| {
                                "ASTRA_EMU_HEADLESS_TEXT_PRESENTATION_DECODE".to_owned()
                            })?;
                        binding.validate().map_err(|error| error.to_string())?;
                        if text_presentations
                            .insert(binding.lease_id, binding.presentation)
                            .is_some()
                        {
                            return Err("ASTRA_EMU_HEADLESS_TEXT_PRESENTATION_DUPLICATE".into());
                        }
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
                                &[command.as_bytes(), payload.as_bytes()].concat(),
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
                        if text.lease_id != lease_id
                            || text.text.len() != byte_len as usize
                            || Hash256::from_sha256(text.text.as_bytes()) != text_hash
                            || text
                                .speaker
                                .as_ref()
                                .map(|value| Hash256::from_sha256(value.as_bytes()))
                                != speaker_hash
                        {
                            return Err("ASTRA_EMU_HEADLESS_TEXT_LEASE_IDENTITY".into());
                        }
                        if let Some(presentation) = text_presentations.remove(&lease_id) {
                            let underlay = self.underlay_frame.clone().ok_or_else(|| {
                                "ASTRA_EMU_HEADLESS_TEXT_UNDERLAY_MISSING".to_owned()
                            })?;
                            let text_started = Instant::now();
                            let presented =
                                self.text_presenter
                                    .render(&underlay, &text, &presentation)?;
                            self.raster_timings_ns.push(elapsed_ns(text_started)?);
                            self.record_perfetto_phase(
                                "astra.emu.adapter.text_raster",
                                4,
                                text_started,
                            )?;
                            for hash in presented.layout_hashes {
                                self.state_trace
                                    .extend_from_slice(hash.to_string().as_bytes());
                                self.state_trace.push(b'\n');
                            }
                            self.base_frame = Some((underlay.0, underlay.1, presented.rgba8));
                            rendered = true;
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
        if !text_presentations.is_empty() {
            return Err("ASTRA_EMU_HEADLESS_TEXT_PRESENTATION_ORPHANED".into());
        }
        self.effect_timings_ns.push(elapsed_ns(effect_started)?);
        self.record_perfetto_phase("astra.emu.adapter.effect_dispatch", 2, effect_started)?;
        if let Some(metrics) = self.pending_scene_metrics.take() {
            self.record_perfetto_counter(
                "astra.emu.adapter.texture_resource_operations",
                metrics.resource_operations,
            )?;
            self.record_perfetto_counter(
                "astra.emu.adapter.texture_create_bytes",
                metrics.create_bytes,
            )?;
            self.record_perfetto_counter(
                "astra.emu.adapter.texture_update_bytes",
                metrics.update_bytes,
            )?;
            self.record_perfetto_counter("astra.emu.adapter.scene_draw_count", metrics.draw_count)?;
            self.record_perfetto_counter(
                "astra.emu.adapter.texture_live_generations",
                metrics.live_textures,
            )?;
            self.record_perfetto_counter(
                "astra.emu.adapter.texture_generation",
                metrics.generation,
            )?;
        }
        let media_started = Instant::now();
        let audio_telemetry = if self.audio_enabled {
            Some(self.audio.pump(self.platform, self.audio_pump).await?)
        } else {
            None
        };
        let video_changed = self.advance_video().await?;
        self.media_timings_ns.push(elapsed_ns(media_started)?);
        self.record_perfetto_phase("astra.emu.adapter.media_queue", 3, media_started)?;
        // Complete the encompassing media slice before emitting its instantaneous
        // counters. Perfetto's streaming writer rejects timestamp regression,
        // so counters cannot be recorded while a later-completed parent slice
        // still has an earlier start timestamp.
        if let Some(telemetry) = audio_telemetry {
            self.record_audio_perfetto(telemetry)?;
        }
        let presentation_changed = rendered || video_changed;
        let sample_due = self.fixed_step.is_multiple_of(self.frame_sample_interval);
        if sample_due
            && self.gpu_scene.is_some()
            && (self.pending_scene_frame.is_some()
                || self
                    .gpu_scene
                    .as_ref()
                    .is_some_and(|scene| scene.width != 0 && scene.height != 0))
        {
            if self.synchronous_gpu_presents {
                if self.pending_scene_present.is_some() {
                    return Err("ASTRA_EMU_SYNCHRONOUS_PRESENT_RECEIPT_PENDING".into());
                }
                let mut submitted = 0u8;
                if let Some(scene) = self.pending_scene_frame.take() {
                    self.present_scene_sync(scene).await?;
                    self.visual_dirty = false;
                    submitted = 1;
                }
                while submitted < self.presentation_substeps {
                    let scene = self
                        .gpu_scene
                        .as_ref()
                        .expect("checked GPU presentation path")
                        .draw_scene()?;
                    self.present_scene_sync(scene).await?;
                    submitted += 1;
                }
            } else if self.visual_dirty
                && self.pending_scene_frame.is_some()
                && self.pending_scene_present.is_none()
            {
                let present_started = Instant::now();
                self.present_sequence = self
                    .present_sequence
                    .checked_add(1)
                    .ok_or_else(|| "ASTRA_EMU_NATIVE_PRESENT_SEQUENCE_OVERFLOW".to_owned())?;
                let mut scene = self
                    .pending_scene_frame
                    .take()
                    .expect("checked pending native scene frame");
                scene.sequence = self.present_sequence;
                self.pending_scene_present = Some(
                    self.platform
                        .submit_scene(self.surface, scene)
                        .map_err(|error| error.to_string())?,
                );
                self.present_timings_ns.push(elapsed_ns(present_started)?);
                self.record_perfetto_phase("astra.emu.adapter.gpu_submit", 5, present_started)?;
                self.visual_dirty = false;
            }
        } else if sample_due && (self.visual_dirty || video_changed) {
            if self.visual_dirty {
                {
                    let frame = self
                        .pending_render_frame
                        .as_ref()
                        .ok_or_else(|| "ASTRA_EMU_HEADLESS_PENDING_FRAME_MISSING".to_owned())?;
                    let width = frame.width;
                    let height = frame.height;
                    let raster_started = Instant::now();
                    let rgba8 = self.rasterizer.render_prepared(frame)?;
                    self.raster_timings_ns.push(elapsed_ns(raster_started)?);
                    self.record_perfetto_phase("astra.emu.adapter.cpu_oracle", 4, raster_started)?;
                    self.base_frame = Some((width, height, rgba8));
                    self.visual_dirty = false;
                }
            }
            let present_started = Instant::now();
            self.present().await?;
            self.present_timings_ns.push(elapsed_ns(present_started)?);
            self.record_perfetto_phase("astra.emu.adapter.cpu_present", 5, present_started)?;
        }
        if presentation_changed {
            for wait in self.pending_waits.values_mut() {
                if matches!(wait, PendingWait::Presentation) {
                    *wait = PendingWait::DueStep(next_step.saturating_add(1));
                }
            }
        }
        self.terminal = output.status == "terminal";
        self.step_timings_ns.push(elapsed_ns(step_started)?);
        self.end_perfetto_phase("astra.emu.adapter.fixed_tick", 0)
    }

    fn poll_native_scene_present(&mut self) -> Result<(), String> {
        let completed = self
            .pending_scene_present
            .as_mut()
            .map(|receipt| receipt.try_complete().map_err(|error| error.to_string()))
            .transpose()?
            .unwrap_or(false);
        if completed {
            self.pending_scene_present = None;
        }
        Ok(())
    }

    async fn present_scene_sync(&mut self, mut scene: SceneFrame) -> Result<(), String> {
        let present_started = Instant::now();
        self.present_sequence = self
            .present_sequence
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_PRESENT_SEQUENCE_OVERFLOW".to_owned())?;
        scene.sequence = self.present_sequence;
        self.platform
            .present_scene(self.surface, scene)
            .await
            .map_err(|error| error.to_string())?;
        if self.capture_performance_samples
            && self.present_sequence == PERFORMANCE_WARMUP_PRESENTATIONS as u64
        {
            self.performance_memory_after_warmup =
                Some(sample_process_memory().map_err(|error| error.to_string())?);
        }
        self.present_timings_ns.push(elapsed_ns(present_started)?);
        self.record_perfetto_phase("astra.emu.adapter.gpu_submit", 5, present_started)
    }

    fn queue_render_frame(&mut self, frame: LegacyRenderFrameV1) -> Result<(), String> {
        // The FVP adapter may publish its complete visual state on every VM tick. A
        // byte-identical packet cannot change the platform surface, so keep the
        // content identity host-local and avoid re-rasterizing/re-presenting it.
        // This is intentionally before `prepare`: it neither changes the provider
        // packet nor exposes resource pixels in logs, save data, or evidence.
        let visual_hash = Hash256::from_sha256(
            &postcard::to_allocvec(&frame)
                .map_err(|_| "ASTRA_EMU_NATIVE_RENDER_FRAME_ENCODE".to_owned())?,
        );
        if self.queued_visual_hash == Some(visual_hash) {
            return Ok(());
        }
        self.pending_render_frame = Some(self.rasterizer.prepare(frame)?);
        self.queued_visual_hash = Some(visual_hash);
        self.visual_dirty = true;
        Ok(())
    }

    fn queue_scene_commit(&mut self, commit: LegacyPreparedSceneCommitV1) -> Result<(), String> {
        // A v5 semantic commit is already a single, bounded transaction.
        // Its identity contains resource content hashes rather than resource
        // bytes, so frame deduplication does not duplicate texture payloads on
        // the hot path. The rasterizer independently validates
        // `next_resources` before retained CPU state changes, which keeps this
        // direct hand-off fail-stop.
        let visual_hash = scene_commit_visual_hash(&commit)?;
        if self.queued_visual_hash == Some(visual_hash) {
            return Ok(());
        }
        tracing::debug!(
            event = "astra_emu_headless_scene_commit",
            draw_count = commit.packet.draws.len(),
            resource_operation_count = commit.packet.resources.len(),
            retained_texture_count = commit.next_resources.textures.len(),
            reset_resources = commit.reset_resources,
            "queued validated semantic scene commit"
        );
        if let Some(gpu_scene) = self.gpu_scene.as_mut() {
            let (delta, metrics) = gpu_scene.prepare(commit)?;
            // An in-flight receipt forms a strict submission boundary.  A
            // later frame is therefore applied only after that receipt, but
            // it must retain every resource mutation on which its latest
            // draw list depends.  Coalescing keeps the old resource prefix
            // and replaces presentation commands; it never rebuilds and
            // reuploads the retained texture table.
            self.pending_scene_frame = Some(match self.pending_scene_frame.take() {
                Some(queued) => merge_scene_frames(queued, delta)?,
                None => delta,
            });
            if let Some(current) = self.pending_scene_metrics.as_mut() {
                current.accumulate(metrics)?;
            } else {
                self.pending_scene_metrics = Some(metrics);
            }
            self.pending_render_frame = None;
        } else {
            self.pending_render_frame = Some(self.rasterizer.prepare_scene_commit(commit)?);
        }
        self.queued_visual_hash = Some(visual_hash);
        self.visual_dirty = true;
        Ok(())
    }

    fn materialize_resource_frame(
        &mut self,
        resource_frame: LegacyRenderResourceFrameV1,
    ) -> Result<LegacyRenderFrameV1, String> {
        resource_frame
            .validate()
            .map_err(|error| error.to_string())?;
        let mut texture_updates = Vec::with_capacity(resource_frame.texture_resources.len());
        for resource in resource_frame.texture_resources {
            if resource.decoded_format != LegacyTextureFormat::Rgba8 {
                return Err("ASTRA_EMU_HEADLESS_IMAGE_FORMAT_UNSUPPORTED".into());
            }
            let bytes = self.runtime.read_session_resource(
                &self.session_id,
                &resource.resource_uri,
                1024 * 1024 * 1024,
            )?;
            if Hash256::from_sha256(&bytes) != resource.encoded_hash {
                return Err("ASTRA_EMU_HEADLESS_IMAGE_RESOURCE_IDENTITY".into());
            }
            let profile = "emu-headless-image-v1";
            let decoded = self
                .image_decoders
                .decode(
                    &DecodeRequest {
                        kind: astra_media::DecodeKind::Image,
                        codec: resource.codec,
                        bytes,
                        profile: profile.into(),
                    },
                    &DecodeBindingContext::shipping("astra.decode.image", "headless", profile),
                )
                .map_err(|error| error.to_string())?;
            let MediaDecodeOutput::CpuBuffer {
                bytes,
                format,
                hash,
            } = decoded.output
            else {
                return Err("ASTRA_EMU_HEADLESS_IMAGE_CPU_BUFFER_REQUIRED".into());
            };
            if format != "rgba8" || Hash256::from_sha256(&bytes) != hash {
                return Err("ASTRA_EMU_HEADLESS_IMAGE_DECODE_IDENTITY".into());
            }
            let expected = usize::try_from(resource.decoded_width)
                .ok()
                .and_then(|width| {
                    usize::try_from(resource.decoded_height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_IMAGE_DIMENSION_OVERFLOW".to_owned())?;
            if bytes.len() != expected {
                return Err("ASTRA_EMU_HEADLESS_IMAGE_DIMENSION_MISMATCH".into());
            }
            texture_updates.push(LegacyTextureUpdateV1 {
                texture_id: resource.texture_id,
                width: resource.decoded_width,
                height: resource.decoded_height,
                format: resource.decoded_format,
                content_hash: hash,
                pixels: bytes,
            });
        }
        let frame = LegacyRenderFrameV1 {
            width: resource_frame.width,
            height: resource_frame.height,
            texture_updates,
            draws: resource_frame.draws,
        };
        frame.validate().map_err(|error| error.to_string())?;
        Ok(frame)
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
                mode,
                stage_width,
                stage_height,
            } => {
                self.open_video(
                    playback_id,
                    resource_uri,
                    mode,
                    stage_width,
                    stage_height,
                    self.fixed_step,
                )
                .await
            }
            LegacyVideoCommandV1::Stop { playback_id } => {
                let active = self
                    .video
                    .take()
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_NOT_ACTIVE".to_owned())?;
                if active.playback_id != playback_id {
                    self.video = Some(active);
                    return Err("ASTRA_EMU_HEADLESS_VIDEO_IDENTITY".into());
                }
                if let Some(stream_id) = active.audio_stream_id {
                    self.audio
                        .close_movie_stream(stream_id, self.platform)
                        .await?;
                }
                self.completed_media.push(playback_id);
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn open_video(
        &mut self,
        playback_id: String,
        resource_uri: String,
        mode: LegacyVideoMode,
        stage_width: u32,
        stage_height: u32,
        started_step: u64,
    ) -> Result<(), String> {
        if self.video.is_some() || started_step > self.fixed_step {
            return Err("ASTRA_EMU_HEADLESS_VIDEO_ALREADY_ACTIVE".into());
        }
        let bytes = self.runtime.read_session_resource(
            &self.session_id,
            &resource_uri,
            512 * 1024 * 1024,
        )?;
        let extension = resource_uri
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_EXTENSION_MISSING".to_owned())?;
        let elapsed_ns = self
            .fixed_step
            .saturating_sub(started_step)
            .saturating_mul(self.delta_ns);
        let (stream, audio_stream_id) = match fvp_movie_compatibility(&extension) {
            FvpMovieCompatibility::Native => {
                let movie = decode_fvp_movie(
                    &extension,
                    &bytes,
                    MAX_MOVIE_FRAMES,
                    MAX_MOVIE_DECODED_BYTES,
                    MAX_MOVIE_AUDIO_SAMPLES,
                )
                .map_err(|error| error.to_string())?;
                let audio_stream_id = if self.audio_enabled
                    && matches!(mode, LegacyVideoMode::ModalWithAudio)
                {
                    match movie.audio {
                        Some(audio) => {
                            let stream_id = MOVIE_AUDIO_STREAM_BASE
                                .checked_add(self.movie_audio_sequence)
                                .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_ID".to_owned())?;
                            self.movie_audio_sequence = self
                                .movie_audio_sequence
                                .checked_add(1)
                                .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_ID".to_owned())?;
                            self.audio
                                .play_buffered_movie(
                                    stream_id,
                                    audio.sample_rate,
                                    audio.channels,
                                    audio.samples,
                                    elapsed_ns,
                                    self.platform,
                                )
                                .await?;
                            Some(stream_id)
                        }
                        None => None,
                    }
                } else {
                    None
                };
                (
                    decoded_native_video_stream(movie.frames, movie.duration_ms)?,
                    audio_stream_id,
                )
            }
            FvpMovieCompatibility::PlatformProviderRequired => {
                (self.decode_platform_video(&extension, bytes).await?, None)
            }
            FvpMovieCompatibility::Unsupported => {
                return Err("ASTRA_EMU_HEADLESS_VIDEO_CODEC_UNSUPPORTED".into());
            }
        };
        tracing::debug!(
            event = "astra_emu_headless_video_opened",
            codec = extension,
            decoded_frame_count = stream.frames.len(),
            duration_us = stream.duration_us,
            audio_stream_active = audio_stream_id.is_some(),
            "opened bounded Headless video stream"
        );
        self.video = Some(ActiveVideo {
            playback_id,
            resource_uri,
            mode,
            stage_width,
            stage_height,
            started_step,
            stream,
            audio_stream_id,
        });
        Ok(())
    }

    async fn decode_platform_video(
        &self,
        extension: &str,
        bytes: Vec<u8>,
    ) -> Result<DecodedVideoStream, String> {
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
                    codec: extension.to_owned(),
                    description: Vec::new(),
                    sample_rate: None,
                    channels: None,
                    coded_width: None,
                    coded_height: None,
                    keyframe: true,
                    stream_action: astra_platform::DecodeStreamAction::OneShot,
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
        DecodedVideoStream::decode(
            &bytes,
            MAX_MOVIE_FRAMES as u64,
            MAX_MOVIE_DECODED_BYTES as u64,
        )
        .map_err(|error| error.to_string())
    }

    async fn advance_video(&mut self) -> Result<bool, String> {
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
            tracing::debug!(
                event = "astra_emu_headless_video_completed",
                elapsed_us,
                "completed bounded Headless video stream"
            );
            if let Some(stream_id) = completed.audio_stream_id {
                self.audio
                    .close_movie_stream(stream_id, self.platform)
                    .await?;
            }
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

fn decoded_native_video_stream(
    frames: Vec<astra_emu_fvp::FvpMovieFrame>,
    duration_ms: u64,
) -> Result<DecodedVideoStream, String> {
    let duration_us = duration_ms
        .checked_mul(1_000)
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_TIMELINE_BOUNDS".to_owned())?;
    let mut decoded = Vec::with_capacity(frames.len());
    for (index, frame) in frames.iter().enumerate() {
        let pts_us = frame
            .pts_ms
            .checked_mul(1_000)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_TIMELINE_BOUNDS".to_owned())?;
        let next_pts_us = match frames.get(index + 1) {
            Some(next) => next
                .pts_ms
                .checked_mul(1_000)
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_TIMELINE_BOUNDS".to_owned())?,
            None => duration_us,
        };
        let frame_duration_us = next_pts_us
            .checked_sub(pts_us)
            .filter(|duration| *duration > 0)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_TIMELINE_ORDER".to_owned())?;
        let mut bgra8 = frame.rgba8.clone();
        for pixel in bgra8.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        decoded.push(DecodedVideoFrame {
            sequence: index as u64 + 1,
            pts_us,
            duration_us: frame_duration_us,
            width: frame.width,
            height: frame.height,
            content_hash: Hash256::from_sha256(&bgra8),
            bgra8,
        });
    }
    let stream = DecodedVideoStream {
        schema: DECODED_VIDEO_STREAM_SCHEMA.into(),
        duration_us,
        frames: decoded,
    };
    stream
        .validate(MAX_MOVIE_FRAMES as u64, MAX_MOVIE_DECODED_BYTES as u64)
        .map_err(|error| error.to_string())?;
    Ok(stream)
}

#[derive(Default)]
struct AudioStream {
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
    cursor: usize,
    decoder: Option<SymphoniaAudioStreamDecoder>,
    stream_source: Option<(String, Arc<[u8]>)>,
    end_of_stream: bool,
    fully_buffered: bool,
    integer_pcm: bool,
    playing: bool,
    paused: bool,
    awaiting_priming: bool,
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
    observed_underflows: BTreeMap<u32, u64>,
    realtime_poll_countdown: u8,
}

enum AudioExecutor {
    Deterministic(HeadlessAudioExecutor),
    Worker(LegacyAudioPlaybackService),
}

impl Default for AudioExecutor {
    fn default() -> Self {
        Self::Deterministic(HeadlessAudioExecutor::default())
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct AudioPumpTelemetry {
    active_streams: u64,
    packets_submitted: u64,
    submitted_frames: u64,
    consumed_frames: u64,
    queued_frames: u64,
    underflow_count: u64,
    decoder_refills: u64,
}

impl AudioExecutor {
    fn underflow_count(&self) -> Result<u64, String> {
        match self {
            Self::Deterministic(executor) => executor.underflow_count(),
            Self::Worker(service) => Ok(service.telemetry().underflow_count),
        }
    }

    async fn reset_for_restore(&mut self, platform: &PlatformHostClient) -> Result<(), String> {
        match self {
            Self::Deterministic(executor) => executor.reset_for_restore(platform).await,
            Self::Worker(service) => service.reset(),
        }
    }

    async fn execute(
        &mut self,
        command: LegacyAudioCommandV1,
        resource: Option<Vec<u8>>,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        match self {
            Self::Deterministic(executor) => executor.execute(command, resource, platform).await,
            Self::Worker(service) => service.execute(command, resource),
        }
    }

    async fn play_buffered_movie(
        &mut self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        mut samples: Vec<f32>,
        elapsed_ns: u64,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        match self {
            Self::Deterministic(executor) => {
                executor
                    .play_buffered_movie(
                        stream_id,
                        sample_rate,
                        channels,
                        samples,
                        elapsed_ns,
                        platform,
                    )
                    .await
            }
            Self::Worker(service) => {
                let elapsed_frames =
                    elapsed_ns.saturating_mul(u64::from(sample_rate)) / 1_000_000_000;
                let elapsed_samples = usize::try_from(elapsed_frames)
                    .ok()
                    .and_then(|frames| frames.checked_mul(usize::from(channels)))
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_MOVIE_TIMELINE".to_owned())?;
                if elapsed_samples >= samples.len() {
                    samples.clear();
                } else if elapsed_samples != 0 {
                    samples.drain(..elapsed_samples);
                }
                service.begin_movie_stream(stream_id, sample_rate, channels, samples)
            }
        }
    }

    async fn close_movie_stream(
        &mut self,
        stream_id: u32,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        match self {
            Self::Deterministic(executor) => executor.close_movie_stream(stream_id, platform).await,
            Self::Worker(service) => service.stop_movie_pcm(stream_id),
        }
    }

    async fn pump(
        &mut self,
        platform: &PlatformHostClient,
        policy: AudioPumpPolicy,
    ) -> Result<AudioPumpTelemetry, String> {
        match self {
            Self::Deterministic(executor) => executor.pump(platform, policy).await,
            Self::Worker(service) => {
                service.pump()?;
                let telemetry = service.telemetry();
                Ok(AudioPumpTelemetry {
                    active_streams: telemetry.active_streams,
                    packets_submitted: telemetry.packet_count,
                    submitted_frames: telemetry.submitted_frames,
                    consumed_frames: telemetry.consumed_frames,
                    queued_frames: telemetry.queued_frames,
                    underflow_count: telemetry.underflow_count,
                    decoder_refills: telemetry.decoder_refills,
                })
            }
        }
    }

    fn set_suspended(&self, suspended: bool) -> Result<(), String> {
        match self {
            Self::Deterministic(_) => Ok(()),
            Self::Worker(service) => service.set_suspended(suspended),
        }
    }

    async fn shutdown(self, platform: &PlatformHostClient) -> Result<Vec<u8>, String> {
        match self {
            Self::Deterministic(executor) => executor.shutdown(platform).await,
            Self::Worker(service) => service.shutdown(),
        }
    }
}

impl Default for HeadlessAudioExecutor {
    fn default() -> Self {
        Self {
            streams: BTreeMap::new(),
            master_volume: 1.0,
            meter_trace: Vec::new(),
            observed_underflows: BTreeMap::new(),
            realtime_poll_countdown: 0,
        }
    }
}

impl HeadlessAudioExecutor {
    fn underflow_count(&self) -> Result<u64, String> {
        self.observed_underflows
            .values()
            .try_fold(0_u64, |total, count| {
                total
                    .checked_add(*count)
                    .ok_or_else(|| "ASTRA_EMU_AUDIO_UNDERFLOW_COUNTER_OVERFLOW".to_owned())
            })
    }

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
                encoding,
                resource_uri,
            } => {
                let encoded = resolved_resource
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_RESOURCE_MISSING".to_owned())?;
                let codec = resolve_audio_codec(encoding, &resource_uri, &encoded)?;
                let resource_hash = Hash256::from_sha256(&encoded);
                let source = Arc::<[u8]>::from(encoded);
                let decoder = open_symphonia_audio_stream(
                    &codec,
                    Arc::clone(&source),
                    MAX_STREAM_DECODED_AUDIO_BYTES,
                )
                .map_err(|error| redacted_stream_media_error(error, &codec, resource_hash))?;
                let sample_rate = decoder.sample_rate();
                let channels = decoder.channels();
                self.replace_stream(
                    stream_id,
                    AudioStream {
                        sample_rate,
                        channels,
                        decoder: Some(decoder),
                        stream_source: Some((codec, source)),
                        integer_pcm: true,
                        volume: 1.0,
                        ..AudioStream::default()
                    },
                    platform,
                )
                .await?;
            }
            LegacyAudioCommandV1::CreateStream {
                stream_id,
                sample_rate,
                channels,
                sample_format,
            } => {
                self.replace_stream(
                    stream_id,
                    AudioStream {
                        sample_rate,
                        channels,
                        integer_pcm: sample_format == LegacyAudioSampleFormat::I16,
                        volume: 1.0,
                        ..AudioStream::default()
                    },
                    platform,
                )
                .await?;
            }
            LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => {
                let stream = stream_mut(&mut self.streams, stream_id)
                    .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_SUBMIT_STREAM_MISSING".to_owned())?;
                if !stream.integer_pcm {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_SAMPLE_FORMAT_MISMATCH".into());
                }
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
                let stream = stream_mut(&mut self.streams, stream_id)
                    .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_SUBMIT_STREAM_MISSING".to_owned())?;
                if stream.integer_pcm {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_SAMPLE_FORMAT_MISMATCH".into());
                }
                stream.samples.extend(samples);
            }
            LegacyAudioCommandV1::Play {
                stream_id,
                volume,
                pan,
                repeat,
                ..
            } => {
                let output_format = platform
                    .query_audio_device_format()
                    .await
                    .map_err(|error| error.to_string())?;
                let stream = stream_mut(&mut self.streams, stream_id)
                    .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_PLAY_STREAM_MISSING".to_owned())?;
                if (stream.samples.is_empty() && stream.decoder.is_none())
                    || stream.output.is_some()
                {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_PLAY_STATE".into());
                }
                prepare_audio_stream_for_output(
                    stream,
                    output_format.sample_rate,
                    output_format.channels,
                )?;
                stream.output = Some(
                    platform
                        .open_audio_output(AudioOutputRequest {
                            sample_rate: output_format.sample_rate,
                            channels: output_format.channels,
                            max_buffered_frames: (output_format.sample_rate as usize * 4).max(1),
                            start_paused: true,
                        })
                        .await
                        .map_err(|e| e.to_string())?,
                );
                stream.cursor = 0;
                // `AudioOutputHandle` owns its own strictly increasing packet
                // sequence. A stopped stream may later receive a fresh output
                // handle, so its producer sequence must restart at one rather
                // than inheriting the retired handle's sequence.
                stream.packet_sequence = 0;
                stream.playing = true;
                stream.paused = false;
                stream.awaiting_priming = true;
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
                        if !stream.awaiting_priming {
                            platform
                                .resume_audio(output)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
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

    async fn play_buffered_movie(
        &mut self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
        elapsed_ns: u64,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        if self.streams.contains_key(&stream_id) {
            return Err("ASTRA_EMU_HEADLESS_MOVIE_AUDIO_STREAM_DUPLICATE".into());
        }
        let output_format = platform
            .query_audio_device_format()
            .await
            .map_err(|error| error.to_string())?;
        let mut stream = AudioStream {
            sample_rate,
            channels,
            samples,
            integer_pcm: false,
            volume: 1.0,
            ..AudioStream::default()
        };
        prepare_audio_stream_for_output(
            &mut stream,
            output_format.sample_rate,
            output_format.channels,
        )?;
        let elapsed_frames =
            elapsed_ns.saturating_mul(u64::from(stream.sample_rate)) / 1_000_000_000;
        let elapsed_samples = usize::try_from(elapsed_frames)
            .ok()
            .and_then(|frames| frames.checked_mul(usize::from(stream.channels)))
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_TIMELINE".to_owned())?;
        stream.cursor = elapsed_samples.min(stream.samples.len());
        stream.packet_sequence = 0;
        stream.playing = stream.cursor < stream.samples.len();
        stream.output = Some(
            platform
                .open_audio_output(AudioOutputRequest {
                    sample_rate: output_format.sample_rate,
                    channels: output_format.channels,
                    max_buffered_frames: (output_format.sample_rate as usize * 4).max(1),
                    start_paused: true,
                })
                .await
                .map_err(|error| error.to_string())?,
        );
        stream.awaiting_priming = true;
        self.streams.insert(stream_id, stream);
        Ok(())
    }

    async fn close_movie_stream(
        &mut self,
        stream_id: u32,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        if self
            .streams
            .get(&stream_id)
            .is_some_and(|stream| stream.output.is_some())
        {
            self.close_stream(stream_id, platform).await?;
        }
        self.streams
            .remove(&stream_id)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_STREAM_MISSING".to_owned())?;
        Ok(())
    }

    async fn pump(
        &mut self,
        platform: &PlatformHostClient,
        policy: AudioPumpPolicy,
    ) -> Result<AudioPumpTelemetry, String> {
        let realtime_poll_due = match policy {
            AudioPumpPolicy::FixedTick => true,
            AudioPumpPolicy::Realtime {
                poll_interval_ticks,
                ..
            } => {
                if poll_interval_ticks == 0 || poll_interval_ticks > 8 {
                    return Err("ASTRA_EMU_NATIVE_AUDIO_POLL_INTERVAL_INVALID".into());
                }
                if self.realtime_poll_countdown == 0 {
                    self.realtime_poll_countdown = poll_interval_ticks - 1;
                    true
                } else {
                    self.realtime_poll_countdown -= 1;
                    false
                }
            }
        };
        let mut telemetry = AudioPumpTelemetry::default();
        for (stream_id, stream) in self
            .streams
            .iter_mut()
            .filter(|(_, stream)| stream.playing && !stream.paused)
        {
            telemetry.active_streams = telemetry
                .active_streams
                .checked_add(1)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            if !realtime_poll_due && !stream.awaiting_priming {
                continue;
            }
            let output = stream
                .output
                .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_OUTPUT_MISSING".to_owned())?;
            // A native output's pre-submit state is sufficient both to compute
            // the bounded target deficit and to observe prior callback
            // underflows. Do not immediately issue a second synchronous query
            // after each submit: that serializes the fixed Runtime tick behind
            // the platform event loop and, in practice, couples audio queue
            // maintenance to unrelated GPU present work. Headless intentionally
            // keeps its post-submit query because it advances the deterministic
            // simulated device callback.
            let realtime_state = match policy {
                AudioPumpPolicy::FixedTick => None,
                AudioPumpPolicy::Realtime { .. } => Some(
                    platform
                        .query_audio(output)
                        .await
                        .map_err(|e| e.to_string())?,
                ),
            };
            let queued_frames = realtime_state.as_ref().map(|state| state.queued_frames);
            let frames = match policy {
                AudioPumpPolicy::FixedTick => usize::try_from(
                    u64::from(stream.sample_rate).saturating_mul(FIXED_DELTA_NS) / 1_000_000_000,
                )
                .map_err(|_| "ASTRA_EMU_HEADLESS_AUDIO_TICK_BOUNDS".to_owned())?
                .max(1),
                AudioPumpPolicy::Realtime {
                    target_latency_ms,
                    refill_low_water_ms,
                    ..
                } => {
                    let target = usize::try_from(
                        u64::from(stream.sample_rate).saturating_mul(u64::from(target_latency_ms))
                            / 1_000,
                    )
                    .map_err(|_| "ASTRA_EMU_NATIVE_AUDIO_TARGET_BOUNDS".to_owned())?
                    .max(1);
                    let refill_low_water = usize::try_from(
                        u64::from(stream.sample_rate)
                            .saturating_mul(u64::from(refill_low_water_ms))
                            / 1_000,
                    )
                    .map_err(|_| "ASTRA_EMU_NATIVE_AUDIO_TARGET_BOUNDS".to_owned())?
                    .max(1);
                    if refill_low_water >= target {
                        return Err("ASTRA_EMU_NATIVE_AUDIO_WATERMARK_INVALID".into());
                    }
                    let queued = queued_frames.unwrap_or(0);
                    if queued > refill_low_water {
                        0
                    } else {
                        target.saturating_sub(queued)
                    }
                }
            };
            if frames == 0 {
                continue;
            }
            let sample_count = frames.saturating_mul(usize::from(stream.channels));
            let mut samples = Vec::with_capacity(sample_count);
            while samples.len() < sample_count && stream.playing {
                if stream.cursor >= stream.samples.len() {
                    if stream.fully_buffered {
                        if stream.repeat {
                            stream.cursor = 0;
                            continue;
                        }
                        stream.playing = false;
                        break;
                    }
                    stream.samples.clear();
                    stream.cursor = 0;
                    if !stream.end_of_stream {
                        if let Some(decoder) = stream.decoder.as_mut() {
                            match decoder.next_chunk().map_err(redacted_audio_stream_error)? {
                                Some(chunk) => {
                                    if chunk.sample_rate != stream.sample_rate
                                        || chunk.channels != stream.channels
                                        || !chunk
                                            .pcm_s16le
                                            .len()
                                            .is_multiple_of(2 * usize::from(stream.channels))
                                    {
                                        return Err(
                                            "ASTRA_EMU_HEADLESS_AUDIO_STREAM_FORMAT_CHANGE".into(),
                                        );
                                    }
                                    stream.samples.extend(chunk.pcm_s16le.chunks_exact(2).map(
                                        |pair| {
                                            f32::from(i16::from_le_bytes([pair[0], pair[1]]))
                                                / 32768.0
                                        },
                                    ));
                                    telemetry.decoder_refills =
                                        telemetry.decoder_refills.checked_add(1).ok_or_else(
                                            || "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned(),
                                        )?;
                                    continue;
                                }
                                None => stream.end_of_stream = true,
                            }
                        }
                    }
                    if stream.end_of_stream && stream.repeat {
                        let (codec, source) = stream.stream_source.as_ref().ok_or_else(|| {
                            "ASTRA_EMU_HEADLESS_AUDIO_REPEAT_SOURCE_MISSING".to_owned()
                        })?;
                        stream.decoder = Some(
                            open_symphonia_audio_stream(
                                codec,
                                Arc::clone(source),
                                MAX_STREAM_DECODED_AUDIO_BYTES,
                            )
                            .map_err(redacted_audio_stream_error)?,
                        );
                        stream.end_of_stream = false;
                        continue;
                    }
                    if stream.end_of_stream {
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
            telemetry.packets_submitted = telemetry
                .packets_submitted
                .checked_add(1)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            if stream.awaiting_priming {
                platform
                    .resume_audio(output)
                    .await
                    .map_err(|e| e.to_string())?;
                stream.awaiting_priming = false;
            }
            let state = match realtime_state {
                Some(state) => state,
                None => {
                    // Headless advances its deterministic device callback only
                    // through `query_audio`. `query_audio_output` is an
                    // observational snapshot and deliberately does not consume
                    // queued samples. Using it here lets one fixed-step packet
                    // accumulate every frame until the bounded platform queue
                    // overflows.
                    platform
                        .query_audio(output)
                        .await
                        .map_err(|e| e.to_string())?
                }
            };
            let previous_underflows = self
                .observed_underflows
                .insert(*stream_id, state.underflow_count)
                .unwrap_or(0);
            if state.underflow_count > previous_underflows {
                tracing::warn!(
                    event = "astra_emu_audio_underflow",
                    stream_id = *stream_id,
                    previous_underflows,
                    underflow_count = state.underflow_count,
                    queued_frames = state.queued_frames,
                    "platform audio callback underflowed"
                );
            }
            let channels = u64::from(stream.channels);
            telemetry.submitted_frames = telemetry
                .submitted_frames
                .checked_add(state.submitted_samples / channels)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            telemetry.consumed_frames = telemetry
                .consumed_frames
                .checked_add(state.consumed_samples / channels)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            telemetry.queued_frames = telemetry
                .queued_frames
                .checked_add(state.queued_frames as u64)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            telemetry.underflow_count = telemetry
                .underflow_count
                .checked_add(state.underflow_count)
                .ok_or_else(|| "ASTRA_EMU_AUDIO_TELEMETRY_OVERFLOW".to_owned())?;
            self.meter_trace.extend_from_slice(
                format!(
                    "{}:{}:{}:{}\n",
                    state.submitted_samples / u64::from(stream.channels),
                    state.consumed_samples / u64::from(stream.channels),
                    state.meter.sample_count,
                    state.meter.peak_dbfs.to_bits()
                )
                .as_bytes(),
            );
        }
        Ok(telemetry)
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

    async fn replace_stream(
        &mut self,
        stream_id: u32,
        replacement: AudioStream,
        platform: &PlatformHostClient,
    ) -> Result<(), String> {
        let previous_active = self
            .streams
            .get(&stream_id)
            .is_some_and(|stream| stream.output.is_some());
        if previous_active {
            self.close_stream(stream_id, platform).await?;
        }
        let replaced = self.streams.insert(stream_id, replacement).is_some();
        if replaced {
            tracing::debug!(
                event = "astra_emu_headless_audio_stream_reloaded",
                stream_id,
                previous_active
            );
        }
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

fn redacted_stream_media_error(error: MediaError, codec: &str, resource_hash: Hash256) -> String {
    format!(
        "ASTRA_EMU_HEADLESS_AUDIO_STREAM_OPEN: codec={} resource_hash={} {}",
        codec,
        resource_hash,
        redacted_audio_stream_error(error)
    )
}

fn redacted_audio_stream_error(error: MediaError) -> String {
    match error {
        MediaError::Diagnostics(diagnostics) => format!(
            "diagnostic_codes={}",
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
        MediaError::Message(_) => "diagnostic_codes=ASTRA_MEDIA_PROVIDER_MESSAGE".into(),
    }
}

fn resolve_audio_codec(
    declared: LegacyAudioEncoding,
    resource_uri: &str,
    encoded: &[u8],
) -> Result<String, String> {
    let declared = match declared {
        LegacyAudioEncoding::Unknown => None,
        LegacyAudioEncoding::Wav => Some("wav"),
        LegacyAudioEncoding::Ogg => Some("ogg"),
        LegacyAudioEncoding::Mp3 => Some("mp3"),
        LegacyAudioEncoding::Flac => Some("flac"),
    };
    let extension = resource_uri
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .filter(|extension| matches!(extension.as_str(), "wav" | "ogg" | "mp3" | "flac"));
    let detected = detect_audio_codec(encoded);

    let selected = declared
        .map(str::to_owned)
        .or(extension)
        .or_else(|| detected.map(str::to_owned))
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_CODEC_UNIDENTIFIED".to_owned())?;
    if detected.is_some_and(|detected| detected != selected) {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_CODEC_IDENTITY_MISMATCH".into());
    }
    Ok(selected)
}

fn detect_audio_codec(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"OggS") {
        Some("ogg")
    } else if bytes.starts_with(b"fLaC") {
        Some("flac")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        Some("wav")
    } else if bytes.starts_with(b"ID3")
        || bytes
            .get(..2)
            .is_some_and(|header| header[0] == 0xff && header[1] & 0xe0 == 0xe0)
    {
        Some("mp3")
    } else {
        None
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

/// The visual identity deliberately substitutes every validated upload with
/// its SHA-256 content identity.  It is used only to suppress a byte-for-byte
/// equivalent presentation; validation still checks the actual bytes before
/// they reach retained renderer state.
#[derive(Serialize)]
struct SceneCommitVisualIdentity<'a> {
    reset_resources: bool,
    width: u32,
    height: u32,
    resources: Vec<SceneResourceVisualIdentity>,
    draws: &'a [LegacyDrawV1],
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SceneResourceVisualIdentity {
    Create {
        texture_id: u32,
        width: u32,
        height: u32,
        format: LegacyTextureFormat,
        content_hash: Hash256,
    },
    Update {
        texture_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: LegacyTextureFormat,
        content_hash: Hash256,
    },
    Destroy {
        texture_id: u32,
    },
}

fn scene_commit_visual_hash(commit: &LegacyPreparedSceneCommitV1) -> Result<Hash256, String> {
    let resources = commit
        .packet
        .resources
        .iter()
        .map(|operation| match operation {
            LegacySceneResourceOperationV1::CreateTexture(texture) => {
                SceneResourceVisualIdentity::Create {
                    texture_id: texture.texture_id,
                    width: texture.width,
                    height: texture.height,
                    format: texture.format,
                    content_hash: texture.content_hash,
                }
            }
            LegacySceneResourceOperationV1::UpdateTexture(texture) => {
                SceneResourceVisualIdentity::Update {
                    texture_id: texture.texture_id,
                    x: texture.x,
                    y: texture.y,
                    width: texture.width,
                    height: texture.height,
                    format: texture.format,
                    content_hash: texture.content_hash,
                }
            }
            LegacySceneResourceOperationV1::DestroyTexture { texture_id } => {
                SceneResourceVisualIdentity::Destroy {
                    texture_id: *texture_id,
                }
            }
        })
        .collect();
    let identity = SceneCommitVisualIdentity {
        reset_resources: commit.reset_resources,
        width: commit.packet.width,
        height: commit.packet.height,
        resources,
        draws: &commit.packet.draws,
    };
    let bytes = postcard::to_allocvec(&identity)
        .map_err(|_| "ASTRA_EMU_HEADLESS_SCENE_IDENTITY_ENCODE".to_owned())?;
    Ok(Hash256::from_sha256(&bytes))
}

fn prepare_audio_stream_for_output(
    stream: &mut AudioStream,
    output_sample_rate: u32,
    output_channels: u16,
) -> Result<(), String> {
    if output_sample_rate == 0 || output_channels == 0 || output_channels > 2 {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_OUTPUT_FORMAT".into());
    }
    while let Some(decoder) = stream.decoder.as_mut() {
        match decoder.next_chunk().map_err(redacted_audio_stream_error)? {
            Some(chunk) => {
                if chunk.sample_rate != stream.sample_rate
                    || chunk.channels != stream.channels
                    || !chunk
                        .pcm_s16le
                        .len()
                        .is_multiple_of(2 * usize::from(stream.channels))
                {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_STREAM_FORMAT_CHANGE".into());
                }
                let next_samples = chunk.pcm_s16le.len() / 2;
                let total_samples = stream
                    .samples
                    .len()
                    .checked_add(next_samples)
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_DECODE_BOUNDS".to_owned())?;
                let decoded_bytes = total_samples
                    .checked_mul(std::mem::size_of::<f32>())
                    .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_DECODE_BOUNDS".to_owned())?;
                if decoded_bytes as u64 > MAX_STREAM_DECODED_AUDIO_BYTES {
                    return Err("ASTRA_EMU_HEADLESS_AUDIO_DECODE_BUDGET".into());
                }
                stream.samples.extend(
                    chunk
                        .pcm_s16le
                        .chunks_exact(2)
                        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0),
                );
            }
            None => {
                stream.decoder = None;
                stream.end_of_stream = true;
            }
        }
    }
    if stream.samples.is_empty()
        || stream.sample_rate == 0
        || stream.channels == 0
        || stream.channels > 2
        || !stream
            .samples
            .len()
            .is_multiple_of(usize::from(stream.channels))
    {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_SOURCE_FORMAT".into());
    }
    stream.samples = resample_audio_linear(
        &stream.samples,
        stream.sample_rate,
        stream.channels,
        output_sample_rate,
        output_channels,
        stream.integer_pcm,
    )?;
    stream.sample_rate = output_sample_rate;
    stream.channels = output_channels;
    stream.cursor = 0;
    stream.end_of_stream = true;
    stream.fully_buffered = true;
    Ok(())
}

fn resample_audio_linear(
    samples: &[f32],
    source_sample_rate: u32,
    source_channels: u16,
    output_sample_rate: u32,
    output_channels: u16,
    integer_pcm: bool,
) -> Result<Vec<f32>, String> {
    if source_sample_rate == 0
        || output_sample_rate == 0
        || !(1..=2).contains(&source_channels)
        || !(1..=2).contains(&output_channels)
        || samples.is_empty()
        || !samples.len().is_multiple_of(usize::from(source_channels))
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_RESAMPLE_FORMAT".into());
    }
    let source_frames = samples.len() / usize::from(source_channels);
    let step_fp = (u64::from(source_sample_rate) << 16) / u64::from(output_sample_rate);
    if step_fp == 0 {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_RESAMPLE_RATIO".into());
    }
    let estimated_frames = u64::try_from(source_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(u64::from(output_sample_rate)))
        .and_then(|scaled| scaled.checked_add(u64::from(source_sample_rate) - 1))
        .map(|scaled| scaled / u64::from(source_sample_rate))
        .and_then(|frames| usize::try_from(frames).ok())
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_RESAMPLE_BOUNDS".to_owned())?;
    let output_samples = estimated_frames
        .checked_mul(usize::from(output_channels))
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_RESAMPLE_BOUNDS".to_owned())?;
    if output_samples
        .checked_mul(std::mem::size_of::<f32>())
        .is_none_or(|bytes| bytes as u64 > MAX_STREAM_DECODED_AUDIO_BYTES)
    {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_RESAMPLE_BUDGET".into());
    }
    let mut output = Vec::with_capacity(output_samples);
    let total_fp = (source_frames as u64) << 16;
    let mut phase_fp = 0_u64;
    while phase_fp < total_fp {
        let frame = (phase_fp >> 16) as usize;
        let next = (frame + 1).min(source_frames - 1);
        let fraction = (phase_fp & 0xffff) as u32;
        let read = |source_channel: usize| {
            let channel = source_channel.min(usize::from(source_channels) - 1);
            let a = samples[frame * usize::from(source_channels) + channel];
            let b = samples[next * usize::from(source_channels) + channel];
            if integer_pcm {
                let a = (a * 32768.0).round().clamp(-32768.0, 32767.0) as i32;
                let b = (b * 32768.0).round().clamp(-32768.0, 32767.0) as i32;
                let mixed = (a * (65_536 - fraction as i32) + b * fraction as i32) >> 16;
                mixed as f32 / 32768.0
            } else {
                a + (b - a) * (fraction as f32 / 65_536.0)
            }
        };
        match (source_channels, output_channels) {
            (1, 1) => output.push(read(0)),
            (1, 2) => {
                let mono = read(0);
                output.extend_from_slice(&[mono, mono]);
            }
            (2, 1) => output.push((read(0) + read(1)) * 0.5),
            (2, 2) => output.extend_from_slice(&[read(0), read(1)]),
            _ => unreachable!("audio channel bounds checked above"),
        }
        phase_fp = phase_fp
            .checked_add(step_fp)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_AUDIO_RESAMPLE_BOUNDS".to_owned())?;
    }
    if output.is_empty() {
        return Err("ASTRA_EMU_HEADLESS_AUDIO_RESAMPLE_EMPTY".into());
    }
    Ok(output)
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
        LegacyWaitRequest::Input { token_id, keys } => {
            (token_id.clone(), PendingWait::Input(keys.clone()))
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

fn pressed_input_keys(edges: &[LegacyInputEdge]) -> BTreeSet<String> {
    edges
        .iter()
        .filter(|edge| edge.pressed)
        .map(|edge| edge.control.clone())
        .collect()
}

fn retain_unconsumed_input_edges(
    edges: Vec<LegacyInputEdge>,
    consumed_keys: &BTreeSet<String>,
) -> Vec<LegacyInputEdge> {
    edges
        .into_iter()
        .filter(|edge| !consumed_keys.contains(&edge.control))
        .collect()
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
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| "ASTRA_EMU_HEADLESS_REPORT_ENCODE".to_owned())?;
    write_atomic_bytes(path, &bytes)
}

fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("partial");
    fs::write(&partial, bytes).map_err(|_| "ASTRA_EMU_HEADLESS_REPORT_WRITE".to_owned())?;
    fs::rename(partial, path).map_err(|_| "ASTRA_EMU_HEADLESS_REPORT_COMMIT".to_owned())
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use astra_emu_family_api::{
        FamilyId, LegacyScenePacketV1, LegacySceneResourceOperationV1, LegacySceneResourceStateV1,
        LegacySceneTextureCreateV1, LegacySceneTextureDescriptorV1, LegacySceneTextureUpdateV1,
    };

    #[test]
    fn coalesced_gpu_scene_retains_resource_generations_without_full_resync() {
        let queued = SceneFrame {
            sequence: 0,
            width: 1280,
            height: 720,
            clear_rgba: [0, 0, 0, 255],
            commands: vec![
                SceneCommand::ReleaseResource {
                    resource_id: "rfvp-texture-1-1".into(),
                },
                SceneCommand::Clear {
                    rgba: [1, 2, 3, 255],
                },
            ],
            semantics: None,
        };
        let latest = SceneFrame {
            sequence: 0,
            width: 1280,
            height: 720,
            clear_rgba: [4, 5, 6, 255],
            commands: vec![
                SceneCommand::ReleaseResource {
                    resource_id: "rfvp-texture-1-2".into(),
                },
                SceneCommand::Clear {
                    rgba: [7, 8, 9, 255],
                },
            ],
            semantics: None,
        };

        let merged = merge_scene_frames(queued, latest).expect("coalescing must succeed");

        assert_eq!(merged.commands.len(), 3);
        assert!(matches!(
            &merged.commands[0],
            SceneCommand::ReleaseResource { resource_id } if resource_id == "rfvp-texture-1-1"
        ));
        assert!(matches!(
            &merged.commands[1],
            SceneCommand::ReleaseResource { resource_id } if resource_id == "rfvp-texture-1-2"
        ));
        assert!(matches!(
            merged.commands[2],
            SceneCommand::Clear {
                rgba: [7, 8, 9, 255]
            }
        ));
    }

    #[test]
    fn gpu_scene_region_update_preserves_resource_id_and_generation() {
        let mut adapter = GpuSceneAdapter::default();
        let descriptor = LegacySceneTextureDescriptorV1 {
            width: 2,
            height: 1,
            format: LegacyTextureFormat::Rgba8,
        };
        let resources = LegacySceneResourceStateV1 {
            textures: [(7, descriptor)].into_iter().collect(),
        };
        let initial = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let (created, created_metrics) = adapter
            .prepare(LegacyPreparedSceneCommitV1 {
                packet: LegacyScenePacketV1 {
                    width: 2,
                    height: 1,
                    resources: vec![LegacySceneResourceOperationV1::CreateTexture(
                        LegacySceneTextureCreateV1 {
                            texture_id: 7,
                            width: 2,
                            height: 1,
                            format: LegacyTextureFormat::Rgba8,
                            content_hash: Hash256::from_sha256(&initial),
                            pixels: initial,
                        },
                    )],
                    draws: vec![],
                },
                next_resources: resources.clone(),
                reset_resources: false,
            })
            .unwrap();
        let resource_id = match &created.commands[0] {
            SceneCommand::UploadTexture { resource_id, .. } => resource_id.clone(),
            other => panic!("unexpected create command: {other:?}"),
        };

        let patch = vec![0, 0, 255, 255];
        let (updated, updated_metrics) = adapter
            .prepare(LegacyPreparedSceneCommitV1 {
                packet: LegacyScenePacketV1 {
                    width: 2,
                    height: 1,
                    resources: vec![LegacySceneResourceOperationV1::UpdateTexture(
                        LegacySceneTextureUpdateV1 {
                            texture_id: 7,
                            x: 0,
                            y: 0,
                            width: 1,
                            height: 1,
                            format: LegacyTextureFormat::Rgba8,
                            content_hash: Hash256::from_sha256(&patch),
                            pixels: patch,
                        },
                    )],
                    draws: vec![],
                },
                next_resources: resources,
                reset_resources: false,
            })
            .unwrap();

        assert_eq!(created_metrics.generation, 1);
        assert_eq!(updated_metrics.generation, 1);
        assert_eq!(updated.commands.len(), 1);
        assert!(matches!(
            &updated.commands[0],
            SceneCommand::UpdateTextureRegion { resource_id: updated_id, width: 1, height: 1, .. }
                if updated_id == &resource_id
        ));
    }

    #[test]
    fn fvp_hosted_font_overlay_fills_only_a_missing_game_font() {
        let directory = tempfile::tempdir().expect("temporary source directory");
        let registry = DesktopVfsRegistry::default();
        registry
            .bind(
                "font.missing",
                directory.path().to_str().expect("utf-8 path"),
            )
            .expect("mount without a font");
        install_fvp_hosted_font_overlay(&registry, "font.missing").expect("install fallback");
        let fallback = registry
            .list_resources("font.missing")
            .expect("list fallback mount")
            .into_iter()
            .find(|resource| resource.path == "default.ttf")
            .expect("fallback font must be mounted");
        assert_eq!(fallback.source_layer, "overlay");
        assert_eq!(fallback.byte_size, FVP_HOSTED_FALLBACK_FONT.len() as u64);

        std::fs::write(directory.path().join("default.ttf"), b"game-font")
            .expect("write game font");
        let game_registry = DesktopVfsRegistry::default();
        game_registry
            .bind("font.game", directory.path().to_str().expect("utf-8 path"))
            .expect("mount with a font");
        install_fvp_hosted_font_overlay(&game_registry, "font.game")
            .expect("leave game font intact");
        let game_font = game_registry
            .list_resources("font.game")
            .expect("list game font mount")
            .into_iter()
            .find(|resource| resource.path == "default.ttf")
            .expect("game font must be mounted");
        assert_eq!(game_font.source_layer, "base");
        assert_eq!(game_font.byte_size, b"game-font".len() as u64);
    }

    fn case_record() -> CaseRecord {
        CaseRecord {
            case_identity: "case-test".into(),
            source_id: "source-test".into(),
            relative_path: "main.hcb".into(),
            content_hash: Hash256::from_sha256(b"installation").to_string(),
            modified_ns: 1,
            byte_size: 128,
            title: "fixture".into(),
            family_override: None,
        }
    }

    fn resume_snapshot_fixture() -> HeadlessResumeSnapshotV1 {
        let bytes = vec![1, 2, 3];
        HeadlessResumeSnapshotV1 {
            schema: HEADLESS_RESUME_SNAPSHOT_SCHEMA.into(),
            build_identity_hash: Hash256::from_sha256(b"build"),
            family_provider_id: "astra.emu.family.fvp".into(),
            family_binary_hash: Hash256::from_sha256(b"family"),
            game_identity_hash: Hash256::from_sha256(b"game"),
            entry_identity_hash: Hash256::from_sha256(b"entry"),
            fixed_delta_ns: FIXED_DELTA_NS,
            stage_width: 1280,
            stage_height: 720,
            fixed_step: 42,
            session_seed: 7,
            runtime_sections: vec![RuntimeSectionPayload {
                section_id: "runtime.world".into(),
                schema: "astra.runtime.save_blob.v2".into(),
                version: SchemaVersion::new(2, 0, 0),
                codec: RuntimeSectionCodec::Raw,
                hash: Hash256::from_sha256(&bytes),
                bytes,
            }],
            driver: HeadlessDriverResumeV1 {
                fixed_step: 42,
                input_sequence: 9,
                await_sequence: 3,
                pending_inputs: vec![],
                pending_waits: BTreeMap::from([(
                    "wait.input.1".into(),
                    PendingWait::Input(vec!["enter".into()]),
                )]),
                completed_media: vec![],
                active_video: None,
                state_hash: Hash256::from_sha256(b"state"),
                active_touch: None,
            },
        }
    }

    fn resume_identity_fixture() -> HeadlessResumeIdentity<'static> {
        HeadlessResumeIdentity {
            build_identity_hash: Hash256::from_sha256(b"build"),
            family_provider_id: "astra.emu.family.fvp",
            family_binary_hash: Hash256::from_sha256(b"family"),
            game_identity_hash: Hash256::from_sha256(b"game"),
            entry_identity_hash: Hash256::from_sha256(b"entry"),
            fixed_delta_ns: FIXED_DELTA_NS,
            stage_width: 1280,
            stage_height: 720,
            session_seed: 7,
        }
    }

    #[test]
    fn semantic_scene_identity_uses_validated_resource_content_identity() {
        let pixels = vec![7, 8, 9, 255];
        let packet = LegacyScenePacketV1 {
            width: 1,
            height: 1,
            resources: vec![LegacySceneResourceOperationV1::CreateTexture(
                LegacySceneTextureCreateV1 {
                    texture_id: 11,
                    width: 1,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&pixels),
                    pixels,
                },
            )],
            draws: Vec::new(),
        };
        let commit = LegacySceneResourceStateV1::default()
            .prepare(packet)
            .unwrap();
        let same = commit.clone();
        assert_eq!(
            scene_commit_visual_hash(&commit).unwrap(),
            scene_commit_visual_hash(&same).unwrap()
        );

        let replacement = vec![9, 8, 7, 255];
        let changed_packet = LegacyScenePacketV1 {
            width: 1,
            height: 1,
            resources: vec![LegacySceneResourceOperationV1::CreateTexture(
                LegacySceneTextureCreateV1 {
                    texture_id: 11,
                    width: 1,
                    height: 1,
                    format: LegacyTextureFormat::Rgba8,
                    content_hash: Hash256::from_sha256(&replacement),
                    pixels: replacement,
                },
            )],
            draws: Vec::new(),
        };
        let changed = LegacySceneResourceStateV1::default()
            .prepare(changed_packet)
            .unwrap();
        assert_ne!(
            scene_commit_visual_hash(&commit).unwrap(),
            scene_commit_visual_hash(&changed).unwrap()
        );
    }

    #[test]
    fn fvp_probe_does_not_confuse_installation_identity_with_script_identity() {
        let request = fvp_probe_request("mount-test", "main.hcb");
        assert!(request.marker_hashes.is_empty());

        let script_identity = Hash256::from_sha256(b"script");
        let probe = profile_from_probe_report(
            &case_record(),
            LegacyProbeReport {
                family_id: FamilyId("fvp".into()),
                confidence_permyriad: 10_000,
                markers: vec![
                    "fvp.hcb.descriptor".into(),
                    "fvp.game_mode.0".into(),
                    "fvp.stage_width.1280".into(),
                    "fvp.stage_height.720".into(),
                    "fvp.nls.shift_jis".into(),
                ],
                blockers: Vec::new(),
                content_identity: script_identity,
            },
        )
        .unwrap();

        assert_eq!(probe.content_identity, script_identity);
        assert_ne!(
            probe.content_identity.to_string(),
            case_record().content_hash
        );
    }

    #[test]
    fn resume_snapshot_is_bounded_hash_checked_and_identity_bound() {
        let snapshot = resume_snapshot_fixture();
        validate_resume_snapshot(&snapshot, &resume_identity_fixture()).unwrap();
        let encoded = postcard::to_allocvec(&snapshot).unwrap();
        let decoded: HeadlessResumeSnapshotV1 = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(decoded, snapshot);

        let mut wrong_build = snapshot.clone();
        wrong_build.build_identity_hash = Hash256::from_sha256(b"other-build");
        assert_eq!(
            validate_resume_snapshot(&wrong_build, &resume_identity_fixture()).unwrap_err(),
            "ASTRA_EMU_HEADLESS_RESUME_IDENTITY"
        );

        let mut corrupt = snapshot.clone();
        corrupt.runtime_sections[0].bytes.push(4);
        assert_eq!(
            validate_resume_snapshot(&corrupt, &resume_identity_fixture()).unwrap_err(),
            "ASTRA_EMU_HEADLESS_RESUME_SECTION_INVALID"
        );

        let mut duplicate = snapshot;
        duplicate
            .runtime_sections
            .push(duplicate.runtime_sections[0].clone());
        assert_eq!(
            validate_resume_snapshot(&duplicate, &resume_identity_fixture()).unwrap_err(),
            "ASTRA_EMU_HEADLESS_RESUME_SECTION_INVALID"
        );
    }

    #[test]
    fn resume_input_rejects_ticks_before_restored_step() {
        let message = InputMessage {
            schema: "astra.user_input_sequence.v1".into(),
            session: "resume.fixture".into(),
            sequence: 1,
            tick: 41,
            event: PhysicalInput::Resume,
        };
        assert_eq!(
            validate_resume_input_ticks(&[message], 42).unwrap_err(),
            "ASTRA_EMU_HEADLESS_RESUME_INPUT_TICK"
        );
    }

    #[test]
    fn native_movie_frames_are_bounded_and_converted_to_bgra() {
        let stream = decoded_native_video_stream(
            vec![
                astra_emu_fvp::FvpMovieFrame {
                    pts_ms: 0,
                    width: 1,
                    height: 1,
                    rgba8: vec![1, 2, 3, 4],
                },
                astra_emu_fvp::FvpMovieFrame {
                    pts_ms: 17,
                    width: 1,
                    height: 1,
                    rgba8: vec![5, 6, 7, 8],
                },
            ],
            34,
        )
        .unwrap();

        assert_eq!(stream.duration_us, 34_000);
        assert_eq!(stream.frames[0].bgra8, vec![3, 2, 1, 4]);
        assert_eq!(stream.frames[1].bgra8, vec![7, 6, 5, 8]);
        assert_eq!(stream.frames[0].duration_us, 17_000);
        assert_eq!(stream.frames[1].duration_us, 17_000);
    }

    #[test]
    fn resume_snapshot_rejects_unsafe_or_future_movie_state() {
        let mut snapshot = resume_snapshot_fixture();
        snapshot.driver.active_video = Some(HeadlessVideoResumeV1 {
            playback_id: "movie.1".into(),
            resource_uri: "../private/movie.wmv".into(),
            mode: LegacyVideoMode::ModalWithAudio,
            stage_width: 1280,
            stage_height: 720,
            started_step: 40,
        });
        assert_eq!(
            validate_resume_snapshot(&snapshot, &resume_identity_fixture()).unwrap_err(),
            "ASTRA_EMU_HEADLESS_RESUME_DRIVER_STATE"
        );

        let video = snapshot.driver.active_video.as_mut().unwrap();
        video.resource_uri = "movie/opening.wmv".into();
        video.started_step = 43;
        assert_eq!(
            validate_resume_snapshot(&snapshot, &resume_identity_fixture()).unwrap_err(),
            "ASTRA_EMU_HEADLESS_RESUME_DRIVER_STATE"
        );
    }

    fn test_hash(label: &[u8]) -> String {
        Hash256::from_sha256(label).to_string()
    }

    #[test]
    fn standard_headless_report_is_review_tool_compatible() {
        let mut profile = HeadlessHostProfile::reference(
            "headless-test",
            "astra.emu.quick_case",
            test_hash(b"build"),
            test_hash(b"package"),
        );
        profile.id = "astra-emu-cli-headless".into();
        let renderer_identity = astra_headless_protocol::RendererExecutionIdentity::cpu_reference();
        let manifest = ArtifactManifest {
            schema: astra_headless_protocol::HEADLESS_ARTIFACT_MANIFEST_SCHEMA.into(),
            run_id: "review-run".into(),
            build_fingerprint: profile.build_fingerprint.clone(),
            package_hash: profile.package_hash.clone(),
            input_sequence_hash: test_hash(b"input"),
            provider_identity_hash: test_hash(b"providers"),
            renderer_identity_hash: renderer_identity.hash().unwrap(),
            renderer_identity,
            render_policy: "checkpoints".into(),
            submitted_frame_count: 1,
            rasterized_frame_count: 1,
            audio_frame_count: 0,
            submitted_scene_stream_hash: test_hash(b"scenes"),
            rasterized_frame_stream_hash: test_hash(b"frames"),
            audio_stream_hash: test_hash(b"audio"),
            audio_peak_dbfs: None,
            audio_rms_dbfs: None,
            silence: true,
            clipping: false,
            artifacts: Vec::new(),
        };
        let input = ValidatedInputSequence {
            session: "review-run".into(),
            hash: Hash256::from_sha256(b"input"),
            messages: vec![InputMessage {
                schema: astra_headless_protocol::USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: "review-run".into(),
                sequence: 7,
                tick: 12,
                event: PhysicalInput::Shutdown,
            }],
            final_tick: 12,
        };
        let zero = duration_distribution(Vec::new());
        let execution = ExecutionEvidence {
            input_trace: Vec::new(),
            visual_trace: Vec::new(),
            audio_trace: Vec::new(),
            state_trace: Vec::new(),
            checkpoints: vec![HeadlessCheckpointEvidenceV1 {
                checkpoint_id: "message".into(),
                fixed_step: 12,
                frame_hash: Hash256::from_sha256(b"frame"),
                observation_hash: Hash256::from_sha256(b"observation"),
            }],
            checkpoint_frames: Vec::new(),
            diagnostics: BTreeSet::new(),
            fixed_step: 12,
            present_sequence: 1,
            snapshot_verified: true,
            terminal: false,
            phase_timings: HeadlessPhaseTimingEvidenceV1 {
                step_total: zero,
                runtime_step: zero,
                effect_dispatch: zero,
                raster: zero,
                media: zero,
                present: zero,
            },
            runtime_samples_ns: Vec::new(),
            presentation_samples_ns: Vec::new(),
            gpu_samples: Vec::new(),
            performance_memory_after_warmup: None,
            scene_full_resync_count: 0,
            audio_underflow_count: 0,
            perfetto_trace: None,
            resume_snapshot: None,
        };

        let report = standard_headless_run_report(
            &profile,
            &manifest,
            Hash256::from_sha256(b"manifest"),
            &input,
            &execution,
        )
        .unwrap();

        assert_eq!(report.schema, STANDARD_HEADLESS_RUN_REPORT_SCHEMA);
        assert_eq!(report.status, RunStatus::Passed);
        assert_eq!(report.completed_sequence, 7);
        assert_eq!(report.checkpoint_results.len(), 1);
        assert_eq!(report.checkpoint_results[0].id, "message");
        report.validate().unwrap();
    }

    #[test]
    fn duration_distribution_uses_deterministic_nearest_rank_percentiles() {
        let distribution = duration_distribution(vec![50, 10, 40, 20, 30]);
        assert_eq!(distribution.sample_count, 5);
        assert_eq!(distribution.total_ns, 150);
        assert_eq!(distribution.median_ns, 30);
        assert_eq!(distribution.p95_ns, 50);
        assert_eq!(distribution.p99_ns, 50);
        assert_eq!(distribution.max_ns, 50);
    }

    #[test]
    fn fvp_performance_budget_requires_full_profile_bound_metric_set() {
        let profile = HeadlessHostProfile::reference(
            "headless-test",
            "astra.emu.quick_case",
            Hash256::from_sha256(b"build").to_string(),
            Hash256::from_sha256(b"package").to_string(),
        );
        let profile_hash: Hash256 = profile.hash().unwrap().parse().unwrap();
        let per_presentation = [
            ("presentation.e2e_ns", PerformanceUnit::Nanoseconds),
            ("gpu.upload_bytes", PerformanceUnit::Bytes),
            ("gpu.readback_bytes", PerformanceUnit::Bytes),
            ("heap.allocation_bytes", PerformanceUnit::Bytes),
            ("heap.allocation_count", PerformanceUnit::Count),
        ];
        let mut metrics = vec![PerformanceMetricBudget {
            id: "runtime.fixed_tick_ns".into(),
            unit: PerformanceUnit::Nanoseconds,
            min_samples: PERFORMANCE_MEASURED_PRESENTATIONS / 2,
            max_samples: PERFORMANCE_MEASURED_PRESENTATIONS / 2,
            thresholds: astra_core::PerformanceThresholds {
                min_p50: None,
                min_p95: None,
                max_p50: None,
                max_p95: None,
                max_p99: Some(PERFORMANCE_RUNTIME_P99_NS),
                max: None,
            },
        }];
        metrics.extend(
            per_presentation
                .into_iter()
                .map(|(id, unit)| PerformanceMetricBudget {
                    id: id.into(),
                    unit,
                    min_samples: PERFORMANCE_MEASURED_PRESENTATIONS,
                    max_samples: PERFORMANCE_MEASURED_PRESENTATIONS,
                    thresholds: astra_core::PerformanceThresholds {
                        min_p50: None,
                        min_p95: None,
                        max_p50: None,
                        max_p95: (id != "presentation.e2e_ns").then_some(0),
                        max_p99: (id == "presentation.e2e_ns")
                            .then_some(PERFORMANCE_PRESENTATION_P99_NS),
                        max: None,
                    },
                }),
        );
        for id in [
            "deadline.miss_count",
            "audio.underflow_count",
            "scene.full_resync_count",
            "trace.dropped_count",
        ] {
            metrics.push(PerformanceMetricBudget {
                id: id.into(),
                unit: PerformanceUnit::Count,
                min_samples: 1,
                max_samples: 1,
                thresholds: astra_core::PerformanceThresholds {
                    min_p50: None,
                    min_p95: None,
                    max_p50: None,
                    max_p95: None,
                    max_p99: None,
                    max: Some(0),
                },
            });
        }
        for id in [
            "memory.working_set_bytes",
            "memory.private_bytes",
            "memory.growth_bytes",
        ] {
            metrics.push(PerformanceMetricBudget {
                id: id.into(),
                unit: PerformanceUnit::Bytes,
                min_samples: 1,
                max_samples: 1,
                thresholds: astra_core::PerformanceThresholds {
                    min_p50: None,
                    min_p95: None,
                    max_p50: None,
                    max_p95: None,
                    max_p99: None,
                    max: Some(u64::MAX),
                },
            });
        }
        let budget = PerformanceBudget {
            schema: astra_core::PERFORMANCE_BUDGET_SCHEMA.into(),
            budget_id: "fvp-real-game-120hz".into(),
            target: profile.target.clone(),
            profile: profile.product_profile.clone(),
            profile_hash: profile_hash.to_string(),
            min_run_duration_us: 600_000_000,
            metrics,
        };
        validate_fvp_performance_budget(&budget, &profile, profile_hash).unwrap();
    }

    #[test]
    fn extensionless_audio_uses_bounded_signature_detection() {
        assert_eq!(
            resolve_audio_codec(LegacyAudioEncoding::Unknown, "bgm/002", b"OggSdata").unwrap(),
            "ogg"
        );
        assert_eq!(
            resolve_audio_codec(
                LegacyAudioEncoding::Unknown,
                "se/003",
                b"RIFF\x04\0\0\0WAVEdata",
            )
            .unwrap(),
            "wav"
        );
    }

    #[test]
    fn audio_codec_identity_mismatch_is_blocking() {
        assert_eq!(
            resolve_audio_codec(LegacyAudioEncoding::Wav, "bgm/002", b"OggSdata").unwrap_err(),
            "ASTRA_EMU_HEADLESS_AUDIO_CODEC_IDENTITY_MISMATCH"
        );
        assert_eq!(
            resolve_audio_codec(LegacyAudioEncoding::Unknown, "bgm/002", b"opaque").unwrap_err(),
            "ASTRA_EMU_HEADLESS_AUDIO_CODEC_UNIDENTIFIED"
        );
    }

    #[test]
    fn audio_resampler_matches_fixed_point_linear_mono_to_stereo_contract() {
        let high = 32767.0 / 32768.0;
        let converted = resample_audio_linear(&[0.0, high], 24_000, 1, 48_000, 2, true).unwrap();

        assert_eq!(converted.len(), 8);
        assert!(converted.chunks_exact(2).all(|frame| frame[0] == frame[1]));
        assert_eq!(converted[0], 0.0);
        assert_eq!(converted[2], 16383.0 / 32768.0);
        assert_eq!(converted[4], high);
        assert_eq!(converted[6], high);
    }

    #[test]
    fn native_key_mapping_is_explicit_and_does_not_capture_unbound_keys() {
        assert_eq!(
            native_key_control(Some("Enter"), "Unidentified"),
            Some("enter")
        );
        assert_eq!(
            native_key_control(Some("ArrowLeft"), "Unidentified"),
            Some("arrow_left")
        );
        assert_eq!(native_key_control(None, "Space"), Some("space"));
        assert_eq!(
            native_key_control(Some("Shift"), "ShiftLeft"),
            Some("shift")
        );
        assert_eq!(native_key_control(None, "ControlRight"), Some("control"));
        assert_eq!(native_key_control(Some("F12"), "F12"), None);
    }

    #[test]
    fn input_await_consumes_matching_press_and_release_edges() {
        let edges = vec![
            LegacyInputEdge {
                control: "enter".into(),
                pressed: true,
                value: 1.0,
                sequence: 1,
            },
            LegacyInputEdge {
                control: "enter".into(),
                pressed: false,
                value: 0.0,
                sequence: 2,
            },
            LegacyInputEdge {
                control: "arrow_left".into(),
                pressed: true,
                value: 1.0,
                sequence: 3,
            },
        ];
        assert_eq!(
            pressed_input_keys(&edges),
            BTreeSet::from(["enter".to_string(), "arrow_left".to_string()])
        );

        let retained = retain_unconsumed_input_edges(edges, &BTreeSet::from(["enter".to_string()]));

        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].control, "arrow_left");
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
