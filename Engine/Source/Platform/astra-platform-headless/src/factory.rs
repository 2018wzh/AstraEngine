use std::{
    collections::VecDeque,
    fmt::Debug,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use crate::artifact::{ArtifactRecorder, AudioArtifactStream};
use astra_headless_protocol::RendererExecutionIdentity;
#[cfg(feature = "ffmpeg-vcpkg")]
use astra_media::FfmpegDecodedPacket;
use astra_media::{
    DecodeKind as MediaDecodeKind, DecodeOutput as MediaDecodeOutput, DecodeProvider,
    DecodeRequest, ImageDecodeProvider, SymphoniaAudioDecodeProvider,
};
use astra_media_core::{
    CpuRendererProvider, HeadlessRenderer, MediaError, RenderTargetFormat, Renderer2DProvider,
    RendererCreateRequest, SceneCommand,
};
use astra_platform::{
    host_channel, AudioDeviceFormat, AudioOutputHandle, AudioOutputLane, AudioWakeRegistration,
    CapturedFrame, DecodeKind, DecodeOutput, DecodeSessionHandle, HeadlessHostProfile,
    HeadlessReadbackPolicy, HeadlessRenderPolicy, HostCommand, HostLaunchProfile,
    OpenedAudioOutput, PackageSourceHandle, PackageSourceRequest, PlatformError, PlatformErrorCode,
    PlatformHostFactory, PlatformHostSession, RgbaFrame, SaveTransactionHandle, SurfaceHandle,
    WindowHandle,
};
use astra_platform_common::WGPU_TIMESTAMP_RING_SIZE;
use astra_platform_common::{
    AtomicSaveStore, FilePackageSource, ResourceTable, SaveTransaction, WgpuOffscreenRenderer,
    WgpuPendingProfile, WgpuProfiledSubmission,
};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct HeadlessPlatformFactory {
    run_root: PathBuf,
    package_root: PathBuf,
    user_authorized_package: Option<PathBuf>,
    input_sequence_hash: String,
    https_root_certificates: Vec<Vec<u8>>,
    gpu_enabled: bool,
    performance_observer: Option<Arc<dyn HeadlessPerformanceObserver>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadlessGpuFrameSample {
    pub sequence: u64,
    pub input_flow_id: Option<u64>,
    pub scene_build_ns: u64,
    pub scene_digest_ns: u64,
    pub scene_validation_ns: u64,
    pub scene_pending_ns: u64,
    pub cpu_submit_ns: u64,
    pub gpu_duration_ns: u64,
    pub scene_cpu_ns: u64,
    pub filter_cpu_ns: u64,
    pub scene_command_cpu_ns: u64,
    pub scene_atlas_cpu_ns: u64,
    pub scene_geometry_cpu_ns: u64,
    pub scene_vertex_upload_cpu_ns: u64,
    pub scene_render_encode_cpu_ns: u64,
    pub scene_queue_submit_cpu_ns: u64,
    pub scene_render_submit_cpu_ns: u64,
    pub atlas_upload_gpu_ns: u64,
    pub scene_gpu_ns: u64,
    pub filter_gpu_ns: u64,
    pub gpu_resource_bytes: u64,
    pub atlas_bytes: u64,
    pub upload_bytes: u64,
    pub readback_bytes: u64,
    pub draw_calls: u64,
    pub queue_submissions: u64,
    pub pipeline_count: u64,
    pub heap_allocation_bytes: u64,
    pub heap_allocation_count: u64,
    pub command_allocation_bytes: u64,
    pub atlas_allocation_bytes: u64,
    pub geometry_allocation_bytes: u64,
}

pub trait HeadlessPerformanceObserver: Debug + Send + Sync {
    fn pace_gpu_frame(&self, sequence: u64) -> Result<(), PlatformError>;
    fn bind_gpu_frame(&self, sequence: u64) -> Result<Option<u64>, PlatformError>;
    fn record_gpu_frame(&self, sample: HeadlessGpuFrameSample) -> Result<(), PlatformError>;
}

impl HeadlessPlatformFactory {
    pub fn new(run_root: impl Into<PathBuf>, package_root: impl Into<PathBuf>) -> Self {
        Self {
            run_root: run_root.into(),
            package_root: package_root.into(),
            user_authorized_package: None,
            input_sequence_hash: astra_core::Hash256::from_sha256(&[]).to_string(),
            https_root_certificates: Vec::new(),
            gpu_enabled: false,
            performance_observer: None,
        }
    }
    pub fn with_gpu(mut self, enabled: bool) -> Self {
        self.gpu_enabled = enabled;
        self
    }
    pub fn with_performance_observer(
        mut self,
        observer: Arc<dyn HeadlessPerformanceObserver>,
    ) -> Self {
        self.performance_observer = Some(observer);
        self
    }
    pub fn with_input_sequence_hash(mut self, hash: impl Into<String>) -> Self {
        self.input_sequence_hash = hash.into();
        self
    }
    pub fn with_user_authorized_package(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_authorized_package = Some(path.into());
        self
    }
    pub fn with_https_root_certificate_pem(mut self, certificate: impl Into<Vec<u8>>) -> Self {
        self.https_root_certificates.push(certificate.into());
        self
    }
}

impl PlatformHostFactory for HeadlessPlatformFactory {
    fn start(&self, launch: HostLaunchProfile) -> astra_platform::HostStartFuture {
        let factory = self.clone();
        Box::pin(async move {
            let profile = launch.require_headless()?.clone();
            launch.validate()?;
            validate_provider_bindings(&profile, factory.gpu_enabled)?;
            if factory.performance_observer.is_some() {
                astra_platform::validate_headless_performance_profile(&profile)?;
            }
            let (client, backend, events) = host_channel(
                launch.clone(),
                profile.limits.command_queue_capacity,
                profile.limits.event_queue_capacity,
            )?;
            let performance_session = factory.performance_observer.is_some();
            let state = HostState::new(factory, profile, backend)?;
            tracing::info!(
                event = "platform.headless.session.start",
                "started isolated Headless platform session"
            );
            if performance_session {
                spawn_performance_host(state)?;
            } else {
                tokio::spawn(async move {
                    state.run().await;
                });
            }
            Ok(PlatformHostSession {
                client,
                events,
                profile: launch,
            })
        })
    }
}

fn spawn_performance_host(state: HostState) -> Result<(), PlatformError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| performance_thread_error("runtime", error.to_string()))?;
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("astra-headless-performance-host".into())
        .spawn(move || {
            let scheduling = astra_platform_common::PerformanceSchedulingGuard::activate();
            match scheduling {
                Ok(scheduling) => {
                    if ready_tx.send(Ok(())).is_err() {
                        return;
                    }
                    runtime.block_on(state.run());
                    if let Err(error) = scheduling.restore() {
                        tracing::error!(
                            event = "platform.headless.performance_scheduling.restore_failed",
                            diagnostic = %error,
                            "failed to restore Headless performance host scheduling policy"
                        );
                    }
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
        })
        .map_err(|error| performance_thread_error("spawn", error.to_string()))?;
    ready_rx
        .recv()
        .map_err(|error| performance_thread_error("handshake", error.to_string()))?
        .map_err(|error| performance_thread_error("scheduling", error))
}

fn performance_thread_error(stage: &str, diagnostic: String) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "headless.performance.thread",
        "dedicated performance host thread failed",
    )
    .with_field("stage", stage)
    .with_field("diagnostic", diagnostic)
}

fn validate_provider_bindings(
    profile: &HeadlessHostProfile,
    gpu_enabled: bool,
) -> Result<(), PlatformError> {
    match (profile.providers.renderer.as_str(), gpu_enabled) {
        ("cpu_reference", false) | ("wgpu_offscreen", true) => {}
        ("cpu_reference", true) => {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "headless.provider.bind",
                "--gpu requires the profile to bind wgpu_offscreen",
            ))
        }
        ("wgpu_offscreen", false) => {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "headless.provider.bind",
                "wgpu_offscreen requires explicit --gpu authorization",
            ))
        }
        (provider, _) => {
            return Err(PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "headless.provider.bind",
                "headless profile binds an unavailable renderer",
            )
            .with_field("provider", provider))
        }
    }
    for (field, actual, expected) in [
        ("text", profile.providers.text.as_str(), "cosmic_text_cpu"),
        (
            "audio_mixer",
            profile.providers.audio_mixer.as_str(),
            "kira",
        ),
        (
            "image_decode",
            profile.providers.image_decode.as_str(),
            "image_cpu",
        ),
        (
            "audio_decode",
            profile.providers.audio_decode.as_str(),
            "symphonia",
        ),
        (
            "save",
            profile.providers.save.as_str(),
            "transactional_file",
        ),
        (
            "package",
            profile.providers.package.as_str(),
            "verified_bounded",
        ),
    ] {
        if actual != expected {
            return Err(PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "headless.provider.bind",
                "headless profile binds an unavailable provider",
            )
            .with_field("field", field)
            .with_field("provider", actual));
        }
    }
    match profile.providers.video_decode.as_str() {
        "disabled" => Ok(()),
        "ffmpeg-vcpkg" => validate_ffmpeg_binding(),
        provider => Err(PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "headless.provider.bind",
            "headless profile binds an unavailable video provider",
        )
        .with_field("field", "video_decode")
        .with_field("provider", provider)),
    }
}

#[cfg(feature = "ffmpeg-vcpkg")]
fn validate_ffmpeg_binding() -> Result<(), PlatformError> {
    astra_media::probe_ffmpeg_provider()
        .map(|_| ())
        .map_err(media_error)
}

#[cfg(not(feature = "ffmpeg-vcpkg"))]
fn validate_ffmpeg_binding() -> Result<(), PlatformError> {
    Err(PlatformError::new(
        PlatformErrorCode::ProviderUnavailable,
        "headless.provider.bind",
        "ffmpeg-vcpkg is bound but the Headless backend was built without that feature",
    ))
}

struct WindowState {
    surface_count: usize,
}
struct SurfaceState {
    window: WindowHandle,
    renderer: HeadlessRenderer,
    width: u32,
    height: u32,
    last_sequence: u64,
    frame: Option<Arc<[u8]>>,
    pending: Option<PendingScene>,
    materialized_sequence: Option<u64>,
    gpu_renderer: Option<WgpuOffscreenRenderer>,
    pending_gpu_profiles: VecDeque<PendingGpuProfile>,
    deferred_gpu_resource_commands: Vec<SceneCommand>,
}
struct PendingGpuProfile {
    pending: WgpuPendingProfile,
    sample: HeadlessGpuFrameSample,
}
struct PendingScene {
    sequence: u64,
    width: u32,
    height: u32,
    renderer: Option<HeadlessRenderer>,
    commands: Vec<SceneCommand>,
    gpu_commands: Vec<SceneCommand>,
    clear_rgba: [u8; 4],
    semantics: Option<astra_ui_core::UiSemanticSnapshot>,
    scene_build_ns: u64,
    scene_digest_ns: u64,
    scene_validation_ns: u64,
    scene_pending_ns: u64,
}
struct AudioState {
    paused: Arc<AtomicBool>,
    wake: AudioWakeRegistration,
    artifact_stream: Arc<Mutex<Option<AudioArtifactStream>>>,
}

struct HeadlessAudioLane {
    sample_rate: u32,
    channels: u16,
    chunk_samples: usize,
    started: Instant,
    submitted_samples: u64,
    consumed_samples: Arc<AtomicU64>,
    paused: Arc<AtomicBool>,
    wake: AudioWakeRegistration,
    observed_wake: u64,
    artifact_stream: Arc<Mutex<Option<AudioArtifactStream>>>,
    artifacts: Arc<Mutex<ArtifactRecorder>>,
    capture: Option<astra_platform::AudioCaptureReader>,
}

impl AudioOutputLane for HeadlessAudioLane {
    fn wait_for_capacity(
        &mut self,
        requested_samples: usize,
        stop: &AtomicBool,
    ) -> Result<(), PlatformError> {
        if requested_samples != self.chunk_samples {
            return Err(invalid(
                "audio.lane.wait",
                "mixer chunk size does not match the Headless output lane",
            ));
        }
        while self.paused.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
            if let Some(sequence) = self
                .wake
                .wait_timeout(self.observed_wake, Duration::from_millis(20))
            {
                self.observed_wake = sequence;
            }
        }
        Ok(())
    }

    fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError> {
        if samples.len() != self.chunk_samples || samples.iter().any(|sample| !sample.is_finite()) {
            return Err(invalid(
                "audio.lane.submit",
                "Headless audio chunk has an invalid size or non-finite sample",
            ));
        }
        let next_samples = self
            .submitted_samples
            .checked_add(samples.len() as u64)
            .ok_or_else(|| invalid("audio.lane.submit", "sample count overflowed"))?;
        let next_frames = next_samples / u64::from(self.channels);
        let deadline_ns = u128::from(next_frames)
            .checked_mul(1_000_000_000)
            .ok_or_else(|| invalid("audio.lane.submit", "audio deadline overflowed"))?
            / u128::from(self.sample_rate);
        let deadline = self
            .started
            .checked_add(Duration::from_nanos(u64::try_from(deadline_ns).map_err(
                |_| invalid("audio.lane.submit", "audio deadline overflowed"),
            )?))
            .ok_or_else(|| invalid("audio.lane.submit", "audio deadline overflowed"))?;
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline.duration_since(now));
        }
        {
            let mut stream = self
                .artifact_stream
                .lock()
                .map_err(|_| invalid("audio.lane.submit", "audio stream lock is poisoned"))?;
            if let Some(stream) = stream.as_mut() {
                self.artifacts
                    .lock()
                    .map_err(|_| {
                        invalid("audio.lane.submit", "artifact recorder lock is poisoned")
                    })?
                    .append_audio_stream(stream, &samples)?;
            }
        }
        if let Some(capture) = &self.capture {
            capture.append(&samples)?;
        }
        self.submitted_samples = next_samples;
        self.consumed_samples.store(next_samples, Ordering::Release);
        Ok(samples)
    }

    fn consumed_samples(&self) -> u64 {
        self.consumed_samples.load(Ordering::Acquire)
    }

    fn underflow_count(&self) -> u64 {
        0
    }
}
struct DecodeState {
    kind: DecodeKind,
    last_sequence: u64,
    #[cfg(feature = "ffmpeg-vcpkg")]
    video_stream: Option<HeadlessVideoStream>,
}

#[cfg(feature = "ffmpeg-vcpkg")]
struct HeadlessVideoStream {
    decoder: astra_media::FfmpegPlaybackDecoder,
    duration_us: u64,
    max_frames: u64,
    max_decoded_byte_count: u64,
    frame_count: u64,
    decoded_byte_count: u64,
    end_emitted: bool,
}
enum PackageState {
    File(FilePackageSource),
    HttpsRange {
        client: reqwest::Client,
        url: url::Url,
        length: u64,
        block_size: usize,
        block_hashes: Vec<[u8; 32]>,
    },
}

struct HostState {
    profile: HeadlessHostProfile,
    backend: astra_platform::PlatformBackendChannels,
    windows: ResourceTable<WindowState, WindowHandle>,
    surfaces: ResourceTable<SurfaceState, SurfaceHandle>,
    audio: ResourceTable<AudioState, AudioOutputHandle>,
    decoders: ResourceTable<DecodeState, DecodeSessionHandle>,
    saves: ResourceTable<SaveTransaction, SaveTransactionHandle>,
    packages: ResourceTable<PackageState, PackageSourceHandle>,
    save_store: AtomicSaveStore,
    package_root: PathBuf,
    user_authorized_package: Option<PathBuf>,
    artifacts: Arc<Mutex<ArtifactRecorder>>,
    https_root_certificates: Vec<Vec<u8>>,
    performance_observer: Option<Arc<dyn HeadlessPerformanceObserver>>,
}

impl HostState {
    fn new(
        factory: HeadlessPlatformFactory,
        profile: HeadlessHostProfile,
        backend: astra_platform::PlatformBackendChannels,
    ) -> Result<Self, PlatformError> {
        let save_store = AtomicSaveStore::new(&factory.run_root, &profile.package_id)?;
        let artifacts = Arc::new(Mutex::new(ArtifactRecorder::new(
            factory.run_root.clone(),
            &profile,
            factory.input_sequence_hash,
        )?));
        Ok(Self {
            profile,
            backend,
            windows: ResourceTable::new("window"),
            surfaces: ResourceTable::new("surface"),
            audio: ResourceTable::new("audio_output"),
            decoders: ResourceTable::new("decode_session"),
            saves: ResourceTable::new("save_transaction"),
            packages: ResourceTable::new("package_source"),
            save_store,
            package_root: factory.package_root,
            user_authorized_package: factory.user_authorized_package,
            artifacts,
            https_root_certificates: factory.https_root_certificates,
            performance_observer: factory.performance_observer,
        })
    }

    async fn run(mut self) {
        while let Some(command) = self.backend.next_command().await {
            let shutdown = matches!(command, HostCommand::Shutdown { .. });
            self.handle(command).await;
            if shutdown && self.is_empty() {
                break;
            }
        }
    }

    fn flush_gpu_profile(&mut self, surface: SurfaceHandle) -> Result<(), PlatformError> {
        let state = self.surfaces.get_mut(surface)?;
        if state.pending_gpu_profiles.is_empty() {
            return Ok(());
        }
        let renderer = state.gpu_renderer.as_mut().ok_or_else(|| {
            invalid(
                "surface.performance",
                "pending GPU profile has no GPU renderer",
            )
        })?;
        let observer = self.performance_observer.as_ref().ok_or_else(|| {
            invalid(
                "surface.performance",
                "pending GPU profile has no performance observer",
            )
        })?;
        while let Some(profile) = state.pending_gpu_profiles.pop_front() {
            let submission = renderer.resolve_profiled_submission(profile.pending)?;
            observer.record_gpu_frame(complete_gpu_sample(profile.sample, submission))?;
        }
        Ok(())
    }

    fn materialize_surface(
        &mut self,
        surface: SurfaceHandle,
        capture: bool,
    ) -> Result<CapturedFrame, PlatformError> {
        if let Some(cached) = {
            let state = self.surfaces.get(surface)?;
            state.frame.as_ref().map(|rgba8| CapturedFrame {
                width: state.width,
                height: state.height,
                rgba8: Arc::clone(rgba8),
            })
        } {
            return Ok(cached);
        }
        if self.surfaces.get(surface)?.pending.is_none() {
            if !capture {
                let state = self.surfaces.get(surface)?;
                return Ok(CapturedFrame {
                    width: state.width,
                    height: state.height,
                    rgba8: Arc::<[u8]>::from([]),
                });
            }
            let state = self.surfaces.get_mut(surface)?;
            let renderer = state.gpu_renderer.as_mut().ok_or_else(|| {
                invalid("surface.capture", "surface has no pending GPU checkpoint")
            })?;
            let captured = renderer.capture_checkpoint()?;
            self.artifacts
                .lock()
                .map_err(|_| invalid("artifact.frame", "artifact recorder lock is poisoned"))?
                .record_rasterized_frame(
                    state.materialized_sequence.ok_or_else(|| {
                        invalid("surface.capture", "surface has no materialized sequence")
                    })?,
                    captured.width,
                    captured.height,
                    &captured.rgba8,
                )?;
            state.frame = Some(Arc::clone(&captured.rgba8));
            return Ok(captured);
        }
        let mut pending = self
            .surfaces
            .get_mut(surface)?
            .pending
            .take()
            .ok_or_else(|| invalid("surface.capture", "surface has not submitted a scene"))?;
        let captured = {
            let state = self.surfaces.get_mut(surface)?;
            if let Some(renderer) = &mut state.gpu_renderer {
                if state.pending_gpu_profiles.len() >= WGPU_TIMESTAMP_RING_SIZE - 1 {
                    if let Some(profile) = state.pending_gpu_profiles.front() {
                        if let Some(submission) =
                            renderer.try_resolve_profiled_submission(profile.pending)?
                        {
                            let profile = state
                                .pending_gpu_profiles
                                .pop_front()
                                .expect("profile queue is not empty");
                            self.performance_observer
                                .as_ref()
                                .ok_or_else(|| {
                                    invalid(
                                        "surface.performance",
                                        "pending GPU profile has no performance observer",
                                    )
                                })?
                                .record_gpu_frame(complete_gpu_sample(
                                    profile.sample,
                                    submission,
                                ))?;
                        }
                    }
                }
                if state.pending_gpu_profiles.len() == WGPU_TIMESTAMP_RING_SIZE {
                    let profile = state
                        .pending_gpu_profiles
                        .pop_front()
                        .expect("profile queue is not empty");
                    let submission = renderer.resolve_profiled_submission(profile.pending)?;
                    self.performance_observer
                        .as_ref()
                        .ok_or_else(|| {
                            invalid(
                                "surface.performance",
                                "pending GPU profile has no performance observer",
                            )
                        })?
                        .record_gpu_frame(complete_gpu_sample(profile.sample, submission))?;
                }
                let frame = astra_platform::SceneFrame {
                    sequence: pending.sequence,
                    width: pending.width,
                    height: pending.height,
                    clear_rgba: pending.clear_rgba,
                    commands: pending.gpu_commands,
                    semantics: pending.semantics,
                };
                if let Some(observer) = &self.performance_observer {
                    observer.pace_gpu_frame(pending.sequence)?;
                    let pending_profile = renderer.submit_frame_timestamped(&frame)?;
                    let captured = if capture {
                        renderer.capture_checkpoint()?
                    } else {
                        CapturedFrame {
                            width: pending.width,
                            height: pending.height,
                            rgba8: Arc::<[u8]>::from([]),
                        }
                    };
                    let counters = renderer.performance_counters();
                    state.pending_gpu_profiles.push_back(PendingGpuProfile {
                        pending: pending_profile,
                        sample: HeadlessGpuFrameSample {
                            sequence: pending.sequence,
                            input_flow_id: observer.bind_gpu_frame(pending.sequence)?,
                            scene_build_ns: pending.scene_build_ns,
                            scene_digest_ns: pending.scene_digest_ns,
                            scene_validation_ns: pending.scene_validation_ns,
                            scene_pending_ns: pending.scene_pending_ns,
                            cpu_submit_ns: 0,
                            gpu_duration_ns: 0,
                            scene_cpu_ns: 0,
                            filter_cpu_ns: 0,
                            scene_command_cpu_ns: 0,
                            scene_atlas_cpu_ns: 0,
                            scene_geometry_cpu_ns: 0,
                            scene_vertex_upload_cpu_ns: 0,
                            scene_render_encode_cpu_ns: 0,
                            scene_queue_submit_cpu_ns: 0,
                            scene_render_submit_cpu_ns: 0,
                            atlas_upload_gpu_ns: 0,
                            scene_gpu_ns: 0,
                            filter_gpu_ns: 0,
                            gpu_resource_bytes: counters.gpu_resource_bytes,
                            atlas_bytes: counters.atlas_bytes,
                            upload_bytes: counters.upload_bytes,
                            readback_bytes: counters.readback_bytes,
                            draw_calls: counters.draw_calls,
                            queue_submissions: counters.queue_submissions,
                            pipeline_count: counters.pipeline_count,
                            heap_allocation_bytes: counters.engine_allocation_bytes,
                            heap_allocation_count: counters.engine_allocation_count,
                            command_allocation_bytes: counters.command_allocation_bytes,
                            atlas_allocation_bytes: counters.atlas_allocation_bytes,
                            geometry_allocation_bytes: counters.geometry_allocation_bytes,
                        },
                    });
                    captured
                } else if capture {
                    renderer.render(&frame)?
                } else {
                    renderer.submit_frame(&frame)?;
                    CapturedFrame {
                        width: pending.width,
                        height: pending.height,
                        rgba8: Arc::<[u8]>::from([]),
                    }
                }
            } else {
                let mut renderer = pending.renderer.take().ok_or_else(|| {
                    invalid("surface.capture", "CPU pending renderer is unavailable")
                })?;
                let output = renderer
                    .capture_frame(&pending.commands)
                    .map_err(media_error)?;
                CapturedFrame {
                    width: output.width,
                    height: output.height,
                    rgba8: output.bytes.into(),
                }
            }
        };
        if let Some(renderer) = &self.surfaces.get(surface)?.gpu_renderer {
            if renderer.performance_counters().gpu_resource_bytes
                > self.profile.max_gpu_resource_bytes
            {
                return Err(invalid(
                    "surface.capture",
                    "GPU resources exceed the profile-bound residency budget",
                ));
            }
        }
        if captured.width != pending.width || captured.height != pending.height {
            return Err(invalid(
                "surface.capture",
                "materialized frame dimensions do not match the submitted scene",
            ));
        }
        if capture {
            self.artifacts
                .lock()
                .map_err(|_| invalid("artifact.frame", "artifact recorder lock is poisoned"))?
                .record_rasterized_frame(
                    pending.sequence,
                    captured.width,
                    captured.height,
                    &captured.rgba8,
                )?;
        }
        let state = self.surfaces.get_mut(surface)?;
        state.frame = capture.then(|| Arc::clone(&captured.rgba8));
        state.pending = None;
        state.materialized_sequence = Some(pending.sequence);
        state.deferred_gpu_resource_commands.clear();
        Ok(captured)
    }

    async fn handle(&mut self, command: HostCommand) {
        match command {
            HostCommand::CreateWindow { reply, .. } => {
                let _ = reply.send(self.windows.insert(WindowState { surface_count: 0 }));
            }
            HostCommand::CreateSurface { request, reply } => {
                let gpu_renderer = if self.profile.providers.renderer == "wgpu_offscreen" {
                    let renderer = if self.performance_observer.is_some() {
                        match self.profile.gpu_adapter.as_ref() {
                            Some(policy) => {
                                WgpuOffscreenRenderer::new_for_performance(
                                    policy,
                                    self.profile.max_gpu_resource_bytes,
                                    request.width,
                                    request.height,
                                )
                                .await
                            }
                            None => Err(PlatformError::new(
                                PlatformErrorCode::InvalidProfile,
                                "headless.performance.gpu_policy",
                                "performance observer requires an explicit GPU adapter policy",
                            )),
                        }
                    } else if let Some(policy) = &self.profile.gpu_adapter {
                        WgpuOffscreenRenderer::new_with_policy(policy).await
                    } else {
                        WgpuOffscreenRenderer::new().await
                    };
                    match renderer {
                        Ok(renderer) => Some(renderer),
                        Err(error) => {
                            let _ = reply.send(Err(error));
                            return;
                        }
                    }
                } else {
                    None
                };
                let result = (|| {
                    if let Some(renderer) = &gpu_renderer {
                        let identity = renderer.identity();
                        self.artifacts
                            .lock()
                            .map_err(|_| {
                                invalid("artifact.renderer", "artifact recorder lock is poisoned")
                            })?
                            .set_renderer_identity(RendererExecutionIdentity {
                                provider: identity.provider.clone(),
                                backend: identity.backend.clone(),
                                device_type: identity.device_type.clone(),
                                vendor_id: identity.vendor_id,
                                device_id: identity.device_id,
                                adapter_name_hash: identity.adapter_name_hash.clone(),
                                driver_identity_hash: identity.driver_identity_hash.clone(),
                            })?;
                    }
                    let window = self.windows.get_mut(request.window)?;
                    let renderer = CpuRendererProvider
                        .create(RendererCreateRequest {
                            width: request.width,
                            height: request.height,
                            format: RenderTargetFormat::Rgba8Srgb,
                            profile: self.profile.id.clone(),
                        })
                        .map_err(media_error)?;
                    let handle = self.surfaces.insert(SurfaceState {
                        window: request.window,
                        renderer,
                        width: request.width,
                        height: request.height,
                        last_sequence: 0,
                        frame: None,
                        pending: None,
                        materialized_sequence: None,
                        gpu_renderer,
                        pending_gpu_profiles: VecDeque::with_capacity(WGPU_TIMESTAMP_RING_SIZE),
                        deferred_gpu_resource_commands: Vec::new(),
                    })?;
                    window.surface_count += 1;
                    Ok(handle)
                })();
                let _ = reply.send(result);
            }
            HostCommand::CaptureSurface { surface, reply } => {
                let result = self.materialize_surface(surface, true);
                let _ = reply.send(result);
            }
            HostCommand::PresentRgba {
                surface,
                frame,
                reply,
            } => {
                let result = (|| {
                    let state = self.surfaces.get(surface)?;
                    ensure_increasing(state.last_sequence, frame.sequence, "surface.present_rgba")?;
                    if frame.width != state.width || frame.height != state.height {
                        return Err(invalid(
                            "surface.present_rgba",
                            "frame dimensions do not match surface",
                        ));
                    }
                    let canonical = canonical_json_digest(&(
                        frame.sequence,
                        frame.width,
                        frame.height,
                        &frame.rgba8,
                    ))
                    .map_err(|_| invalid("surface.present_rgba", "frame serialization failed"))?;
                    {
                        let mut artifacts = self.artifacts.lock().map_err(|_| {
                            invalid("artifact.frame", "artifact recorder lock is poisoned")
                        })?;
                        artifacts.record_submission(frame.sequence, &canonical)?;
                        artifacts.record_rasterized_frame(
                            frame.sequence,
                            frame.width,
                            frame.height,
                            &frame.rgba8,
                        )?;
                    }
                    present_rgba(self.surfaces.get_mut(surface)?, frame)
                })();
                let _ = reply.send(result);
            }
            HostCommand::PresentScene {
                surface,
                frame,
                reply,
            } => {
                let result = (|| {
                    let scene_build_started = Instant::now();
                    let sequence = frame.sequence;
                    let (canonical, journal, pending, deferred_resources, materialize) = {
                        let s = self.surfaces.get(surface)?;
                        ensure_increasing(
                            s.last_sequence,
                            frame.sequence,
                            "surface.present_scene",
                        )?;
                        if frame.width != s.width || frame.height != s.height {
                            return Err(invalid(
                                "surface.present_scene",
                                "frame dimensions do not match surface",
                            ));
                        }
                        let mut commands = Vec::with_capacity(frame.commands.len() + 1);
                        commands.push(SceneCommand::Clear {
                            rgba: frame.clear_rgba,
                        });
                        commands.extend(frame.commands);
                        let scene_validation_started = Instant::now();
                        let journal = s.renderer.validate_frame(&commands).map_err(media_error)?;
                        let scene_validation_ns = elapsed_ns(
                            scene_validation_started,
                            "surface.present_scene",
                            "scene validation duration overflowed",
                        )?;
                        let scene_digest_started = Instant::now();
                        let canonical = scene_submission_identity(
                            frame.sequence,
                            frame.width,
                            frame.height,
                            frame.clear_rgba,
                            commands.len(),
                            &frame.semantics,
                        )?;
                        let scene_digest_ns = elapsed_ns(
                            scene_digest_started,
                            "surface.present_scene",
                            "scene digest duration overflowed",
                        )?;
                        let scene_pending_started = Instant::now();
                        let deferred_resources: Vec<_> = commands
                            .iter()
                            .filter(|command| {
                                matches!(
                                    command,
                                    SceneCommand::UploadTexture { .. }
                                        | SceneCommand::UpdateTextureRegion { .. }
                                        | SceneCommand::UploadGlyph { .. }
                                        | SceneCommand::ReleaseResource { .. }
                                )
                            })
                            .cloned()
                            .collect();
                        let (pending_renderer, commands, gpu_commands) = if s.gpu_renderer.is_some()
                        {
                            let mut gpu_commands = s.deferred_gpu_resource_commands.clone();
                            gpu_commands.extend(commands.into_iter().skip(1));
                            (None, Vec::new(), gpu_commands)
                        } else {
                            (Some(s.renderer.clone()), commands, Vec::new())
                        };
                        let pending = PendingScene {
                            sequence,
                            width: frame.width,
                            height: frame.height,
                            renderer: pending_renderer,
                            commands,
                            gpu_commands,
                            clear_rgba: frame.clear_rgba,
                            semantics: frame.semantics,
                            scene_build_ns: scene_build_started
                                .elapsed()
                                .as_nanos()
                                .try_into()
                                .map_err(|_| {
                                    invalid(
                                        "surface.present_scene",
                                        "scene build duration overflowed",
                                    )
                                })?,
                            scene_digest_ns,
                            scene_validation_ns,
                            scene_pending_ns: elapsed_ns(
                                scene_pending_started,
                                "surface.present_scene",
                                "scene pending duration overflowed",
                            )?,
                        };
                        (
                            canonical,
                            journal,
                            pending,
                            deferred_resources,
                            self.profile.render_policy == HeadlessRenderPolicy::All
                                || sequence == 1,
                        )
                    };
                    // The canonical submission stream advances only after the full
                    // scene has validated. Invalid skipped frames therefore cannot
                    // alter either retained resources or submitted evidence.
                    self.artifacts
                        .lock()
                        .map_err(|_| {
                            invalid("artifact.scene", "artifact recorder lock is poisoned")
                        })?
                        .record_submission(sequence, &canonical)?;
                    {
                        let s = self.surfaces.get_mut(surface)?;
                        s.renderer.commit_frame(journal);
                        s.pending = Some(pending);
                        s.frame = None;
                        s.materialized_sequence = None;
                        s.last_sequence = sequence;
                        if s.gpu_renderer.is_some() {
                            s.deferred_gpu_resource_commands.extend(deferred_resources);
                        }
                    }
                    if materialize {
                        let capture = self.profile.readback_policy
                            == HeadlessReadbackPolicy::RasterizedFrames;
                        self.materialize_surface(surface, capture)?;
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            HostCommand::InjectSurfaceDeviceLoss { reply, .. } => {
                let _ = reply.send(Err(PlatformError::new(
                    PlatformErrorCode::PlatformNotImplemented,
                    "surface.test.inject_device_loss",
                    "Headless is an E2 test host and does not emulate native device loss",
                )));
            }
            HostCommand::DestroySurface { surface, reply } => {
                let capture =
                    self.profile.readback_policy == HeadlessReadbackPolicy::RasterizedFrames;
                let result = self
                    .surfaces
                    .get(surface)
                    .map(|state| state.pending.is_some() || state.frame.is_some())
                    .and_then(|has_submitted_frame| {
                        if has_submitted_frame {
                            self.materialize_surface(surface, capture).map(|_| ())
                        } else {
                            Ok(())
                        }
                    })
                    .and_then(|_| self.flush_gpu_profile(surface))
                    .and_then(|_| self.surfaces.remove(surface))
                    .and_then(|s| {
                        let window = self.windows.get_mut(s.window)?;
                        window.surface_count =
                            window.surface_count.checked_sub(1).ok_or_else(|| {
                                invalid("surface.destroy", "window surface count underflow")
                            })?;
                        Ok(())
                    });
                let _ = reply.send(result);
            }
            HostCommand::DestroyWindow { window, reply } => {
                let result = self
                    .windows
                    .get(window)
                    .and_then(|w| {
                        if w.surface_count == 0 {
                            Ok(())
                        } else {
                            Err(invalid("window.destroy", "window still owns live surfaces"))
                        }
                    })
                    .and_then(|_| self.windows.remove(window).map(|_| ()));
                let _ = reply.send(result);
            }
            HostCommand::OpenAudioOutput { request, reply } => {
                let result = if request.sample_rate != 48_000
                    || request.channels != 2
                    || request.chunk_frames == 0
                    || request.chunk_frames > request.max_buffered_frames
                {
                    Err(invalid(
                        "audio.open",
                        "headless audio requires 48kHz stereo and a bounded non-zero chunk",
                    ))
                } else {
                    (|| {
                        let artifact_stream = if matches!(
                            self.profile.artifacts.retention,
                            astra_platform::HeadlessArtifactRetention::ManifestOnly
                        ) {
                            None
                        } else {
                            Some(
                                self.artifacts
                                    .lock()
                                    .map_err(|_| {
                                        invalid("audio.open", "artifact recorder lock is poisoned")
                                    })?
                                    .begin_audio_stream()?,
                            )
                        };
                        let paused = Arc::new(AtomicBool::new(request.start_paused));
                        let wake = AudioWakeRegistration::default();
                        let artifact_stream = Arc::new(Mutex::new(artifact_stream));
                        let consumed_samples = Arc::new(AtomicU64::new(0));
                        let capture = request
                            .capture_samples
                            .then(astra_platform::AudioCaptureReader::default);
                        let handle = self.audio.insert(AudioState {
                            paused: Arc::clone(&paused),
                            wake: wake.clone(),
                            artifact_stream: Arc::clone(&artifact_stream),
                        })?;
                        let chunk_samples = request
                            .chunk_frames
                            .checked_mul(usize::from(request.channels))
                            .ok_or_else(|| {
                                invalid("audio.open", "audio chunk sample count overflowed")
                            })?;
                        Ok(OpenedAudioOutput {
                            handle,
                            format: AudioDeviceFormat {
                                sample_rate: request.sample_rate,
                                channels: request.channels,
                            },
                            capture: capture.clone(),
                            lane: Box::new(HeadlessAudioLane {
                                sample_rate: request.sample_rate,
                                channels: request.channels,
                                chunk_samples,
                                started: Instant::now(),
                                submitted_samples: 0,
                                consumed_samples,
                                paused,
                                wake,
                                observed_wake: 0,
                                artifact_stream,
                                artifacts: Arc::clone(&self.artifacts),
                                capture,
                            }),
                        })
                    })()
                };
                let _ = reply.send(result);
            }
            HostCommand::PauseAudio { output, reply } => {
                let result = self.audio.get(output).and_then(|audio| {
                    if audio.paused.swap(true, Ordering::AcqRel) {
                        Err(invalid("audio.pause", "audio output is already paused"))
                    } else {
                        audio.wake.notify();
                        Ok(())
                    }
                });
                let _ = reply.send(result);
            }
            HostCommand::ResumeAudio { output, reply } => {
                let result = self.audio.get(output).and_then(|audio| {
                    if !audio.paused.swap(false, Ordering::AcqRel) {
                        Err(invalid("audio.resume", "audio output is not paused"))
                    } else {
                        audio.wake.notify();
                        Ok(())
                    }
                });
                let _ = reply.send(result);
            }
            HostCommand::AbortAudio { output, reply } => {
                let result = self.audio.remove(output).and_then(|state| {
                    state.wake.notify();
                    let stream = state
                        .artifact_stream
                        .lock()
                        .map_err(|_| invalid("audio.abort", "audio stream lock is poisoned"))?
                        .take();
                    if let Some(stream) = stream {
                        self.artifacts
                            .lock()
                            .map_err(|_| {
                                invalid("audio.abort", "artifact recorder lock is poisoned")
                            })?
                            .abort_audio_stream(stream);
                    }
                    Ok(())
                });
                let _ = reply.send(result);
            }
            HostCommand::InjectAudioDeviceLoss { reply, .. } => {
                let _ = reply.send(Err(PlatformError::new(
                    PlatformErrorCode::PlatformNotImplemented,
                    "audio.test.inject_device_loss",
                    "Headless is an E2 test host and does not emulate native device loss",
                )));
            }
            HostCommand::CloseAudio { output, reply } => {
                let result = (|| {
                    let state = self.audio.remove(output)?;
                    state.wake.notify();
                    let stream = state
                        .artifact_stream
                        .lock()
                        .map_err(|_| invalid("audio.close", "audio stream lock is poisoned"))?
                        .take();
                    if let Some(stream) = stream {
                        self.artifacts
                            .lock()
                            .map_err(|_| {
                                invalid("audio.close", "artifact recorder lock is poisoned")
                            })?
                            .finish_audio_stream(stream)?;
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            HostCommand::OpenDecode { kind, reply } => {
                let result = self.decoders.insert(DecodeState {
                    kind,
                    last_sequence: 0,
                    #[cfg(feature = "ffmpeg-vcpkg")]
                    video_stream: None,
                });
                let _ = reply.send(result);
            }
            HostCommand::Decode {
                session,
                request,
                reply,
            } => {
                let result = self.decoders.get_mut(session).and_then(|state| {
                    decode_session(
                        state,
                        request,
                        &self.profile.providers.video_decode,
                        self.profile.max_video_frames,
                        self.profile.max_decode_output_bytes,
                    )
                });
                let _ = reply.send(result);
            }
            HostCommand::CloseDecode { session, reply } => {
                let result = self.decoders.remove(session).map(|state| {
                    #[cfg(feature = "ffmpeg-vcpkg")]
                    let had_video_stream = state.video_stream.is_some();
                    #[cfg(not(feature = "ffmpeg-vcpkg"))]
                    let had_video_stream = false;
                    tracing::info!(
                        event = "platform.headless.decode.session.closed",
                        kind = ?state.kind,
                        had_video_stream,
                        "closed Headless decode session and released decoder resources"
                    );
                });
                let _ = reply.send(result);
            }
            HostCommand::BeginSave { slot, reply } => {
                let result = self
                    .save_store
                    .begin(&slot)
                    .and_then(|s| self.saves.insert(s));
                let _ = reply.send(result);
            }
            HostCommand::WriteSave {
                transaction,
                bytes,
                reply,
            } => {
                let result = self
                    .saves
                    .get_mut(transaction)
                    .and_then(|s| s.write(&bytes));
                let _ = reply.send(result);
            }
            HostCommand::CommitSave { transaction, reply } => {
                let result = self
                    .saves
                    .remove(transaction)
                    .and_then(SaveTransaction::commit);
                let _ = reply.send(result);
            }
            HostCommand::AbortSave { transaction, reply } => {
                let result = self
                    .saves
                    .remove(transaction)
                    .and_then(SaveTransaction::abort);
                let _ = reply.send(result);
            }
            HostCommand::ReadSave { slot, reply } => {
                let _ = reply.send(self.save_store.read(&slot));
            }
            HostCommand::ListSaves { reply } => {
                let _ = reply.send(self.save_store.list());
            }
            HostCommand::DeleteSave { slot, reply } => {
                let _ = reply.send(self.save_store.delete(&slot));
            }
            HostCommand::OpenPackage { source, reply } => {
                let result = self.open_package(source).await;
                let _ = reply.send(result);
            }
            HostCommand::ReadPackageRange {
                source,
                offset,
                length,
                reply,
            } => {
                let max = self.profile.limits.max_package_read_bytes;
                let result = if length > max {
                    Err(invalid("package.read_range", "range exceeds profile limit"))
                } else {
                    match self.packages.get_mut(source) {
                        Ok(source) => package_range(source, offset, length).await,
                        Err(error) => Err(error),
                    }
                };
                let _ = reply.send(result);
            }
            HostCommand::ClosePackage { source, reply } => {
                let result = self.packages.remove(source).map(|_| ());
                let _ = reply.send(result);
            }
            HostCommand::Shutdown { reply } => {
                let result = self.ensure_empty().and_then(|_| {
                    self.artifacts
                        .lock()
                        .map_err(|_| {
                            invalid("artifact.finish", "artifact recorder lock is poisoned")
                        })?
                        .finish()
                        .map(|_| ())
                });
                let _ = reply.send(result);
            }
        }
    }

    async fn open_package(
        &mut self,
        source: PackageSourceRequest,
    ) -> Result<PackageSourceHandle, PlatformError> {
        let state = match source {
            PackageSourceRequest::Bundled {
                relative_path,
                expected_hash,
            } => {
                let path = safe_join(&self.package_root, &relative_path)?;
                PackageState::File(FilePackageSource::open(path, &expected_hash)?)
            }
            PackageSourceRequest::UserAuthorized { expected_hash } => {
                let path = self.user_authorized_package.as_ref().ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::Cancelled,
                        "package.open_user_authorized",
                        "no user-authorized package was supplied to the test harness",
                    )
                })?;
                PackageState::File(FilePackageSource::open(path, &expected_hash)?)
            }
            PackageSourceRequest::HttpsRange { url, expected_hash } => {
                let parsed = url::Url::parse(&url).map_err(|_| {
                    PlatformError::new(
                        PlatformErrorCode::PermissionDenied,
                        "package.open_https",
                        "HTTPS package URL is invalid",
                    )
                })?;
                if !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.fragment().is_some()
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::PermissionDenied,
                        "package.open_https",
                        "HTTPS package URL must not contain credentials or a fragment",
                    ));
                }
                let origin = parsed.origin().ascii_serialization();
                let allowed = self.profile.package_sources.iter().any(|policy| {
                    matches!(policy, astra_platform::PackageSourcePolicy::HttpsRange { allowed_origins } if allowed_origins.iter().any(|allowed| allowed == &origin))
                });
                if parsed.scheme() != "https" || !allowed {
                    return Err(PlatformError::new(
                        PlatformErrorCode::PermissionDenied,
                        "package.open_https",
                        "HTTPS package origin is not allowed by the profile",
                    ));
                }
                open_https_range(
                    parsed,
                    &expected_hash,
                    self.profile.max_package_bytes,
                    self.profile.limits.max_package_read_bytes,
                    &self.https_root_certificates,
                )
                .await?
            }
        };
        self.packages.insert(state)
    }

    fn ensure_empty(&self) -> Result<(), PlatformError> {
        self.surfaces
            .ensure_empty()
            .and_then(|_| self.windows.ensure_empty())
            .and_then(|_| self.audio.ensure_empty())
            .and_then(|_| self.decoders.ensure_empty())
            .and_then(|_| self.saves.ensure_empty())
            .and_then(|_| self.packages.ensure_empty())
    }
    fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
            && self.windows.is_empty()
            && self.audio.is_empty()
            && self.decoders.is_empty()
            && self.saves.is_empty()
            && self.packages.is_empty()
    }
}

fn present_rgba(surface: &mut SurfaceState, frame: RgbaFrame) -> Result<(), PlatformError> {
    ensure_increasing(
        surface.last_sequence,
        frame.sequence,
        "surface.present_rgba",
    )?;
    if frame.width != surface.width || frame.height != surface.height {
        return Err(invalid(
            "surface.present_rgba",
            "frame dimensions do not match surface",
        ));
    }
    surface.frame = Some(frame.rgba8.into());
    surface.pending = None;
    surface.materialized_sequence = Some(frame.sequence);
    surface.last_sequence = frame.sequence;
    Ok(())
}
fn ensure_increasing(last: u64, next: u64, operation: &'static str) -> Result<(), PlatformError> {
    if next == 0 || next <= last {
        return Err(invalid(
            operation,
            "sequence is zero, duplicated, or reversed",
        ));
    }
    Ok(())
}
fn decode_session(
    state: &mut DecodeState,
    request: astra_platform::PlatformDecodeRequest,
    video_binding: &str,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
) -> Result<DecodeOutput, PlatformError> {
    if request.sequence == 0 || request.sequence <= state.last_sequence {
        return Err(invalid(
            "decode.submit",
            "decode request sequence must be strictly increasing within a session",
        ));
    }
    if state.kind != request.kind {
        return Err(invalid(
            "decode.submit",
            "decode request kind does not match session",
        ));
    }
    let sequence = request.sequence;
    let action = request.stream_action;
    let output = match action {
        astra_platform::DecodeStreamAction::OneShot => {
            if request.bytes.is_empty() {
                return Err(invalid(
                    "decode.submit",
                    "one-shot decode requires non-empty encoded bytes",
                ));
            }
            #[cfg(feature = "ffmpeg-vcpkg")]
            if state.video_stream.is_some() {
                return Err(invalid(
                    "decode.submit",
                    "one-shot decode is invalid while a video stream is active",
                ));
            }
            decode(state.kind, request)
        }
        astra_platform::DecodeStreamAction::Start => {
            if state.kind != DecodeKind::Video || request.bytes.is_empty() {
                return Err(invalid(
                    "decode.video.stream.start",
                    "video stream start requires a video session and encoded bytes",
                ));
            }
            #[cfg(feature = "ffmpeg-vcpkg")]
            {
                if state.video_stream.is_some() {
                    return Err(invalid(
                        "decode.video.stream.start",
                        "video stream is already active",
                    ));
                }
                let stream = open_headless_video_stream(
                    &request.codec,
                    &request.bytes,
                    video_binding,
                    max_video_frames,
                    max_decode_output_bytes,
                )?;
                let output = DecodeOutput::VideoStreamStart {
                    duration_us: Some(stream.duration_us),
                    frame_count: None,
                    decoded_byte_count: None,
                };
                state.video_stream = Some(stream);
                Ok(output)
            }
            #[cfg(not(feature = "ffmpeg-vcpkg"))]
            {
                let _ = (video_binding, max_video_frames, max_decode_output_bytes);
                Err(PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "decode.video.stream.start",
                    "video streaming requires an explicitly compiled ffmpeg-vcpkg provider",
                ))
            }
        }
        astra_platform::DecodeStreamAction::Next => {
            if state.kind != DecodeKind::Video || !request.bytes.is_empty() {
                return Err(invalid(
                    "decode.video.stream.next",
                    "video stream next requires a video session and empty bytes",
                ));
            }
            #[cfg(feature = "ffmpeg-vcpkg")]
            {
                next_headless_video_output(state)
            }
            #[cfg(not(feature = "ffmpeg-vcpkg"))]
            {
                let _ = max_decode_output_bytes;
                Err(PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "decode.video.stream.next",
                    "video streaming requires an explicitly compiled ffmpeg-vcpkg provider",
                ))
            }
        }
    }?;
    state.last_sequence = sequence;
    Ok(output)
}

fn decode(
    kind: DecodeKind,
    request: astra_platform::PlatformDecodeRequest,
) -> Result<DecodeOutput, PlatformError> {
    if kind != request.kind {
        return Err(invalid(
            "decode.submit",
            "decode request kind does not match session",
        ));
    }
    let media_kind = match kind {
        DecodeKind::Image => MediaDecodeKind::Image,
        DecodeKind::Audio => MediaDecodeKind::Audio,
        DecodeKind::Video => MediaDecodeKind::Video,
    };
    let request = DecodeRequest {
        kind: media_kind,
        codec: request.codec,
        bytes: request.bytes,
        profile: "headless".into(),
    };
    if kind == DecodeKind::Video {
        return Err(invalid(
            "decode.video",
            "video decode requires the typed incremental stream path",
        ));
    }
    let result = match kind {
        DecodeKind::Image => ImageDecodeProvider.decode(&request),
        DecodeKind::Audio => SymphoniaAudioDecodeProvider.decode(&request),
        DecodeKind::Video => unreachable!(),
    }
    .map_err(media_error)?;
    match result.output {
        MediaDecodeOutput::CpuBuffer { bytes, format } => {
            Ok(DecodeOutput::CpuBuffer { format, bytes })
        }
        MediaDecodeOutput::AudioPcmI16 {
            sample_rate,
            channels,
            samples,
        } => Ok(DecodeOutput::AudioPcmI16 {
            sample_rate,
            channels,
            samples,
        }),
        MediaDecodeOutput::AudioPcmF32 {
            sample_rate,
            channels,
            samples,
        } => Ok(DecodeOutput::AudioPcmF32 {
            sample_rate,
            channels,
            samples,
        }),
        MediaDecodeOutput::MediaSurfaceToken(_) => Err(invalid(
            "decode.submit",
            "headless decode cannot return a native media token",
        )),
    }
}

#[cfg(feature = "ffmpeg-vcpkg")]
fn open_headless_video_stream(
    codec: &str,
    encoded: &[u8],
    video_binding: &str,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
) -> Result<HeadlessVideoStream, PlatformError> {
    if video_binding != "ffmpeg-vcpkg" {
        return Err(PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "decode.video.stream.start",
            "video decode requires the explicit ffmpeg-vcpkg profile binding",
        ));
    }
    astra_media::probe_ffmpeg_provider().map_err(media_error)?;
    let max_video_frames_usize = usize::try_from(max_video_frames).map_err(|_| {
        invalid(
            "decode.video.stream.start",
            "video frame limit exceeds the current host address space",
        )
    })?;
    let max_decode_output_bytes_usize = usize::try_from(max_decode_output_bytes).map_err(|_| {
        invalid(
            "decode.video.stream.start",
            "video byte limit exceeds the current host address space",
        )
    })?;
    let limits = astra_media::FfmpegStreamLimits {
        max_encoded_bytes: encoded.len(),
        max_video_frames: max_video_frames_usize,
        max_video_frame_bytes: max_decode_output_bytes_usize,
        ..astra_media::FfmpegStreamLimits::default()
    };
    let decoder =
        astra_media::FfmpegPlaybackDecoder::open(codec, encoded, limits).map_err(media_error)?;
    let duration_us = decoder.playback_config().duration_us;
    if duration_us == 0 {
        return Err(invalid(
            "decode.video.stream.start",
            "video decoder reported an empty duration",
        ));
    }
    Ok(HeadlessVideoStream {
        decoder,
        duration_us,
        max_frames: max_video_frames,
        max_decoded_byte_count: max_decode_output_bytes,
        frame_count: 0,
        decoded_byte_count: 0,
        end_emitted: false,
    })
}

#[cfg(feature = "ffmpeg-vcpkg")]
fn next_headless_video_output(state: &mut DecodeState) -> Result<DecodeOutput, PlatformError> {
    let stream = state.video_stream.as_mut().ok_or_else(|| {
        invalid(
            "decode.video.stream.next",
            "video stream has not been started",
        )
    })?;
    while let Some(packet) = stream.decoder.read_next().map_err(media_error)? {
        let FfmpegDecodedPacket::Video { packet, bgra8 } = packet else {
            continue;
        };
        let expected = u64::from(packet.width)
            .checked_mul(u64::from(packet.height))
            .and_then(|pixels| pixels.checked_mul(4));
        if packet.sequence != stream.frame_count.saturating_add(1)
            || packet.duration_us == 0
            || expected != Some(bgra8.len() as u64)
        {
            return Err(invalid(
                "decode.video.stream.next",
                "decoded video frame metadata is invalid",
            ));
        }
        stream.frame_count = packet.sequence;
        stream.decoded_byte_count = stream
            .decoded_byte_count
            .checked_add(bgra8.len() as u64)
            .ok_or_else(|| {
                invalid(
                    "decode.video.stream.next",
                    "decoded video byte accounting overflowed",
                )
            })?;
        if stream.frame_count > stream.max_frames
            || stream.decoded_byte_count > stream.max_decoded_byte_count
        {
            return Err(PlatformError::new(
                PlatformErrorCode::QueueOverflow,
                "decode.video.stream.next",
                "decoded video stream exceeds its profile-bound budget",
            ));
        }
        return Ok(DecodeOutput::VideoFrame {
            sequence: packet.sequence,
            pts_us: packet.pts_us,
            duration_us: packet.duration_us,
            width: packet.width,
            height: packet.height,
            bgra8: bgra8.into(),
        });
    }
    if stream.end_emitted {
        return Err(invalid(
            "decode.video.stream.next",
            "video stream end was already emitted",
        ));
    }
    stream.end_emitted = true;
    Ok(DecodeOutput::VideoStreamEnd {
        frame_count: stream.frame_count,
        decoded_byte_count: stream.decoded_byte_count,
    })
}

async fn package_range(
    source: &mut PackageState,
    offset: u64,
    length: usize,
) -> Result<Vec<u8>, PlatformError> {
    match source {
        PackageState::File(file) => file.read_range(offset, length),
        PackageState::HttpsRange {
            client,
            url,
            length: package_length,
            block_size,
            block_hashes,
        } => {
            if length == 0 {
                return Err(invalid(
                    "package.read_range",
                    "range length must be non-zero",
                ));
            }
            if offset >= *package_length {
                return Err(invalid(
                    "package.read_range",
                    "range starts outside package",
                ));
            }
            let requested_end = offset
                .checked_add(length as u64)
                .ok_or_else(|| invalid("package.read_range", "range overflows"))?
                .min(*package_length);
            let first_block = offset / *block_size as u64;
            let last_block = (requested_end - 1) / *block_size as u64;
            let mut verified = Vec::new();
            for block_index in first_block..=last_block {
                let block_start = block_index * *block_size as u64;
                let block_end = (block_start + *block_size as u64).min(*package_length) - 1;
                let (bytes, _) =
                    fetch_https_range(client, url, block_start, block_end, *package_length).await?;
                let actual: [u8; 32] = Sha256::digest(&bytes).into();
                let expected = block_hashes.get(block_index as usize).ok_or_else(|| {
                    invalid("package.read_range", "HTTPS block identity is missing")
                })?;
                if &actual != expected {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "package.read_range",
                        "HTTPS package block hash mismatch",
                    ));
                }
                verified.extend_from_slice(&bytes);
            }
            let relative_start = usize::try_from(offset - first_block * *block_size as u64)
                .map_err(|_| invalid("package.read_range", "range offset overflows"))?;
            let requested_length = usize::try_from(requested_end - offset)
                .map_err(|_| invalid("package.read_range", "range length overflows"))?;
            Ok(verified[relative_start..relative_start + requested_length].to_vec())
        }
    }
}

async fn open_https_range(
    url: url::Url,
    expected_hash: &str,
    max_package_bytes: u64,
    max_read_bytes: usize,
    root_certificates: &[Vec<u8>],
) -> Result<PackageState, PlatformError> {
    if max_read_bytes == 0 {
        return Err(invalid(
            "package.open_https",
            "HTTPS range block size must be non-zero",
        ));
    }
    let mut client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    for certificate in root_certificates {
        let certificate = reqwest::Certificate::from_pem(certificate)
            .map_err(|_| invalid("package.open_https", "HTTPS root certificate is invalid"))?;
        client = client.add_root_certificate(certificate);
    }
    let client = client.build().map_err(|_| io_error("package.open_https"))?;
    let (_, length) = fetch_https_range(&client, &url, 0, 0, 0).await?;
    if length == 0 || length > max_package_bytes {
        return Err(invalid(
            "package.open_https",
            "HTTPS package length is empty or exceeds the declared package byte limit",
        ));
    }

    let block_size = max_read_bytes.min(1024 * 1024);
    let mut package_hasher = Sha256::new();
    let mut block_hashes = Vec::new();
    let mut start = 0_u64;
    while start < length {
        let end = (start + block_size as u64).min(length) - 1;
        let (bytes, _) = fetch_https_range(&client, &url, start, end, length).await?;
        package_hasher.update(&bytes);
        block_hashes.push(Sha256::digest(&bytes).into());
        start = end + 1;
    }
    let actual = format!("sha256:{:x}", package_hasher.finalize());
    if actual != expected_hash {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.open_https",
            "HTTPS package hash mismatch",
        ));
    }
    Ok(PackageState::HttpsRange {
        client,
        url,
        length,
        block_size,
        block_hashes,
    })
}

async fn fetch_https_range(
    client: &reqwest::Client,
    url: &url::Url,
    start: u64,
    end: u64,
    expected_length: u64,
) -> Result<(Vec<u8>, u64), PlatformError> {
    let response = client
        .get(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={start}-{end}"))
        .send()
        .await
        .map_err(|_| io_error("package.read_https_range"))?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(invalid(
            "package.read_https_range",
            "HTTPS package server must honor byte range requests without redirects",
        ));
    }
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| invalid("package.read_https_range", "Content-Range is missing"))?;
    let (actual_start, actual_end, total) = parse_content_range(content_range)?;
    if actual_start != start
        || actual_end != end
        || (expected_length != 0 && total != expected_length)
    {
        return Err(invalid(
            "package.read_https_range",
            "HTTPS Content-Range does not match the requested package range",
        ));
    }
    let expected_bytes = usize::try_from(end - start + 1)
        .map_err(|_| invalid("package.read_https_range", "range length overflows"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|_| io_error("package.read_https_range"))?;
    if bytes.len() != expected_bytes {
        return Err(invalid(
            "package.read_https_range",
            "HTTPS range response byte length is invalid",
        ));
    }
    Ok((bytes.to_vec(), total))
}

fn parse_content_range(value: &str) -> Result<(u64, u64, u64), PlatformError> {
    let value = value.strip_prefix("bytes ").ok_or_else(|| {
        invalid(
            "package.read_https_range",
            "Content-Range unit must be bytes",
        )
    })?;
    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| invalid("package.read_https_range", "Content-Range shape is invalid"))?;
    let (start, end) = range.split_once('-').ok_or_else(|| {
        invalid(
            "package.read_https_range",
            "Content-Range bounds are invalid",
        )
    })?;
    let start = start
        .parse::<u64>()
        .map_err(|_| invalid("package.read_https_range", "Content-Range start is invalid"))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| invalid("package.read_https_range", "Content-Range end is invalid"))?;
    let total = total
        .parse::<u64>()
        .map_err(|_| invalid("package.read_https_range", "Content-Range total is invalid"))?;
    if start > end || end >= total {
        return Err(invalid(
            "package.read_https_range",
            "Content-Range bounds exceed the package length",
        ));
    }
    Ok((start, end, total))
}
fn complete_gpu_sample(
    mut sample: HeadlessGpuFrameSample,
    submission: WgpuProfiledSubmission,
) -> HeadlessGpuFrameSample {
    sample.cpu_submit_ns = submission.cpu_submit_ns;
    sample.gpu_duration_ns = submission.gpu_duration_ns;
    sample.scene_cpu_ns = submission.scene_cpu_ns;
    sample.filter_cpu_ns = submission.filter_cpu_ns;
    sample.scene_command_cpu_ns = submission.scene_command_cpu_ns;
    sample.scene_atlas_cpu_ns = submission.scene_atlas_cpu_ns;
    sample.scene_geometry_cpu_ns = submission.scene_geometry_cpu_ns;
    sample.scene_vertex_upload_cpu_ns = submission.scene_vertex_upload_cpu_ns;
    sample.scene_render_encode_cpu_ns = submission.scene_render_encode_cpu_ns;
    sample.scene_queue_submit_cpu_ns = submission.scene_queue_submit_cpu_ns;
    sample.scene_render_submit_cpu_ns = submission.scene_render_submit_cpu_ns;
    sample.atlas_upload_gpu_ns = submission.atlas_upload_gpu_ns;
    sample.scene_gpu_ns = submission.scene_gpu_ns;
    sample.filter_gpu_ns = submission.filter_gpu_ns;
    sample
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, PlatformError> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(PlatformError::new(
            PlatformErrorCode::PermissionDenied,
            "package.open",
            "bundled package path is not a safe relative path",
        ));
    }
    Ok(root.join(path))
}

struct Sha256Writer<'a>(&'a mut Sha256);

impl Write for Sha256Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn canonical_json_digest(value: &impl serde::Serialize) -> Result<[u8; 32], serde_json::Error> {
    let mut digest = Sha256::new();
    serde_json::to_writer(Sha256Writer(&mut digest), value)?;
    Ok(digest.finalize().into())
}

fn scene_submission_identity(
    sequence: u64,
    width: u32,
    height: u32,
    clear_rgba: [u8; 4],
    command_count: usize,
    semantics: &Option<astra_ui_core::UiSemanticSnapshot>,
) -> Result<[u8; 32], PlatformError> {
    let command_count = u32::try_from(command_count).map_err(|_| {
        invalid(
            "surface.present_scene",
            "scene command count exceeds the submission identity format",
        )
    })?;
    let mut identity = [0_u8; 32];
    identity[..8].copy_from_slice(&sequence.to_le_bytes());
    identity[8..12].copy_from_slice(&width.to_le_bytes());
    identity[12..16].copy_from_slice(&height.to_le_bytes());
    identity[16..20].copy_from_slice(&command_count.to_le_bytes());
    identity[20..24].copy_from_slice(&clear_rgba);
    identity[24..32].copy_from_slice(
        &semantics
            .as_ref()
            .map_or(0, |snapshot| snapshot.generation)
            .to_le_bytes(),
    );
    Ok(identity)
}

fn elapsed_ns(
    started: Instant,
    operation: &'static str,
    overflow_message: &'static str,
) -> Result<u64, PlatformError> {
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| invalid(operation, overflow_message))
}

fn invalid(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}
fn io_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Io,
        operation,
        "headless I/O operation failed",
    )
}
fn media_error(error: astra_media::MediaError) -> PlatformError {
    let diagnostic = error.to_string();
    let diagnostic_codes = match &error {
        MediaError::Diagnostics(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(","),
        MediaError::Message(_) => "ASTRA_MEDIA_PROVIDER_MESSAGE".to_owned(),
    };
    PlatformError::new(
        PlatformErrorCode::IntegrityMismatch,
        "headless.media",
        format!("headless media provider rejected input: {diagnostic}"),
    )
    .with_field("media_error", diagnostic)
    .with_field("diagnostic_codes", diagnostic_codes)
}

#[cfg(test)]
mod tests {
    use super::parse_content_range;

    #[test]
    fn content_range_parser_rejects_ambiguous_or_out_of_bounds_identity() {
        assert_eq!(parse_content_range("bytes 0-0/42").unwrap(), (0, 0, 42));
        for invalid in [
            "items 0-0/42",
            "bytes */42",
            "bytes 1-0/42",
            "bytes 0-42/42",
            "bytes 0-0/*",
            "bytes 0-0/0",
        ] {
            assert!(parse_content_range(invalid).is_err(), "{invalid}");
        }
    }
}
