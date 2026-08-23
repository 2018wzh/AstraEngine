use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

fn cli_writable_root(family_id: &str, game_id: Hash256) -> Result<PathBuf, String> {
    let project = directories::ProjectDirs::from("dev", "AstraEngine", "AstraEMU")
        .ok_or_else(|| "ASTRA_EMU_WRITABLE_DATA_DIR".to_owned())?;
    Ok(project
        .data_dir()
        .join("SavedGames")
        .join(family_id)
        .join(game_id.to_string()))
}

use astra_core::{
    Hash256, PerformanceBudget, PerformanceMetricBudget, PerformanceRecorder,
    PerformanceRunIdentity, PerformanceStatus, PerformanceTraceManifest, PerformanceUnit,
    SchemaVersion, PERFORMANCE_TRACE_MANIFEST_SCHEMA,
};
#[cfg(test)]
use astra_emu_family_api::LegacyProbeReport;
use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV7, LegacyAudioSampleFormat,
    LegacyAwaitResult, LegacyDrawV1, LegacyInputEdge, LegacyPcmBufferV7, LegacyProbeRequest,
    LegacyResourceRead, LegacyRuntimeHostCtx, LegacyTextureFilter, LegacyTextureFormat,
    LegacyVfsReader, LegacyVideoCommandV1, LegacyVideoMode,
};
use astra_emu_family_support::{
    verify_vfs, FamilyAudioService, LegacyMountedVfsReaderAdapter, LegacyVfsFamilyRegistry,
};
use astra_emu_manager_core::{
    evidence_vm_coverage_ids, AstraEmuRuntimeProvider, CancellationToken, CaseRecord,
    DesktopGrantedSource, DesktopVfsRegistry, EmuCaseProfile, Library, LibraryScanner, ScanLimits,
    SourceGrant,
};
use astra_emu_minori::MinoriVfsFamilyFactory;
use astra_headless_protocol::{
    ArtifactEntry, ArtifactManifest, ButtonState, CheckpointResult, Diagnostic, GamepadControl,
    InputMessage, ObservationPredicate, PhysicalInput, PointerButton, RunReport, RunStatus,
    TouchPhase, HEADLESS_RUN_REPORT_SCHEMA as STANDARD_HEADLESS_RUN_REPORT_SCHEMA,
};
use astra_media::{
    DecodeBindingContext, DecodeOutput as MediaDecodeOutput, DecodeProviderRegistry, DecodeRequest,
    DecodedVideoFrame, ImageDecodeProvider, PlayerDecodedAudio,
};
use astra_media_core::{
    BlendMode, CpuFilterExecutor, CpuFrame, Layer2DContent, Layer2DState, Layer2DTransaction,
    MeshDraw2D, MeshMaterial2D, MeshVertex2D, OwnedPixelBuffer, RectI, RenderTargetFormat,
    RetainedLayer2DState, SceneCommand, SceneCompositing2D, Surface2DFormat, TextureFilter2D,
    TextureFrame,
};
use astra_observability::{
    sample_process_memory, PerfettoFlowPhase, PerfettoTraceConfig, PerfettoTraceSummary,
    PerfettoTraceWriter,
};
use astra_platform::{
    DecodeKind, DecodeOutput, GpuAdapterPolicy, GpuBackendPolicy, GpuDeviceTypePolicy,
    HeadlessArtifactPolicy, HeadlessArtifactRetention, HeadlessHostProfile, HeadlessReadbackPolicy,
    HeadlessRenderPolicy, PlatformDecodeRequest, PlatformHostClient, PlatformHostFactory,
    RgbaFrame, SceneFrame, ScenePresentReceipt, SurfaceHandle, SurfaceRequest, WindowRequest,
};
#[cfg(windows)]
use astra_platform::{FixedDeadlineScheduler, HostLaunchProfile};
#[cfg(target_os = "windows")]
use astra_platform::{
    GamepadControl as PlatformGamepadControl, InputState, PlatformEventKind,
    PointerButton as PlatformPointerButton, TouchPhase as PlatformTouchPhase, WindowHandle,
};
use astra_platform_headless::{
    HeadlessGpuFrameSample, HeadlessPerformanceObserver, HeadlessPlatformFactory,
};
use astra_plugin::ProductRuntimeProvider;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProviderInstanceId, RuntimeAwaitResult, RuntimeInputEdge,
    RuntimeLiveAudioCommand, RuntimeLiveAudioEncoding, RuntimeLiveAudioPacket,
    RuntimeLiveAudioSampleFormat, RuntimeLiveBlendMode, RuntimeLiveDraw, RuntimeLivePcmBuffer,
    RuntimeLiveResourceScene, RuntimeLiveSceneResourceOperation, RuntimeLiveSceneTransaction,
    RuntimeLiveTextureFilter, RuntimeLiveTextureFormat, RuntimeLiveVideoCommand,
    RuntimeLiveVideoCommandKind, RuntimeLiveWait, RuntimeLiveWaitKind, RuntimeOpenRequest,
    RuntimeProviderResult, RuntimeSectionCodec, RuntimeSectionPayload, RuntimeStepBudget,
    RuntimeStepInput, RuntimeStepMode, RuntimeTickIntegrityMode,
};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use rfvp_astra_provider::{
    fvp_movie_compatibility, open_fvp_movie_packet_stream, FvpMovieAudioChunk,
    FvpMovieCompatibility, FvpMovieFrame, FvpMoviePacket, FvpMoviePacketStream,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc as tokio_mpsc, task::JoinHandle};

use crate::{
    family_host::CliFamilyHostConfig,
    input::{read_input_sequence, ValidatedInputSequence},
    rasterizer::{CpuStageRasterizer, PreparedRenderFrame},
};

fn legacy_live_audio_packet(packet: RuntimeLiveAudioPacket) -> LegacyAudioPacketV7 {
    LegacyAudioPacketV7 {
        sequence: packet.sequence,
        stream_id: packet.stream_id,
        sample_rate: packet.sample_rate,
        channels: packet.channels,
        pcm: match packet.pcm {
            RuntimeLivePcmBuffer::I16(samples) => LegacyPcmBufferV7::I16(samples),
            RuntimeLivePcmBuffer::F32(samples) => LegacyPcmBufferV7::F32(samples),
        },
    }
}

fn legacy_live_audio_command(command: RuntimeLiveAudioCommand) -> LegacyAudioCommandV1 {
    match command {
        RuntimeLiveAudioCommand::LoadResource {
            stream_id,
            encoding,
            resource_uri,
            ..
        } => LegacyAudioCommandV1::LoadResource {
            stream_id,
            encoding: match encoding {
                RuntimeLiveAudioEncoding::Unknown => LegacyAudioEncoding::Unknown,
                RuntimeLiveAudioEncoding::Wav => LegacyAudioEncoding::Wav,
                RuntimeLiveAudioEncoding::Ogg => LegacyAudioEncoding::Ogg,
                RuntimeLiveAudioEncoding::Mp3 => LegacyAudioEncoding::Mp3,
                RuntimeLiveAudioEncoding::Flac => LegacyAudioEncoding::Flac,
            },
            resource_uri,
        },
        RuntimeLiveAudioCommand::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format,
            ..
        } => LegacyAudioCommandV1::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format: match sample_format {
                RuntimeLiveAudioSampleFormat::I16 => LegacyAudioSampleFormat::I16,
                RuntimeLiveAudioSampleFormat::F32 => LegacyAudioSampleFormat::F32,
            },
        },
        RuntimeLiveAudioCommand::SubmitI16 {
            stream_id, samples, ..
        } => LegacyAudioCommandV1::SubmitI16 { stream_id, samples },
        RuntimeLiveAudioCommand::SubmitF32 {
            stream_id, samples, ..
        } => LegacyAudioCommandV1::SubmitF32 { stream_id, samples },
        RuntimeLiveAudioCommand::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
            ..
        } => LegacyAudioCommandV1::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        },
        RuntimeLiveAudioCommand::Stop {
            stream_id, fade_ms, ..
        } => LegacyAudioCommandV1::Stop { stream_id, fade_ms },
        RuntimeLiveAudioCommand::Pause { stream_id, .. } => {
            LegacyAudioCommandV1::Pause { stream_id }
        }
        RuntimeLiveAudioCommand::Resume { stream_id, .. } => {
            LegacyAudioCommandV1::Resume { stream_id }
        }
        RuntimeLiveAudioCommand::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
            ..
        } => LegacyAudioCommandV1::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
        },
        RuntimeLiveAudioCommand::DestroyStream { stream_id, .. } => {
            LegacyAudioCommandV1::DestroyStream { stream_id }
        }
        RuntimeLiveAudioCommand::MasterVolume { volume, .. } => {
            LegacyAudioCommandV1::MasterVolume { volume }
        }
    }
}

fn legacy_live_video_command(command: RuntimeLiveVideoCommand) -> LegacyVideoCommandV1 {
    match command.command {
        RuntimeLiveVideoCommandKind::Play {
            playback_id,
            resource_uri,
            mode,
            stage_width,
            stage_height,
        } => LegacyVideoCommandV1::Play {
            playback_id,
            resource_uri,
            mode: match mode {
                astra_plugin_abi::RuntimeLiveVideoMode::ModalWithAudio => {
                    LegacyVideoMode::ModalWithAudio
                }
                astra_plugin_abi::RuntimeLiveVideoMode::LayerNoAudio => {
                    LegacyVideoMode::LayerNoAudio
                }
            },
            stage_width,
            stage_height,
        },
        RuntimeLiveVideoCommandKind::Stop { playback_id } => {
            LegacyVideoCommandV1::Stop { playback_id }
        }
    }
}

fn live_wait_condition(wait: RuntimeLiveWait, step: u64, delta_ns: u64) -> (String, PendingWait) {
    let token_id = wait.token_id;
    let condition = match wait.kind {
        RuntimeLiveWaitKind::Frame { frames } => {
            PendingWait::DueStep(step.saturating_add(u64::from(frames).max(1)))
        }
        RuntimeLiveWaitKind::Time { milliseconds } => {
            let ticks = u64::from(milliseconds)
                .saturating_mul(1_000_000)
                .saturating_add(delta_ns.saturating_sub(1))
                / delta_ns.max(1);
            PendingWait::DueStep(step.saturating_add(ticks.max(1)))
        }
        RuntimeLiveWaitKind::Input { keys } => PendingWait::Input(keys),
        RuntimeLiveWaitKind::MediaFence { media_id } => PendingWait::Media(media_id),
        RuntimeLiveWaitKind::PresentationFence { .. } => PendingWait::Presentation,
        RuntimeLiveWaitKind::ProviderCompletion { .. } => PendingWait::Unsupported,
    };
    (token_id, condition)
}

pub const HEADLESS_RUN_REPORT_SCHEMA: &str = "astra.emu.headless_run_report.v3";
const FIXED_DELTA_NS: u64 = 16_666_667;
const MAX_MOVIE_FRAMES: usize = 18_000;
const MAX_MOVIE_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAX_MOVIE_AUDIO_SAMPLES: usize = 64 * 1024 * 1024;
const MOVIE_AUDIO_STREAM_BASE: u32 = 0xF000_0000;
#[cfg(target_os = "windows")]
/// Matches the shared WGPU scene resource budget.  Native semantic uploads
/// are not RGBA frame payloads, so the platform command bound must admit a
/// bounded texture delta without weakening the renderer's own resource cap.
const MAX_NATIVE_SCENE_UPLOAD_BYTES: usize = 64 * 1024 * 1024;
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
    pub extension: Option<ExtensionBinding>,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub video_provider: String,
    pub artifact_retention: String,
    pub frame_sample_interval: u64,
    /// Presentation cadence. Runtime simulation remains fixed at 60 Hz; 120 Hz
    /// is two GPU presentations per simulation tick.
    pub presentation_rate_hz: u32,
    pub perfetto_trace: Option<PathBuf>,
    pub audit_all_resources: bool,
    pub performance: Option<HeadlessPerformanceArtifacts>,
}

#[derive(Debug, Clone)]
pub struct ExtensionBinding {
    pub library: PathBuf,
    pub timeout_ms: u32,
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
    pub extension: Option<ExtensionBinding>,
    pub enable_audio: bool,
    pub perfetto_trace: Option<PathBuf>,
    pub input_path: Option<PathBuf>,
    pub max_fixed_steps: Option<u64>,
    pub mode: NativeLaunchMode,
}

#[derive(Debug, Clone)]
pub enum NativeLaunchMode {
    Interactive,
    WindowedE2 { artifact_root: PathBuf },
}

pub const WINDOWED_E2_REPORT_SCHEMA: &str = "astra.emu.windowed_e2_report.v1";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowedE2CheckpointV1 {
    pub checkpoint_id: String,
    pub fixed_step: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowedE2ReportV1 {
    pub schema: String,
    pub family_id: String,
    pub family_provider_id: String,
    pub family_binary_hash: Hash256,
    pub build_identity_hash: Hash256,
    pub profile_hash: Hash256,
    pub package_hash: Hash256,
    pub fixed_steps: u64,
    pub terminal_reached: bool,
    pub external_input_rejected: u64,
    pub checkpoints: Vec<WindowedE2CheckpointV1>,
    pub diagnostics: Vec<String>,
}

impl NativeLaunchMode {
    #[cfg(target_os = "windows")]
    fn is_windowed_e2(&self) -> bool {
        matches!(self, Self::WindowedE2 { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadlessCheckpointEvidenceV1 {
    pub checkpoint_id: String,
    pub fixed_step: u64,
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
pub struct HeadlessFrameSampleV1 {
    pub sequence: u64,
    pub fixed_step: u64,
    pub mean_rgba: [u8; 4],
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
    pub package_hash: Hash256,
    pub frame_samples: Vec<HeadlessFrameSampleV1>,
    pub artifact_manifest_hash: Hash256,
    pub fixed_steps: u64,
    pub presented_frames: u64,
    pub frame_sample_interval: u64,
    pub consumed_input_messages: u64,
    pub terminal_reached: bool,
    pub coverage_ids: Vec<String>,
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
    let options: rfvp_astra_provider::FvpVfsFamilyOptions =
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
    let launch_started = Instant::now();
    let windowed_e2 = launch.mode.is_windowed_e2();
    let windowed_artifact_root = match &launch.mode {
        NativeLaunchMode::Interactive => None,
        NativeLaunchMode::WindowedE2 { artifact_root } => {
            if launch.input_path.is_none() {
                return Err("ASTRA_EMU_WINDOWED_E2_INPUT_REQUIRED".into());
            }
            fs::create_dir_all(artifact_root)
                .map_err(|_| "ASTRA_EMU_WINDOWED_E2_ARTIFACT_ROOT".to_owned())?;
            Some(artifact_root.clone())
        }
    };
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
    let build_identity_hash = Hash256::from_sha256(
        &fs::read(&executable).map_err(|_| "ASTRA_EMU_EXECUTABLE_READ".to_owned())?,
    );
    let mount_seed = Hash256::from_sha256(
        format!("{}\0{}", launch.family_id, game_root.to_string_lossy()).as_bytes(),
    );
    let mount_set_id = format!("native-{}", &mount_seed.to_string()[7..39]);
    let phase_started = Instant::now();
    let prepared = prepare_family_case(
        &launch.family_id,
        &game_root,
        &launch.mount_profile,
        launch.entry.as_deref(),
        &mount_set_id,
    )?;
    record_native_launch_phase("case_prepare", phase_started, launch_started);
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
    let phase_started = Instant::now();
    let family_host = astra_emu_manager_core::AstraEmuFamilyHost::new(prepared.reader.clone());
    let (family, family_binary_hash) =
        family_config.create_provider_with_identity(family_host.services())?;
    let family_provider_id = family.descriptor().provider_id.clone();
    record_native_launch_phase("family_load", phase_started, launch_started);
    let mut runtime = AstraEmuRuntimeProvider::new(family, family_host)?;
    runtime.create_instance(ProviderInstanceId("astra.emu.cli.native.instance".into()))?;
    let phase_started = Instant::now();
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
    record_native_launch_phase("family_probe", phase_started, launch_started);
    bind_extension(
        &runtime,
        launch.extension.as_ref(),
        "astra.emu.cli.native.extension",
        &launch.family_id,
        probe.content_identity,
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
    runtime.bind_writable_root_for_open(
        "astra-emu-native-case",
        seed,
        probe.content_identity,
        cli_writable_root(&launch.family_id, game_identity_hash)?,
    )?;
    let phase_started = Instant::now();
    let open = runtime.open(RuntimeOpenRequest {
        target_id: "astra-emu-native-case".into(),
        profile: format!("{}-v1", launch.family_id),
        locale: "und".into(),
        seed,
        integrity_mode: RuntimeTickIntegrityMode::Shipping,
        executor: astra_plugin_abi::RuntimeExecutorConfig::serial(),
        package_hash: game_identity_hash.to_string(),
        sections: vec![section],
    })?;
    record_native_launch_phase("runtime_open", phase_started, launch_started);
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
    let profile_hash: Hash256 = host_profile
        .hash()
        .map_err(|error| error.to_string())?
        .parse()
        .map_err(|_| "ASTRA_EMU_NATIVE_PROFILE_HASH".to_owned())?;
    let phase_started = Instant::now();
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
    record_native_launch_phase("platform_open", phase_started, launch_started);
    tracing::info!(
        event = "astra_emu_cli_native_session_opened",
        family = launch.family_id.as_str(),
        stage_width,
        stage_height,
        audio_enabled = launch.enable_audio
    );

    let phase_started = Instant::now();
    let mut driver = RuntimeDriver::new(
        &mut runtime,
        open.session_id.clone(),
        &host.client,
        surface,
        RuntimeDriverConfig {
            seed,
            delta_ns: probe.runtime.fixed_delta_ns,
            audio_enabled: launch.enable_audio,
            frame_sample_interval: 1,
            perfetto_trace: launch.perfetto_trace.clone(),
            capture_performance_samples: false,
            presentation: PresentationPath::NativeGpu,
            presentation_substeps: 1,
            audio_pump: AudioPumpPolicy::Realtime {
                target_latency_ms: 180,
                refill_low_water_ms: 120,
            },
        },
    )?;
    record_native_launch_phase("driver_ready", phase_started, launch_started);
    let mut viewport = NativeViewport {
        window_width: stage_width,
        window_height: stage_height,
        stage_width,
        stage_height,
    };
    let mut suspended = false;
    let mut native_input_cursor = 0usize;
    let mut native_shutdown_requested = false;
    let mut windowed_checkpoints = Vec::<WindowedE2CheckpointV1>::new();
    let mut external_input_rejected = 0_u64;
    let fixed_step_duration = std::time::Duration::from_nanos(probe.runtime.fixed_delta_ns);
    let windowed_diagnostics = Vec::new();
    if let Some(input) = native_input.as_ref() {
        let due = consume_native_inputs_due(
            &mut driver,
            &input.messages,
            &mut native_input_cursor,
            windowed_e2,
        )?;
        native_shutdown_requested = due.shutdown_requested;
        if windowed_e2 {
            for checkpoint_id in due.checkpoints {
                windowed_checkpoints.push(
                    capture_windowed_checkpoint(&driver, &host.client, surface, checkpoint_id)
                        .await?,
                );
            }
        }
    }
    // Build the initial retained resource set before starting the absolute-
    // deadline scheduler or accepting gameplay input. Three quiet steps after
    // the last resource mutation cover delayed startup transactions without a
    // fixed family-specific tick count. A startup that never converges is a
    // blocking error rather than an unbounded warmup or a hidden frame drop.
    if !native_shutdown_requested && !driver.terminal {
        const REQUIRED_STABLE_PREWARM_STEPS: u8 = 3;
        const MAX_PREWARM_STEPS: u64 = 120;
        let prewarm_started = Instant::now();
        let mut prewarm_steps = 0_u64;
        let mut resource_activity_seen = false;
        let mut stable_steps = 0_u8;
        while !driver.terminal && stable_steps < REQUIRED_STABLE_PREWARM_STEPS {
            if prewarm_steps == MAX_PREWARM_STEPS {
                return Err("ASTRA_EMU_NATIVE_PREWARM_DID_NOT_CONVERGE".into());
            }
            driver.prewarm_step().await?;
            prewarm_steps += 1;
            if driver.last_step_resource_activity {
                resource_activity_seen = true;
                stable_steps = 0;
            } else if resource_activity_seen {
                stable_steps += 1;
            }
        }
        if !driver.terminal && !resource_activity_seen {
            return Err("ASTRA_EMU_NATIVE_PREWARM_RESOURCE_ACTIVITY_MISSING".into());
        }
        tracing::info!(
            event = "astra.emu.native_prewarm_completed",
            fixed_step = driver.fixed_step,
            prewarm_steps,
            duration_ns = u64::try_from(prewarm_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            stable_steps,
            "completed native retained-resource prewarm"
        );
        if let Some(input) = native_input.as_ref() {
            let input_due = consume_native_inputs_due(
                &mut driver,
                &input.messages,
                &mut native_input_cursor,
                windowed_e2,
            )?;
            native_shutdown_requested = input_due.shutdown_requested;
            if windowed_e2 {
                for checkpoint_id in input_due.checkpoints {
                    windowed_checkpoints.push(
                        capture_windowed_checkpoint(&driver, &host.client, surface, checkpoint_id)
                            .await?,
                    );
                }
            }
        }
        if launch
            .max_fixed_steps
            .is_some_and(|limit| driver.fixed_step >= limit)
        {
            native_shutdown_requested = true;
        }
    }
    let scheduler =
        FixedDeadlineScheduler::after_completed_step(fixed_step_duration, driver.fixed_step)
            .map_err(str::to_owned)?;
    let mut scheduler = scheduler;
    let run_result: Result<(), String> = async {
        loop {
            if native_shutdown_requested {
                break Ok(());
            }
            if suspended {
                let event = match host.events.recv().await {
                    Ok(event) => event,
                    Err(error) => break Err(error.to_string()),
                };
                match process_native_event(
                    &mut driver,
                    window,
                    &mut viewport,
                    event.kind,
                    windowed_e2,
                    &mut external_input_rejected,
                ) {
                    Ok(NativeEventAction::Continue) => {}
                    Ok(NativeEventAction::Suspend(value)) => {
                        driver.audio.set_suspended(value)?;
                        suspended = value;
                    }
                    Ok(NativeEventAction::Close) => break Ok(()),
                    Err(error) => break Err(error),
                }
                continue;
            }
            let deadline =
                tokio::time::sleep_until(tokio::time::Instant::from_std(scheduler.next_deadline()));
            tokio::pin!(deadline);
            tokio::select! {
                // A burst of native window/user events must not starve an already
                // due fixed tick.  The default unbiased selection can repeatedly
                // choose the ready event branch while the deadline is ready too,
                // turning a bounded event burst into artificial fixed-step debt.
                // Keep the absolute-deadline branch first and drain events only
                // when no tick is due.
                biased;
                _ = &mut deadline => {
                    let due = scheduler.consume_due(Instant::now()).map_err(|debt| {
                        format!(
                            "ASTRA_FIXED_DEADLINE_DEBT:{}:{}",
                            debt.overdue_steps,
                            debt.lateness.as_nanos()
                        )
                    })?;
                    let Some(due) = due else { continue };
                    for _ in 0..due.steps {
                        driver.step().await?;
                        if driver.terminal {
                            break;
                        }
                        if let Some(input) = native_input.as_ref() {
                            let input_due = consume_native_inputs_due(
                                &mut driver,
                                &input.messages,
                                &mut native_input_cursor,
                                windowed_e2,
                            )?;
                            native_shutdown_requested = input_due.shutdown_requested;
                            if windowed_e2 {
                                for checkpoint_id in input_due.checkpoints {
                                    windowed_checkpoints.push(
                                        capture_windowed_checkpoint(
                                            &driver,
                                            &host.client,
                                            surface,
                                            checkpoint_id,
                                        )
                                        .await?,
                                    );
                                }
                            }
                        }
                        if launch.max_fixed_steps.is_some_and(|limit| driver.fixed_step >= limit) {
                            native_shutdown_requested = true;
                            break;
                        }
                    }
                }
                event = host.events.recv() => {
                    let event = match event {
                        Ok(event) => event,
                        Err(error) => break Err(error.to_string()),
                    };
                    match process_native_event(
                        &mut driver,
                        window,
                        &mut viewport,
                        event.kind,
                        windowed_e2,
                        &mut external_input_rejected,
                    ) {
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
        }
    }
    .await;
    let fixed_step = driver.fixed_step;
    let terminal_reached = driver.terminal;
    let audio_resource_cleanup = driver.flush_pending_audio_commands().await;
    let scene_cleanup = driver.drain_pending_scene_presents().await;
    let perfetto_cleanup = driver.finish_perfetto().map(|_| ());
    let media_cleanup = driver.close_active_media().await;
    let audio_cleanup = driver.audio.shutdown(&host.client).await.map(|_| ());
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
        ("audio_resource", audio_resource_cleanup),
        ("scene", scene_cleanup),
        ("perfetto", perfetto_cleanup),
        ("media", media_cleanup),
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
    if let Some(artifact_root) = windowed_artifact_root {
        if native_input.is_none() {
            return Err("ASTRA_EMU_WINDOWED_E2_INPUT_REQUIRED".into());
        }
        let report = WindowedE2ReportV1 {
            schema: WINDOWED_E2_REPORT_SCHEMA.to_owned(),
            family_id: launch.family_id.clone(),
            family_provider_id,
            family_binary_hash,
            build_identity_hash,
            profile_hash,
            package_hash: game_identity_hash,
            fixed_steps: fixed_step,
            terminal_reached,
            external_input_rejected,
            checkpoints: windowed_checkpoints,
            diagnostics: windowed_diagnostics,
        };
        let report_bytes = serde_json::to_vec_pretty(&report)
            .map_err(|_| "ASTRA_EMU_WINDOWED_E2_REPORT_ENCODE".to_owned())?;
        fs::write(artifact_root.join("windowed-e2-report.json"), report_bytes)
            .map_err(|_| "ASTRA_EMU_WINDOWED_E2_REPORT_WRITE".to_owned())?;
    }
    tracing::info!(
        event = "astra_emu_cli_native_session_closed",
        fixed_step,
        family = launch.family_id.as_str()
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn record_native_launch_phase(phase: &'static str, started: Instant, launch_started: Instant) {
    let phase_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let total_ms = launch_started.elapsed().as_secs_f64() * 1_000.0;
    tracing::info!(
        event = "astra_emu_cli_native_launch_phase",
        phase,
        phase_ms,
        total_ms
    );
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
    let family_host = astra_emu_manager_core::AstraEmuFamilyHost::new(prepared.reader.clone());
    let (family, family_binary_hash) =
        family_config.create_provider_with_identity(family_host.services())?;
    let family_provider_id = family.descriptor().provider_id.clone();
    let mut runtime = AstraEmuRuntimeProvider::new(family, family_host)?;
    runtime.create_instance(ProviderInstanceId("astra.emu.cli.headless.instance".into()))?;
    let mut probe = probe_profile(
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
    bind_extension(
        &runtime,
        launch.extension.as_ref(),
        "astra.emu.cli.headless.extension",
        &launch.family_id,
        probe.content_identity,
    )?;
    if launch.perfetto_trace.is_none() && launch.performance.is_none() {
        probe
            .runtime
            .family_options
            .insert("astra.hosted_trace_profile".into(), "evidence".into());
    }
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
    runtime.bind_writable_root_for_open(
        "astra-emu-headless-case",
        seed,
        probe.content_identity,
        cli_writable_root(&launch.family_id, game_identity_hash)?,
    )?;
    let open = runtime.open(RuntimeOpenRequest {
        target_id: "astra-emu-headless-case".into(),
        profile: format!("{}-v1", launch.family_id),
        locale: "und".into(),
        seed,
        integrity_mode: if launch.perfetto_trace.is_some() || launch.performance.is_some() {
            RuntimeTickIntegrityMode::Shipping
        } else {
            RuntimeTickIntegrityMode::Evidence
        },
        executor: astra_plugin_abi::RuntimeExecutorConfig::serial(),
        package_hash: game_identity_hash.to_string(),
        sections: vec![section],
    })?;
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
            frame_sample_interval: launch.frame_sample_interval,
            presentation: PresentationPath::NativeGpu,
            presentation_substeps: (launch.presentation_rate_hz / 60) as u8,
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
        let (_, family_report) = runtime.shutdown_with_family_report(open.session_id.clone())?;
        host.client
            .shutdown()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(family_report)
    }
    .await;
    prepared.evidence.cleanup();
    let (mut execution, vfs_access, resource_audit, family_report) = match (result, cleanup) {
        (Ok((execution, access, audit)), Ok(family_report)) => {
            (execution, access, audit, family_report)
        }
        (Err(error), Ok(_)) => return Err(error),
        (Ok(_), Err(cleanup)) => return Err(cleanup),
        (Err(error), Err(cleanup)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_RUN_AND_CLEANUP_FAILED:{error};{cleanup}"
            ))
        }
    };
    let coverage_ids = evidence_vm_coverage_ids(&family_report.evidence_vm_trace);
    if let Some(observer) = gpu_observer {
        execution.gpu_samples = observer.finish()?;
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
        package_hash: game_identity_hash,
        frame_samples: execution.frame_samples,
        artifact_manifest_hash,
        fixed_steps: execution.fixed_step,
        presented_frames: execution.present_sequence,
        frame_sample_interval: launch.frame_sample_interval,
        consumed_input_messages: input.messages.len() as u64,
        terminal_reached: execution.terminal,
        coverage_ids,
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
            steps.extend(["session.shutdown".into(), "host.shutdown".into()]);
            steps
        },
        diagnostic_codes: execution.diagnostics.into_iter().collect(),
    };
    let report_path = launch.artifact_root.join("astra-emu-headless-run.json");
    write_atomic_json(&report_path, &report)?;
    Ok(report)
}

fn bind_extension(
    runtime: &AstraEmuRuntimeProvider,
    binding: Option<&ExtensionBinding>,
    instance_id: &str,
    family_id: &str,
    family_game_id: Hash256,
) -> Result<(), String> {
    let Some(binding) = binding else {
        return Ok(());
    };
    let provider = astra_emu_manager_core::LoadedExtensionHookProvider::load(
        &binding.library,
        instance_id,
        family_id,
        family_game_id.to_string(),
    )
    .map_err(|error| error.to_string())?;
    runtime.bind_hook_provider(family_game_id, binding.timeout_ms, Arc::new(provider))
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
                // The shared Headless v3 schema still names this integrity
                // field `observation_hash`. AstraEMU no longer computes a
                // runtime observation digest, so bind it to the persisted
                // artifact manifest instead.
                observation_hash: manifest_hash.to_string(),
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
    if let Some(performance) = &launch.performance {
        if launch.frame_sample_interval != 1
            || launch.presentation_rate_hz != 120
            || launch.perfetto_trace.is_none()
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
    let hash = Hash256::from_sha256(&bytes);
    Ok(RuntimeSectionPayload {
        section_id: "emu.case_profile".into(),
        schema: "astra.emu.case_profile.v1".into(),
        version: SchemaVersion::new(1, 0, 0),
        codec: RuntimeSectionCodec::Postcard,
        hash,
        bytes,
    })
}

struct ExecutionEvidence {
    frame_samples: Vec<HeadlessFrameSampleV1>,
    checkpoints: Vec<HeadlessCheckpointEvidenceV1>,
    checkpoint_frames: Vec<CheckpointFrame>,
    diagnostics: BTreeSet<String>,
    fixed_step: u64,
    present_sequence: u64,
    terminal: bool,
    phase_timings: HeadlessPhaseTimingEvidenceV1,
    runtime_samples_ns: Vec<u64>,
    presentation_samples_ns: Vec<u64>,
    gpu_samples: Vec<HeadlessGpuFrameSample>,
    performance_memory_after_warmup: Option<astra_observability::ProcessMemorySample>,
    scene_full_resync_count: u64,
    audio_underflow_count: u64,
    perfetto_trace: Option<PerfettoTraceSummary>,
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
    stage_width: u32,
    stage_height: u32,
    started_step: u64,
    stream: ActiveVideoStream,
    audio_stream_id: Option<u32>,
    audio_stream: Option<PlatformAudioCursor>,
    native_audio_started: bool,
}

enum ActiveVideoStream {
    Native(FvpNativeVideoCursor),
    Platform(PlatformVideoCursor),
}

struct FvpNativeVideoCursor {
    packets: FvpMoviePacketStream,
    pending: Option<FvpMovieFrame>,
    current: Option<DecodedVideoFrame>,
    audio: Vec<FvpMovieAudioChunk>,
    inferred_frame_step_us: u64,
    duration_us: Option<u64>,
    ended: bool,
}

enum PlatformVideoEvent {
    Frame(DecodedVideoFrame),
    End { duration_us: u64 },
}

enum PlatformAudioEvent {
    Chunk(PlayerDecodedAudio),
    End,
}

/// Lazily decoded PlatformHost audio for native movie playback. The decoder
/// owns the encoded source session and only transfers bounded PCM chunks to the
/// shared `FamilyAudioService`; no historical sample buffer is rebuilt.
struct PlatformAudioCursor {
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    receiver: tokio_mpsc::Receiver<Result<PlatformAudioEvent, String>>,
    worker: Option<JoinHandle<()>>,
    queue: VecDeque<PlayerDecodedAudio>,
    ended: bool,
    closed: bool,
}

impl PlatformAudioCursor {
    async fn open(
        client: PlatformHostClient,
        codec: &str,
        bytes: astra_byte_source::OwnedByteBuffer,
    ) -> Result<Self, String> {
        let session = client
            .open_decode(DecodeKind::Audio)
            .await
            .map_err(|error| error.to_string())?;
        let start = client
            .decode(
                session,
                PlatformDecodeRequest {
                    sequence: 1,
                    kind: DecodeKind::Audio,
                    codec: codec.to_owned(),
                    description: Vec::new(),
                    sample_rate: None,
                    channels: None,
                    coded_width: None,
                    coded_height: None,
                    keyframe: true,
                    stream_action: astra_platform::DecodeStreamAction::Start,
                    bytes,
                },
            )
            .await;
        let output = match start {
            Ok(output) => output,
            Err(error) => {
                let _ = client.close_decode(session).await;
                return Err(error.to_string());
            }
        };
        let first = match parse_platform_audio_event(output, None) {
            Ok(PlatformAudioEvent::Chunk(chunk)) => chunk,
            Ok(PlatformAudioEvent::End) => {
                let _ = client.close_decode(session).await;
                return Err("ASTRA_EMU_NATIVE_AUDIO_FIRST_CHUNK_MISSING".to_owned());
            }
            Err(error) => {
                let _ = client.close_decode(session).await;
                return Err(error);
            }
        };
        let sample_rate = first.sample_rate;
        let channels = first.channels;
        let (sender, receiver) = tokio_mpsc::channel(16);
        let worker = tokio::spawn(platform_audio_cursor_worker(
            client.clone(),
            session,
            sample_rate,
            channels,
            sender,
        ));
        let mut queue = VecDeque::new();
        queue.push_back(first);
        Ok(Self {
            client,
            session,
            receiver,
            worker: Some(worker),
            queue,
            ended: false,
            closed: false,
        })
    }

    fn drain_ready(&mut self) -> Result<Vec<PlayerDecodedAudio>, String> {
        loop {
            match self.receiver.try_recv() {
                Ok(Ok(PlatformAudioEvent::Chunk(chunk))) => self.queue.push_back(chunk),
                Ok(Ok(PlatformAudioEvent::End)) => {
                    self.ended = true;
                }
                Ok(Err(error)) => return Err(error),
                Err(tokio_mpsc::error::TryRecvError::Empty) => break,
                Err(tokio_mpsc::error::TryRecvError::Disconnected) => {
                    if !self.ended {
                        return Err("ASTRA_EMU_NATIVE_AUDIO_WORKER_CLOSED".to_owned());
                    }
                    break;
                }
            }
        }
        Ok(std::mem::take(&mut self.queue).into_iter().collect())
    }

    async fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        if let Some(worker) = self.worker.take() {
            worker.abort();
            let _ = worker.await;
        }
        let result = self
            .client
            .close_decode(self.session)
            .await
            .map_err(|error| error.to_string());
        self.closed = true;
        result
    }
}

impl Drop for PlatformAudioCursor {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

async fn platform_audio_cursor_worker(
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    sample_rate: u32,
    channels: u16,
    sender: tokio_mpsc::Sender<Result<PlatformAudioEvent, String>>,
) {
    let mut sequence = 2_u64;
    loop {
        let output = match client
            .decode(
                session,
                PlatformDecodeRequest {
                    sequence,
                    kind: DecodeKind::Audio,
                    codec: String::new(),
                    description: Vec::new(),
                    sample_rate: None,
                    channels: None,
                    coded_width: None,
                    coded_height: None,
                    keyframe: false,
                    stream_action: astra_platform::DecodeStreamAction::Next,
                    bytes: Vec::new().into(),
                },
            )
            .await
        {
            Ok(output) => output,
            Err(error)
                if error.operation == "decode.stream.next"
                    && error.fields.get("diagnostic_code").is_some_and(|code| {
                        code == astra_platform::DECODE_STREAM_EOS_DIAGNOSTIC
                    }) =>
            {
                let _ = sender.send(Ok(PlatformAudioEvent::End)).await;
                return;
            }
            Err(error) => {
                let _ = sender.send(Err(error.to_string())).await;
                return;
            }
        };
        let event = parse_platform_audio_event(output, Some((sample_rate, channels)));
        let is_end = matches!(event, Ok(PlatformAudioEvent::End));
        if sender.send(event).await.is_err() {
            return;
        }
        if is_end {
            return;
        }
        sequence = match sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => {
                let _ = sender
                    .send(Err("ASTRA_EMU_NATIVE_AUDIO_SEQUENCE".to_owned()))
                    .await;
                return;
            }
        };
    }
}

fn parse_platform_audio_event(
    output: DecodeOutput,
    expected: Option<(u32, u16)>,
) -> Result<PlatformAudioEvent, String> {
    let audio = match output {
        DecodeOutput::AudioPcmI16 {
            sample_rate,
            channels,
            samples,
        } => PlayerDecodedAudio::from_i16(sample_rate, channels, samples, MAX_MOVIE_AUDIO_SAMPLES),
        DecodeOutput::AudioPcmF32 {
            sample_rate,
            channels,
            samples,
        } => PlayerDecodedAudio::from_f32(sample_rate, channels, samples, MAX_MOVIE_AUDIO_SAMPLES),
        _ => return Err("ASTRA_EMU_NATIVE_AUDIO_OUTPUT_KIND".to_owned()),
    };
    let audio = audio.map_err(|error| error.to_string())?;
    if expected.is_some_and(|(sample_rate, channels)| {
        audio.sample_rate != sample_rate || audio.channels != channels
    }) {
        return Err("ASTRA_EMU_NATIVE_AUDIO_FORMAT_DRIFT".to_owned());
    }
    Ok(PlatformAudioEvent::Chunk(audio))
}

struct PlatformVideoCursor {
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    receiver: tokio_mpsc::Receiver<Result<PlatformVideoEvent, String>>,
    worker: Option<JoinHandle<()>>,
    queue: VecDeque<DecodedVideoFrame>,
    current: Option<DecodedVideoFrame>,
    ended: bool,
    duration_us: Option<u64>,
    closed: bool,
}

impl PlatformVideoCursor {
    async fn open(
        client: PlatformHostClient,
        codec: &str,
        bytes: astra_byte_source::OwnedByteBuffer,
    ) -> Result<Self, String> {
        let session = client
            .open_decode(DecodeKind::Video)
            .await
            .map_err(|error| error.to_string())?;
        let start = client
            .decode(
                session,
                PlatformDecodeRequest {
                    sequence: 1,
                    kind: DecodeKind::Video,
                    codec: codec.to_owned(),
                    description: Vec::new(),
                    sample_rate: None,
                    channels: None,
                    coded_width: None,
                    coded_height: None,
                    keyframe: true,
                    stream_action: astra_platform::DecodeStreamAction::Start,
                    bytes,
                },
            )
            .await;
        let output = match start {
            Ok(output) => output,
            Err(error) => {
                let _ = client.close_decode(session).await;
                return Err(error.to_string());
            }
        };
        let stream_duration_us = match output {
            DecodeOutput::VideoStreamStart { duration_us, .. } => Ok(duration_us),
            _ => Err("ASTRA_EMU_NATIVE_VIDEO_CURSOR_KIND".to_owned()),
        };
        let stream_duration_us = match stream_duration_us {
            Ok(duration_us) => duration_us,
            Err(error) => {
                let _ = client.close_decode(session).await;
                return Err(error);
            }
        };
        let first = client
            .decode(
                session,
                PlatformDecodeRequest {
                    sequence: 2,
                    kind: DecodeKind::Video,
                    codec: String::new(),
                    description: Vec::new(),
                    sample_rate: None,
                    channels: None,
                    coded_width: None,
                    coded_height: None,
                    keyframe: false,
                    stream_action: astra_platform::DecodeStreamAction::Next,
                    bytes: Vec::new().into(),
                },
            )
            .await;
        let first = match first {
            Ok(output) => match parse_platform_video_event(output) {
                Ok(PlatformVideoEvent::Frame(frame)) => frame,
                Ok(PlatformVideoEvent::End { .. }) => {
                    let _ = client.close_decode(session).await;
                    return Err("ASTRA_EMU_NATIVE_VIDEO_FIRST_FRAME_MISSING".to_owned());
                }
                Err(error) => {
                    let _ = client.close_decode(session).await;
                    return Err(error);
                }
            },
            Err(error) => {
                let _ = client.close_decode(session).await;
                return Err(error.to_string());
            }
        };
        let (sender, receiver) = tokio_mpsc::channel(16);
        let worker_client = client.clone();
        let first_frame_end_us = first
            .pts_us
            .checked_add(first.duration_us)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_DURATION_BOUNDS".to_owned())?;
        let worker = tokio::spawn(async move {
            platform_video_cursor_worker(worker_client, session, first_frame_end_us, sender).await;
        });
        let mut queue = VecDeque::new();
        queue.push_back(first);
        Ok(Self {
            client,
            session,
            receiver,
            worker: Some(worker),
            queue,
            current: None,
            ended: false,
            duration_us: stream_duration_us,
            closed: false,
        })
    }

    fn drain_ready(&mut self) -> Result<(), String> {
        loop {
            match self.receiver.try_recv() {
                Ok(Ok(PlatformVideoEvent::Frame(frame))) => self.queue.push_back(frame),
                Ok(Ok(PlatformVideoEvent::End { duration_us })) => {
                    self.duration_us = Some(duration_us);
                    self.ended = true;
                }
                Ok(Err(error)) => return Err(error),
                Err(tokio_mpsc::error::TryRecvError::Empty) => break,
                Err(tokio_mpsc::error::TryRecvError::Disconnected) => {
                    if !self.ended {
                        return Err("ASTRA_EMU_NATIVE_VIDEO_WORKER_CLOSED".to_owned());
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    async fn advance(&mut self, elapsed_us: u64) -> Result<bool, String> {
        self.drain_ready()?;
        while self
            .queue
            .front()
            .is_some_and(|frame| frame.pts_us <= elapsed_us)
        {
            self.current = self.queue.pop_front();
        }
        Ok(self.current.is_some() || !self.ended)
    }

    fn current_frame(&self) -> Option<&DecodedVideoFrame> {
        self.current.as_ref()
    }

    fn duration_us(&self) -> Option<u64> {
        self.duration_us
    }

    async fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        if let Some(worker) = self.worker.take() {
            worker.abort();
            let _ = worker.await;
        }
        let result = self
            .client
            .close_decode(self.session)
            .await
            .map_err(|error| error.to_string());
        self.closed = true;
        result
    }
}

impl Drop for PlatformVideoCursor {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

async fn platform_video_cursor_worker(
    client: PlatformHostClient,
    session: astra_platform::DecodeSessionHandle,
    mut last_frame_end_us: u64,
    sender: tokio_mpsc::Sender<Result<PlatformVideoEvent, String>>,
) {
    let mut sequence = 3_u64;
    loop {
        let output = match client
            .decode(
                session,
                PlatformDecodeRequest {
                    sequence,
                    kind: DecodeKind::Video,
                    codec: String::new(),
                    description: Vec::new(),
                    sample_rate: None,
                    channels: None,
                    coded_width: None,
                    coded_height: None,
                    keyframe: false,
                    stream_action: astra_platform::DecodeStreamAction::Next,
                    bytes: Vec::new().into(),
                },
            )
            .await
        {
            Ok(output) => output,
            Err(error) => {
                let _ = sender.send(Err(error.to_string())).await;
                return;
            }
        };
        match parse_platform_video_event(output) {
            Ok(PlatformVideoEvent::Frame(frame)) => {
                last_frame_end_us = match frame.pts_us.checked_add(frame.duration_us) {
                    Some(end) if end > frame.pts_us => end,
                    _ => {
                        let _ = sender
                            .send(Err("ASTRA_EMU_NATIVE_VIDEO_DURATION_BOUNDS".to_owned()))
                            .await;
                        return;
                    }
                };
                if sender
                    .send(Ok(PlatformVideoEvent::Frame(frame)))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Ok(PlatformVideoEvent::End { .. }) => {
                let _ = sender
                    .send(Ok(PlatformVideoEvent::End {
                        duration_us: last_frame_end_us,
                    }))
                    .await;
                return;
            }
            Err(error) => {
                let _ = sender.send(Err(error)).await;
                return;
            }
        }
        sequence = match sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => {
                let _ = sender
                    .send(Err("ASTRA_EMU_NATIVE_VIDEO_SEQUENCE".to_owned()))
                    .await;
                return;
            }
        };
    }
}

fn parse_platform_video_event(output: DecodeOutput) -> Result<PlatformVideoEvent, String> {
    match output {
        DecodeOutput::VideoFrame {
            sequence,
            pts_us,
            duration_us,
            width,
            height,
            bgra8,
        } => Ok(PlatformVideoEvent::Frame(DecodedVideoFrame {
            sequence,
            pts_us,
            duration_us,
            width,
            height,
            bgra8,
        })),
        DecodeOutput::VideoStreamEnd { .. } => Ok(PlatformVideoEvent::End { duration_us: 0 }),
        _ => Err("ASTRA_EMU_NATIVE_VIDEO_OUTPUT_KIND".to_owned()),
    }
}

impl FvpNativeVideoCursor {
    fn open(extension: &str, bytes: astra_byte_source::OwnedByteBuffer) -> Result<Self, String> {
        Ok(Self {
            packets: open_fvp_movie_packet_stream(
                extension,
                bytes,
                MAX_MOVIE_FRAMES,
                MAX_MOVIE_DECODED_BYTES,
                MAX_MOVIE_AUDIO_SAMPLES,
                16,
            )
            .map_err(|error| error.to_string())?,
            pending: None,
            current: None,
            audio: Vec::new(),
            inferred_frame_step_us: 34_000,
            duration_us: None,
            ended: false,
        })
    }

    fn advance(&mut self, elapsed_us: u64) -> Result<bool, String> {
        let previous_sequence = self.current.as_ref().map(|frame| frame.sequence);
        while let Some(packet) = self.packets.try_next().map_err(|error| error.to_string())? {
            match packet {
                FvpMoviePacket::Video(next) => {
                    let Some(previous) = self.pending.replace(next) else {
                        continue;
                    };
                    let next_pts_us = self
                        .pending
                        .as_ref()
                        .expect("pending frame was replaced")
                        .pts_ms
                        .checked_mul(1_000)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_TIMELINE_BOUNDS".to_owned())?;
                    let pts_us = previous
                        .pts_ms
                        .checked_mul(1_000)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_TIMELINE_BOUNDS".to_owned())?;
                    let duration_us = next_pts_us
                        .checked_sub(pts_us)
                        .filter(|duration| *duration > 0)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_TIMELINE_ORDER".to_owned())?;
                    self.inferred_frame_step_us = duration_us;
                    if pts_us <= elapsed_us {
                        self.current = Some(native_video_frame(previous, duration_us)?);
                    }
                    if next_pts_us > elapsed_us {
                        break;
                    }
                }
                FvpMoviePacket::Audio(chunk) => self.audio.push(chunk),
                FvpMoviePacket::End => {
                    if let Some(last) = self.pending.take() {
                        let pts_us = last
                            .pts_ms
                            .checked_mul(1_000)
                            .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_TIMELINE_BOUNDS".to_owned())?;
                        if pts_us <= elapsed_us || self.current.is_none() {
                            self.current =
                                Some(native_video_frame(last, self.inferred_frame_step_us)?);
                        }
                        self.duration_us =
                            Some(pts_us.checked_add(self.inferred_frame_step_us).ok_or_else(
                                || "ASTRA_EMU_NATIVE_VIDEO_TIMELINE_BOUNDS".to_owned(),
                            )?);
                    }
                    self.ended = true;
                    break;
                }
            }
        }
        Ok(previous_sequence != self.current.as_ref().map(|frame| frame.sequence))
    }

    fn drain_audio(&mut self) -> Vec<FvpMovieAudioChunk> {
        std::mem::take(&mut self.audio)
    }
}

fn native_video_frame(frame: FvpMovieFrame, duration_us: u64) -> Result<DecodedVideoFrame, String> {
    let pts_us = frame
        .pts_ms
        .checked_mul(1_000)
        .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_TIMELINE_BOUNDS".to_owned())?;
    let mut bgra8 = frame.rgba8;
    for pixel in bgra8.as_chunks_mut::<4>().0.iter_mut() {
        pixel.swap(0, 2);
    }
    Ok(DecodedVideoFrame {
        sequence: frame.pts_ms.saturating_add(1),
        pts_us,
        duration_us,
        width: frame.width,
        height: frame.height,
        bgra8: bgra8.into(),
    })
}

impl ActiveVideoStream {
    async fn advance(&mut self, elapsed_us: u64) -> Result<bool, String> {
        match self {
            Self::Native(cursor) => cursor.advance(elapsed_us),
            Self::Platform(cursor) => cursor.advance(elapsed_us).await,
        }
    }

    fn frame_for_elapsed(&self, elapsed_us: u64) -> Option<&DecodedVideoFrame> {
        match self {
            Self::Native(cursor) => cursor
                .current
                .as_ref()
                .filter(|frame| frame.pts_us <= elapsed_us),
            Self::Platform(cursor) => cursor.current_frame(),
        }
    }

    fn duration_us(&self) -> Option<u64> {
        match self {
            Self::Native(cursor) => cursor.duration_us,
            Self::Platform(cursor) => cursor.duration_us(),
        }
    }

    async fn close(&mut self) -> Result<(), String> {
        match self {
            Self::Native(_) => Ok(()),
            Self::Platform(cursor) => cursor.close().await,
        }
    }
}

/// Native-only bridge from the bounded family scene contract to the shared
/// platform GPU scene.  It retains texture bytes solely to apply validated
/// subresource updates; it never rasterizes a framebuffer on the CPU.
#[derive(Default)]
struct GpuSceneAdapter {
    textures: BTreeMap<u32, GpuSceneTexture>,
    width: u32,
    height: u32,
    draws: Vec<LegacyDrawV1>,
    compositing: SceneCompositing2D,
    last_live_sequence: u64,
    resource_epoch: u64,
}

#[derive(Clone)]
struct GpuSceneTexture {
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    revision: u64,
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
    fn prepare_live(
        &mut self,
        transaction: RuntimeLiveSceneTransaction,
    ) -> Result<(SceneFrame, GpuScenePrepareMetrics), String> {
        transaction.validate().map_err(|error| error.to_string())?;
        if transaction.sequence <= self.last_live_sequence {
            return Err("ASTRA_EMU_NATIVE_GPU_LIVE_SEQUENCE_REWIND".into());
        }
        let reset_resources = transaction.reset_resources;
        if reset_resources {
            self.resource_epoch = self
                .resource_epoch
                .checked_add(1)
                .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_RESOURCE_EPOCH_OVERFLOW".to_owned())?;
        }
        let mut mutations: BTreeMap<u32, Option<GpuSceneTexture>> = BTreeMap::new();
        let mut commands = Vec::with_capacity(
            transaction.resources.len().saturating_mul(2)
                + transaction.draws.len().saturating_mul(3)
                + self.textures.len(),
        );
        if reset_resources {
            for texture in self.textures.values() {
                commands.push(SceneCommand::ReleaseResource {
                    resource_id: texture.resource_id.clone(),
                });
            }
        }
        let mut metrics = GpuScenePrepareMetrics {
            resource_operations: transaction.resources.len() as u64,
            draw_count: transaction.draws.len() as u64,
            ..GpuScenePrepareMetrics::default()
        };
        for operation in transaction.resources {
            match operation {
                RuntimeLiveSceneResourceOperation::CreateTexture {
                    texture_id,
                    generation,
                    width,
                    height,
                    format,
                    pixels,
                } => {
                    if resolve_gpu_texture(&self.textures, &mutations, reset_resources, texture_id)
                        .is_some()
                    {
                        return Err("ASTRA_EMU_NATIVE_GPU_LIVE_TEXTURE_EXISTS".into());
                    }
                    metrics.create_bytes = metrics
                        .create_bytes
                        .checked_add(pixels.len() as u64)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_UPLOAD_BYTES_OVERFLOW".to_owned())?;
                    let format = runtime_live_texture_format(format);
                    let rgba8 = gpu_rgba8_owned(width, height, format, pixels)?;
                    let resource_id = gpu_resource_id(self.resource_epoch, texture_id, generation);
                    let frame = TextureFrame::from_buffer(width, height, rgba8)
                        .map_err(|error| error.to_string())?;
                    commands.push(SceneCommand::UploadTexture {
                        resource_id: resource_id.clone(),
                        frame,
                    });
                    mutations.insert(
                        texture_id,
                        Some(GpuSceneTexture {
                            width,
                            height,
                            format,
                            revision: generation,
                            resource_id,
                        }),
                    );
                }
                RuntimeLiveSceneResourceOperation::UpdateTexture {
                    texture_id,
                    generation,
                    x,
                    y,
                    width,
                    height,
                    format,
                    pixels,
                } => {
                    metrics.update_bytes = metrics
                        .update_bytes
                        .checked_add(pixels.len() as u64)
                        .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_UPLOAD_BYTES_OVERFLOW".to_owned())?;
                    let old = resolve_gpu_texture(
                        &self.textures,
                        &mutations,
                        reset_resources,
                        texture_id,
                    )
                    .cloned()
                    .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_LIVE_TEXTURE_MISSING".to_owned())?;
                    let format = runtime_live_texture_format(format);
                    if old.format != format
                        || generation <= old.revision
                        || x.checked_add(width).is_none_or(|right| right > old.width)
                        || y.checked_add(height)
                            .is_none_or(|bottom| bottom > old.height)
                    {
                        return Err("ASTRA_EMU_NATIVE_GPU_LIVE_TEXTURE_REGION".into());
                    }
                    let rgba8 = gpu_rgba8_owned(width, height, format, pixels)?;
                    commands.push(SceneCommand::UpdateTextureRegion {
                        resource_id: old.resource_id.clone(),
                        x,
                        y,
                        width,
                        height,
                        rgba8,
                    });
                    mutations.insert(
                        texture_id,
                        Some(GpuSceneTexture {
                            width: old.width,
                            height: old.height,
                            format: old.format,
                            revision: generation,
                            resource_id: old.resource_id,
                        }),
                    );
                }
                RuntimeLiveSceneResourceOperation::DestroyTexture {
                    texture_id,
                    generation,
                } => {
                    let texture = resolve_gpu_texture(
                        &self.textures,
                        &mutations,
                        reset_resources,
                        texture_id,
                    )
                    .cloned()
                    .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_LIVE_TEXTURE_MISSING".to_owned())?;
                    if generation <= texture.revision {
                        return Err("ASTRA_EMU_NATIVE_GPU_LIVE_TEXTURE_GENERATION".into());
                    }
                    mutations.insert(texture_id, None);
                    commands.push(SceneCommand::ReleaseResource {
                        resource_id: texture.resource_id,
                    });
                }
            }
        }
        let compositing = gpu_scene_compositing(transaction.compositing);
        let draws = transaction
            .draws
            .into_iter()
            .map(legacy_draw_from_live)
            .collect::<Result<Vec<_>, String>>()?;
        commands.extend(gpu_draw_commands(&draws, compositing, |texture_id| {
            resolve_gpu_texture(&self.textures, &mutations, reset_resources, texture_id)
                .map(|texture| texture.resource_id.clone())
        })?);
        if reset_resources {
            self.textures.clear();
        }
        for (texture_id, mutation) in mutations {
            match mutation {
                Some(texture) => {
                    self.textures.insert(texture_id, texture);
                }
                None => {
                    self.textures.remove(&texture_id);
                }
            }
        }
        self.width = transaction.width;
        self.height = transaction.height;
        self.draws = draws;
        self.compositing = compositing;
        self.last_live_sequence = transaction.sequence;
        metrics.live_textures = self.textures.len() as u64;
        metrics.generation = self.last_live_sequence;
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
            commands: gpu_draw_commands(&self.draws, self.compositing, |texture_id| {
                self.textures
                    .get(&texture_id)
                    .map(|texture| texture.resource_id.clone())
            })?,
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

fn resolve_gpu_texture<'a>(
    textures: &'a BTreeMap<u32, GpuSceneTexture>,
    mutations: &'a BTreeMap<u32, Option<GpuSceneTexture>>,
    reset_resources: bool,
    texture_id: u32,
) -> Option<&'a GpuSceneTexture> {
    mutations
        .get(&texture_id)
        .map(Option::as_ref)
        .unwrap_or_else(|| {
            (!reset_resources)
                .then(|| textures.get(&texture_id))
                .flatten()
        })
}

fn gpu_draw_commands(
    draws: &[LegacyDrawV1],
    compositing: SceneCompositing2D,
    resolve_texture: impl Fn(u32) -> Option<String>,
) -> Result<Vec<SceneCommand>, String> {
    if draws.is_empty() {
        return Ok(Vec::new());
    }
    let mut vertices = Vec::with_capacity(draws.len().saturating_mul(4));
    let mut indices = Vec::with_capacity(draws.len().saturating_mul(6));
    let mut mesh_draws = Vec::with_capacity(draws.len());
    for draw in draws {
        let scissor = draw
            .scissor
            .map(|scissor| {
                if scissor.x < 0 || scissor.y < 0 || scissor.width <= 0 || scissor.height <= 0 {
                    return Err("ASTRA_EMU_NATIVE_GPU_SCISSOR_INVALID".to_owned());
                }
                Ok(RectI::new(
                    scissor.x,
                    scissor.y,
                    scissor.width as u32,
                    scissor.height as u32,
                ))
            })
            .transpose()?;
        let (material, texture_id) = if draw.texture_id == u32::MAX {
            (MeshMaterial2D::Solid, None)
        } else {
            let resource_id = resolve_texture(draw.texture_id)
                .ok_or_else(|| "ASTRA_EMU_NATIVE_GPU_TEXTURE_MISSING".to_owned())?;
            (MeshMaterial2D::ColorTexture, Some(resource_id))
        };
        let vertex_start = u32::try_from(vertices.len())
            .map_err(|_| "ASTRA_EMU_NATIVE_GPU_VERTEX_BOUNDS".to_owned())?;
        let index_start = u32::try_from(indices.len())
            .map_err(|_| "ASTRA_EMU_NATIVE_GPU_INDEX_BOUNDS".to_owned())?;
        vertices.extend(draw.vertices.map(gpu_vertex));
        indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
        mesh_draws.push(MeshDraw2D {
            vertex_start,
            vertex_count: 4,
            index_start,
            index_count: 6,
            material,
            texture_id,
            texture_filter: match draw.texture_filter {
                LegacyTextureFilter::Nearest => TextureFilter2D::Nearest,
                LegacyTextureFilter::Linear => TextureFilter2D::Linear,
            },
            opacity: 1.0,
            blend: match draw.blend {
                astra_emu_family_api::LegacyBlendMode::Alpha => BlendMode::Alpha,
                astra_emu_family_api::LegacyBlendMode::Add => BlendMode::Add,
                astra_emu_family_api::LegacyBlendMode::Opaque => BlendMode::Opaque,
                astra_emu_family_api::LegacyBlendMode::Multiply => BlendMode::Multiply,
                astra_emu_family_api::LegacyBlendMode::Screen => BlendMode::Screen,
            },
            scissor,
        });
    }
    Ok(vec![SceneCommand::MeshBatch2D {
        vertices: vertices.into(),
        indices: indices.into(),
        draws: mesh_draws.into(),
        compositing,
    }])
}

fn gpu_scene_compositing(
    compositing: astra_plugin_abi::RuntimeLiveSceneCompositing,
) -> SceneCompositing2D {
    match compositing {
        astra_plugin_abi::RuntimeLiveSceneCompositing::LinearSrgb => SceneCompositing2D::LinearSrgb,
        astra_plugin_abi::RuntimeLiveSceneCompositing::EncodedSrgb => {
            SceneCompositing2D::EncodedSrgb
        }
    }
}

fn gpu_resource_id(epoch: u64, texture_id: u32, generation: u64) -> String {
    format!("astra-emu-texture-{epoch}-{texture_id}-{generation}")
}

fn runtime_live_texture_format(format: RuntimeLiveTextureFormat) -> LegacyTextureFormat {
    match format {
        RuntimeLiveTextureFormat::Rgba8 => LegacyTextureFormat::Rgba8,
        RuntimeLiveTextureFormat::LumaAlpha8 => LegacyTextureFormat::LumaAlpha8,
    }
}

fn rgba8_to_luma_alpha8(rgba8: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(rgba8.len() / 2);
    for pixel in rgba8.as_chunks::<4>().0.iter() {
        let luma = ((u16::from(pixel[0]) * 77
            + u16::from(pixel[1]) * 150
            + u16::from(pixel[2]) * 29
            + 128)
            / 256) as u8;
        output.push(luma);
        output.push(pixel[3]);
    }
    output
}

fn legacy_draw_from_live(draw: astra_plugin_abi::RuntimeLiveDraw) -> Result<LegacyDrawV1, String> {
    let scissor = draw
        .scissor
        .map(
            |scissor| -> Result<astra_emu_family_api::LegacyScissorV1, String> {
                Ok(astra_emu_family_api::LegacyScissorV1 {
                    x: i32::try_from(scissor.x)
                        .map_err(|_| "ASTRA_EMU_NATIVE_GPU_LIVE_SCISSOR_BOUNDS")?,
                    y: i32::try_from(scissor.y)
                        .map_err(|_| "ASTRA_EMU_NATIVE_GPU_LIVE_SCISSOR_BOUNDS")?,
                    width: i32::try_from(scissor.width)
                        .map_err(|_| "ASTRA_EMU_NATIVE_GPU_LIVE_SCISSOR_BOUNDS")?,
                    height: i32::try_from(scissor.height)
                        .map_err(|_| "ASTRA_EMU_NATIVE_GPU_LIVE_SCISSOR_BOUNDS")?,
                })
            },
        )
        .transpose()?;
    Ok(LegacyDrawV1 {
        texture_id: draw.texture_id,
        vertices: draw
            .vertices
            .map(|vertex| astra_emu_family_api::LegacyVertexV1 {
                position: [vertex.x, vertex.y],
                tex_coord: [vertex.u, vertex.v],
                color: vertex.color.map(|channel| f32::from(channel) / 255.0),
            }),
        blend: match draw.blend {
            RuntimeLiveBlendMode::Alpha => astra_emu_family_api::LegacyBlendMode::Alpha,
            RuntimeLiveBlendMode::Additive => astra_emu_family_api::LegacyBlendMode::Add,
            RuntimeLiveBlendMode::Opaque => astra_emu_family_api::LegacyBlendMode::Opaque,
            RuntimeLiveBlendMode::Multiply => astra_emu_family_api::LegacyBlendMode::Multiply,
            RuntimeLiveBlendMode::Screen => astra_emu_family_api::LegacyBlendMode::Screen,
        },
        texture_filter: match draw.texture_filter {
            RuntimeLiveTextureFilter::Nearest => LegacyTextureFilter::Nearest,
            RuntimeLiveTextureFilter::Linear => LegacyTextureFilter::Linear,
        },
        scissor,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn gpu_rgba8_owned(
    width: u32,
    height: u32,
    format: LegacyTextureFormat,
    pixels: astra_byte_source::OwnedByteBuffer,
) -> Result<OwnedPixelBuffer, String> {
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
        LegacyTextureFormat::Rgba8 => OwnedPixelBuffer::from_owned(pixels),
        LegacyTextureFormat::LumaAlpha8 => OwnedPixelBuffer::from_vec(
            pixels
                .as_slice()
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
                .collect(),
        ),
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

#[allow(clippy::too_many_arguments)]
fn composite_layer_cpu(
    target: &mut [u8],
    target_width: u32,
    target_height: u32,
    layer: &Layer2DState,
    source: &[u8],
    source_width: u32,
    source_height: u32,
    source_stride: u32,
    source_format: Surface2DFormat,
) -> Result<(), String> {
    let expected = usize::try_from(source_stride)
        .ok()
        .and_then(|stride| {
            usize::try_from(source_height)
                .ok()
                .and_then(|height| stride.checked_mul(height))
        })
        .ok_or_else(|| "ASTRA_EMU_LAYER_SOURCE_BOUNDS".to_owned())?;
    if source_width == 0
        || source_height == 0
        || source_stride < source_width.saturating_mul(4)
        || source.len() != expected
    {
        return Err("ASTRA_EMU_LAYER_SOURCE_INVALID".into());
    }
    let target_expected = usize::try_from(target_width)
        .ok()
        .and_then(|width| {
            usize::try_from(target_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "ASTRA_EMU_LAYER_TARGET_BOUNDS".to_owned())?;
    if target.len() != target_expected {
        return Err("ASTRA_EMU_LAYER_TARGET_INVALID".into());
    }

    let filtered;
    let (source, source_stride, source_format) = if let Some(graph) = &layer.filter_graph {
        let tight_len = usize::try_from(source_width)
            .ok()
            .and_then(|width| {
                usize::try_from(source_height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_LAYER_FILTER_BOUNDS".to_owned())?;
        let mut rgba = vec![0_u8; tight_len];
        for y in 0..source_height {
            for x in 0..source_width {
                let pixel = read_surface_pixel(source, source_stride, source_format, x, y)?;
                let offset = (usize::try_from(y).unwrap() * usize::try_from(source_width).unwrap()
                    + usize::try_from(x).unwrap())
                    * 4;
                rgba[offset..offset + 4].copy_from_slice(&pixel);
            }
        }
        filtered = CpuFilterExecutor
            .execute(
                graph,
                CpuFrame {
                    width: source_width,
                    height: source_height,
                    format: RenderTargetFormat::Rgba8Srgb,
                    bytes: rgba,
                },
            )
            .map_err(|error| error.to_string())?
            .0
            .bytes;
        (
            filtered.as_slice(),
            source_width * 4,
            Surface2DFormat::Rgba8SrgbPremultiplied,
        )
    } else {
        (source, source_stride, source_format)
    };

    let transform = layer.transform;
    let determinant = transform.m11 * transform.m22 - transform.m12 * transform.m21;
    if !determinant.is_finite() || determinant.abs() < f32::EPSILON {
        return Err("ASTRA_EMU_LAYER_TRANSFORM_SINGULAR".into());
    }
    let inverse = (
        transform.m22 / determinant,
        -transform.m12 / determinant,
        -transform.m21 / determinant,
        transform.m11 / determinant,
    );
    let point = |x: f32, y: f32| {
        (
            transform.m11 * x + transform.m21 * y + transform.tx,
            transform.m12 * x + transform.m22 * y + transform.ty,
        )
    };
    let corners = [
        point(0.0, 0.0),
        point(source_width as f32, 0.0),
        point(0.0, source_height as f32),
        point(source_width as f32, source_height as f32),
    ];
    let mut min_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let mut min_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let mut max_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    let mut max_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    min_x = min_x.max(0);
    min_y = min_y.max(0);
    max_x = max_x.min(target_width as i32);
    max_y = max_y.min(target_height as i32);
    if let Some(clip) = layer.clip {
        min_x = min_x.max(clip.x);
        min_y = min_y.max(clip.y);
        max_x = max_x.min(clip.x.saturating_add(clip.width as i32));
        max_y = max_y.min(clip.y.saturating_add(clip.height as i32));
    }
    for y in min_y..max_y {
        for x in min_x..max_x {
            let dx = x as f32 + 0.5 - transform.tx;
            let dy = y as f32 + 0.5 - transform.ty;
            let source_x = inverse.0 * dx + inverse.2 * dy - 0.5;
            let source_y = inverse.1 * dx + inverse.3 * dy - 0.5;
            if source_x < -0.5
                || source_y < -0.5
                || source_x >= source_width as f32 - 0.5
                || source_y >= source_height as f32 - 0.5
            {
                continue;
            }
            let mut pixel = match layer.texture_filter {
                TextureFilter2D::Nearest => read_surface_pixel(
                    source,
                    source_stride,
                    source_format,
                    source_x.round().clamp(0.0, source_width as f32 - 1.0) as u32,
                    source_y.round().clamp(0.0, source_height as f32 - 1.0) as u32,
                )?,
                TextureFilter2D::Linear => sample_surface_linear(
                    source,
                    source_stride,
                    source_format,
                    source_width,
                    source_height,
                    source_x,
                    source_y,
                )?,
            };
            let opacity = (layer.opacity * 255.0).round() as u16;
            for channel in &mut pixel {
                *channel = ((u16::from(*channel) * opacity + 127) / 255) as u8;
            }
            let offset = (usize::try_from(y).unwrap() * usize::try_from(target_width).unwrap()
                + usize::try_from(x).unwrap())
                * 4;
            blend_premultiplied(&mut target[offset..offset + 4], pixel, layer.blend);
        }
    }
    Ok(())
}

fn read_surface_pixel(
    source: &[u8],
    stride: u32,
    format: Surface2DFormat,
    x: u32,
    y: u32,
) -> Result<[u8; 4], String> {
    let offset = usize::try_from(u64::from(y) * u64::from(stride) + u64::from(x) * 4)
        .map_err(|_| "ASTRA_EMU_LAYER_SAMPLE_BOUNDS".to_owned())?;
    let pixel = source
        .get(offset..offset + 4)
        .ok_or_else(|| "ASTRA_EMU_LAYER_SAMPLE_BOUNDS".to_owned())?;
    Ok(match format {
        Surface2DFormat::Rgba8SrgbPremultiplied => [pixel[0], pixel[1], pixel[2], pixel[3]],
        Surface2DFormat::Bgra8SrgbPremultiplied => [pixel[2], pixel[1], pixel[0], pixel[3]],
    })
}

#[allow(clippy::too_many_arguments)]
fn sample_surface_linear(
    source: &[u8],
    stride: u32,
    format: Surface2DFormat,
    width: u32,
    height: u32,
    x: f32,
    y: f32,
) -> Result<[u8; 4], String> {
    let x0 = x.floor().clamp(0.0, width as f32 - 1.0) as u32;
    let y0 = y.floor().clamp(0.0, height as f32 - 1.0) as u32;
    let x1 = x0.saturating_add(1).min(width - 1);
    let y1 = y0.saturating_add(1).min(height - 1);
    let fx = (x - x.floor()).clamp(0.0, 1.0);
    let fy = (y - y.floor()).clamp(0.0, 1.0);
    let samples = [
        read_surface_pixel(source, stride, format, x0, y0)?,
        read_surface_pixel(source, stride, format, x1, y0)?,
        read_surface_pixel(source, stride, format, x0, y1)?,
        read_surface_pixel(source, stride, format, x1, y1)?,
    ];
    Ok(std::array::from_fn(|channel| {
        let top = samples[0][channel] as f32 * (1.0 - fx) + samples[1][channel] as f32 * fx;
        let bottom = samples[2][channel] as f32 * (1.0 - fx) + samples[3][channel] as f32 * fx;
        (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8
    }))
}

fn blend_premultiplied(target: &mut [u8], source: [u8; 4], blend: BlendMode) {
    let multiply =
        |left: u8, right: u8| -> u8 { ((u16::from(left) * u16::from(right) + 127) / 255) as u8 };
    let source_alpha = source[3];
    let target_alpha = target[3];
    for channel in 0..3 {
        target[channel] = match blend {
            BlendMode::Opaque => source[channel],
            BlendMode::Alpha => {
                source[channel].saturating_add(multiply(target[channel], 255 - source_alpha))
            }
            BlendMode::Add => source[channel].saturating_add(target[channel]),
            BlendMode::Multiply => multiply(source[channel], target[channel])
                .saturating_add(multiply(source[channel], 255 - target_alpha))
                .saturating_add(multiply(target[channel], 255 - source_alpha)),
            BlendMode::Screen => source[channel]
                .saturating_add(target[channel])
                .saturating_sub(multiply(source[channel], target[channel])),
        };
    }
    target[3] = match blend {
        BlendMode::Opaque => source_alpha,
        BlendMode::Add => source_alpha.saturating_add(target_alpha),
        BlendMode::Alpha | BlendMode::Multiply | BlendMode::Screen => {
            source_alpha.saturating_add(multiply(target_alpha, 255 - source_alpha))
        }
    };
}

struct RuntimeDriver<'a> {
    runtime: &'a mut AstraEmuRuntimeProvider,
    session_id: GameRuntimeSessionId,
    seed: u64,
    delta_ns: u64,
    platform: &'a PlatformHostClient,
    surface: SurfaceHandle,
    fixed_step: u64,
    input_sequence: u64,
    await_sequence: u64,
    pending_inputs: Vec<LegacyInputEdge>,
    pending_waits: BTreeMap<String, PendingWait>,
    rasterizer: CpuStageRasterizer,
    layer_state: RetainedLayer2DState,
    direct_layer_frame: bool,
    gpu_scene: Option<GpuSceneAdapter>,
    pending_scene_metrics: Option<GpuScenePrepareMetrics>,
    pending_render_frame: Option<PreparedRenderFrame>,
    pending_scene_frame: Option<SceneFrame>,
    visual_dirty: bool,
    image_decoders: DecodeProviderRegistry,
    base_frame: Option<(u32, u32, Vec<u8>)>,
    present_sequence: u64,
    pending_scene_presents: VecDeque<PendingScenePresent>,
    state_revision: u64,
    terminal: bool,
    audio: AudioExecutor,
    pending_audio_commands: VecDeque<PendingAudioCommand>,
    video: Option<ActiveVideo>,
    movie_audio_sequence: u32,
    completed_media: Vec<String>,
    frame_samples: Vec<HeadlessFrameSampleV1>,
    diagnostics: BTreeSet<String>,
    active_touch: Option<u64>,
    audio_enabled: bool,
    audio_pump: AudioPumpPolicy,
    frame_sample_interval: u64,
    presentation_substeps: u8,
    step_timings_ns: Vec<u64>,
    runtime_timings_ns: Vec<u64>,
    effect_timings_ns: Vec<u64>,
    raster_timings_ns: Vec<u64>,
    media_timings_ns: Vec<u64>,
    present_timings_ns: Vec<u64>,
    perfetto: Option<NativePerfettoCapture>,
    capture_performance_samples: bool,
    performance_memory_after_warmup: Option<astra_observability::ProcessMemorySample>,
    scene_full_resync_count: u64,
    last_step_resource_activity: bool,
}

enum PendingAudioCommand {
    Ready(LegacyAudioCommandV1),
    Resource {
        command: LegacyAudioCommandV1,
        read: Option<LegacyResourceRead>,
        started: Option<Instant>,
    },
}

struct PendingScenePresent {
    sequence: u64,
    submitted: Instant,
    receipt: ScenePresentReceipt,
}

struct RuntimeDriverConfig {
    seed: u64,
    delta_ns: u64,
    audio_enabled: bool,
    frame_sample_interval: u64,
    perfetto_trace: Option<PathBuf>,
    capture_performance_samples: bool,
    presentation: PresentationPath,
    presentation_substeps: u8,
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
    #[allow(dead_code)] // Constructed only by the Windows native host, which is cfg'd out here.
    Realtime {
        target_latency_ms: u32,
        refill_low_water_ms: u32,
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

    fn flow(
        &mut self,
        name: &'static str,
        track: u32,
        flow_id: u64,
        phase: PerfettoFlowPhase,
    ) -> Result<(), String> {
        self.writer
            .flow(
                perfetto_domain(name),
                name,
                track,
                flow_id,
                elapsed_ns(self.started)?,
                phase,
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
    if name.starts_with("rfvp.vm") {
        "rfvp.vm"
    } else if name.starts_with("rfvp.capture") {
        "rfvp.capture"
    } else if name.starts_with("runtime.") {
        "runtime"
    } else if name.starts_with("scene.") {
        "scene"
    } else if name.starts_with("gpu.") {
        "gpu"
    } else if name.starts_with("audio.") {
        "audio"
    } else if name.starts_with("media.") {
        "media"
    } else if name.starts_with("vfs.") {
        "vfs"
    } else {
        "astra.emu.adapter"
    }
}

struct ExecutionConfig {
    seed: u64,
    delta_ns: u64,
    frame_sample_interval: u64,
    presentation: PresentationPath,
    presentation_substeps: u8,
    perfetto_trace: Option<PathBuf>,
    capture_performance_samples: bool,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeEventAction {
    Continue,
    Suspend(bool),
    Close,
}

#[derive(Debug, Clone, Copy)]
#[cfg(any(target_os = "windows", test))]
struct NativeViewport {
    window_width: u32,
    window_height: u32,
    stage_width: u32,
    stage_height: u32,
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
fn process_native_event(
    driver: &mut RuntimeDriver<'_>,
    window: WindowHandle,
    viewport: &mut NativeViewport,
    event: PlatformEventKind,
    windowed_e2: bool,
    external_input_rejected: &mut u64,
) -> Result<NativeEventAction, String> {
    if !windowed_e2 {
        return route_native_event(driver, window, viewport, event);
    }
    match event {
        PlatformEventKind::Keyboard { .. }
        | PlatformEventKind::ImePreedit { .. }
        | PlatformEventKind::ImeCommit { .. }
        | PlatformEventKind::PointerMoved { .. }
        | PlatformEventKind::PointerButton { .. }
        | PlatformEventKind::MouseWheel { .. }
        | PlatformEventKind::Touch { .. }
        | PlatformEventKind::AccessibilityAction { .. }
        | PlatformEventKind::GamepadConnected { .. }
        | PlatformEventKind::GamepadDisconnected { .. }
        | PlatformEventKind::GamepadInput { .. } => {
            *external_input_rejected = external_input_rejected.saturating_add(1);
            tracing::debug!(
                event = "astra.emu.windowed_e2.external_input_rejected",
                count = *external_input_rejected,
                "external gameplay input was rejected by the automated windowed host"
            );
            Ok(NativeEventAction::Continue)
        }
        PlatformEventKind::WindowClosed {
            window: event_window,
        } if event_window == window => Err("ASTRA_EMU_WINDOWED_E2_EXTERNAL_CLOSE".into()),
        other => route_native_event(driver, window, viewport, other),
    }
}

#[cfg(target_os = "windows")]
async fn capture_windowed_checkpoint(
    driver: &RuntimeDriver<'_>,
    platform: &PlatformHostClient,
    surface: SurfaceHandle,
    checkpoint_id: String,
) -> Result<WindowedE2CheckpointV1, String> {
    let _captured = platform
        .capture_surface(surface)
        .await
        .map_err(|error| error.to_string())?;
    Ok(WindowedE2CheckpointV1 {
        checkpoint_id,
        fixed_step: driver.fixed_step,
    })
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

#[cfg(target_os = "windows")]
/// Applies the portion of a validated physical input sequence that becomes due
/// at the driver's current fixed-step boundary. Native replay deliberately
/// shares the exact `consume_physical_input` mapper used by Headless: the host
/// does not synthesize legacy control names or bypass RuntimeDriver queues.
///
/// A native window cannot make deterministic Headless captures, so checkpoint
/// records remain observable trace markers rather than silently becoming a
/// second capture protocol. `AdvanceTicks` and `Await` are rejected because a
/// real-time native host must not skip simulation or poll internal state.
#[derive(Debug, Default)]
struct NativeInputDue {
    shutdown_requested: bool,
    checkpoints: Vec<String>,
}

#[cfg(target_os = "windows")]
fn consume_native_inputs_due(
    driver: &mut RuntimeDriver<'_>,
    messages: &[InputMessage],
    cursor: &mut usize,
    allow_checkpoints: bool,
) -> Result<NativeInputDue, String> {
    let mut due = NativeInputDue::default();
    while let Some(message) = messages.get(*cursor) {
        if message.tick > driver.fixed_step {
            break;
        }
        match &message.event {
            PhysicalInput::Shutdown => due.shutdown_requested = true,
            PhysicalInput::Checkpoint { id } => {
                if allow_checkpoints {
                    due.checkpoints.push(id.clone());
                }
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
    Ok(due)
}

#[cfg(any(target_os = "windows", test))]
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
        platform,
        surface,
        RuntimeDriverConfig {
            seed: config.seed,
            delta_ns: config.delta_ns,
            audio_enabled: true,
            frame_sample_interval: config.frame_sample_interval,
            perfetto_trace: config.perfetto_trace,
            capture_performance_samples: config.capture_performance_samples,
            presentation: config.presentation,
            presentation_substeps: config.presentation_substeps,
            audio_pump: AudioPumpPolicy::FixedTick,
        },
    )?;
    let mut checkpoints = Vec::new();
    let mut checkpoint_frames = Vec::new();
    let run_result: Result<(), String> = async {
        for (message_index, message) in messages.iter().enumerate() {
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
                    let trailing_same_tick = &messages[message_index + 1..];
                    if trailing_same_tick
                        .iter()
                        .take_while(|candidate| candidate.tick == message.tick)
                        .any(|candidate| {
                            !matches!(
                                &candidate.event,
                                PhysicalInput::Checkpoint { .. } | PhysicalInput::Shutdown
                            )
                        })
                    {
                        return Err("ASTRA_EMU_HEADLESS_CHECKPOINT_ORDER".into());
                    }
                    let shutdown_same_tick = trailing_same_tick
                        .iter()
                        .take_while(|candidate| candidate.tick == message.tick)
                        .any(|candidate| matches!(&candidate.event, PhysicalInput::Shutdown));
                    // Input ticks are zero-based, while Runtime fixed steps are
                    // one-based. A checkpoint observes the frame produced for
                    // its tick, after all earlier messages at that tick have
                    // been queued. Advancing here also makes tick-zero capture
                    // deterministic instead of depending on whether the host
                    // happened to have submitted an initialization frame. A
                    // same-tick shutdown ends the oracle tick before rendering,
                    // so its preceding checkpoint observes the last committed
                    // frame without advancing.
                    let checkpoint_step = message
                        .tick
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_HEADLESS_CHECKPOINT_TICK_OVERFLOW".to_owned())?;
                    if !shutdown_same_tick {
                        while driver.fixed_step < checkpoint_step && !driver.terminal {
                            driver.step().await?;
                        }
                        if driver.fixed_step < checkpoint_step {
                            return Err("ASTRA_EMU_HEADLESS_CHECKPOINT_AFTER_TERMINAL".into());
                        }
                    }
                    let captured = platform
                        .capture_surface(surface)
                        .await
                        .map_err(|error| error.to_string())?;
                    let width = captured.width;
                    let height = captured.height;
                    let rgba8 = captured.rgba8.to_vec();
                    checkpoints.push(HeadlessCheckpointEvidenceV1 {
                        checkpoint_id: id.clone(),
                        fixed_step: driver.fixed_step,
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
                    if matches!(observation, ObservationPredicate::Equals { .. }) {
                        return Err("ASTRA_EMU_HEADLESS_OBSERVATION_HASH_REMOVED".into());
                    }
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
        }
        Ok(())
    }
    .await;
    let run_result = match (run_result, driver.flush_pending_audio_commands().await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(resource)) => Err(resource),
        (Err(error), Err(resource)) => Err(format!(
            "ASTRA_EMU_HEADLESS_RUN_AND_AUDIO_RESOURCE_FAILED:{error};resource={resource}"
        )),
    };
    let run_result = match (run_result, driver.drain_pending_scene_presents().await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(scene)) => Err(scene),
        (Err(error), Err(scene)) => Err(format!(
            "ASTRA_EMU_HEADLESS_RUN_AND_SCENE_PRESENT_FAILED:{error};scene={scene}"
        )),
    };
    let perfetto_trace = driver.finish_perfetto()?;
    let audio_underflow_count = driver.audio.underflow_count()?;
    let media_cleanup = driver.close_active_media().await;
    let audio_cleanup = driver.audio.shutdown(platform).await;
    match (run_result, media_cleanup, audio_cleanup) {
        (Ok(()), Ok(()), Ok(_)) => {}
        (Err(error), Ok(()), Ok(_)) => return Err(error),
        (Ok(()), Err(media), Ok(_)) => return Err(media),
        (Ok(()), Ok(()), Err(audio)) => {
            return Err(format!("ASTRA_EMU_HEADLESS_AUDIO_CLEANUP_FAILED:{audio}"));
        }
        (Err(error), Err(media), Ok(_)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_RUN_AND_MEDIA_CLEANUP_FAILED:{error};media={media}"
            ));
        }
        (Err(error), Ok(()), Err(audio)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_RUN_AND_AUDIO_CLEANUP_FAILED:{error};audio={audio}"
            ));
        }
        (Ok(()), Err(media), Err(audio)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_MEDIA_AND_AUDIO_CLEANUP_FAILED:media={media};audio={audio}"
            ));
        }
        (Err(error), Err(media), Err(audio)) => {
            return Err(format!(
                "ASTRA_EMU_HEADLESS_RUN_MEDIA_AUDIO_CLEANUP_FAILED:{error};media={media};audio={audio}"
            ));
        }
    }
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
        frame_samples: driver.frame_samples,
        checkpoints,
        checkpoint_frames,
        diagnostics: driver.diagnostics,
        fixed_step: driver.fixed_step,
        present_sequence: driver.present_sequence,
        terminal: driver.terminal,
        phase_timings,
        runtime_samples_ns,
        presentation_samples_ns,
        gpu_samples: Vec::new(),
        performance_memory_after_warmup: driver.performance_memory_after_warmup,
        scene_full_resync_count: driver.scene_full_resync_count,
        audio_underflow_count,
        perfetto_trace,
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

    fn record_perfetto_flow(
        &mut self,
        name: &'static str,
        track: u32,
        flow_id: u64,
        phase: PerfettoFlowPhase,
    ) -> Result<(), String> {
        if let Some(perfetto) = self.perfetto.as_mut() {
            perfetto.flow(name, track, flow_id, phase)?;
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
        self.record_perfetto_counter("audio.active_streams", telemetry.active_streams)?;
        self.record_perfetto_counter("audio.packets_submitted", telemetry.packets_submitted)?;
        self.record_perfetto_counter("audio.submitted_frames", telemetry.submitted_frames)?;
        self.record_perfetto_counter("audio.consumed_frames", telemetry.consumed_frames)?;
        self.record_perfetto_counter("queue_depth", telemetry.queued_frames)?;
        self.record_perfetto_counter("audio.underflow_count", telemetry.underflow_count)?;
        self.record_perfetto_counter("audio.decoder_refills", telemetry.decoder_refills)
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
        config: RuntimeDriverConfig,
    ) -> Result<RuntimeDriver<'a>, String> {
        if config.presentation_substeps == 0 || config.presentation_substeps > 2 {
            return Err("ASTRA_EMU_PRESENTATION_SUBSTEPS_INVALID".into());
        }
        if config.presentation_substeps != 1 && config.presentation != PresentationPath::NativeGpu {
            return Err("ASTRA_EMU_PRESENTATION_SUBSTEPS_REQUIRE_GPU".into());
        }
        let mut image_decoders = DecodeProviderRegistry::default();
        image_decoders
            .register(Box::new(ImageDecodeProvider))
            .map_err(|error| error.to_string())?;
        let driver = RuntimeDriver {
            runtime,
            session_id,
            seed: config.seed,
            delta_ns: config.delta_ns,
            platform,
            surface,
            fixed_step: 0,
            input_sequence: 0,
            await_sequence: 0,
            pending_inputs: Vec::new(),
            pending_waits: BTreeMap::new(),
            rasterizer: CpuStageRasterizer::default(),
            layer_state: RetainedLayer2DState::default(),
            direct_layer_frame: false,
            gpu_scene: (config.presentation == PresentationPath::NativeGpu)
                .then(GpuSceneAdapter::default),
            pending_scene_metrics: None,
            pending_render_frame: None,
            pending_scene_frame: None,
            visual_dirty: false,
            image_decoders,
            base_frame: None,
            present_sequence: 0,
            pending_scene_presents: VecDeque::new(),
            state_revision: 0,
            terminal: false,
            audio: AudioExecutor::new(FamilyAudioService::start_with_client(
                platform.clone(),
                false,
            )?),
            pending_audio_commands: VecDeque::new(),
            video: None,
            movie_audio_sequence: 0,
            completed_media: Vec::new(),
            frame_samples: Vec::new(),
            diagnostics: BTreeSet::new(),
            active_touch: None,
            audio_enabled: config.audio_enabled,
            audio_pump: config.audio_pump,
            frame_sample_interval: config.frame_sample_interval,
            presentation_substeps: config.presentation_substeps,
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
            capture_performance_samples: config.capture_performance_samples,
            performance_memory_after_warmup: None,
            scene_full_resync_count: 0,
            last_step_resource_activity: false,
        };
        Ok(driver)
    }

    async fn close_active_media(&mut self) -> Result<(), String> {
        let Some(mut video) = self.video.take() else {
            return Ok(());
        };
        let mut errors = Vec::new();
        if let Some(audio_stream) = video.audio_stream.as_mut() {
            if let Err(error) = audio_stream.close().await {
                errors.push(format!("audio_decode={error}"));
            }
        }
        if let Some(stream_id) = video.audio_stream_id {
            if let Err(error) = self
                .audio
                .close_movie_stream(stream_id, self.platform)
                .await
            {
                errors.push(format!("audio_output={error}"));
            }
        }
        if let Err(error) = video.stream.close().await {
            errors.push(format!("video_decode={error}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "ASTRA_EMU_NATIVE_MEDIA_CLEANUP_FAILED:{}",
                errors.join(";")
            ))
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
        self.step_traced("runtime.fixed_tick", true).await
    }

    async fn drain_pending_audio_commands(&mut self, wait: bool) -> Result<(), String> {
        while let Some(pending) = self.pending_audio_commands.pop_front() {
            match pending {
                PendingAudioCommand::Ready(command) => {
                    let started = Instant::now();
                    self.audio.execute(command, None, self.platform).await?;
                    self.record_perfetto_phase("audio.queue", 8, started)?;
                }
                PendingAudioCommand::Resource {
                    command,
                    mut read,
                    mut started,
                } => {
                    // Startup convergence must include resource work, not only
                    // retained-scene mutations. Otherwise the deadline
                    // scheduler can start while this read still owns the FVP
                    // session worker and the next VM step waits behind it.
                    self.last_step_resource_activity = true;
                    if read.is_none() {
                        let resource_uri = match &command {
                            LegacyAudioCommandV1::LoadResource { resource_uri, .. } => resource_uri,
                            _ => return Err("ASTRA_EMU_AUDIO_RESOURCE_COMMAND_INVALID".into()),
                        };
                        let read_started = Instant::now();
                        read = Some(self.runtime.begin_vfs_resource_read(
                            &self.session_id,
                            resource_uri,
                            512 * 1024 * 1024,
                        )?);
                        self.begin_perfetto_phase("vfs.range_read", 8, read_started)?;
                        started = Some(read_started);
                    }
                    let mut active = read.expect("resource read was created");
                    let completion = if wait {
                        active
                            .complete()
                            .map(Some)
                            .map_err(|error| error.to_string())
                    } else {
                        active.try_complete().map_err(|error| error.to_string())
                    };
                    let bytes = match completion {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            let trace = self.end_perfetto_phase("vfs.range_read", 8);
                            return match trace {
                                Ok(()) => Err(error),
                                Err(trace) => Err(format!(
                                    "ASTRA_EMU_AUDIO_RESOURCE_AND_TRACE_FAILED:{error};trace={trace}"
                                )),
                            };
                        }
                    };
                    let Some(bytes) = bytes else {
                        self.pending_audio_commands
                            .push_front(PendingAudioCommand::Resource {
                                command,
                                read: Some(active),
                                started,
                            });
                        break;
                    };
                    started.ok_or_else(|| "ASTRA_EMU_AUDIO_RESOURCE_START_MISSING".to_owned())?;
                    self.end_perfetto_phase("vfs.range_read", 8)?;
                    let queued = Instant::now();
                    self.audio
                        .execute(command, Some(bytes), self.platform)
                        .await?;
                    self.record_perfetto_phase("audio.queue", 8, queued)?;
                }
            }
        }
        Ok(())
    }

    async fn flush_pending_audio_commands(&mut self) -> Result<(), String> {
        self.drain_pending_audio_commands(true).await
    }

    async fn prewarm_step(&mut self) -> Result<(), String> {
        self.step_traced("runtime.prewarm_tick", false).await
    }

    async fn step_traced(
        &mut self,
        trace_name: &'static str,
        deadline_budget_active: bool,
    ) -> Result<(), String> {
        let step_started = Instant::now();
        self.begin_perfetto_phase(trace_name, 0, step_started)?;
        let step_result = self.step_body(step_started).await;
        let trace_result = self.end_perfetto_phase(trace_name, 0);
        match (step_result, trace_result) {
            (Ok(()), Ok(())) if deadline_budget_active => {
                let step_duration_ns = elapsed_ns(step_started)?;
                self.record_perfetto_counter(
                    "deadline_debt_ns",
                    step_duration_ns.saturating_sub(self.delta_ns),
                )
            }
            (Ok(()), Ok(())) => {
                self.record_perfetto_counter("startup.prewarm_tick_ns", elapsed_ns(step_started)?)
            }
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(step_error), Err(trace_error)) => Err(format!(
                "ASTRA_EMU_NATIVE_STEP_AND_TRACE_FAILED:{step_error};{trace_error}"
            )),
        }
    }

    async fn step_body(&mut self, step_started: Instant) -> Result<(), String> {
        self.last_step_resource_activity = false;
        self.poll_pending_scene_presents()?;
        self.drain_pending_audio_commands(false).await?;
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
                payload_len: 0,
                sequence: self.await_sequence,
            });
        }
        let input_edges = retain_unconsumed_input_edges(
            std::mem::take(&mut self.pending_inputs),
            &consumed_input_keys,
        );
        let typed_conversion_started = Instant::now();
        let input_edges = input_edges
            .into_iter()
            .map(|edge| RuntimeInputEdge {
                control: edge.control,
                pressed: edge.pressed,
                value: edge.value,
                sequence: edge.sequence,
            })
            .collect();
        let await_results = await_results
            .into_iter()
            .map(|result| RuntimeAwaitResult {
                token_id: result.token_id,
                status: result.status,
                payload_len: result.payload_len,
                sequence: result.sequence,
            })
            .collect();
        self.record_perfetto_phase("runtime.typed_conversion", 1, typed_conversion_started)?;
        let runtime_started = Instant::now();
        let output = self.runtime.step(RuntimeStepInput {
            session_id: self.session_id.clone(),
            fixed_step: next_step,
            delta_ns: self.delta_ns,
            session_seed: self.seed,
            mode: RuntimeStepMode::Live,
            action: "emu.step".into(),
            argument: None,
            auxiliary: None,
            flag: None,
            input_edges,
            await_results,
            provider_results: Vec::<RuntimeProviderResult>::new(),
            budget: RuntimeStepBudget {
                max_instructions: 100_000,
                max_effects: 65_536,
                max_trace_entries: 100_000,
            },
        })?;
        let runtime_duration_ns = elapsed_ns(runtime_started)?;
        tracing::trace!(
            event = "astra.emu.native_provider_step_timing",
            fixed_step = next_step,
            duration_ns = runtime_duration_ns,
            "measured provider/runtime output boundary"
        );
        self.runtime_timings_ns.push(runtime_duration_ns);
        // The host can measure only the complete provider call here. VM and
        // Family FFI phases require timestamps from their actual owners; do
        // not duplicate this duration under narrower names.
        self.record_perfetto_phase("runtime.provider_step", 6, runtime_started)?;
        let world_transaction_started = Instant::now();
        let live = output.live;
        self.state_revision = live.state_revision;
        let coverage = live.coverage;
        self.fixed_step = next_step;
        // Complete the owner-side transaction slice before emitting counters
        // observed at its end. Writing those counters first would make the
        // later complete event carry an earlier start timestamp.
        self.record_perfetto_phase("runtime.live_output_accept", 1, world_transaction_started)?;
        self.record_perfetto_counter("rfvp.capture_bytes", coverage.capture_bytes)?;
        self.record_perfetto_counter("rfvp.operation_bytes", coverage.operation_bytes)?;
        self.record_perfetto_counter("rfvp.scene_moved_bytes", coverage.scene_moved_bytes)?;
        self.record_perfetto_counter("rfvp.scene_copied_bytes", coverage.scene_copied_bytes)?;
        self.record_perfetto_counter("rfvp.pcm_moved_bytes", coverage.pcm_moved_bytes)?;
        self.record_perfetto_counter("rfvp.pcm_copied_bytes", coverage.pcm_copied_bytes)?;
        let mut rendered = false;
        let effect_started = Instant::now();
        self.begin_perfetto_phase("runtime.live_output_routing", 2, effect_started)?;
        for transaction in live.scenes {
            let scene_started = Instant::now();
            self.queue_scene_commit_live(transaction)?;
            self.record_perfetto_phase("scene.transaction_enqueue", 7, scene_started)?;
            rendered = true;
        }
        for transaction in live.layers {
            let scene_started = Instant::now();
            self.queue_layer_commit(transaction)?;
            self.record_perfetto_phase("layer.transaction_compose", 7, scene_started)?;
            rendered = true;
        }
        for scene in live.resource_scenes {
            let scene_started = Instant::now();
            self.queue_resource_scene_live(scene)?;
            self.record_perfetto_phase("scene.transaction_enqueue", 7, scene_started)?;
            rendered = true;
        }
        for packet in live.audio {
            let audio_started = Instant::now();
            if self.audio_enabled {
                self.audio
                    .execute_live_pcm(legacy_live_audio_packet(packet))
                    .await?;
            }
            self.record_perfetto_phase("audio.queue", 8, audio_started)?;
        }
        for command in live.audio_commands {
            let audio_started = Instant::now();
            let command = legacy_live_audio_command(command);
            if !self.audio_enabled {
                command.validate().map_err(|error| error.to_string())?;
            } else if self.audio.uses_resource_worker() {
                if matches!(command, LegacyAudioCommandV1::LoadResource { .. }) {
                    self.pending_audio_commands
                        .push_back(PendingAudioCommand::Resource {
                            command,
                            read: None,
                            started: None,
                        });
                } else {
                    self.pending_audio_commands
                        .push_back(PendingAudioCommand::Ready(command));
                }
                self.drain_pending_audio_commands(false).await?;
            } else {
                let resource = match &command {
                    LegacyAudioCommandV1::LoadResource { resource_uri, .. } => {
                        Some(self.runtime.read_vfs_resource(
                            &self.session_id,
                            resource_uri,
                            512 * 1024 * 1024,
                        )?)
                    }
                    _ => None,
                };
                self.audio.execute(command, resource, self.platform).await?;
                self.record_perfetto_phase("audio.queue", 8, audio_started)?;
            }
        }
        if !live.audio_cues.is_empty() {
            return Err("ASTRA_EMU_LIVE_PRODUCT_AUDIO_CUE_REJECTED".into());
        }
        if !live.text.is_empty() || !live.text_presentations.is_empty() {
            return Err("ASTRA_EMU_LAYER_LANE_TEXT_CHANNEL_FORBIDDEN".into());
        }
        for command in live.video {
            let media_started = Instant::now();
            self.execute_video(legacy_live_video_command(command))
                .await?;
            self.record_perfetto_phase("media.worker", 9, media_started)?;
        }
        for wait in live.waits {
            let (token, condition) = live_wait_condition(wait, next_step, self.delta_ns);
            if self.pending_waits.insert(token, condition).is_some() {
                return Err("ASTRA_EMU_HEADLESS_WAIT_DUPLICATE".into());
            }
        }
        self.effect_timings_ns.push(elapsed_ns(effect_started)?);
        self.end_perfetto_phase("runtime.live_output_routing", 2)?;
        if let Some(metrics) = self.pending_scene_metrics.take() {
            self.last_step_resource_activity |= metrics.resource_operations != 0
                || metrics.create_bytes != 0
                || metrics.update_bytes != 0;
            self.record_perfetto_counter("scene.resource_operations", metrics.resource_operations)?;
            self.record_perfetto_counter("allocation_bytes", metrics.create_bytes)?;
            self.record_perfetto_counter("upload_bytes", metrics.update_bytes)?;
            self.record_perfetto_counter("scene.draw_count", metrics.draw_count)?;
            self.record_perfetto_counter("allocation_count", metrics.live_textures)?;
            self.record_perfetto_counter("scene.generation", metrics.generation)?;
        }
        let media_started = Instant::now();
        let audio_refill_started = Instant::now();
        let audio_telemetry = if self.audio_enabled {
            Some(self.audio.pump(self.platform, self.audio_pump).await?)
        } else {
            None
        };
        self.record_perfetto_phase("audio.refill", 8, audio_refill_started)?;
        let video_started = Instant::now();
        let video_changed = self.advance_video().await?;
        self.media_timings_ns.push(elapsed_ns(media_started)?);
        self.record_perfetto_phase("media.worker", 9, video_started)?;
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
            let mut submitted = 0u8;
            if let Some(scene) = self.pending_scene_frame.take() {
                self.submit_scene(scene).await?;
                self.visual_dirty = false;
                submitted = 1;
            }
            while submitted < self.presentation_substeps {
                let scene = self
                    .gpu_scene
                    .as_ref()
                    .expect("checked GPU presentation path")
                    .draw_scene()?;
                self.submit_scene(scene).await?;
                submitted += 1;
            }
        } else if sample_due && (self.visual_dirty || video_changed) {
            if self.visual_dirty {
                if self.direct_layer_frame {
                    self.direct_layer_frame = false;
                    self.visual_dirty = false;
                } else {
                    let frame = self
                        .pending_render_frame
                        .as_ref()
                        .ok_or_else(|| "ASTRA_EMU_HEADLESS_PENDING_FRAME_MISSING".to_owned())?;
                    let (width, height) = frame.dimensions();
                    let raster_started = Instant::now();
                    let rgba8 = self.rasterizer.render_prepared(frame)?;
                    self.raster_timings_ns.push(elapsed_ns(raster_started)?);
                    self.record_perfetto_phase("scene.cpu_oracle", 4, raster_started)?;
                    self.base_frame = Some((width, height, rgba8));
                    self.visual_dirty = false;
                }
            }
            let present_started = Instant::now();
            self.present().await?;
            self.present_timings_ns.push(elapsed_ns(present_started)?);
            self.record_perfetto_phase("gpu.present", 5, present_started)?;
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
        Ok(())
    }

    async fn submit_scene(&mut self, mut scene: SceneFrame) -> Result<(), String> {
        let submitted = Instant::now();
        self.present_sequence = self
            .present_sequence
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_NATIVE_PRESENT_SEQUENCE_OVERFLOW".to_owned())?;
        scene.sequence = self.present_sequence;
        let receipt = self
            .platform
            .submit_scene(self.surface, scene)
            .map_err(|error| {
                format!(
                    "{error}; fixed_step={}; present_sequence={}",
                    self.fixed_step, self.present_sequence
                )
            })?;
        self.record_perfetto_phase("gpu.submit", 5, submitted)?;
        self.record_perfetto_flow(
            "gpu.present",
            5,
            self.present_sequence,
            PerfettoFlowPhase::Start,
        )?;
        if self.capture_performance_samples {
            self.pending_scene_presents.push_back(PendingScenePresent {
                sequence: self.present_sequence,
                submitted,
                receipt,
            });
        } else {
            receipt.complete().await.map_err(|error| {
                format!(
                    "{error}; fixed_step={}; present_sequence={}",
                    self.fixed_step, self.present_sequence
                )
            })?;
            self.present_timings_ns.push(elapsed_ns(submitted)?);
            self.record_perfetto_flow(
                "gpu.present",
                5,
                self.present_sequence,
                PerfettoFlowPhase::End,
            )?;
            self.record_current_surface_frame().await?;
        }
        self.record_perfetto_counter(
            "gpu.present_queue_depth",
            u64::try_from(self.pending_scene_presents.len())
                .map_err(|_| "ASTRA_EMU_NATIVE_PRESENT_QUEUE_DEPTH_OVERFLOW".to_owned())?,
        )?;
        Ok(())
    }

    async fn record_current_surface_frame(&mut self) -> Result<(), String> {
        let captured = self
            .platform
            .capture_surface(self.surface)
            .await
            .map_err(|error| error.to_string())?;
        let mean_rgba = frame_mean_rgba(&captured.rgba8, captured.width, captured.height)?;
        self.frame_samples.push(HeadlessFrameSampleV1 {
            sequence: self.present_sequence,
            fixed_step: self.fixed_step,
            mean_rgba,
        });
        Ok(())
    }

    fn poll_pending_scene_presents(&mut self) -> Result<(), String> {
        let mut queue_changed = false;
        loop {
            let complete = match self.pending_scene_presents.front_mut() {
                Some(pending) => pending
                    .receipt
                    .try_complete()
                    .map_err(|error| error.to_string())?,
                None => false,
            };
            if !complete {
                break;
            }
            let pending = self
                .pending_scene_presents
                .pop_front()
                .expect("front receipt was just observed");
            self.present_timings_ns.push(elapsed_ns(pending.submitted)?);
            self.record_perfetto_flow("gpu.present", 5, pending.sequence, PerfettoFlowPhase::End)?;
            if self.capture_performance_samples
                && pending.sequence == PERFORMANCE_WARMUP_PRESENTATIONS as u64
            {
                self.performance_memory_after_warmup =
                    Some(sample_process_memory().map_err(|error| error.to_string())?);
            }
            queue_changed = true;
        }
        if queue_changed {
            self.record_perfetto_counter(
                "gpu.present_queue_depth",
                u64::try_from(self.pending_scene_presents.len())
                    .map_err(|_| "ASTRA_EMU_NATIVE_PRESENT_QUEUE_DEPTH_OVERFLOW".to_owned())?,
            )?;
        }
        Ok(())
    }

    async fn drain_pending_scene_presents(&mut self) -> Result<(), String> {
        let mut first_error = None;
        while let Some(pending) = self.pending_scene_presents.pop_front() {
            if let Err(error) = pending.receipt.complete().await {
                first_error.get_or_insert_with(|| error.to_string());
            }
            self.present_timings_ns.push(elapsed_ns(pending.submitted)?);
            self.record_perfetto_flow("gpu.present", 5, pending.sequence, PerfettoFlowPhase::End)?;
        }
        self.record_perfetto_counter("gpu.present_queue_depth", 0)?;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn queue_scene_commit_live(
        &mut self,
        transaction: RuntimeLiveSceneTransaction,
    ) -> Result<(), String> {
        if let Some(gpu_scene) = self.gpu_scene.as_mut() {
            let (delta, metrics) = gpu_scene.prepare_live(transaction)?;
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
            self.pending_render_frame = Some(self.rasterizer.prepare_scene_live(transaction)?);
        }
        self.visual_dirty = true;
        Ok(())
    }

    fn queue_layer_commit(&mut self, transaction: Layer2DTransaction) -> Result<(), String> {
        if self.pending_scene_frame.is_some() || self.pending_render_frame.is_some() {
            return Err("ASTRA_EMU_PRESENTATION_LANE_MIXED".into());
        }
        let width = transaction.viewport_width;
        let height = transaction.viewport_height;
        let layers = self
            .layer_state
            .apply(&transaction)
            .map_err(|error| error.to_string())?;
        let len = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "ASTRA_EMU_LAYER_FRAME_BOUNDS".to_owned())?;
        let mut frame = match self.base_frame.take() {
            Some((existing_width, existing_height, mut bytes))
                if existing_width == width && existing_height == height && bytes.len() == len =>
            {
                bytes.fill(0);
                bytes
            }
            _ => vec![0_u8; len],
        };
        for layer in &layers {
            let Layer2DContent::WritableSurface(surface) = &layer.content else {
                return Err("ASTRA_EMU_FAMILY_TEXTURE_RESOURCE_FORBIDDEN".into());
            };
            self.runtime.with_published_surface(
                &self.session_id,
                &surface.surface_id.0,
                surface.generation,
                |bytes, surface_width, surface_height, stride, format| {
                    composite_layer_cpu(
                        &mut frame,
                        width,
                        height,
                        layer,
                        bytes,
                        surface_width,
                        surface_height,
                        stride,
                        match format {
                            astra_emu_family_api::LegacySurfaceFormatV9::Rgba8SrgbPremultiplied => {
                                Surface2DFormat::Rgba8SrgbPremultiplied
                            }
                            astra_emu_family_api::LegacySurfaceFormatV9::Bgra8SrgbPremultiplied => {
                                Surface2DFormat::Bgra8SrgbPremultiplied
                            }
                        },
                    )
                },
            )??;
        }
        self.base_frame = Some((width, height, frame));
        self.pending_render_frame = None;
        self.direct_layer_frame = true;
        self.visual_dirty = true;
        Ok(())
    }

    fn queue_resource_scene_live(&mut self, scene: RuntimeLiveResourceScene) -> Result<(), String> {
        if scene.width == 0 || scene.height == 0 || scene.width > 16_384 || scene.height > 16_384 {
            return Err("ASTRA_EMU_LIVE_RESOURCE_SCENE_DIMENSIONS".into());
        }
        let mut resources = Vec::with_capacity(scene.textures.len());
        for texture in scene.textures {
            let format = runtime_live_texture_format(texture.decoded_format);
            let retained = self
                .gpu_scene
                .as_ref()
                .and_then(|scene| scene.textures.get(&texture.texture_id));
            if let Some(retained) = retained {
                if retained.revision == texture.revision
                    && retained.width == texture.decoded_width
                    && retained.height == texture.decoded_height
                    && retained.format == format
                {
                    continue;
                }
                if retained.width != texture.decoded_width
                    || retained.height != texture.decoded_height
                    || retained.format != format
                {
                    return Err("ASTRA_EMU_LIVE_RESOURCE_SCENE_DIMENSION_CHANGE".into());
                }
            }
            let bytes = self.runtime.read_vfs_resource(
                &self.session_id,
                &texture.resource_uri,
                1024 * 1024 * 1024,
            )?;
            let decoded = self
                .image_decoders
                .decode(
                    &DecodeRequest {
                        kind: astra_media::DecodeKind::Image,
                        codec: texture.codec,
                        bytes,
                        profile: "emu-live-image-v1".into(),
                    },
                    &DecodeBindingContext::shipping(
                        "astra.decode.image",
                        "headless",
                        "emu-live-image-v1",
                    ),
                )
                .map_err(|error| error.to_string())?;
            let MediaDecodeOutput::CpuBuffer {
                bytes,
                format: decoded_format,
                ..
            } = decoded.output
            else {
                return Err("ASTRA_EMU_LIVE_RESOURCE_SCENE_CPU_BUFFER_REQUIRED".into());
            };
            if decoded_format != "rgba8" {
                return Err("ASTRA_EMU_LIVE_RESOURCE_SCENE_DECODE_FORMAT".into());
            }
            let expected_rgba = usize::try_from(texture.decoded_width)
                .ok()
                .and_then(|width| {
                    usize::try_from(texture.decoded_height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| "ASTRA_EMU_LIVE_RESOURCE_SCENE_DIMENSION_OVERFLOW".to_owned())?;
            if bytes.len() != expected_rgba {
                return Err("ASTRA_EMU_LIVE_RESOURCE_SCENE_DIMENSION_MISMATCH".into());
            }
            let pixels = match format {
                LegacyTextureFormat::Rgba8 => bytes,
                LegacyTextureFormat::LumaAlpha8 => rgba8_to_luma_alpha8(&bytes).into(),
            };
            if let Some(retained) = retained {
                resources.push(RuntimeLiveSceneResourceOperation::UpdateTexture {
                    texture_id: texture.texture_id,
                    generation: texture.revision,
                    x: 0,
                    y: 0,
                    width: texture.decoded_width,
                    height: texture.decoded_height,
                    format: texture.decoded_format,
                    pixels,
                });
                if retained.revision == 0 {
                    // A resource emitted by the pre-v7 scene cache cannot be
                    // silently reused by the typed resource contract.
                    return Err("ASTRA_EMU_LIVE_RESOURCE_SCENE_REVISION_UNKNOWN".into());
                }
            } else {
                resources.push(RuntimeLiveSceneResourceOperation::CreateTexture {
                    texture_id: texture.texture_id,
                    generation: texture.revision,
                    width: texture.decoded_width,
                    height: texture.decoded_height,
                    format: texture.decoded_format,
                    pixels,
                });
            }
        }
        let draws = scene
            .draws
            .into_iter()
            .map(legacy_draw_from_live)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|draw| RuntimeLiveDraw {
                texture_id: draw.texture_id,
                vertices: draw
                    .vertices
                    .map(|vertex| astra_plugin_abi::RuntimeLiveVertex {
                        x: vertex.position[0],
                        y: vertex.position[1],
                        u: vertex.tex_coord[0],
                        v: vertex.tex_coord[1],
                        color: [
                            (vertex.color[0].clamp(0.0, 1.0) * 255.0) as u8,
                            (vertex.color[1].clamp(0.0, 1.0) * 255.0) as u8,
                            (vertex.color[2].clamp(0.0, 1.0) * 255.0) as u8,
                            (vertex.color[3].clamp(0.0, 1.0) * 255.0) as u8,
                        ],
                    }),
                blend: match draw.blend {
                    astra_emu_family_api::LegacyBlendMode::Alpha => RuntimeLiveBlendMode::Alpha,
                    astra_emu_family_api::LegacyBlendMode::Add => RuntimeLiveBlendMode::Additive,
                    astra_emu_family_api::LegacyBlendMode::Opaque => RuntimeLiveBlendMode::Opaque,
                    astra_emu_family_api::LegacyBlendMode::Multiply => {
                        RuntimeLiveBlendMode::Multiply
                    }
                    astra_emu_family_api::LegacyBlendMode::Screen => RuntimeLiveBlendMode::Screen,
                },
                texture_filter: match draw.texture_filter {
                    LegacyTextureFilter::Nearest => RuntimeLiveTextureFilter::Nearest,
                    LegacyTextureFilter::Linear => RuntimeLiveTextureFilter::Linear,
                },
                scissor: draw
                    .scissor
                    .map(|scissor| astra_plugin_abi::RuntimeLiveScissor {
                        x: scissor.x.max(0) as u32,
                        y: scissor.y.max(0) as u32,
                        width: scissor.width.max(0) as u32,
                        height: scissor.height.max(0) as u32,
                    }),
            })
            .collect();
        self.queue_scene_commit_live(RuntimeLiveSceneTransaction {
            sequence: scene.sequence,
            width: scene.width,
            height: scene.height,
            compositing: astra_plugin_abi::RuntimeLiveSceneCompositing::LinearSrgb,
            resources,
            draws,
            reset_resources: false,
        })
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
            if let Some(frame) = video.stream.frame_for_elapsed(elapsed_us) {
                composite_bgra(&mut rgba8, width, height, frame)?;
            }
        }
        self.present_sequence = self
            .present_sequence
            .checked_add(1)
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_PRESENT_SEQUENCE_OVERFLOW".to_owned())?;
        let mean_rgba = frame_mean_rgba(&rgba8, width, height)?;
        self.platform
            .present_rgba(
                self.surface,
                RgbaFrame {
                    sequence: self.present_sequence,
                    width,
                    height,
                    rgba8,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        self.frame_samples.push(HeadlessFrameSampleV1 {
            sequence: self.present_sequence,
            fixed_step: self.fixed_step,
            mean_rgba,
        });
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
                let mut active = active;
                if let Some(audio_stream) = active.audio_stream.as_mut() {
                    audio_stream.close().await?;
                }
                if let Some(stream_id) = active.audio_stream_id {
                    self.audio
                        .close_movie_stream(stream_id, self.platform)
                        .await?;
                }
                active.stream.close().await?;
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
        let bytes =
            self.runtime
                .read_vfs_resource(&self.session_id, &resource_uri, 512 * 1024 * 1024)?;
        let extension = resource_uri
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .ok_or_else(|| "ASTRA_EMU_HEADLESS_VIDEO_EXTENSION_MISSING".to_owned())?;
        let (stream, audio_stream_id, audio_stream) = match fvp_movie_compatibility(&extension) {
            FvpMovieCompatibility::Native => {
                let audio_stream_id =
                    if self.audio_enabled && matches!(mode, LegacyVideoMode::ModalWithAudio) {
                        let stream_id = MOVIE_AUDIO_STREAM_BASE
                            .checked_add(self.movie_audio_sequence)
                            .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_ID".to_owned())?;
                        self.movie_audio_sequence = self
                            .movie_audio_sequence
                            .checked_add(1)
                            .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_ID".to_owned())?;
                        Some(stream_id)
                    } else {
                        None
                    };
                (
                    ActiveVideoStream::Native(FvpNativeVideoCursor::open(&extension, bytes)?),
                    audio_stream_id,
                    None,
                )
            }
            FvpMovieCompatibility::PlatformProviderRequired => {
                tracing::info!(
                    event = "astra_emu_native_video_platform_stream_open",
                    codec = extension.as_str(),
                    "using PlatformHost incremental video decode"
                );
                let wants_audio =
                    self.audio_enabled && matches!(mode, LegacyVideoMode::ModalWithAudio);
                let (video_bytes, audio_bytes) = if wants_audio {
                    let audio_bytes = bytes;
                    (audio_bytes.clone(), Some(audio_bytes))
                } else {
                    (bytes, None)
                };
                let video_stream =
                    PlatformVideoCursor::open(self.platform.clone(), &extension, video_bytes)
                        .await?;
                let (audio_stream_id, audio_stream) = if wants_audio {
                    let stream_id = MOVIE_AUDIO_STREAM_BASE
                        .checked_add(self.movie_audio_sequence)
                        .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_ID".to_owned())?;
                    self.movie_audio_sequence = self
                        .movie_audio_sequence
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_EMU_HEADLESS_MOVIE_AUDIO_ID".to_owned())?;
                    // Video and audio are separate PlatformHost decode sessions;
                    // the second session receives the same shared encoded owner,
                    // while decoded payloads stay chunked.
                    let mut audio_stream = PlatformAudioCursor::open(
                        self.platform.clone(),
                        &extension,
                        audio_bytes
                            .ok_or_else(|| "ASTRA_EMU_NATIVE_AUDIO_SOURCE_MISSING".to_owned())?,
                    )
                    .await?;
                    let chunks = match audio_stream.drain_ready() {
                        Ok(chunks) => chunks,
                        Err(error) => {
                            let _ = audio_stream.close().await;
                            return Err(error);
                        }
                    };
                    let mut chunks = chunks.into_iter();
                    let first = match chunks.next() {
                        Some(first) => first,
                        None => {
                            let _ = audio_stream.close().await;
                            return Err("ASTRA_EMU_NATIVE_AUDIO_FIRST_CHUNK_MISSING".to_owned());
                        }
                    };
                    if let Err(error) = self.audio.begin_platform_movie(stream_id, first) {
                        let _ = audio_stream.close().await;
                        return Err(error);
                    }
                    for chunk in chunks {
                        if let Err(error) = self.audio.append_platform_movie(
                            stream_id,
                            chunk.sample_rate,
                            chunk.channels,
                            chunk.samples,
                        ) {
                            let _ = audio_stream.close().await;
                            return Err(error);
                        }
                    }
                    (Some(stream_id), Some(audio_stream))
                } else {
                    (None, None)
                };
                (
                    ActiveVideoStream::Platform(video_stream),
                    audio_stream_id,
                    audio_stream,
                )
            }
            FvpMovieCompatibility::Unsupported => {
                return Err("ASTRA_EMU_HEADLESS_VIDEO_CODEC_UNSUPPORTED".into());
            }
        };
        tracing::debug!(
            event = "astra_emu_headless_video_opened",
            codec = extension,
            decoded_frame_count = match &stream {
                ActiveVideoStream::Native(_) => 0,
                ActiveVideoStream::Platform(_) => 1,
            },
            duration_us = stream.duration_us(),
            audio_stream_active = audio_stream_id.is_some(),
            "opened bounded Headless video stream"
        );
        self.video = Some(ActiveVideo {
            playback_id,
            stage_width,
            stage_height,
            started_step,
            stream,
            audio_stream_id,
            audio_stream,
            native_audio_started: false,
        });
        Ok(())
    }

    async fn advance_video(&mut self) -> Result<bool, String> {
        let Some(video) = self.video.as_ref() else {
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
        let ready_audio = {
            let video = self
                .video
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_MISSING".to_owned())?;
            match video.audio_stream.as_mut() {
                Some(stream) => stream.drain_ready()?,
                None => Vec::new(),
            }
        };
        if let Some(stream_id) = self.video.as_ref().and_then(|video| video.audio_stream_id) {
            for chunk in ready_audio {
                self.audio.append_platform_movie(
                    stream_id,
                    chunk.sample_rate,
                    chunk.channels,
                    chunk.samples,
                )?;
            }
        } else if !ready_audio.is_empty() {
            return Err("ASTRA_EMU_NATIVE_AUDIO_STREAM_ID_MISSING".into());
        }
        let (video_changed, duration_us) = {
            let video = self
                .video
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_MISSING".to_owned())?;
            let changed = video.stream.advance(elapsed_us).await?;
            (changed, video.stream.duration_us())
        };
        let (native_audio, native_stream_id, native_audio_started) = {
            let video = self
                .video
                .as_mut()
                .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_MISSING".to_owned())?;
            let chunks = match &mut video.stream {
                ActiveVideoStream::Native(cursor) => cursor.drain_audio(),
                ActiveVideoStream::Platform(_) => Vec::new(),
            };
            (chunks, video.audio_stream_id, video.native_audio_started)
        };
        if !native_audio.is_empty() {
            let stream_id = native_stream_id
                .ok_or_else(|| "ASTRA_EMU_NATIVE_AUDIO_STREAM_ID_MISSING".to_owned())?;
            let mut chunks = native_audio.into_iter();
            if !native_audio_started {
                let first = chunks
                    .next()
                    .ok_or_else(|| "ASTRA_EMU_NATIVE_AUDIO_FIRST_CHUNK_MISSING".to_owned())?;
                self.audio.begin_platform_movie(
                    stream_id,
                    PlayerDecodedAudio {
                        sample_rate: first.sample_rate,
                        channels: first.channels,
                        samples: first.samples,
                    },
                )?;
                self.video
                    .as_mut()
                    .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_MISSING".to_owned())?
                    .native_audio_started = true;
            }
            for chunk in chunks {
                self.audio.append_platform_movie(
                    stream_id,
                    chunk.sample_rate,
                    chunk.channels,
                    chunk.samples,
                )?;
            }
        }
        if duration_us.is_some_and(|duration| elapsed_us >= duration) {
            let mut completed = self
                .video
                .take()
                .ok_or_else(|| "ASTRA_EMU_NATIVE_VIDEO_MISSING".to_owned())?;
            tracing::debug!(
                event = "astra_emu_headless_video_completed",
                elapsed_us,
                "completed bounded Headless video stream"
            );
            if let Some(stream_id) = completed
                .audio_stream_id
                .filter(|_| completed.audio_stream.is_some() || completed.native_audio_started)
            {
                if let Some(audio_stream) = completed.audio_stream.as_mut() {
                    audio_stream.close().await?;
                }
                self.audio
                    .close_movie_stream(stream_id, self.platform)
                    .await?;
            }
            completed.stream.close().await?;
            self.completed_media.push(completed.playback_id);
            return Ok(video_changed);
        }
        Ok(video_changed)
    }

    fn observations(&self) -> std::collections::BTreeSet<&'static str> {
        ["runtime.state_revision", "runtime.terminal", "runtime.tick"]
            .into_iter()
            .chain((!self.frame_samples.is_empty()).then_some("frame.presented"))
            .collect()
    }

    fn observation_matches(&self, predicate: &ObservationPredicate) -> bool {
        match predicate {
            ObservationPredicate::Exists { key } => self.observations().contains(key.as_str()),
            ObservationPredicate::Equals { .. } => false,
        }
    }
}

fn frame_mean_rgba(rgba8: &[u8], width: u32, height: u32) -> Result<[u8; 4], String> {
    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| "ASTRA_EMU_HEADLESS_FRAME_SIGNATURE_BOUNDS".to_owned())?;
    if pixel_count == 0 || rgba8.len() != pixel_count.saturating_mul(4) {
        return Err("ASTRA_EMU_HEADLESS_FRAME_SIGNATURE_LENGTH".into());
    }
    let mut sums = [0_u64; 4];
    for pixel in rgba8.as_chunks::<4>().0.iter() {
        for channel in 0..4 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    Ok(sums.map(|sum| (sum / pixel_count as u64) as u8))
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

struct AudioExecutor {
    service: Option<FamilyAudioService>,
}

impl AudioExecutor {
    fn new(service: FamilyAudioService) -> Self {
        Self {
            service: Some(service),
        }
    }

    fn service(&self) -> Result<&FamilyAudioService, String> {
        self.service
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SESSION_CLOSED".to_owned())
    }

    fn uses_resource_worker(&self) -> bool {
        true
    }

    fn underflow_count(&self) -> Result<u64, String> {
        Ok(self.service()?.telemetry().underflow_count)
    }

    async fn execute(
        &mut self,
        command: LegacyAudioCommandV1,
        resource: Option<astra_byte_source::OwnedByteBuffer>,
        _platform: &PlatformHostClient,
    ) -> Result<(), String> {
        self.service()?.execute(command, resource)
    }

    async fn execute_live_pcm(&mut self, packet: LegacyAudioPacketV7) -> Result<(), String> {
        self.service()?.execute_live_pcm(packet)
    }

    fn begin_platform_movie(
        &mut self,
        stream_id: u32,
        chunk: PlayerDecodedAudio,
    ) -> Result<(), String> {
        self.service()?.begin_movie_stream(
            stream_id,
            chunk.sample_rate,
            chunk.channels,
            chunk.samples,
        )
    }

    fn append_platform_movie(
        &mut self,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    ) -> Result<(), String> {
        self.service()?
            .append_movie_stream(stream_id, sample_rate, channels, samples)
    }

    async fn close_movie_stream(
        &mut self,
        stream_id: u32,
        _platform: &PlatformHostClient,
    ) -> Result<(), String> {
        self.service()?.stop_movie_pcm(stream_id)
    }

    async fn pump(
        &mut self,
        _platform: &PlatformHostClient,
        _policy: AudioPumpPolicy,
    ) -> Result<AudioPumpTelemetry, String> {
        let service = self.service()?;
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

    #[cfg(target_os = "windows")]
    fn set_suspended(&self, suspended: bool) -> Result<(), String> {
        self.service()?.set_suspended(suspended)
    }

    async fn shutdown(&mut self, _platform: &PlatformHostClient) -> Result<Vec<u8>, String> {
        self.service
            .take()
            .ok_or_else(|| "ASTRA_EMU_AUDIO_SESSION_CLOSED".to_owned())?
            .shutdown()
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
    use astra_emu_family_api::FamilyId;

    #[test]
    fn frame_sample_preserves_mean_rgba() {
        let mut frame = vec![0_u8; 9 * 8 * 4];
        for y in 0..8 {
            for x in 0..9 {
                let offset = (y * 9 + x) * 4;
                frame[offset..offset + 4].copy_from_slice(&[x as u8, x as u8, x as u8, 255]);
            }
        }
        let mean = frame_mean_rgba(&frame, 9, 8).unwrap();
        assert_eq!(mean, [4, 4, 4, 255]);
    }

    #[test]
    fn coalesced_gpu_scene_retains_resource_generations_without_full_resync() {
        let queued = SceneFrame {
            sequence: 0,
            width: 1280,
            height: 720,
            clear_rgba: [0, 0, 0, 255],
            commands: vec![
                SceneCommand::ReleaseResource {
                    resource_id: "astra-emu-texture-1-1".into(),
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
                    resource_id: "astra-emu-texture-1-2".into(),
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
            SceneCommand::ReleaseResource { resource_id } if resource_id == "astra-emu-texture-1-1"
        ));
        assert!(matches!(
            &merged.commands[1],
            SceneCommand::ReleaseResource { resource_id } if resource_id == "astra-emu-texture-1-2"
        ));
        assert!(matches!(
            merged.commands[2],
            SceneCommand::Clear {
                rgba: [7, 8, 9, 255]
            }
        ));
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
            frame_samples: Vec::new(),
            checkpoints: vec![HeadlessCheckpointEvidenceV1 {
                checkpoint_id: "message".into(),
                fixed_step: 12,
            }],
            checkpoint_frames: Vec::new(),
            diagnostics: BTreeSet::new(),
            fixed_step: 12,
            present_sequence: 1,
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

    fn cpu_layer(blend: BlendMode) -> Layer2DState {
        Layer2DState {
            id: astra_media_core::Layer2DId("layer".into()),
            role: astra_media_core::Layer2DRole("content".into()),
            z_index: 0,
            content: Layer2DContent::WritableSurface(astra_media_core::WritableSurface2DRef {
                surface_id: astra_media_core::Surface2DId("surface".into()),
                generation: 1,
                width: 1,
                height: 1,
                stride: 4,
                format: Surface2DFormat::Rgba8SrgbPremultiplied,
                damage: astra_media_core::Layer2DDamage::Full,
            }),
            transform: astra_media_core::Transform2D::translation(1.0, 0.0),
            clip: Some(RectI::new(1, 0, 1, 1)),
            opacity: 1.0,
            texture_filter: TextureFilter2D::Nearest,
            blend,
            filter_graph: None,
        }
    }

    #[test]
    fn cpu_layer_compositor_supports_rgba_bgra_transform_clip_and_blends() {
        for blend in [
            BlendMode::Alpha,
            BlendMode::Add,
            BlendMode::Opaque,
            BlendMode::Multiply,
            BlendMode::Screen,
        ] {
            let mut frame = vec![0_u8; 8];
            composite_layer_cpu(
                &mut frame,
                2,
                1,
                &cpu_layer(blend),
                &[0, 0, 128, 128],
                1,
                1,
                4,
                Surface2DFormat::Bgra8SrgbPremultiplied,
            )
            .unwrap();
            assert_eq!(&frame[..4], &[0, 0, 0, 0]);
            assert!(frame[4] > 0, "blend {blend:?} did not draw red");
        }
    }

    #[test]
    fn cpu_layer_filter_graph_uses_explicit_fallback_contract() {
        let mut layer = cpu_layer(BlendMode::Alpha);
        layer.transform = astra_media_core::Transform2D::IDENTITY;
        layer.clip = None;
        layer.filter_graph = Some(astra_media_core::FilterGraph {
            schema: "astra.filter_graph.v1".into(),
            nodes: vec![astra_media_core::FilterNode {
                id: "fade".into(),
                kind: "astra.filter.fade".into(),
                input: astra_media_core::FilterTarget::Final,
                output: astra_media_core::FilterTarget::Final,
                params: BTreeMap::from([(
                    "amount".into(),
                    astra_media_core::FilterParam::Float(0.5),
                )]),
                deterministic: true,
                allow_cpu_fallback: true,
            }],
        });
        let mut frame = vec![0_u8; 4];
        composite_layer_cpu(
            &mut frame,
            1,
            1,
            &layer,
            &[128, 64, 32, 128],
            1,
            1,
            4,
            Surface2DFormat::Rgba8SrgbPremultiplied,
        )
        .unwrap();
        assert_eq!(frame, [64, 32, 16, 128]);
    }
}
