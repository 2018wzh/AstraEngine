use astra_platform::{HostLaunchProfile, HostStartFuture, PlatformHostFactory};

#[cfg(not(target_os = "macos"))]
use astra_platform::{PlatformError, PlatformErrorCode, PlatformId};

#[derive(Clone, Default)]
pub struct MacosPlatformFactory {
    #[cfg(target_os = "macos")]
    roots: Option<HostRoots>,
    #[cfg(target_os = "macos")]
    pending: std::rc::Rc<std::cell::RefCell<Option<macos::BackendSetup>>>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
struct HostRoots {
    save_base: std::path::PathBuf,
    bundle_root: std::path::PathBuf,
}

pub fn factory() -> MacosPlatformFactory {
    MacosPlatformFactory::default()
}

#[cfg(target_os = "macos")]
pub fn main_thread_host(
) -> Result<(MacosHostRunner, MacosPlatformFactory), astra_platform::PlatformError> {
    MacosHostRunner::new()
}

#[cfg(target_os = "macos")]
pub use macos::MacosHostRunner;

#[cfg(all(target_os = "macos", feature = "platform-test-driver"))]
pub fn factory_with_test_roots(
    save_base: impl AsRef<std::path::Path>,
    bundle_root: impl AsRef<std::path::Path>,
) -> MacosPlatformFactory {
    MacosPlatformFactory {
        roots: Some(HostRoots {
            save_base: save_base.as_ref().to_path_buf(),
            bundle_root: bundle_root.as_ref().to_path_buf(),
        }),
        pending: Default::default(),
    }
}

impl PlatformHostFactory for MacosPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_os = "macos")]
        {
            Box::pin(crate::factory::macos::start(
                profile,
                self.roots.clone(),
                self.pending.clone(),
            ))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "macOS host can only start on macOS",
                )
                .with_field("platform", PlatformId::Macos.as_str()))
            })
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        future::Future,
        rc::Rc,
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
            mpsc as std_mpsc, Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use crate::accessibility::MacosAccessibilityBridge;
    use astra_media::{DecodeOutput as MediaDecodeOutput, DecodeProvider};
    use astra_platform::{
        host_channel, AudioDeviceFormat, AudioMeter, AudioOutputHandle, AudioOutputRequest,
        AudioOutputStatus, AudioPacket, CapturedFrame, DecodeKind, DecodeOutput,
        DecodeSessionHandle, HostCommand, HostLaunchProfile, InputState, PackageSourceHandle,
        PackageSourceRequest, PlatformBackendChannels, PlatformDecodeRequest, PlatformError,
        PlatformErrorCode, PlatformEvent, PlatformEventKind, PlatformHostProfile,
        PlatformHostSession, PointerButton, SaveTransactionHandle, SurfaceHandle, TouchPhase,
        WindowHandle,
    };
    use astra_platform_common::{
        AtomicSaveStore, CachedPackageSource, FilePackageSource, ResourceTable, SaveTransaction,
        VerifiedPackageCache,
    };
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use tokio::sync::oneshot;
    use winit::{
        application::ApplicationHandler,
        event::{
            ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase as WinitTouchPhase,
            WindowEvent,
        },
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        platform::pump_events::EventLoopExtPumpEvents,
        window::{Window, WindowAttributes, WindowId},
    };

    pub(super) struct BackendSetup {
        backend: PlatformBackendChannels,
        profile: PlatformHostProfile,
        roots: Option<super::HostRoots>,
        _instance_guard: SingleInstanceGuard,
    }

    pub struct MacosHostRunner {
        event_loop: EventLoop<()>,
        app: MacosRunnerApp,
    }

    impl MacosHostRunner {
        pub(super) fn new() -> Result<(Self, super::MacosPlatformFactory), PlatformError> {
            let pending = Rc::new(RefCell::new(None));
            let event_loop = EventLoop::builder().build().map_err(|_| {
                host_error(
                    "host.runner.create",
                    "macOS event loop could not be created",
                )
            })?;
            event_loop.set_control_flow(ControlFlow::Poll);
            Ok((
                Self {
                    event_loop,
                    app: MacosRunnerApp {
                        pending: pending.clone(),
                        host: None,
                    },
                },
                super::MacosPlatformFactory {
                    roots: None,
                    pending,
                },
            ))
        }

        pub fn run<F: Future>(&mut self, future: F) -> Result<F::Output, PlatformError> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| host_error("host.runner.run", "Tokio runtime could not be created"))?;
            runtime.block_on(async {
                tokio::pin!(future);
                loop {
                    self.event_loop
                        .pump_app_events(Some(Duration::ZERO), &mut self.app);
                    tokio::select! {
                        result = &mut future => break Ok(result),
                        _ = tokio::time::sleep(Duration::from_millis(1)) => {}
                    }
                }
            })
        }
    }

    struct MacosRunnerApp {
        pending: Rc<RefCell<Option<BackendSetup>>>,
        host: Option<MacosHostApp>,
    }

    impl MacosRunnerApp {
        fn initialize(&mut self) -> Result<(), PlatformError> {
            let Some(setup) = self.pending.borrow_mut().take() else {
                return Ok(());
            };
            let roots = setup
                .roots
                .or_else(|| default_roots(&setup.profile.package_id))
                .ok_or_else(|| {
                    host_error("host.start", "macOS save or bundle root is unavailable")
                })?;
            let save_store = AtomicSaveStore::new(&roots.save_base, &setup.profile.package_id)?;
            let package_cache =
                VerifiedPackageCache::platform_cache_root(&setup.profile.package_id).and_then(
                    |root| VerifiedPackageCache::open(root, setup.profile.package_cache.clone()),
                )?;
            let (ready, _ready_rx) = std_mpsc::sync_channel(1);
            self.host = Some(MacosHostApp::new(
                setup.backend,
                ready,
                save_store,
                package_cache,
                PackageHostConfig {
                    source_policies: setup.profile.package_sources,
                    package_id: setup.profile.package_id,
                    cache_policy: setup.profile.package_cache,
                    bundle_root: roots.bundle_root,
                },
            )?);
            Ok(())
        }
    }

    impl ApplicationHandler for MacosRunnerApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.initialize().is_err() {
                event_loop.exit();
            }
            if let Some(host) = self.host.as_mut() {
                host.resumed(event_loop);
            }
        }

        fn suspended(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(host) = self.host.as_mut() {
                host.suspended(event_loop);
            }
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            if let Some(host) = self.host.as_mut() {
                host.window_event(event_loop, id, event);
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.initialize().is_err() {
                event_loop.exit();
            }
            if let Some(host) = self.host.as_mut() {
                host.about_to_wait(event_loop);
            }
        }
    }

    pub async fn start(
        launch_profile: HostLaunchProfile,
        roots: Option<super::HostRoots>,
        pending: Rc<RefCell<Option<BackendSetup>>>,
    ) -> Result<PlatformHostSession, PlatformError> {
        let profile = launch_profile.require_platform()?.clone();
        if profile.platform != astra_platform::PlatformId::Macos {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "host.start",
                "macOS factory requires a macOS profile",
            ));
        }
        let command_capacity = profile.limits.command_queue_capacity;
        let event_capacity = profile.limits.event_queue_capacity;
        let instance_guard = SingleInstanceGuard::acquire(&profile)?;
        let (client, backend, events) = host_channel(
            HostLaunchProfile::platform(profile.clone()),
            command_capacity,
            event_capacity,
        )?;
        if Rc::strong_count(&pending) < 2 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.start",
                "macOS host requires main_thread_host and MacosHostRunner",
            ));
        }
        if pending.borrow().is_some() {
            return Err(PlatformError::new(
                PlatformErrorCode::AlreadyInUse,
                "host.start",
                "macOS factory already has a pending host",
            ));
        }
        *pending.borrow_mut() = Some(BackendSetup {
            backend,
            profile,
            roots,
            _instance_guard: instance_guard,
        });
        Ok(PlatformHostSession {
            client,
            events,
            profile: launch_profile,
        })
    }

    struct SingleInstanceGuard {
        _lock: std::fs::File,
    }

    impl SingleInstanceGuard {
        fn acquire(profile: &PlatformHostProfile) -> Result<Self, PlatformError> {
            use std::{fs::OpenOptions, os::fd::AsRawFd};

            use astra_core::Hash256;

            let identity = format!("{}\n{}\n{}", profile.package_id, profile.target, profile.id);
            let hash = Hash256::from_sha256(identity.as_bytes()).to_string();
            let lock_dir =
                directories::ProjectDirs::from("com", "AstraEngine", &profile.package_id)
                    .ok_or_else(|| {
                        host_error(
                            "host.instance.acquire",
                            "Application Support is unavailable",
                        )
                    })?
                    .cache_dir()
                    .join("locks");
            std::fs::create_dir_all(&lock_dir).map_err(|_| {
                host_error(
                    "host.instance.acquire",
                    "macOS player lock directory could not be created",
                )
            })?;
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(lock_dir.join(format!(
                    "player-{}.lock",
                    hash.trim_start_matches("sha256:")
                )))
                .map_err(|_| {
                    host_error(
                        "host.instance.acquire",
                        "macOS player lock could not be opened",
                    )
                })?;
            let result = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                if matches!(error.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::AlreadyInUse,
                        "host.instance.acquire",
                        "the same game target and profile is already running",
                    ));
                }
                return Err(host_error(
                    "host.instance.acquire",
                    "macOS player lock could not be acquired",
                ));
            }
            Ok(Self { _lock: lock })
        }
    }

    struct MacosHostApp {
        backend: PlatformBackendChannels,
        ready: Option<std_mpsc::SyncSender<Result<(), PlatformError>>>,
        windows: ResourceTable<Arc<Window>, WindowHandle>,
        window_ids: BTreeMap<WindowId, WindowHandle>,
        accessibility: BTreeMap<WindowId, MacosAccessibilityBridge>,
        surfaces: ResourceTable<SurfaceResource, SurfaceHandle>,
        surface_windows: BTreeMap<SurfaceHandle, WindowId>,
        audio_outputs: ResourceTable<AudioResource, AudioOutputHandle>,
        decode_sessions: ResourceTable<DecodeResource, DecodeSessionHandle>,
        save_store: AtomicSaveStore,
        package_cache: VerifiedPackageCache,
        package_source_policies: Vec<astra_platform::PackageSourcePolicy>,
        package_id: String,
        package_cache_policy: astra_platform::PackageCachePolicy,
        package_completion_tx: std_mpsc::Sender<PackageCompletion>,
        package_completion_rx: std_mpsc::Receiver<PackageCompletion>,
        pending_package_opens: usize,
        save_transactions: ResourceTable<SaveTransaction, SaveTransactionHandle>,
        bundle_root: std::path::PathBuf,
        package_sources: ResourceTable<PackageSourceResource, PackageSourceHandle>,
        event_sequence: u64,
        gamepads: gilrs::Gilrs,
        gamepad_mapper: astra_platform_common::GamepadMapper,
    }

    struct PackageHostConfig {
        source_policies: Vec<astra_platform::PackageSourcePolicy>,
        package_id: String,
        cache_policy: astra_platform::PackageCachePolicy,
        bundle_root: std::path::PathBuf,
    }

    impl MacosHostApp {
        fn new(
            backend: PlatformBackendChannels,
            ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
            save_store: AtomicSaveStore,
            package_cache: VerifiedPackageCache,
            package: PackageHostConfig,
        ) -> Result<Self, PlatformError> {
            let gamepads = gilrs::Gilrs::new().map_err(|_| {
                let error = host_error(
                    "input.gamepad.open",
                    "macOS Gaming Input initialization failed",
                );
                let _ = ready.send(Err(error.clone()));
                error
            })?;
            let gamepad_mapper = astra_platform_common::GamepadMapper::new(0.2)?;
            let (package_completion_tx, package_completion_rx) = std_mpsc::channel();
            Ok(Self {
                backend,
                ready: Some(ready),
                windows: ResourceTable::new("window"),
                window_ids: BTreeMap::new(),
                accessibility: BTreeMap::new(),
                surfaces: ResourceTable::new("surface"),
                surface_windows: BTreeMap::new(),
                audio_outputs: ResourceTable::new("audio_output"),
                decode_sessions: ResourceTable::new("decode_session"),
                save_store,
                package_cache,
                package_source_policies: package.source_policies,
                package_id: package.package_id,
                package_cache_policy: package.cache_policy,
                package_completion_tx,
                package_completion_rx,
                pending_package_opens: 0,
                save_transactions: ResourceTable::new("save_transaction"),
                bundle_root: package.bundle_root,
                package_sources: ResourceTable::new("package_source"),
                event_sequence: 0,
                gamepads,
                gamepad_mapper,
            })
        }

        fn next_sequence(&mut self) -> u64 {
            self.event_sequence += 1;
            self.event_sequence
        }

        fn emit(&mut self, kind: PlatformEventKind) {
            let sequence = self.next_sequence();
            if let Err(error) = self.backend.emit_event(PlatformEvent::new(sequence, kind)) {
                tracing::error!(
                    event = "platform.macos.event.emit_failed",
                    diagnostic_code = ?error.code,
                    operation = %error.operation,
                    "macOS platform event could not be emitted"
                );
            }
        }

        fn process_commands(&mut self, event_loop: &ActiveEventLoop) {
            loop {
                let command = match self.backend.try_next_command() {
                    Ok(Some(command)) => command,
                    Ok(None) => break,
                    Err(_) => {
                        event_loop.exit();
                        break;
                    }
                };
                match command {
                    HostCommand::CreateWindow { request, reply } => {
                        let attributes = WindowAttributes::default()
                            .with_title(request.title)
                            .with_visible(false)
                            .with_inner_size(winit::dpi::PhysicalSize::new(
                                request.width,
                                request.height,
                            ));
                        let result = event_loop
                            .create_window(attributes)
                            .map_err(|_| host_error("window.create", "window creation failed"))
                            .and_then(|window| {
                                let window = Arc::new(window);
                                window.set_ime_allowed(true);
                                let id = window.id();
                                let handle = self.windows.insert(window)?;
                                self.window_ids.insert(id, handle);
                                let native = self.windows.get(handle)?.clone();
                                self.accessibility.insert(
                                    id,
                                    MacosAccessibilityBridge::new(
                                        event_loop,
                                        native.as_ref(),
                                        handle,
                                    ),
                                );
                                native.set_visible(request.visible);
                                Ok(handle)
                            });
                        let _ = reply.send(result);
                    }
                    HostCommand::CreateSurface { request, reply } => {
                        let window_id = self.windows.get(request.window).map(|window| window.id());
                        let result = self
                            .windows
                            .get(request.window)
                            .cloned()
                            .and_then(|window| {
                                create_surface(window, request.width, request.height)
                            })
                            .and_then(|surface| self.surfaces.insert(surface));
                        if let (Ok(surface), Ok(window_id)) = (&result, window_id) {
                            self.surface_windows.insert(*surface, window_id);
                        }
                        let _ = reply.send(result);
                    }
                    HostCommand::PresentRgba {
                        surface,
                        frame,
                        reply,
                    } => {
                        let result = self
                            .surfaces
                            .get_mut(surface)
                            .and_then(|surface| surface.present(frame));
                        if result
                            .as_ref()
                            .is_err_and(|error| error.code == PlatformErrorCode::ContextLost)
                        {
                            let recovered = self
                                .surfaces
                                .get_mut(surface)
                                .and_then(|surface| surface.reconfigure_after_loss())
                                .is_ok();
                            for event in astra_platform_common::wgpu_recovery_events(
                                "wgpu_hardware",
                                recovered,
                            ) {
                                self.emit(event);
                            }
                        } else if result
                            .as_ref()
                            .is_err_and(|error| error.code == PlatformErrorCode::DeviceLost)
                        {
                            let recovered = self
                                .surfaces
                                .get_mut(surface)
                                .and_then(|surface| {
                                    pollster::block_on(surface.recover_device()).map(|_| ())
                                })
                                .is_ok();
                            for event in astra_platform_common::wgpu_device_recovery_events(
                                "wgpu_hardware",
                                recovered,
                            ) {
                                self.emit(event);
                            }
                        }
                        let _ = reply.send(result);
                    }
                    HostCommand::PresentScene {
                        surface,
                        frame,
                        reply,
                    } => {
                        let semantics = frame.semantics.clone();
                        let result = self
                            .surfaces
                            .get_mut(surface)
                            .and_then(|surface| surface.present_scene(frame));
                        let result = result.and_then(|()| {
                            let Some(semantics) = semantics.as_ref() else {
                                return Ok(());
                            };
                            let window_id =
                                self.surface_windows.get(&surface).ok_or_else(|| {
                                    host_error(
                                        "accessibility.macos.update",
                                        "surface is not bound to a window",
                                    )
                                })?;
                            self.accessibility
                                .get_mut(window_id)
                                .ok_or_else(|| {
                                    host_error(
                                        "accessibility.macos.update",
                                        "window accessibility bridge is unavailable",
                                    )
                                })?
                                .update(semantics)
                        });
                        if result
                            .as_ref()
                            .is_err_and(|error| error.code == PlatformErrorCode::ContextLost)
                        {
                            let recovered = self
                                .surfaces
                                .get_mut(surface)
                                .and_then(|surface| surface.reconfigure_after_loss())
                                .is_ok();
                            for event in astra_platform_common::wgpu_recovery_events(
                                "wgpu_hardware",
                                recovered,
                            ) {
                                self.emit(event);
                            }
                        } else if result
                            .as_ref()
                            .is_err_and(|error| error.code == PlatformErrorCode::DeviceLost)
                        {
                            let recovered = self
                                .surfaces
                                .get_mut(surface)
                                .and_then(|surface| {
                                    pollster::block_on(surface.recover_device()).map(|_| ())
                                })
                                .is_ok();
                            for event in astra_platform_common::wgpu_device_recovery_events(
                                "wgpu_hardware",
                                recovered,
                            ) {
                                self.emit(event);
                            }
                        }
                        let _ = reply.send(result);
                    }
                    #[cfg(feature = "platform-test-driver")]
                    HostCommand::InjectSurfaceDeviceLoss { surface, reply } => {
                        let result = self.surfaces.get(surface).map(|surface| {
                            surface.inject_device_loss_for_test();
                        });
                        let _ = reply.send(result);
                    }
                    HostCommand::CaptureSurface { surface, reply } => {
                        let result = self.surfaces.get_mut(surface).and_then(capture_surface);
                        let _ = reply.send(result);
                    }
                    HostCommand::DestroySurface { surface, reply } => {
                        self.surface_windows.remove(&surface);
                        let result = self.surfaces.remove(surface).map(|_| ());
                        let _ = reply.send(result);
                    }
                    HostCommand::DestroyWindow { window, reply } => {
                        let result = self.windows.remove(window).map(|window| {
                            self.window_ids.remove(&window.id());
                            self.accessibility.remove(&window.id());
                        });
                        let _ = reply.send(result);
                    }
                    HostCommand::OpenAudioOutput { request, reply } => {
                        let result = AudioResource::new(request)
                            .and_then(|resource| self.audio_outputs.insert(resource));
                        let _ = reply.send(result);
                    }
                    HostCommand::QueryAudioOutputFormat { reply } => {
                        let result = preferred_audio_output_format();
                        let _ = reply.send(result);
                    }
                    HostCommand::QueryAudioDeviceFormat { reply } => {
                        let _ = reply.send(default_audio_device_format());
                    }
                    HostCommand::SubmitAudio {
                        output,
                        packet,
                        reply,
                    } => {
                        let result = self
                            .audio_outputs
                            .get_mut(output)
                            .and_then(|resource| resource.submit(packet));
                        if result
                            .as_ref()
                            .is_err_and(|error| error.code == PlatformErrorCode::DeviceLost)
                        {
                            let _ = self.audio_outputs.remove(output);
                            self.emit(PlatformEventKind::DeviceLost {
                                provider: "coreaudio".to_string(),
                            });
                        }
                        let _ = reply.send(result);
                    }
                    HostCommand::QueryAudio { output, reply } => {
                        let result = self.audio_outputs.get(output).map(AudioResource::state);
                        let _ = reply.send(result);
                    }
                    HostCommand::DrainAudio { output, reply } => {
                        let result = self
                            .audio_outputs
                            .get_mut(output)
                            .and_then(AudioResource::drain);
                        if result
                            .as_ref()
                            .is_err_and(|error| error.code == PlatformErrorCode::DeviceLost)
                        {
                            let _ = self.audio_outputs.remove(output);
                            self.emit(PlatformEventKind::DeviceLost {
                                provider: "coreaudio".to_string(),
                            });
                        }
                        let _ = reply.send(result);
                    }
                    HostCommand::QueryAudioOutput { output, reply } => {
                        let result = self
                            .audio_outputs
                            .get(output)
                            .and_then(AudioResource::status);
                        if result
                            .as_ref()
                            .is_err_and(|error| error.code == PlatformErrorCode::DeviceLost)
                        {
                            let _ = self.audio_outputs.remove(output);
                            self.emit(PlatformEventKind::DeviceLost {
                                provider: "coreaudio".to_string(),
                            });
                        }
                        let _ = reply.send(result);
                    }
                    HostCommand::PauseAudio { output, reply } => {
                        let result = self
                            .audio_outputs
                            .get_mut(output)
                            .and_then(AudioResource::pause);
                        let _ = reply.send(result);
                    }
                    HostCommand::ResumeAudio { output, reply } => {
                        let result = self
                            .audio_outputs
                            .get_mut(output)
                            .and_then(AudioResource::resume);
                        let _ = reply.send(result);
                    }
                    HostCommand::AbortAudio { output, reply } => {
                        let result = self.audio_outputs.remove(output).map(|_| ());
                        let _ = reply.send(result);
                    }
                    #[cfg(feature = "platform-test-driver")]
                    HostCommand::InjectAudioDeviceLoss { output, reply } => {
                        let result = self
                            .audio_outputs
                            .get_mut(output)
                            .map(AudioResource::inject_device_loss);
                        let _ = reply.send(result);
                    }
                    HostCommand::CloseAudio { output, reply } => {
                        let drain = self
                            .audio_outputs
                            .get_mut(output)
                            .and_then(AudioResource::drain);
                        let remove = self.audio_outputs.remove(output).map(|_| ());
                        let result = match (drain, remove) {
                            (Err(error), _) => Err(error),
                            (Ok(_), result) => result,
                        };
                        let _ = reply.send(result);
                    }
                    HostCommand::OpenDecode { kind, reply } => {
                        let result = DecodeResource::new(kind)
                            .and_then(|resource| self.decode_sessions.insert(resource));
                        let _ = reply.send(result);
                    }
                    HostCommand::Decode {
                        session,
                        request,
                        reply,
                    } => {
                        let result = self
                            .decode_sessions
                            .get_mut(session)
                            .and_then(|resource| resource.decode(request));
                        let _ = reply.send(result);
                    }
                    HostCommand::CloseDecode { session, reply } => {
                        let result = self.decode_sessions.remove(session).map(|_| ());
                        let _ = reply.send(result);
                    }
                    HostCommand::BeginSave { slot, reply } => {
                        let result = self
                            .save_store
                            .begin(&slot)
                            .and_then(|transaction| self.save_transactions.insert(transaction));
                        let _ = reply.send(result);
                    }
                    HostCommand::WriteSave {
                        transaction,
                        bytes,
                        reply,
                    } => {
                        let result = self
                            .save_transactions
                            .get_mut(transaction)
                            .and_then(|transaction| transaction.write(&bytes));
                        let _ = reply.send(result);
                    }
                    HostCommand::CommitSave { transaction, reply } => {
                        let result = self
                            .save_transactions
                            .remove(transaction)
                            .and_then(SaveTransaction::commit);
                        let _ = reply.send(result);
                    }
                    HostCommand::AbortSave { transaction, reply } => {
                        let result = self
                            .save_transactions
                            .remove(transaction)
                            .and_then(SaveTransaction::abort);
                        let _ = reply.send(result);
                    }
                    HostCommand::ReadSave { slot, reply } => {
                        let _ = reply.send(self.save_store.read(&slot));
                    }
                    HostCommand::DeleteSave { slot, reply } => {
                        let _ = reply.send(self.save_store.delete(&slot));
                    }
                    HostCommand::OpenPackage { source, reply } => {
                        let result = match source {
                            PackageSourceRequest::Bundled {
                                relative_path,
                                expected_hash,
                            } => FilePackageSource::open(
                                self.bundle_root.join(relative_path),
                                &expected_hash,
                            )
                            .and_then(|source| {
                                self.package_sources
                                    .insert(PackageSourceResource::Bundled(source))
                            }),
                            PackageSourceRequest::UserAuthorized { expected_hash } => self
                                .open_user_authorized_package(&expected_hash)
                                .and_then(|source| {
                                    self.package_sources
                                        .insert(PackageSourceResource::Cached(source))
                                }),
                            PackageSourceRequest::HttpsRange { url, expected_hash } => {
                                self.start_https_package_open(url, expected_hash, reply);
                                continue;
                            }
                        };
                        let _ = reply.send(result);
                    }
                    HostCommand::ReadPackageRange {
                        source,
                        offset,
                        length,
                        reply,
                    } => {
                        let result = self
                            .package_sources
                            .get_mut(source)
                            .and_then(|source| source.read_range(offset, length));
                        let _ = reply.send(result);
                    }
                    HostCommand::ClosePackage { source, reply } => {
                        let result = self.package_sources.remove(source).map(|_| ());
                        let _ = reply.send(result);
                    }
                    HostCommand::Shutdown { reply } => {
                        let result = self
                            .surfaces
                            .ensure_empty()
                            .and_then(|_| self.windows.ensure_empty())
                            .and_then(|_| self.audio_outputs.ensure_empty())
                            .and_then(|_| self.decode_sessions.ensure_empty())
                            .and_then(|_| self.save_transactions.ensure_empty())
                            .and_then(|_| self.package_sources.ensure_empty())
                            .and_then(|_| {
                                if self.pending_package_opens == 0 {
                                    Ok(())
                                } else {
                                    Err(PlatformError::new(
                                        PlatformErrorCode::InvalidState,
                                        "host.shutdown",
                                        "package source requests are still in flight",
                                    ))
                                }
                            });
                        let should_exit = result.is_ok();
                        let _ = reply.send(result);
                        if should_exit {
                            event_loop.exit();
                        }
                    }
                }
            }
        }

        fn open_user_authorized_package(
            &mut self,
            expected_hash: &str,
        ) -> Result<CachedPackageSource, PlatformError> {
            let path = pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title("Open Astra package")
                    .add_filter("Astra package", &["astrapkg"])
                    .pick_file(),
            )
            .map(|file| file.path().to_path_buf())
            .ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::Cancelled,
                    "package.open_user_authorized",
                    "user cancelled package selection",
                )
            })?;
            let bytes = std::fs::read(path).map_err(|_| {
                host_error(
                    "package.open_user_authorized",
                    "user-authorized package could not be read",
                )
            })?;
            self.package_cache.store_verified(expected_hash, &bytes)?;
            self.package_cache.open_source(expected_hash)
        }

        fn start_https_package_open(
            &mut self,
            url: String,
            expected_hash: String,
            reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
        ) {
            let completion_tx = self.package_completion_tx.clone();
            let policies = self.package_source_policies.clone();
            let package_id = self.package_id.clone();
            let policy = self.package_cache_policy.clone();
            self.pending_package_opens += 1;
            thread::spawn(move || {
                let result = (|| {
                    let cache_root = VerifiedPackageCache::platform_cache_root(&package_id)?;
                    let mut cache = VerifiedPackageCache::open(cache_root, policy)?;
                    let client = astra_platform_common::HttpRangeClient::from_policies(&policies)?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| {
                            host_error("package.https.open", "HTTPS runtime could not start")
                        })?;
                    runtime.block_on(client.fetch_into_cache(&url, &expected_hash, &mut cache))?;
                    cache.open_source(&expected_hash)
                })();
                let _ = completion_tx.send(PackageCompletion { reply, result });
            });
        }

        fn process_package_completions(&mut self) {
            while let Ok(completion) = self.package_completion_rx.try_recv() {
                self.pending_package_opens = self.pending_package_opens.saturating_sub(1);
                let result = completion.result.and_then(|source| {
                    self.package_sources
                        .insert(PackageSourceResource::Cached(source))
                });
                let _ = completion.reply.send(result);
            }
        }
    }

    impl ApplicationHandler for MacosHostApp {
        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
            let sequence = self.next_sequence();
            let result = self
                .backend
                .emit_event(PlatformEvent::new(sequence, PlatformEventKind::Resumed));
            if let Some(ready) = self.ready.take() {
                let _ = ready.send(result);
            }
        }

        fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
            self.emit(PlatformEventKind::Suspended);
        }

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            window_id: WindowId,
            event: WindowEvent,
        ) {
            let Some(window) = self.window_ids.get(&window_id).copied() else {
                return;
            };
            if let (Ok(native), Some(bridge)) = (
                self.windows.get(window),
                self.accessibility.get_mut(&window_id),
            ) {
                bridge.process_event(native.as_ref(), &event);
            }
            let kind =
                match event {
                    WindowEvent::Focused(focused) => {
                        Some(PlatformEventKind::WindowFocused { window, focused })
                    }
                    WindowEvent::CloseRequested => Some(PlatformEventKind::WindowClosed { window }),
                    WindowEvent::Resized(size) => self.windows.get(window).ok().map(|native| {
                        PlatformEventKind::WindowResized {
                            window,
                            width: size.width,
                            height: size.height,
                            scale_factor: native.scale_factor(),
                        }
                    }),
                    WindowEvent::KeyboardInput { event, .. } => Some(PlatformEventKind::Keyboard {
                        window,
                        physical_key: format!("{:?}", event.physical_key),
                        logical_key: event.logical_key.to_text().map(str::to_string),
                        state: input_state(event.state),
                        repeat: event.repeat,
                    }),
                    WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                        Some(PlatformEventKind::ImePreedit {
                            window,
                            text,
                            cursor,
                        })
                    }
                    WindowEvent::Ime(Ime::Commit(text)) => {
                        Some(PlatformEventKind::ImeCommit { window, text })
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        Some(PlatformEventKind::PointerMoved {
                            window,
                            x: position.x,
                            y: position.y,
                        })
                    }
                    WindowEvent::MouseInput { state, button, .. } => {
                        Some(PlatformEventKind::PointerButton {
                            window,
                            button: pointer_button(button),
                            state: input_state(state),
                        })
                    }
                    WindowEvent::MouseWheel { delta, .. } => {
                        let (delta_x, delta_y) = match delta {
                            MouseScrollDelta::LineDelta(x, y) => (x, y),
                            MouseScrollDelta::PixelDelta(position) => {
                                (position.x as f32, position.y as f32)
                            }
                        };
                        Some(PlatformEventKind::MouseWheel {
                            window,
                            delta_x,
                            delta_y,
                        })
                    }
                    WindowEvent::Touch(touch) => Some(PlatformEventKind::Touch {
                        window,
                        id: touch.id,
                        x: touch.location.x,
                        y: touch.location.y,
                        phase: touch_phase(touch.phase),
                    }),
                    _ => None,
                };
            if let Some(kind) = kind {
                self.emit(kind);
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.process_package_completions();
            self.process_commands(event_loop);
            let actions = self
                .accessibility
                .values_mut()
                .flat_map(MacosAccessibilityBridge::drain_actions)
                .collect::<Vec<_>>();
            for request in actions {
                self.emit(PlatformEventKind::AccessibilityAction {
                    window: request.window,
                    semantic_id: request.semantic_id,
                    action: request.action,
                    value: request.value,
                });
            }
            self.poll_gamepad();
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(4),
            ));
        }
    }

    struct PackageCompletion {
        reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
        result: Result<CachedPackageSource, PlatformError>,
    }

    enum PackageSourceResource {
        Bundled(FilePackageSource),
        Cached(CachedPackageSource),
    }

    impl PackageSourceResource {
        fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
            match self {
                Self::Bundled(source) => source.read_range(offset, length),
                Self::Cached(source) => source.read_range(offset, length),
            }
        }
    }

    impl MacosHostApp {
        fn poll_gamepad(&mut self) {
            while let Some(event) = self.gamepads.next_event() {
                let Some(event) = raw_gamepad_event(event) else {
                    continue;
                };
                match self.gamepad_mapper.apply_checked(event) {
                    Ok(events) => {
                        for event in events {
                            self.emit(event);
                        }
                    }
                    Err(error) => tracing::warn!(
                        event = "platform.macos.gamepad.invalid_event",
                        diagnostic_code = ?error.code,
                        operation = %error.operation,
                        "macOS Gaming Input event was rejected"
                    ),
                }
            }
        }
    }

    fn raw_gamepad_event(event: gilrs::Event) -> Option<astra_platform_common::RawGamepadEvent> {
        use astra_platform::GamepadControl;
        use astra_platform_common::RawGamepadEvent;
        use gilrs::{Axis, Button, EventType};

        let raw_device_id = u32::try_from(usize::from(event.id)).ok()?;
        let map_button = |button| match button {
            Button::South => Some(GamepadControl::South),
            Button::East => Some(GamepadControl::East),
            Button::West => Some(GamepadControl::West),
            Button::North => Some(GamepadControl::North),
            Button::DPadUp => Some(GamepadControl::DpadUp),
            Button::DPadDown => Some(GamepadControl::DpadDown),
            Button::DPadLeft => Some(GamepadControl::DpadLeft),
            Button::DPadRight => Some(GamepadControl::DpadRight),
            Button::LeftTrigger => Some(GamepadControl::LeftShoulder),
            Button::RightTrigger => Some(GamepadControl::RightShoulder),
            Button::LeftTrigger2 => Some(GamepadControl::LeftTrigger),
            Button::RightTrigger2 => Some(GamepadControl::RightTrigger),
            Button::LeftThumb => Some(GamepadControl::LeftStickButton),
            Button::RightThumb => Some(GamepadControl::RightStickButton),
            Button::Start => Some(GamepadControl::Start),
            Button::Select => Some(GamepadControl::Select),
            _ => None,
        };
        let map_axis = |axis| match axis {
            Axis::LeftStickX => Some(GamepadControl::LeftStickX),
            Axis::LeftStickY => Some(GamepadControl::LeftStickY),
            Axis::RightStickX => Some(GamepadControl::RightStickX),
            Axis::RightStickY => Some(GamepadControl::RightStickY),
            _ => None,
        };
        match event.event {
            EventType::Connected => Some(RawGamepadEvent::Connected { raw_device_id }),
            EventType::Disconnected => Some(RawGamepadEvent::Disconnected { raw_device_id }),
            EventType::ButtonPressed(button, _) | EventType::ButtonRepeated(button, _) => {
                map_button(button).map(|control| RawGamepadEvent::Button {
                    raw_device_id,
                    control,
                    pressed: true,
                })
            }
            EventType::ButtonReleased(button, _) => {
                map_button(button).map(|control| RawGamepadEvent::Button {
                    raw_device_id,
                    control,
                    pressed: false,
                })
            }
            EventType::ButtonChanged(button, value, _) => map_button(button).map(|control| {
                if matches!(
                    control,
                    GamepadControl::LeftTrigger | GamepadControl::RightTrigger
                ) {
                    RawGamepadEvent::Axis {
                        raw_device_id,
                        control,
                        value,
                    }
                } else {
                    RawGamepadEvent::Button {
                        raw_device_id,
                        control,
                        pressed: value >= 0.5,
                    }
                }
            }),
            EventType::AxisChanged(axis_value, value, _) => {
                map_axis(axis_value).map(|control| RawGamepadEvent::Axis {
                    raw_device_id,
                    control,
                    value,
                })
            }
            EventType::Dropped | EventType::ForceFeedbackEffectCompleted => None,
            _ => None,
        }
    }

    fn input_state(state: ElementState) -> InputState {
        match state {
            ElementState::Pressed => InputState::Pressed,
            ElementState::Released => InputState::Released,
        }
    }

    fn pointer_button(button: MouseButton) -> PointerButton {
        match button {
            MouseButton::Left => PointerButton::Primary,
            MouseButton::Right => PointerButton::Secondary,
            MouseButton::Middle => PointerButton::Middle,
            MouseButton::Back => PointerButton::Back,
            MouseButton::Forward => PointerButton::Forward,
            MouseButton::Other(value) => PointerButton::Other(value),
        }
    }

    fn touch_phase(phase: WinitTouchPhase) -> TouchPhase {
        match phase {
            WinitTouchPhase::Started => TouchPhase::Started,
            WinitTouchPhase::Moved => TouchPhase::Moved,
            WinitTouchPhase::Ended => TouchPhase::Ended,
            WinitTouchPhase::Cancelled => TouchPhase::Cancelled,
        }
    }

    type SurfaceResource = astra_platform_common::WgpuPresentationCore;

    fn create_surface(
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<SurfaceResource, PlatformError> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(window)
            .map_err(|_| host_error("surface.create", "wgpu surface creation failed"))?;
        pollster::block_on(SurfaceResource::new(instance, surface, width, height, true))
    }

    fn capture_surface(surface: &mut SurfaceResource) -> Result<CapturedFrame, PlatformError> {
        let readback = surface.begin_capture()?;
        let (mapped_tx, mapped_rx) = std_mpsc::sync_channel(1);
        readback.map_async(move |result| {
            let _ = mapped_tx.send(result);
        });
        surface.poll(wgpu::PollType::wait_indefinitely())?;
        mapped_rx
            .recv()
            .map_err(|_| host_error("surface.capture", "GPU readback callback was lost"))?
            .map_err(|_| host_error("surface.capture", "GPU readback mapping failed"))?;
        readback.finish()
    }

    struct AudioResource {
        stream: cpal::Stream,
        producer: astra_platform_common::NativeAudioProducer,
        queue_telemetry: astra_platform_common::AudioQueueTelemetryReader,
        meter: Arc<CallbackMeter>,
        stream_error: Arc<AtomicBool>,
        channels: u16,
        sample_rate: u32,
        next_sequence: u64,
        submitted_samples: u64,
        paused: bool,
    }

    fn preferred_audio_output_format() -> Result<astra_platform::AudioOutputFormat, PlatformError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                host_error(
                    "audio.format",
                    "CoreAudio default output device is unavailable",
                )
            })?;
        let supported = device.default_output_config().map_err(|_| {
            host_error(
                "audio.format",
                "CoreAudio default output config is unavailable",
            )
        })?;
        Ok(astra_platform::AudioOutputFormat {
            sample_rate: supported.sample_rate(),
            channels: supported.channels(),
        })
    }

    impl AudioResource {
        fn new(request: AudioOutputRequest) -> Result<Self, PlatformError> {
            if request.sample_rate == 0 || request.channels == 0 || request.max_buffered_frames == 0
            {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.open",
                    "audio output format and queue capacity must be non-zero",
                ));
            }
            let host = cpal::default_host();
            let device = host.default_output_device().ok_or_else(|| {
                host_error(
                    "audio.open",
                    "CoreAudio default output device is unavailable",
                )
            })?;
            let requested_rate = request.sample_rate;
            let supported = device
                .supported_output_configs()
                .map_err(|_| host_error("audio.open", "CoreAudio output format enumeration failed"))?
                .filter(|range| {
                    range.channels() == request.channels
                        && range.min_sample_rate() <= requested_rate
                        && range.max_sample_rate() >= requested_rate
                        && sample_format_rank(range.sample_format()).is_some()
                })
                .map(|range| range.with_sample_rate(requested_rate))
                .min_by_key(|config| sample_format_rank(config.sample_format()))
                .ok_or_else(|| {
                    host_error(
                        "audio.open",
                        "CoreAudio has no exact supported format for the requested rate and channels",
                    )
                })?;
            let config: cpal::StreamConfig = supported.clone().into();
            let capacity = request
                .max_buffered_frames
                .checked_mul(usize::from(request.channels))
                .ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "audio.open",
                        "audio output queue capacity overflows",
                    )
                })?;
            let (producer, consumer, queue_telemetry) =
                astra_platform_common::NativeAudioQueue::create(capacity)?;
            let meter = Arc::new(CallbackMeter::default());
            let stream_error = Arc::new(AtomicBool::new(false));
            let stream = match supported.sample_format() {
                cpal::SampleFormat::F32 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [f32], _| fill_f32(output, &mut consumer, &meter),
                        move |stream_error_value| set_stream_error(&error, stream_error_value),
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [i16], _| fill_i16(output, &mut consumer, &meter),
                        move |stream_error_value| set_stream_error(&error, stream_error_value),
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [u16], _| fill_u16(output, &mut consumer, &meter),
                        move |stream_error_value| set_stream_error(&error, stream_error_value),
                        None,
                    )
                }
                _ => {
                    return Err(host_error(
                        "audio.open",
                        "CoreAudio sample format is unsupported",
                    ));
                }
            }
            .map_err(|_| host_error("audio.open", "CoreAudio output stream creation failed"))?;
            stream
                .play()
                .map_err(|_| host_error("audio.open", "CoreAudio output stream could not start"))?;
            Ok(Self {
                stream,
                producer,
                queue_telemetry,
                meter,
                stream_error,
                channels: request.channels,
                sample_rate: request.sample_rate,
                next_sequence: 1,
                submitted_samples: 0,
                paused: false,
            })
        }

        fn submit(&mut self, packet: AudioPacket) -> Result<(), PlatformError> {
            if self.stream_error.load(Ordering::Acquire) {
                return Err(PlatformError::new(
                    PlatformErrorCode::DeviceLost,
                    "audio.submit",
                    "CoreAudio output stream reported a device error",
                ));
            }
            if packet.sequence != self.next_sequence
                || packet.channels != self.channels
                || packet.samples.is_empty()
                || !packet
                    .samples
                    .len()
                    .is_multiple_of(usize::from(packet.channels))
                || packet.samples.iter().any(|sample| !sample.is_finite())
            {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.submit",
                    "audio packet sequence or channel count is invalid",
                ));
            }
            let next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.submit",
                    "audio packet sequence overflowed",
                )
            })?;
            let submitted_samples = self
                .submitted_samples
                .checked_add(packet.samples.len() as u64)
                .ok_or_else(|| host_error("audio.submit", "audio sample counter overflowed"))?;
            self.producer.push_samples(&packet.samples)?;
            self.next_sequence = next_sequence;
            self.submitted_samples = submitted_samples;
            Ok(())
        }

        fn pause(&mut self) -> Result<(), PlatformError> {
            if self.paused {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.pause",
                    "CoreAudio output is already paused",
                ));
            }
            self.stream
                .pause()
                .map_err(|_| host_error("audio.pause", "CoreAudio output could not pause"))?;
            self.paused = true;
            Ok(())
        }

        fn resume(&mut self) -> Result<(), PlatformError> {
            if !self.paused {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.resume",
                    "CoreAudio output is not paused",
                ));
            }
            self.stream
                .play()
                .map_err(|_| host_error("audio.resume", "CoreAudio output could not resume"))?;
            self.paused = false;
            Ok(())
        }

        #[cfg(feature = "platform-test-driver")]
        fn inject_device_loss(&mut self) {
            self.stream_error.store(true, Ordering::Release);
        }

        fn drain(&mut self) -> Result<AudioMeter, PlatformError> {
            if self.paused {
                self.resume()?;
            }
            let request = AudioOutputRequest {
                sample_rate: self.sample_rate,
                channels: self.channels,
                max_buffered_frames: 1,
            };
            let deadline = Instant::now() + request.drain_timeout(self.submitted_samples);
            loop {
                if self.stream_error.load(Ordering::Acquire) {
                    return Err(PlatformError::new(
                        PlatformErrorCode::DeviceLost,
                        "audio.drain",
                        "CoreAudio output stream reported a device error",
                    ));
                }
                if self.queue_telemetry.snapshot().sample_count >= self.submitted_samples {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(host_error(
                        "audio.drain",
                        "CoreAudio output drain timed out",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Ok(self.meter.snapshot())
        }

        fn state(&self) -> astra_platform::AudioOutputState {
            let telemetry = self.queue_telemetry.snapshot();
            let queued_samples = self
                .submitted_samples
                .saturating_sub(telemetry.sample_count);
            astra_platform::AudioOutputState {
                queued_frames: usize::try_from(queued_samples / u64::from(self.channels))
                    .unwrap_or(usize::MAX),
                callback_count: self.meter.callback_count.load(Ordering::Acquire),
                submitted_samples: self.submitted_samples,
                consumed_samples: telemetry.sample_count,
                underflow_count: telemetry.underflow_count,
                meter: self.meter.snapshot(),
            }
        }

        fn status(&self) -> Result<AudioOutputStatus, PlatformError> {
            if self.stream_error.load(Ordering::Acquire) {
                return Err(PlatformError::new(
                    PlatformErrorCode::DeviceLost,
                    "audio.query",
                    "CoreAudio output stream reported a device error",
                ));
            }
            let consumed_samples = self.queue_telemetry.snapshot();
            let channels = u64::from(self.channels);
            if consumed_samples.sample_count > self.submitted_samples
                || !self.submitted_samples.is_multiple_of(channels)
            {
                return Err(PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "audio.query",
                    "CoreAudio queue telemetry is inconsistent with submitted audio",
                ));
            }
            let submitted_frames = self.submitted_samples / channels;
            let played_frames = consumed_samples.sample_count / channels;
            Ok(AudioOutputStatus {
                submitted_frames,
                played_frames,
                buffered_frames: submitted_frames - played_frames,
                underflow_count: consumed_samples.underflow_count,
                meter: self.meter.snapshot(),
            })
        }
    }

    fn default_audio_device_format() -> Result<AudioDeviceFormat, PlatformError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                host_error(
                    "audio.query_device_format",
                    "CoreAudio default output device is unavailable",
                )
            })?;
        let config = device.default_output_config().map_err(|_| {
            host_error(
                "audio.query_device_format",
                "CoreAudio default output format is unavailable",
            )
        })?;
        if sample_format_rank(config.sample_format()).is_none()
            || config.sample_rate() == 0
            || config.channels() == 0
        {
            return Err(host_error(
                "audio.query_device_format",
                "CoreAudio default output format is unsupported",
            ));
        }
        Ok(AudioDeviceFormat {
            sample_rate: config.sample_rate(),
            channels: config.channels(),
        })
    }

    #[derive(Default)]
    struct CallbackMeter {
        callback_count: AtomicU64,
        sample_count: AtomicU64,
        peak_bits: AtomicU32,
        sum_squares_bits: AtomicU64,
    }

    impl CallbackMeter {
        fn begin_callback(&self) {
            self.callback_count.fetch_add(1, Ordering::Release);
        }

        fn record(&self, sample: f32) {
            let magnitude = sample.abs();
            let magnitude_bits = magnitude.to_bits();
            let mut peak_bits = self.peak_bits.load(Ordering::Relaxed);
            while magnitude_bits > peak_bits {
                match self.peak_bits.compare_exchange_weak(
                    peak_bits,
                    magnitude_bits,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => peak_bits = actual,
                }
            }
            let contribution = f64::from(sample) * f64::from(sample);
            let mut sum_bits = self.sum_squares_bits.load(Ordering::Relaxed);
            loop {
                let next = f64::from_bits(sum_bits) + contribution;
                match self.sum_squares_bits.compare_exchange_weak(
                    sum_bits,
                    next.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => sum_bits = actual,
                }
            }
            self.sample_count.fetch_add(1, Ordering::Release);
        }

        fn snapshot(&self) -> AudioMeter {
            let sample_count = self.sample_count.load(Ordering::Acquire);
            let rms = if sample_count == 0 {
                0.0
            } else {
                (f64::from_bits(self.sum_squares_bits.load(Ordering::Acquire))
                    / sample_count as f64)
                    .sqrt() as f32
            };
            AudioMeter {
                sample_count,
                peak_dbfs: amplitude_dbfs(f32::from_bits(self.peak_bits.load(Ordering::Acquire))),
                rms_dbfs: amplitude_dbfs(rms),
            }
        }
    }

    fn amplitude_dbfs(value: f32) -> f32 {
        if value <= 0.0 {
            -120.0
        } else {
            20.0 * value.log10()
        }
    }

    fn sample_format_rank(format: cpal::SampleFormat) -> Option<u8> {
        match format {
            cpal::SampleFormat::F32 => Some(0),
            cpal::SampleFormat::I16 => Some(1),
            cpal::SampleFormat::U16 => Some(2),
            _ => None,
        }
    }

    fn pop_sample(
        consumer: &mut astra_platform_common::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) -> Option<f32> {
        match consumer.pop_sample() {
            Some(sample) => {
                meter.record(sample);
                Some(sample)
            }
            None => None,
        }
    }

    fn fill_f32(
        output: &mut [f32],
        consumer: &mut astra_platform_common::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) {
        meter.begin_callback();
        let mut underflowed = false;
        for target in output {
            *target = pop_sample(consumer, meter).unwrap_or_else(|| {
                underflowed = true;
                0.0
            });
        }
        if underflowed {
            consumer.record_underflow();
        }
    }

    fn fill_i16(
        output: &mut [i16],
        consumer: &mut astra_platform_common::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) {
        meter.begin_callback();
        let mut underflowed = false;
        for target in output {
            let sample = pop_sample(consumer, meter).unwrap_or_else(|| {
                underflowed = true;
                0.0
            });
            *target = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        }
        if underflowed {
            consumer.record_underflow();
        }
    }

    fn fill_u16(
        output: &mut [u16],
        consumer: &mut astra_platform_common::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) {
        meter.begin_callback();
        let mut underflowed = false;
        for target in output {
            let sample = pop_sample(consumer, meter)
                .unwrap_or_else(|| {
                    underflowed = true;
                    0.0
                })
                .clamp(-1.0, 1.0);
            *target = ((sample * 0.5 + 0.5) * f32::from(u16::MAX)) as u16;
        }
        if underflowed {
            consumer.record_underflow();
        }
    }

    fn set_stream_error(error: &AtomicBool, _value: cpal::StreamError) {
        error.store(true, Ordering::Release);
    }

    struct DecodeResource {
        kind: DecodeKind,
        next_sequence: u64,
    }

    impl DecodeResource {
        fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
            Ok(Self {
                kind,
                next_sequence: 1,
            })
        }

        fn decode(
            &mut self,
            request: PlatformDecodeRequest,
        ) -> Result<DecodeOutput, PlatformError> {
            if request.sequence != self.next_sequence
                || request.kind != self.kind
                || request.bytes.is_empty()
                || !supported_codec(request.kind, &request.codec)
                || !request.description.is_empty()
                || request.sample_rate.is_some()
                || request.channels.is_some()
                || request.coded_width.is_some()
                || request.coded_height.is_some()
            {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "decode.submit",
                    "decode request sequence, kind, codec, payload, or metadata is invalid",
                ));
            }
            let next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "decode.submit",
                    "decode request sequence overflowed",
                )
            })?;
            let output = match request.kind {
                DecodeKind::Image => decode_image(request)?,
                DecodeKind::Audio | DecodeKind::Video => decode_avfoundation(request)?,
            };
            self.next_sequence = next_sequence;
            Ok(output)
        }
    }

    fn supported_codec(kind: DecodeKind, codec: &str) -> bool {
        match kind {
            DecodeKind::Image => matches!(codec, "png" | "jpeg" | "jpg" | "webp"),
            DecodeKind::Audio => matches!(codec, "mp3" | "aac" | "opus" | "mp4" | "webm"),
            DecodeKind::Video => matches!(codec, "h264" | "vp9" | "mp4" | "webm"),
        }
    }

    fn decode_image(request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
        let result = astra_media::ImageDecodeProvider
            .decode(&astra_media::DecodeRequest {
                kind: astra_media::DecodeKind::Image,
                codec: request.codec,
                bytes: request.bytes,
                profile: "macos-release".to_string(),
            })
            .map_err(media_decode_error)?;
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
            MediaDecodeOutput::MediaSurfaceToken(_) => Err(host_error(
                "decode.submit",
                "image decoder returned an unsupported external media surface",
            )),
        }
    }

    #[allow(deprecated)]
    fn decode_avfoundation(request: PlatformDecodeRequest) -> Result<DecodeOutput, PlatformError> {
        use std::io::Write;
        use std::ptr::NonNull;

        use objc2::runtime::AnyObject;
        use objc2_av_foundation::{
            AVAsset, AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeAudio, AVMediaTypeVideo,
            AVURLAsset,
        };
        use objc2_core_video::{
            CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
            CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
            CVPixelBufferUnlockBaseAddress,
        };
        use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

        const MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;
        let suffix = format!(".{}", request.codec);
        let mut input = tempfile::Builder::new()
            .suffix(&suffix)
            .tempfile()
            .map_err(|_| {
                host_error(
                    "decode.submit",
                    "temporary media input could not be created",
                )
            })?;
        input.write_all(&request.bytes).map_err(|_| {
            host_error(
                "decode.submit",
                "temporary media input could not be written",
            )
        })?;
        let path = input.path().to_str().ok_or_else(|| {
            host_error("decode.submit", "temporary media input path is not UTF-8")
        })?;
        let path = NSString::from_str(path);
        let url = NSURL::fileURLWithPath(&path);

        unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            let asset: &AVAsset = &asset;
            let media_type = match request.kind {
                DecodeKind::Audio => AVMediaTypeAudio,
                DecodeKind::Video => AVMediaTypeVideo,
                DecodeKind::Image => unreachable!(),
            }
            .ok_or_else(|| host_error("decode.submit", "AVFoundation media type is unavailable"))?;
            let track = asset
                .tracksWithMediaType(media_type)
                .firstObject()
                .ok_or_else(|| {
                    host_error("decode.submit", "AVFoundation found no matching track")
                })?;
            let settings = if request.kind == DecodeKind::Audio {
                let keys = [
                    NSString::from_str("AVFormatIDKey"),
                    NSString::from_str("AVLinearPCMIsFloatKey"),
                    NSString::from_str("AVLinearPCMBitDepthKey"),
                    NSString::from_str("AVLinearPCMIsBigEndianKey"),
                    NSString::from_str("AVLinearPCMIsNonInterleavedKey"),
                ];
                let values = [
                    NSNumber::numberWithUnsignedInt(u32::from_be_bytes(*b"lpcm")),
                    NSNumber::numberWithBool(true),
                    NSNumber::numberWithInt(32),
                    NSNumber::numberWithBool(false),
                    NSNumber::numberWithBool(false),
                ];
                NSDictionary::from_slices(
                    &keys.iter().map(|value| &**value).collect::<Vec<_>>(),
                    &values.iter().map(|value| &**value).collect::<Vec<_>>(),
                )
            } else {
                NSDictionary::from_slices(
                    &[&*NSString::from_str("PixelFormatType")],
                    &[&*NSNumber::numberWithUnsignedInt(u32::from_be_bytes(
                        *b"BGRA",
                    ))],
                )
            };
            let settings = settings.cast_unchecked::<NSString, AnyObject>();
            let output = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                &track,
                Some(settings),
            );
            let reader = AVAssetReader::assetReaderWithAsset_error(asset)
                .map_err(|_| host_error("decode.submit", "AVAssetReader could not open input"))?;
            if !reader.canAddOutput(&output) {
                return Err(host_error(
                    "decode.submit",
                    "AVAssetReader rejected track output",
                ));
            }
            reader.addOutput(&output);
            if !reader.startReading() {
                return Err(host_error("decode.submit", "AVAssetReader could not start"));
            }
            let mut bytes = Vec::new();
            while let Some(sample) = output.copyNextSampleBuffer() {
                if request.kind == DecodeKind::Audio {
                    let block = sample.data_buffer().ok_or_else(|| {
                        host_error("decode.submit", "AVFoundation audio sample has no data")
                    })?;
                    let length = block.data_length();
                    let start = bytes.len();
                    bytes.resize(
                        start
                            .checked_add(length)
                            .filter(|size| *size <= MAX_DECODED_BYTES)
                            .ok_or_else(|| {
                                host_error("decode.submit", "decoded audio exceeds the byte limit")
                            })?,
                        0,
                    );
                    let destination = NonNull::new(bytes[start..].as_mut_ptr().cast()).unwrap();
                    if block.copy_data_bytes(0, length, destination) != 0 {
                        return Err(host_error(
                            "decode.submit",
                            "AVFoundation audio copy failed",
                        ));
                    }
                } else {
                    let image = sample.image_buffer().ok_or_else(|| {
                        host_error("decode.submit", "AVFoundation video sample has no image")
                    })?;
                    let flags = CVPixelBufferLockFlags(1);
                    if CVPixelBufferLockBaseAddress(&image, flags) != 0 {
                        return Err(host_error(
                            "decode.submit",
                            "video pixel buffer lock failed",
                        ));
                    }
                    let width = CVPixelBufferGetWidth(&image);
                    let height = CVPixelBufferGetHeight(&image);
                    let stride = CVPixelBufferGetBytesPerRow(&image);
                    let base = CVPixelBufferGetBaseAddress(&image).cast::<u8>();
                    if base.is_null() || stride < width.saturating_mul(4) {
                        let _ = CVPixelBufferUnlockBaseAddress(&image, flags);
                        return Err(host_error(
                            "decode.submit",
                            "video pixel buffer layout is invalid",
                        ));
                    }
                    let frame_bytes = width
                        .checked_mul(height)
                        .and_then(|value| value.checked_mul(4))
                        .ok_or_else(|| {
                            host_error("decode.submit", "decoded video dimensions overflow")
                        })?;
                    if bytes
                        .len()
                        .checked_add(frame_bytes)
                        .is_none_or(|size| size > MAX_DECODED_BYTES)
                    {
                        let _ = CVPixelBufferUnlockBaseAddress(&image, flags);
                        return Err(host_error(
                            "decode.submit",
                            "decoded video exceeds the byte limit",
                        ));
                    }
                    for row in 0..height {
                        let source = std::slice::from_raw_parts(base.add(row * stride), width * 4);
                        for pixel in source.chunks_exact(4) {
                            bytes.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                        }
                    }
                    let _ = CVPixelBufferUnlockBaseAddress(&image, flags);
                }
            }
            if bytes.is_empty() {
                return Err(host_error(
                    "decode.submit",
                    "AVFoundation produced no decoded samples",
                ));
            }
            Ok(DecodeOutput::CpuBuffer {
                format: if request.kind == DecodeKind::Audio {
                    "f32le"
                } else {
                    "rgba8"
                }
                .to_string(),
                hash: astra_core::Hash256::from_sha256(&bytes).to_string(),
                bytes,
            })
        }
    }

    fn media_decode_error(error: astra_media::MediaError) -> PlatformError {
        match error {
            astra_media::MediaError::Diagnostics(diagnostics) => {
                let diagnostic = diagnostics.into_iter().next();
                let mut error = PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "decode.submit",
                    diagnostic
                        .as_ref()
                        .map_or("image decode failed", |value| value.message.as_str()),
                );
                if let Some(diagnostic) = diagnostic {
                    error = error.with_field("diagnostic_code", diagnostic.code);
                }
                error
            }
            astra_media::MediaError::Message(message) => PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "decode.submit",
                message,
            ),
        }
    }

    fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
        PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
    }

    fn default_roots(package_id: &str) -> Option<super::HostRoots> {
        let project = directories::ProjectDirs::from("com", "AstraEngine", package_id)?;
        let executable = std::env::current_exe().ok()?;
        let contents = executable.parent()?.parent()?;
        Some(super::HostRoots {
            save_base: project.data_dir().join("SavedGames"),
            bundle_root: contents.join("Resources"),
        })
    }
}
