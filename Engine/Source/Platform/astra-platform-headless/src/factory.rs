use std::{
    collections::VecDeque,
    fmt::Debug,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use crate::artifact::ArtifactRecorder;
use astra_media::{
    DecodeKind as MediaDecodeKind, DecodeOutput as MediaDecodeOutput, DecodeProvider,
    DecodeRequest, ImageDecodeProvider, SymphoniaAudioDecodeProvider,
};
#[cfg(feature = "ffmpeg-vcpkg")]
use astra_media::{
    DecodedVideoFrame, DecodedVideoStream, FfmpegDecodedPacket, DECODED_VIDEO_STREAM_SCHEMA,
};
use astra_media_core::{
    CpuRendererProvider, HeadlessRenderer, MediaError, RenderTargetFormat, Renderer2DProvider,
    RendererCreateRequest, SceneCommand,
};
use astra_platform::{
    host_channel, AudioDeviceFormat, AudioMeter, AudioOutputHandle, AudioOutputState,
    AudioOutputStatus, CapturedFrame, DecodeKind, DecodeOutput, DecodeSessionHandle,
    HeadlessHostProfile, HeadlessReadbackPolicy, HeadlessRenderPolicy, HostCommand,
    HostLaunchProfile, PackageSourceHandle, PackageSourceRequest, PlatformError, PlatformErrorCode,
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
    pub cpu_submit_ns: u64,
    pub gpu_duration_ns: u64,
    pub scene_cpu_ns: u64,
    pub filter_cpu_ns: u64,
    pub scene_command_cpu_ns: u64,
    pub scene_atlas_cpu_ns: u64,
    pub scene_geometry_cpu_ns: u64,
    pub scene_vertex_upload_cpu_ns: u64,
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
}

pub trait HeadlessPerformanceObserver: Debug + Send + Sync {
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
            let state = HostState::new(factory, profile, backend)?;
            tracing::info!(
                event = "platform.headless.session.start",
                "started isolated Headless platform session"
            );
            tokio::spawn(async move {
                state.run().await;
            });
            Ok(PlatformHostSession {
                client,
                events,
                profile: launch,
            })
        })
    }
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
            "audio_graph_cpu",
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
}
struct AudioState {
    channels: u16,
    max_frames: usize,
    last_sequence: u64,
    timeline: Vec<f32>,
    retain_timeline: bool,
    submitted_samples: u64,
    square_sum: f64,
    peak: f32,
    queued: Vec<f32>,
    paused: bool,
    consumed: u64,
    callback_count: u64,
    underflow_count: u64,
}
struct DecodeState {
    kind: DecodeKind,
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
    artifacts: ArtifactRecorder,
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
        let artifacts = ArtifactRecorder::new(
            factory.run_root.clone(),
            &profile,
            factory.input_sequence_hash,
        )?;
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
            self.artifacts.record_rasterized_frame(
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
                            cpu_submit_ns: 0,
                            gpu_duration_ns: 0,
                            scene_cpu_ns: 0,
                            filter_cpu_ns: 0,
                            scene_command_cpu_ns: 0,
                            scene_atlas_cpu_ns: 0,
                            scene_geometry_cpu_ns: 0,
                            scene_vertex_upload_cpu_ns: 0,
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
            self.artifacts.record_rasterized_frame(
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
                    let renderer = if let Some(policy) = &self.profile.gpu_adapter {
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
                        self.artifacts
                            .set_renderer_identity(renderer.identity().clone())?;
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
                    self.artifacts
                        .record_submission(frame.sequence, &canonical)?;
                    self.artifacts.record_rasterized_frame(
                        frame.sequence,
                        frame.width,
                        frame.height,
                        &frame.rgba8,
                    )?;
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
                    let canonical = canonical_json_digest(&(
                        frame.sequence,
                        frame.width,
                        frame.height,
                        frame.clear_rgba,
                        &frame.commands,
                        &frame.semantics,
                    ))
                    .map_err(|_| invalid("surface.present_scene", "scene serialization failed"))?;
                    let (journal, pending, deferred_resources, materialize) = {
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
                        let journal = s.renderer.validate_frame(&commands).map_err(media_error)?;
                        let deferred_resources: Vec<_> = commands
                            .iter()
                            .filter(|command| {
                                matches!(
                                    command,
                                    SceneCommand::UploadTexture { .. }
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
                        };
                        (
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
                    self.artifacts.record_submission(sequence, &canonical)?;
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
            HostCommand::DestroySurface { surface, reply } => {
                let capture =
                    self.profile.readback_policy == HeadlessReadbackPolicy::RasterizedFrames;
                let result = self
                    .materialize_surface(surface, capture)
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
                let result = if request.sample_rate != 48_000 || request.channels != 2 {
                    Err(invalid(
                        "audio.open",
                        "headless audio requires 48kHz stereo",
                    ))
                } else {
                    self.audio.insert(AudioState {
                        channels: request.channels,
                        max_frames: request.max_buffered_frames,
                        last_sequence: 0,
                        timeline: Vec::new(),
                        retain_timeline: !matches!(
                            self.profile.artifacts.retention,
                            astra_platform::HeadlessArtifactRetention::ManifestOnly
                        ),
                        submitted_samples: 0,
                        square_sum: 0.0,
                        peak: 0.0,
                        queued: Vec::new(),
                        paused: false,
                        consumed: 0,
                        callback_count: 0,
                        underflow_count: 0,
                    })
                };
                let _ = reply.send(result);
            }
            HostCommand::QueryAudioOutputFormat { reply }
            | HostCommand::QueryAudioDeviceFormat { reply } => {
                let _ = reply.send(Ok(AudioDeviceFormat {
                    sample_rate: 48_000,
                    channels: 2,
                }));
            }
            HostCommand::SubmitAudio {
                output,
                packet,
                reply,
            } => {
                let result = (|| {
                    let audio = self.audio.get(output)?;
                    let submitted_samples = audio
                        .submitted_samples
                        .checked_add(packet.samples.len() as u64)
                        .ok_or_else(|| invalid("audio.submit", "sample count overflowed"))?;
                    let retained_samples = if audio.retain_timeline {
                        usize::try_from(submitted_samples)
                            .map_err(|_| invalid("audio.submit", "sample count overflowed"))?
                    } else {
                        packet.samples.len()
                    };
                    self.artifacts.validate_audio_timeline(retained_samples)?;
                    if !audio.retain_timeline {
                        self.artifacts
                            .record_audio(packet.sequence, &packet.samples)?;
                    }
                    let a = self.audio.get_mut(output)?;
                    ensure_sequence(a.last_sequence, packet.sequence, "audio.submit")?;
                    if packet.channels != a.channels
                        || packet.samples.iter().any(|s| !s.is_finite())
                    {
                        return Err(invalid(
                            "audio.submit",
                            "audio packet format or sample is invalid",
                        ));
                    }
                    if packet.frame_count() > a.max_frames
                        || a.queued.len().saturating_add(packet.samples.len())
                            > a.max_frames.saturating_mul(usize::from(a.channels))
                    {
                        return Err(PlatformError::new(
                            PlatformErrorCode::QueueOverflow,
                            "audio.submit",
                            format!(
                                "audio buffer limit exceeded: queued_samples={}, packet_samples={}, max_samples={}",
                                a.queued.len(),
                                packet.samples.len(),
                                a.max_frames.saturating_mul(usize::from(a.channels))
                            ),
                        ));
                    }
                    for sample in &packet.samples {
                        let value = f64::from(*sample);
                        a.square_sum += value * value;
                        a.peak = a.peak.max(sample.abs());
                    }
                    a.submitted_samples = submitted_samples;
                    if a.retain_timeline {
                        a.timeline.extend_from_slice(&packet.samples);
                    }
                    a.queued.extend(packet.samples);
                    a.last_sequence = packet.sequence;
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            HostCommand::QueryAudio { output, reply } => {
                let result = self.audio.get_mut(output).map(|audio| {
                    consume_audio_callback(audio);
                    audio_state(audio)
                });
                let _ = reply.send(result);
            }
            HostCommand::DrainAudio { output, reply } => {
                let result = self.audio.get_mut(output).map(|a| {
                    a.consumed = a.submitted_samples;
                    a.queued.clear();
                    aggregate_audio_meter(a)
                });
                let _ = reply.send(result);
            }
            HostCommand::QueryAudioOutput { output, reply } => {
                let result = self.audio.get(output).map(audio_status);
                let _ = reply.send(result);
            }
            HostCommand::PauseAudio { output, reply } => {
                let result = self.audio.get_mut(output).and_then(|a| {
                    if a.paused {
                        Err(invalid("audio.pause", "audio output is already paused"))
                    } else {
                        a.paused = true;
                        Ok(())
                    }
                });
                let _ = reply.send(result);
            }
            HostCommand::ResumeAudio { output, reply } => {
                let result = self.audio.get_mut(output).and_then(|a| {
                    if !a.paused {
                        Err(invalid("audio.resume", "audio output is not paused"))
                    } else {
                        a.paused = false;
                        Ok(())
                    }
                });
                let _ = reply.send(result);
            }
            HostCommand::AbortAudio { output, reply } => {
                let result = self.audio.remove(output).map(|_| ());
                let _ = reply.send(result);
            }
            HostCommand::CloseAudio { output, reply } => {
                let result = (|| {
                    // Handle lifetime is independent from artifact persistence. Once close
                    // begins, remove the platform resource even if bounded artifact commit
                    // fails, so cleanup reports the owning error instead of a secondary leak.
                    let state = self.audio.remove(output)?;
                    if state.retain_timeline {
                        self.artifacts
                            .record_audio(state.last_sequence.max(1), &state.timeline)?;
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            HostCommand::OpenDecode { kind, reply } => {
                let result = self.decoders.insert(DecodeState { kind });
                let _ = reply.send(result);
            }
            HostCommand::Decode {
                session,
                request,
                reply,
            } => {
                let result = self.decoders.get(session).and_then(|state| {
                    decode(
                        state.kind,
                        request,
                        &self.profile.providers.video_decode,
                        self.profile.max_video_frames,
                        self.profile.max_decode_output_bytes,
                    )
                });
                let _ = reply.send(result);
            }
            HostCommand::CloseDecode { session, reply } => {
                let result = self.decoders.remove(session).map(|_| ());
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
                let result = self
                    .ensure_empty()
                    .and_then(|_| self.artifacts.finish().map(|_| ()));
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
fn ensure_sequence(last: u64, next: u64, operation: &'static str) -> Result<(), PlatformError> {
    if next
        != last
            .checked_add(1)
            .ok_or_else(|| invalid(operation, "sequence overflow"))?
    {
        return Err(invalid(
            operation,
            "sequence is duplicated, skipped, or reversed",
        ));
    }
    Ok(())
}
fn amplitude_db(value: f32) -> f32 {
    if value <= 0.0 {
        -120.0
    } else {
        (20.0 * value.log10()).max(-120.0)
    }
}
fn audio_state(a: &AudioState) -> AudioOutputState {
    let frames = a.queued.len() / usize::from(a.channels);
    AudioOutputState {
        queued_frames: frames,
        callback_count: a.callback_count,
        submitted_samples: a.submitted_samples,
        consumed_samples: a.consumed,
        underflow_count: a.underflow_count,
        meter: aggregate_audio_meter(a),
    }
}
fn audio_status(a: &AudioState) -> AudioOutputStatus {
    let frames = a.submitted_samples / u64::from(a.channels);
    let played = a.consumed / u64::from(a.channels);
    AudioOutputStatus {
        submitted_frames: frames,
        played_frames: played,
        buffered_frames: (a.queued.len() / usize::from(a.channels)) as u64,
        underflow_count: a.underflow_count,
        meter: aggregate_audio_meter(a),
    }
}

fn aggregate_audio_meter(audio: &AudioState) -> AudioMeter {
    if audio.submitted_samples == 0 {
        return AudioMeter {
            sample_count: 0,
            peak_dbfs: -120.0,
            rms_dbfs: -120.0,
        };
    }
    let rms = (audio.square_sum / audio.submitted_samples as f64).sqrt() as f32;
    AudioMeter {
        sample_count: audio.submitted_samples,
        peak_dbfs: amplitude_db(audio.peak),
        rms_dbfs: amplitude_db(rms),
    }
}

fn consume_audio_callback(audio: &mut AudioState) {
    if audio.paused {
        return;
    }
    audio.callback_count = audio.callback_count.saturating_add(1);
    if audio.queued.is_empty() {
        if audio.submitted_samples > 0 {
            audio.underflow_count = audio.underflow_count.saturating_add(1);
        }
        return;
    }
    let samples = (800_usize * usize::from(audio.channels)).min(audio.queued.len());
    audio.queued.drain(..samples);
    audio.consumed = audio.consumed.saturating_add(samples as u64);
}

fn decode(
    kind: DecodeKind,
    request: astra_platform::PlatformDecodeRequest,
    video_binding: &str,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
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
    let result = match kind {
        DecodeKind::Image => ImageDecodeProvider.decode(&request),
        DecodeKind::Audio => SymphoniaAudioDecodeProvider.decode(&request),
        DecodeKind::Video => Ok(decode_video(
            &request,
            video_binding,
            max_video_frames,
            max_decode_output_bytes,
        )?),
    }
    .map_err(media_error)?;
    match result.output {
        MediaDecodeOutput::CpuBuffer {
            bytes,
            format,
            hash,
        } => Ok(DecodeOutput::CpuBuffer {
            format,
            bytes,
            hash: hash.to_string(),
        }),
        MediaDecodeOutput::MediaSurfaceToken(_) => Err(invalid(
            "decode.submit",
            "headless decode cannot return a native media token",
        )),
    }
}

#[cfg(feature = "ffmpeg-vcpkg")]
fn decode_video(
    request: &DecodeRequest,
    video_binding: &str,
    max_video_frames: u64,
    max_decode_output_bytes: u64,
) -> Result<astra_media::DecodeResult, PlatformError> {
    if video_binding != "ffmpeg-vcpkg" {
        return Err(PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "decode.video",
            "video decode requires the explicit ffmpeg-vcpkg profile binding",
        ));
    }
    astra_media::probe_ffmpeg_provider().map_err(media_error)?;
    let max_video_frames = usize::try_from(max_video_frames).map_err(|_| {
        invalid(
            "decode.video",
            "video frame limit exceeds the current host address space",
        )
    })?;
    let max_decode_output_bytes = usize::try_from(max_decode_output_bytes).map_err(|_| {
        invalid(
            "decode.video",
            "video byte limit exceeds the current host address space",
        )
    })?;
    let limits = astra_media::FfmpegStreamLimits {
        max_encoded_bytes: request.bytes.len(),
        max_video_frames,
        max_video_frame_bytes: max_decode_output_bytes,
        ..astra_media::FfmpegStreamLimits::default()
    };
    let mut decoder =
        astra_media::FfmpegPlaybackDecoder::open(&request.codec, &request.bytes, limits)
            .map_err(media_error)?;
    let duration_us = decoder.playback_config().duration_us;
    let mut frames = Vec::new();
    let mut decoded_bytes = 0_usize;
    while let Some(packet) = decoder.read_next().map_err(media_error)? {
        if let FfmpegDecodedPacket::Video { packet, bgra8 } = packet {
            decoded_bytes = decoded_bytes.checked_add(bgra8.len()).ok_or_else(|| {
                invalid("decode.video", "decoded video byte accounting overflowed")
            })?;
            if frames.len() >= max_video_frames || decoded_bytes > max_decode_output_bytes {
                return Err(PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    "decode.video",
                    "decoded video exceeds its profile-bound frame or byte limit",
                ));
            }
            frames.push(DecodedVideoFrame {
                sequence: packet.sequence,
                pts_us: packet.pts_us,
                duration_us: packet.duration_us,
                width: packet.width,
                height: packet.height,
                content_hash: packet.content_hash,
                bgra8,
            });
        }
    }
    let stream = DecodedVideoStream {
        schema: DECODED_VIDEO_STREAM_SCHEMA.to_string(),
        duration_us,
        frames,
    };
    let bytes = stream
        .encode(max_video_frames as u64, max_decode_output_bytes as u64)
        .map_err(media_error)?;
    let hash = astra_core::Hash256::from_sha256(&bytes);
    Ok(astra_media::DecodeResult {
        provider_id: "astra.decode.ffmpeg".to_string(),
        kind: MediaDecodeKind::Video,
        codec: request.codec.clone(),
        output: MediaDecodeOutput::CpuBuffer {
            bytes,
            format: format!("postcard:{DECODED_VIDEO_STREAM_SCHEMA}"),
            hash,
        },
        diagnostics: Vec::new(),
    })
}

#[cfg(not(feature = "ffmpeg-vcpkg"))]
fn decode_video(
    _request: &DecodeRequest,
    _video_binding: &str,
    _max_video_frames: u64,
    _max_decode_output_bytes: u64,
) -> Result<astra_media::DecodeResult, PlatformError> {
    Err(PlatformError::new(
        PlatformErrorCode::ProviderUnavailable,
        "decode.video",
        "video requires an explicitly compiled and bound ffmpeg-vcpkg provider",
    ))
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
