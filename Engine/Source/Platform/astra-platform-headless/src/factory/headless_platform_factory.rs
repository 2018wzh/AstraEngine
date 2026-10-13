use super::*;

#[derive(Debug, Clone)]
pub struct HeadlessPlatformFactory {
    pub(super) run_root: PathBuf,
    pub(super) package_root: PathBuf,
    pub(super) user_authorized_package: Option<PathBuf>,
    pub(super) input_sequence_hash: String,
    pub(super) https_root_certificates: Vec<Vec<u8>>,
    pub(super) gpu_enabled: bool,
    pub(super) performance_observer: Option<Arc<dyn HeadlessPerformanceObserver>>,
    pub(super) thread_registry: Option<Arc<thread_owner::ThreadRegistry>>,
    pub(super) gpu_instance: Arc<Mutex<Option<wgpu::Instance>>>,
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
            thread_registry: None,
            gpu_instance: Arc::new(Mutex::new(None)),
        }
    }
    pub fn with_gpu(mut self, enabled: bool) -> Self {
        self.gpu_enabled = enabled;
        self
    }
    pub fn with_thread_owner(mut self, owner: &HeadlessThreadOwner) -> Self {
        self.thread_registry = Some(owner.registry.clone());
        self.gpu_instance = owner.gpu_instance.clone();
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
            spawn_host(factory, profile, backend).await?;
            tracing::info!(
                event = "platform.headless.session.start",
                "started isolated Headless platform session"
            );
            Ok(PlatformHostSession {
                client,
                events,
                profile: launch,
            })
        })
    }
}

pub(super) fn validate_provider_bindings(
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
pub(super) fn validate_ffmpeg_binding() -> Result<(), PlatformError> {
    astra_media::probe_ffmpeg_provider()
        .map(|_| ())
        .map_err(media_error)
}

#[cfg(not(feature = "ffmpeg-vcpkg"))]
pub(super) fn validate_ffmpeg_binding() -> Result<(), PlatformError> {
    Err(PlatformError::new(
        PlatformErrorCode::ProviderUnavailable,
        "headless.provider.bind",
        "ffmpeg-vcpkg is bound but the Headless backend was built without that feature",
    ))
}

pub(super) struct WindowState {
    pub(super) surface_count: usize,
}

pub(super) struct SurfaceState {
    pub(super) window: WindowHandle,
    pub(super) renderer: HeadlessRenderer,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) last_sequence: u64,
    pub(super) frame: Option<Arc<[u8]>>,
    pub(super) pending: Option<PendingScene>,
    pub(super) materialized_sequence: Option<u64>,
    pub(super) gpu_renderer: Option<WgpuOffscreenRenderer>,
    pub(super) pending_gpu_profiles: VecDeque<PendingGpuProfile>,
    pub(super) deferred_gpu_resource_commands: Vec<SceneCommand>,
}

pub(super) struct PendingGpuProfile {
    pub(super) pending: WgpuPendingProfile,
    pub(super) sample: HeadlessGpuFrameSample,
}

pub(super) struct PendingScene {
    pub(super) sequence: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) renderer: Option<HeadlessRenderer>,
    pub(super) commands: Vec<SceneCommand>,
    pub(super) gpu_commands: Vec<SceneCommand>,
    pub(super) clear_rgba: [u8; 4],
    pub(super) semantics: Option<astra_ui_core::UiSemanticSnapshot>,
    pub(super) scene_build_ns: u64,
    pub(super) scene_digest_ns: u64,
    pub(super) scene_validation_ns: u64,
    pub(super) scene_pending_ns: u64,
}

pub(super) struct AudioState {
    pub(super) paused: Arc<AtomicBool>,
    pub(super) wake: AudioWakeRegistration,
    pub(super) artifact_stream: Arc<Mutex<Option<AudioArtifactStream>>>,
}

pub(super) struct HeadlessAudioLane {
    pub(super) chunk_samples: usize,
    pub(super) submitted_samples: u64,
    pub(super) consumed_samples: Arc<AtomicU64>,
    pub(super) paused: Arc<AtomicBool>,
    pub(super) wake: AudioWakeRegistration,
    pub(super) observed_wake: u64,
    pub(super) artifact_stream: Arc<Mutex<Option<AudioArtifactStream>>>,
    pub(super) artifacts: Arc<Mutex<ArtifactRecorder>>,
    pub(super) capture: Option<astra_platform::AudioCaptureReader>,
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

pub(super) struct DecodeState {
    pub(super) kind: DecodeKind,
    pub(super) last_sequence: u64,
    #[cfg(feature = "ffmpeg-vcpkg")]
    pub(super) video_stream: Option<HeadlessVideoStream>,
}

#[cfg(feature = "ffmpeg-vcpkg")]
pub(super) struct HeadlessVideoStream {
    pub(super) decoder: astra_media::FfmpegPlaybackDecoder,
    pub(super) duration_us: u64,
    pub(super) max_frames: u64,
    pub(super) max_decoded_byte_count: u64,
    pub(super) frame_count: u64,
    pub(super) decoded_byte_count: u64,
    pub(super) end_emitted: bool,
}

pub(super) enum PackageState {
    File(FilePackageSource),
    HttpsRange {
        client: reqwest::Client,
        url: url::Url,
        length: u64,
        block_size: usize,
        block_hashes: Vec<[u8; 32]>,
    },
}

pub(super) struct HostState {
    pub(super) gpu_instance: Arc<Mutex<Option<wgpu::Instance>>>,
    pub(super) profile: HeadlessHostProfile,
    pub(super) backend: astra_platform::PlatformBackendChannels,
    pub(super) windows: ResourceTable<WindowState, WindowHandle>,
    pub(super) surfaces: ResourceTable<SurfaceState, SurfaceHandle>,
    pub(super) audio: ResourceTable<AudioState, AudioOutputHandle>,
    pub(super) decoders: ResourceTable<DecodeState, DecodeSessionHandle>,
    pub(super) saves: ResourceTable<SaveTransaction, SaveTransactionHandle>,
    pub(super) packages: ResourceTable<PackageState, PackageSourceHandle>,
    pub(super) save_store: AtomicSaveStore,
    pub(super) package_root: PathBuf,
    pub(super) user_authorized_package: Option<PathBuf>,
    pub(super) artifacts: Arc<Mutex<ArtifactRecorder>>,
    pub(super) https_root_certificates: Vec<Vec<u8>>,
    pub(super) performance_observer: Option<Arc<dyn HeadlessPerformanceObserver>>,
}
