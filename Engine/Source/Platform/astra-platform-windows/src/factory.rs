use astra_platform::{HostLaunchProfile, HostStartFuture, PlatformHostFactory};

#[cfg(not(target_os = "windows"))]
use astra_platform::{PlatformError, PlatformErrorCode, PlatformId};

#[derive(Debug, Clone, Default)]
pub struct WindowsPlatformFactory {
    #[cfg(target_os = "windows")]
    roots: Option<HostRoots>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct HostRoots {
    save_base: std::path::PathBuf,
    bundle_root: std::path::PathBuf,
}

pub fn factory() -> WindowsPlatformFactory {
    WindowsPlatformFactory::default()
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
pub fn factory_with_test_roots(
    save_base: impl AsRef<std::path::Path>,
    bundle_root: impl AsRef<std::path::Path>,
) -> WindowsPlatformFactory {
    WindowsPlatformFactory {
        roots: Some(HostRoots {
            save_base: save_base.as_ref().to_path_buf(),
            bundle_root: bundle_root.as_ref().to_path_buf(),
        }),
    }
}

impl PlatformHostFactory for WindowsPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_os = "windows")]
        {
            Box::pin(crate::factory::windows::start(profile, self.roots.clone()))
        }
        #[cfg(not(target_os = "windows"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Windows host can only start on Windows",
                )
                .with_field("platform", PlatformId::Windows.as_str()))
            })
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        collections::BTreeMap,
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
            mpsc as std_mpsc, Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use crate::accessibility::WindowsAccessibilityBridge;
    use astra_core::Hash256;
    use astra_media::{DecodeOutput as MediaDecodeOutput, DecodeProvider};
    use astra_platform::{
        host_channel_with_command_wake, AudioDeviceFormat, AudioMeter, AudioOutputHandle,
        AudioOutputRequest, AudioOutputStatus, AudioPacket, AudioWakeRegistration, CapturedFrame,
        DecodeKind, DecodeOutput, DecodeSessionHandle, HostCommand, HostLaunchProfile, InputState,
        PackageSourceHandle, PackageSourceRequest, PlatformBackendChannels,
        PlatformCommandWakeRegistration, PlatformDecodeRequest, PlatformError, PlatformErrorCode,
        PlatformEvent, PlatformEventKind, PlatformHostProfile, PlatformHostSession, PointerButton,
        SaveTransactionHandle, SurfaceHandle, TouchPhase, WindowHandle,
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
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
        platform::windows::EventLoopBuilderExtWindows,
        window::{Window, WindowAttributes, WindowId},
    };

    pub async fn start(
        launch_profile: HostLaunchProfile,
        roots: Option<super::HostRoots>,
    ) -> Result<PlatformHostSession, PlatformError> {
        let profile = launch_profile.require_platform()?.clone();
        if profile.platform != astra_platform::PlatformId::Windows {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "host.start",
                "Windows factory requires a Windows profile",
            ));
        }
        let command_capacity = profile.limits.command_queue_capacity;
        let event_capacity = profile.limits.event_queue_capacity;
        let instance_guard = SingleInstanceGuard::acquire(&profile)?;
        let command_wake = PlatformCommandWakeRegistration::default();
        let (client, backend, events) = host_channel_with_command_wake(
            HostLaunchProfile::platform(profile.clone()),
            command_capacity,
            event_capacity,
            command_wake.clone(),
        )?;
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let backend_profile = profile.clone();
        thread::Builder::new()
            .name("astra-platform-windows".to_string())
            .spawn(move || {
                run_backend(
                    backend,
                    command_wake,
                    ready_tx,
                    backend_profile,
                    roots,
                    instance_guard,
                )
            })
            .map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "host.start",
                    "Windows platform thread could not be started",
                )
            })?;
        ready_rx.recv().map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::QueueClosed,
                "host.start",
                "Windows platform thread stopped during startup",
            )
        })??;
        Ok(PlatformHostSession {
            client,
            events,
            profile: launch_profile,
        })
    }

    fn run_backend(
        backend: PlatformBackendChannels,
        command_wake: PlatformCommandWakeRegistration,
        ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
        profile: PlatformHostProfile,
        roots: Option<super::HostRoots>,
        _instance_guard: SingleInstanceGuard,
    ) {
        let roots = match roots.or_else(default_roots) {
            Some(roots) => roots,
            None => {
                let _ = ready.send(Err(host_error(
                    "host.start",
                    "Windows save or bundle root is unavailable",
                )));
                return;
            }
        };
        let save_store = match AtomicSaveStore::new(&roots.save_base, &profile.package_id) {
            Ok(store) => store,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        let package_cache = match VerifiedPackageCache::platform_cache_root(&profile.package_id)
            .and_then(|root| VerifiedPackageCache::open(root, profile.package_cache.clone()))
        {
            Ok(cache) => cache,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        let event_loop = match EventLoop::builder().with_any_thread(true).build() {
            Ok(event_loop) => event_loop,
            Err(_) => {
                let _ = ready.send(Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "host.start",
                    "Windows event loop could not be created",
                )));
                return;
            }
        };
        let event_loop_proxy = event_loop.create_proxy();
        let command_proxy = event_loop_proxy.clone();
        if let Err(error) = command_wake.bind(move || {
            if command_proxy.send_event(()).is_err() {
                tracing::error!(
                    event = "platform.windows.command_wake.failed",
                    diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                    "Windows platform command could not wake the event loop"
                );
            }
        }) {
            let _ = ready.send(Err(error));
            return;
        }
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = match WindowsHostApp::new(
            backend,
            ready,
            save_store,
            package_cache,
            PackageHostConfig {
                source_policies: profile.package_sources.clone(),
                package_id: profile.package_id.clone(),
                cache_policy: profile.package_cache.clone(),
                bundle_root: roots.bundle_root,
            },
            event_loop_proxy,
        ) {
            Ok(app) => app,
            Err(_) => return,
        };
        if let Err(error) = event_loop.run_app(&mut app) {
            tracing::error!(
                event = "platform.windows.event_loop.failed",
                diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP",
                error = %error,
                "Windows platform event loop failed"
            );
        }
    }

    struct SingleInstanceGuard(windows::Win32::Foundation::HANDLE);

    unsafe impl Send for SingleInstanceGuard {}

    impl SingleInstanceGuard {
        fn acquire(profile: &PlatformHostProfile) -> Result<Self, PlatformError> {
            use astra_core::Hash256;
            use windows::{
                core::PCWSTR,
                Win32::{
                    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
                    System::Threading::CreateMutexW,
                },
            };

            let identity = format!("{}\n{}\n{}", profile.package_id, profile.target, profile.id);
            let hash = Hash256::from_sha256(identity.as_bytes()).to_string();
            let name = format!(
                "Local\\AstraEngine.Player.{}",
                hash.trim_start_matches("sha256:")
            );
            let wide = name
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let handle =
                unsafe { CreateMutexW(None, false, PCWSTR(wide.as_ptr())) }.map_err(|_| {
                    host_error(
                        "host.instance.acquire",
                        "single-instance mutex could not be created",
                    )
                })?;
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                let _ = unsafe { CloseHandle(handle) };
                return Err(PlatformError::new(
                    PlatformErrorCode::AlreadyInUse,
                    "host.instance.acquire",
                    "the same game target and profile is already running",
                ));
            }
            Ok(Self(handle))
        }
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            use windows::Win32::Foundation::CloseHandle;
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    struct WindowsHostApp {
        backend: PlatformBackendChannels,
        ready: Option<std_mpsc::SyncSender<Result<(), PlatformError>>>,
        windows: ResourceTable<Arc<Window>, WindowHandle>,
        window_ids: BTreeMap<WindowId, WindowHandle>,
        accessibility: BTreeMap<WindowId, WindowsAccessibilityBridge>,
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
        gamepad_events: std_mpsc::Receiver<Vec<PlatformEventKind>>,
        gamepad_stop: Arc<AtomicBool>,
        gamepad_thread: Option<thread::JoinHandle<()>>,
        event_loop_proxy: EventLoopProxy<()>,
    }

    struct PackageHostConfig {
        source_policies: Vec<astra_platform::PackageSourcePolicy>,
        package_id: String,
        cache_policy: astra_platform::PackageCachePolicy,
        bundle_root: std::path::PathBuf,
    }

    impl WindowsHostApp {
        fn new(
            backend: PlatformBackendChannels,
            ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
            save_store: AtomicSaveStore,
            package_cache: VerifiedPackageCache,
            package: PackageHostConfig,
            event_loop_proxy: EventLoopProxy<()>,
        ) -> Result<Self, PlatformError> {
            let gamepads = gilrs::Gilrs::new().map_err(|_| {
                let error = host_error(
                    "input.gamepad.open",
                    "Windows Gaming Input initialization failed",
                );
                let _ = ready.send(Err(error.clone()));
                error
            })?;
            let gamepad_mapper = astra_platform_common::GamepadMapper::new(0.2)?;
            let (gamepad_tx, gamepad_events) = std_mpsc::sync_channel(32);
            let gamepad_stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&gamepad_stop);
            let worker_proxy = event_loop_proxy.clone();
            let gamepad_thread = thread::Builder::new()
                .name("astra-windows-gamepad".to_owned())
                .spawn(move || {
                    windows_gamepad_worker(
                        gamepads,
                        gamepad_mapper,
                        worker_stop,
                        worker_proxy,
                        gamepad_tx,
                    );
                })
                .map_err(|_| {
                    host_error(
                        "input.gamepad.worker",
                        "Windows Gaming Input worker could not start",
                    )
                })?;
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
                gamepad_events,
                gamepad_stop,
                gamepad_thread: Some(gamepad_thread),
                event_loop_proxy,
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
                    event = "platform.windows.event.emit_failed",
                    diagnostic_code = ?error.code,
                    operation = %error.operation,
                    "Windows platform event could not be emitted"
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
                let operation = command.operation();
                let command_started = Instant::now();
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
                                    WindowsAccessibilityBridge::new(
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
                                        "accessibility.windows.update",
                                        "surface is not bound to a window",
                                    )
                                })?;
                            self.accessibility
                                .get_mut(window_id)
                                .ok_or_else(|| {
                                    host_error(
                                        "accessibility.windows.update",
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
                            self.accessibility.remove(&window.id());
                            self.window_ids.remove(&window.id());
                        });
                        let _ = reply.send(result);
                    }
                    HostCommand::OpenAudioOutput { request, reply } => {
                        let result = AudioResource::new(request, self.backend.audio_wake())
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
                                provider: "windows.wasapi".to_string(),
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
                                provider: "windows.wasapi".to_string(),
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
                                provider: "windows.wasapi".to_string(),
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
                        if let Ok(resource) = self.decode_sessions.get(session) {
                            resource.submit(request, reply);
                        } else {
                            let _ = reply.send(Err(host_error(
                                "decode.submit",
                                "decode session handle is stale or unknown",
                            )));
                        }
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
                    HostCommand::ListSaves { reply } => {
                        let _ = reply.send(self.save_store.list());
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
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(
                        event = "platform.windows.command.completed",
                        operation,
                        duration_ns =
                            u64::try_from(command_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                        "Windows platform host completed one command"
                    );
                }
            }
        }

        fn open_user_authorized_package(
            &mut self,
            expected_hash: &str,
        ) -> Result<CachedPackageSource, PlatformError> {
            let file = pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .add_filter("Astra package", &["astrapkg"])
                    .pick_file(),
            )
            .ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::Cancelled,
                    "package.open_user_authorized",
                    "user cancelled package selection",
                )
            })?;
            let bytes = pollster::block_on(file.read());
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
            let event_loop_proxy = self.event_loop_proxy.clone();
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
                if completion_tx
                    .send(PackageCompletion { reply, result })
                    .is_ok()
                    && event_loop_proxy.send_event(()).is_err()
                {
                    tracing::error!(
                        event = "platform.windows.package_completion_wake.failed",
                        diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                        "Windows package completion could not wake the event loop"
                    );
                }
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

    impl ApplicationHandler for WindowsHostApp {
        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
            let sequence = self.next_sequence();
            let result = self
                .backend
                .emit_event(PlatformEvent::new(sequence, PlatformEventKind::Resumed));
            if let Some(ready) = self.ready.take() {
                let _ = ready.send(result);
            }
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
            self.process_commands(event_loop);
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
            let accessibility_actions = self
                .accessibility
                .values_mut()
                .flat_map(WindowsAccessibilityBridge::drain_actions)
                .collect::<Vec<_>>();
            for request in accessibility_actions {
                self.emit(PlatformEventKind::AccessibilityAction {
                    window: request.window,
                    semantic_id: request.semantic_id,
                    action: request.action,
                    value: request.value,
                });
            }
            while let Ok(events) = self.gamepad_events.try_recv() {
                for event in events {
                    self.emit(event);
                }
            }
            // The gamepad worker owns the backend and wakes this event loop
            // when it has a bounded batch. There is intentionally no render-
            // or input-driven timer here; gilrs' internal 250 ms wait is only
            // a low-frequency discovery fallback on platforms without a
            // hotplug handle.
            event_loop.set_control_flow(ControlFlow::Wait);
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

    impl Drop for WindowsHostApp {
        fn drop(&mut self) {
            self.gamepad_stop.store(true, Ordering::Release);
            self.gamepad_events = std_mpsc::sync_channel(0).1;
            if let Some(worker) = self.gamepad_thread.take() {
                if worker.join().is_err() {
                    tracing::error!(
                        event = "platform.windows.gamepad.worker_panic",
                        diagnostic_code = "ASTRA_PLATFORM_GAMEPAD_WORKER_PANIC",
                        "Windows Gaming Input worker panicked during shutdown"
                    );
                }
            }
        }
    }

    fn windows_gamepad_worker(
        mut gamepads: gilrs::Gilrs,
        mut gamepad_mapper: astra_platform_common::GamepadMapper,
        stop: Arc<AtomicBool>,
        event_loop_proxy: EventLoopProxy<()>,
        tx: std_mpsc::SyncSender<Vec<PlatformEventKind>>,
    ) {
        while !stop.load(Ordering::Acquire) {
            let Some(event) = gamepads.next_event_blocking(Some(Duration::from_millis(250))) else {
                continue;
            };
            let Some(raw_event) = raw_gamepad_event(event) else {
                continue;
            };
            let mapped = match gamepad_mapper.apply_checked(raw_event) {
                Ok(events) => events,
                Err(error) => {
                    tracing::warn!(
                        event = "platform.windows.gamepad.invalid_event",
                        diagnostic_code = ?error.code,
                        operation = %error.operation,
                        "Windows Gaming Input event was rejected"
                    );
                    continue;
                }
            };
            if mapped.is_empty() || tx.send(mapped).is_err() {
                return;
            }
            if event_loop_proxy.send_event(()).is_err() {
                tracing::debug!(
                    event = "platform.windows.gamepad.wake.closed",
                    diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                    "Windows event loop was already closed after a gamepad batch"
                );
                return;
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
        let instance = astra_platform_common::native_wgpu_instance()?;
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
        audio_wake: AudioWakeRegistration,
    }

    fn preferred_audio_output_format() -> Result<astra_platform::AudioOutputFormat, PlatformError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                host_error(
                    "audio.format",
                    "WASAPI default output device is unavailable",
                )
            })?;
        const PRODUCT_SAMPLE_RATE: u32 = 48_000;
        const PRODUCT_CHANNELS: u16 = 2;
        let product_format_supported = device
            .supported_output_configs()
            .map_err(|_| host_error("audio.format", "WASAPI output format enumeration failed"))?
            .any(|range| {
                range.channels() == PRODUCT_CHANNELS
                    && range.min_sample_rate() <= PRODUCT_SAMPLE_RATE
                    && range.max_sample_rate() >= PRODUCT_SAMPLE_RATE
                    && sample_format_rank(range.sample_format()).is_some()
            });
        if product_format_supported {
            tracing::info!(
                event = "platform.windows.audio.format.selected",
                sample_rate = PRODUCT_SAMPLE_RATE,
                channels = PRODUCT_CHANNELS,
                selection = "product_canonical",
                "selected a WASAPI format compatible with the product mixer"
            );
            return Ok(astra_platform::AudioOutputFormat {
                sample_rate: PRODUCT_SAMPLE_RATE,
                channels: PRODUCT_CHANNELS,
            });
        }
        let supported = device.default_output_config().map_err(|_| {
            host_error(
                "audio.format",
                "WASAPI default output config is unavailable",
            )
        })?;
        tracing::warn!(
            event = "platform.windows.audio.format.canonical_unavailable",
            sample_rate = supported.sample_rate(),
            channels = supported.channels(),
            "WASAPI device does not expose the canonical product mixer format"
        );
        Ok(astra_platform::AudioOutputFormat {
            sample_rate: supported.sample_rate(),
            channels: supported.channels(),
        })
    }

    impl AudioResource {
        fn new(
            request: AudioOutputRequest,
            audio_wake: AudioWakeRegistration,
        ) -> Result<Self, PlatformError> {
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
                host_error("audio.open", "WASAPI default output device is unavailable")
            })?;
            let requested_rate = request.sample_rate;
            let supported = device
                .supported_output_configs()
                .map_err(|_| host_error("audio.open", "WASAPI output format enumeration failed"))?
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
                        "WASAPI has no exact supported format for the requested rate and channels",
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
                    let wake = audio_wake.clone();
                    let error_wake = audio_wake.clone();
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [f32], _| {
                            let _ = fill_f32(output, &mut consumer, &meter);
                            wake.notify();
                        },
                        move |stream_error_value| {
                            set_stream_error(&error, stream_error_value);
                            error_wake.notify();
                        },
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let wake = audio_wake.clone();
                    let error_wake = audio_wake.clone();
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [i16], _| {
                            let _ = fill_i16(output, &mut consumer, &meter);
                            wake.notify();
                        },
                        move |stream_error_value| {
                            set_stream_error(&error, stream_error_value);
                            error_wake.notify();
                        },
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let wake = audio_wake.clone();
                    let error_wake = audio_wake.clone();
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [u16], _| {
                            let _ = fill_u16(output, &mut consumer, &meter);
                            wake.notify();
                        },
                        move |stream_error_value| {
                            set_stream_error(&error, stream_error_value);
                            error_wake.notify();
                        },
                        None,
                    )
                }
                _ => {
                    return Err(host_error(
                        "audio.open",
                        "WASAPI sample format is unsupported",
                    ));
                }
            }
            .map_err(|_| host_error("audio.open", "WASAPI output stream creation failed"))?;
            if !request.start_paused {
                stream.play().map_err(|_| {
                    host_error("audio.open", "WASAPI output stream could not start")
                })?;
            }
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
                paused: request.start_paused,
                audio_wake,
            })
        }

        fn submit(&mut self, packet: AudioPacket) -> Result<Vec<f32>, PlatformError> {
            if self.stream_error.load(Ordering::Acquire) {
                return Err(PlatformError::new(
                    PlatformErrorCode::DeviceLost,
                    "audio.submit",
                    "WASAPI output stream reported a device error",
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
            Ok(packet.samples)
        }

        fn pause(&mut self) -> Result<(), PlatformError> {
            if self.paused {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.pause",
                    "WASAPI output is already paused",
                ));
            }
            self.stream
                .pause()
                .map_err(|_| host_error("audio.pause", "WASAPI output could not pause"))?;
            self.paused = true;
            Ok(())
        }

        fn resume(&mut self) -> Result<(), PlatformError> {
            if !self.paused {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.resume",
                    "WASAPI output is not paused",
                ));
            }
            self.stream
                .play()
                .map_err(|_| host_error("audio.resume", "WASAPI output could not resume"))?;
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
                start_paused: false,
            };
            let deadline = Instant::now() + request.drain_timeout(self.submitted_samples);
            let mut observed_wake = 0;
            loop {
                if self.stream_error.load(Ordering::Acquire) {
                    return Err(PlatformError::new(
                        PlatformErrorCode::DeviceLost,
                        "audio.drain",
                        "WASAPI output stream reported a device error",
                    ));
                }
                if self.queue_telemetry.snapshot().sample_count >= self.submitted_samples {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(host_error("audio.drain", "WASAPI output drain timed out"));
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                observed_wake = self
                    .audio_wake
                    .wait_timeout(observed_wake, remaining)
                    .ok_or_else(|| host_error("audio.drain", "WASAPI output drain timed out"))?;
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
                    "WASAPI output stream reported a device error",
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
                    "WASAPI queue telemetry is inconsistent with submitted audio",
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
                    "WASAPI default output device is unavailable",
                )
            })?;
        let config = device.default_output_config().map_err(|_| {
            host_error(
                "audio.query_device_format",
                "WASAPI default output format is unavailable",
            )
        })?;
        if sample_format_rank(config.sample_format()).is_none()
            || config.sample_rate() == 0
            || config.channels() == 0
        {
            return Err(host_error(
                "audio.query_device_format",
                "WASAPI default output format is unsupported",
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

    fn fill_f32(
        output: &mut [f32],
        consumer: &mut astra_platform_common::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) -> bool {
        meter.begin_callback();
        let filled = consumer.pop_samples(output);
        for sample in &output[..filled] {
            meter.record(*sample);
        }
        output[filled..].fill(0.0);
        if filled != output.len() {
            consumer.record_underflow();
        }
        filled != output.len()
    }

    fn fill_i16(
        output: &mut [i16],
        consumer: &mut astra_platform_common::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) -> bool {
        meter.begin_callback();
        let mut scratch = [0.0_f32; 1024];
        let mut written = 0;
        while written < output.len() {
            let requested = scratch.len().min(output.len() - written);
            let filled = consumer.pop_samples(&mut scratch[..requested]);
            for (target, sample) in output[written..written + filled]
                .iter_mut()
                .zip(&scratch[..filled])
            {
                meter.record(*sample);
                *target = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            }
            written += filled;
            if filled != requested {
                break;
            }
        }
        output[written..].fill(0);
        if written != output.len() {
            consumer.record_underflow();
        }
        written != output.len()
    }

    fn fill_u16(
        output: &mut [u16],
        consumer: &mut astra_platform_common::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) -> bool {
        meter.begin_callback();
        let mut scratch = [0.0_f32; 1024];
        let mut written = 0;
        while written < output.len() {
            let requested = scratch.len().min(output.len() - written);
            let filled = consumer.pop_samples(&mut scratch[..requested]);
            for (target, sample) in output[written..written + filled]
                .iter_mut()
                .zip(&scratch[..filled])
            {
                meter.record(*sample);
                *target = ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * f32::from(u16::MAX)) as u16;
            }
            written += filled;
            if filled != requested {
                break;
            }
        }
        output[written..].fill(u16::MAX / 2);
        if written != output.len() {
            consumer.record_underflow();
        }
        written != output.len()
    }

    fn set_stream_error(error: &AtomicBool, _value: cpal::StreamError) {
        error.store(true, Ordering::Release);
    }

    struct DecodeResource {
        requests: Option<std_mpsc::SyncSender<DecodeWork>>,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }

    struct DecodeWork {
        request: PlatformDecodeRequest,
        reply: oneshot::Sender<Result<DecodeOutput, PlatformError>>,
    }

    struct DecodeWorkerState {
        kind: DecodeKind,
        provider: astra_media::WindowsMediaFoundationDecodeProvider,
        next_sequence: u64,
        video_stream: Option<WindowsVideoStreamState>,
        audio_stream: Option<astra_media::WindowsAudioStreamDecoder>,
    }

    struct WindowsVideoStreamState {
        cursor: astra_media::DecodedVideoStreamCursor,
        decoder: astra_media::WindowsVideoStreamDecoder,
        pending: Option<astra_media::DecodedVideoFrame>,
        frame_count: u64,
        decoded_byte_count: u64,
        end_emitted: bool,
    }

    impl DecodeResource {
        fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
            let (request_tx, request_rx) = std_mpsc::sync_channel::<DecodeWork>(2);
            let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let worker = thread::Builder::new()
                .name("astra-windows-decode".to_owned())
                .spawn(move || {
                    let mut state = match DecodeWorkerState::new(kind) {
                        Ok(state) => {
                            let _ = ready_tx.send(Ok(()));
                            state
                        }
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                            return;
                        }
                    };
                    while !worker_stop.load(Ordering::Acquire) {
                        let Ok(work) = request_rx.recv() else {
                            return;
                        };
                        let result = state.decode(work.request);
                        let _ = work.reply.send(result);
                    }
                })
                .map_err(|_| host_error("decode.open", "decode worker could not start"))?;
            match ready_rx.recv() {
                Ok(Ok(())) => Ok(Self {
                    requests: Some(request_tx),
                    stop,
                    worker: Some(worker),
                }),
                Ok(Err(error)) => {
                    drop(request_tx);
                    let _ = worker.join();
                    Err(error)
                }
                Err(_) => {
                    drop(request_tx);
                    let _ = worker.join();
                    Err(host_error(
                        "decode.open",
                        "decode worker stopped before initialization",
                    ))
                }
            }
        }

        fn submit(
            &self,
            request: PlatformDecodeRequest,
            reply: oneshot::Sender<Result<DecodeOutput, PlatformError>>,
        ) {
            let Some(requests) = self.requests.as_ref() else {
                let _ = reply.send(Err(host_error("decode.submit", "decode worker is closed")));
                return;
            };
            match requests.try_send(DecodeWork { request, reply }) {
                Ok(()) => {}
                Err(std_mpsc::TrySendError::Full(work)) => {
                    let _ = work.reply.send(Err(host_error(
                        "decode.submit",
                        "decode worker queue is full",
                    )));
                }
                Err(std_mpsc::TrySendError::Disconnected(work)) => {
                    let _ = work
                        .reply
                        .send(Err(host_error("decode.submit", "decode worker is closed")));
                }
            }
        }
    }

    impl Drop for DecodeResource {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            // Close the sender before joining so a worker waiting for the next
            // request observes channel closure immediately.
            self.requests.take();
            if let Some(worker) = self.worker.take() {
                if worker.join().is_err() {
                    tracing::error!(
                        event = "platform.windows.decode.worker_panic",
                        diagnostic_code = "ASTRA_PLATFORM_DECODE_WORKER_PANIC",
                        "Windows decode worker panicked during shutdown"
                    );
                }
            }
        }
    }

    impl DecodeWorkerState {
        fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
            let provider = astra_media::WindowsMediaFoundationDecodeProvider::probe()
                .map_err(|_| host_error("decode.open", "WMF provider initialization failed"))?;
            Ok(Self {
                kind,
                provider,
                next_sequence: 1,
                video_stream: None,
                audio_stream: None,
            })
        }

        fn decode(
            &mut self,
            request: PlatformDecodeRequest,
        ) -> Result<DecodeOutput, PlatformError> {
            let capability = self.provider.capability();
            if request.sequence != self.next_sequence || request.kind != self.kind {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "decode.submit",
                    "decode request sequence or kind is invalid",
                ));
            }
            let next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "decode.submit",
                    "decode request sequence overflowed",
                )
            })?;
            let kind = match request.kind {
                DecodeKind::Image => astra_media::DecodeKind::Image,
                DecodeKind::Audio => astra_media::DecodeKind::Audio,
                DecodeKind::Video => astra_media::DecodeKind::Video,
            };
            let output = match request.stream_action {
                astra_platform::DecodeStreamAction::OneShot => {
                    if request.bytes.is_empty()
                        || !capability
                            .codecs
                            .iter()
                            .any(|codec| codec == &request.codec)
                        || !request.description.is_empty()
                        || request.sample_rate.is_some()
                        || request.channels.is_some()
                        || request.coded_width.is_some()
                        || request.coded_height.is_some()
                        || self.video_stream.is_some()
                        || self.audio_stream.is_some()
                    {
                        return Err(PlatformError::new(
                            PlatformErrorCode::InvalidState,
                            "decode.submit",
                            "one-shot decode request is invalid while a stream is active",
                        ));
                    }
                    let result = self
                        .provider
                        .decode(&astra_media::DecodeRequest {
                            kind,
                            codec: request.codec,
                            bytes: request.bytes,
                            profile: "desktop-release".to_string(),
                        })
                        .map_err(media_decode_error)?;
                    match result.output {
                        MediaDecodeOutput::CpuBuffer {
                            bytes,
                            format,
                            hash,
                        } => DecodeOutput::CpuBuffer {
                            format,
                            bytes,
                            hash: hash.to_string(),
                        },
                        MediaDecodeOutput::MediaSurfaceToken(_) => {
                            return Err(host_error(
                                "decode.submit",
                                "WMF returned an unsupported external media surface",
                            ));
                        }
                    }
                }
                astra_platform::DecodeStreamAction::Start => {
                    if request.bytes.is_empty()
                        || !matches!(request.kind, DecodeKind::Video | DecodeKind::Audio)
                        || !capability
                            .codecs
                            .iter()
                            .any(|codec| codec == &request.codec)
                        || !request.description.is_empty()
                        || request.sample_rate.is_some()
                        || request.channels.is_some()
                        || request.coded_width.is_some()
                        || request.coded_height.is_some()
                        || self.video_stream.is_some()
                        || self.audio_stream.is_some()
                    {
                        return Err(PlatformError::new(
                            PlatformErrorCode::InvalidState,
                            "decode.stream.start",
                            "stream start request is invalid",
                        ));
                    }
                    match request.kind {
                        DecodeKind::Video => {
                            let mut decoder = astra_media::open_windows_video_stream(
                                &request.bytes,
                                60 * 60 * 4,
                                512 * 1024 * 1024,
                            )
                            .map_err(media_decode_error)?;
                            let pending = decoder
                                .next_frame()
                                .map_err(media_decode_error)?
                                .ok_or_else(|| {
                                    host_error(
                                        "decode.stream.start",
                                        "video stream produced no frames",
                                    )
                                })?;
                            let cursor = astra_media::DecodedVideoStreamCursor {
                                schema: astra_media::DECODED_VIDEO_STREAM_CURSOR_SCHEMA.into(),
                                source_hash: Hash256::from_sha256(&request.bytes),
                                width: pending.width,
                                height: pending.height,
                                max_frames: 60 * 60 * 4,
                                max_decoded_byte_count: 512 * 1024 * 1024,
                            };
                            let bytes = cursor.encode().map_err(media_decode_error)?;
                            self.video_stream = Some(WindowsVideoStreamState {
                                cursor,
                                decoder,
                                pending: Some(pending),
                                frame_count: 0,
                                decoded_byte_count: 0,
                                end_emitted: false,
                            });
                            DecodeOutput::CpuBuffer {
                                format: format!(
                                    "postcard:{}",
                                    astra_media::DECODED_VIDEO_STREAM_CURSOR_SCHEMA
                                ),
                                hash: Hash256::from_sha256(&bytes).to_string(),
                                bytes,
                            }
                        }
                        DecodeKind::Audio => {
                            let mut decoder = astra_media::open_windows_audio_stream(
                                &request.bytes,
                                64 * 1024 * 1024,
                            )
                            .map_err(media_decode_error)?;
                            let chunk = decoder
                                .next_chunk()
                                .map_err(media_decode_error)?
                                .ok_or_else(|| {
                                    host_error(
                                        "decode.stream.start",
                                        "audio stream produced no samples",
                                    )
                                })?;
                            let format =
                                format!("pcm_s16le:{}:{}", chunk.sample_rate, chunk.channels);
                            let hash = Hash256::from_sha256(&chunk.pcm_s16le).to_string();
                            self.audio_stream = Some(decoder);
                            DecodeOutput::CpuBuffer {
                                format,
                                bytes: chunk.pcm_s16le,
                                hash,
                            }
                        }
                        DecodeKind::Image => unreachable!(),
                    }
                }
                astra_platform::DecodeStreamAction::Next => {
                    if !request.bytes.is_empty()
                        || !request.description.is_empty()
                        || request.sample_rate.is_some()
                        || request.channels.is_some()
                        || request.coded_width.is_some()
                        || request.coded_height.is_some()
                    {
                        return Err(PlatformError::new(
                            PlatformErrorCode::InvalidState,
                            "decode.stream.next",
                            "stream next request must not carry payload or metadata",
                        ));
                    }
                    match request.kind {
                        DecodeKind::Video => next_windows_video_frame(&mut self.video_stream)?,
                        DecodeKind::Audio => next_windows_audio_chunk(&mut self.audio_stream)?,
                        DecodeKind::Image => {
                            return Err(host_error(
                                "decode.stream.next",
                                "image streams are not supported",
                            ));
                        }
                    }
                }
            };
            self.next_sequence = next_sequence;
            Ok(output)
        }
    }

    fn next_windows_video_frame(
        stream: &mut Option<WindowsVideoStreamState>,
    ) -> Result<DecodeOutput, PlatformError> {
        let state = stream
            .as_mut()
            .ok_or_else(|| host_error("decode.stream.next", "video stream has not been started"))?;
        let frame = match state.pending.take() {
            Some(frame) => Some(frame),
            None => state.decoder.next_frame().map_err(media_decode_error)?,
        };
        if let Some(frame) = frame {
            state.frame_count = state
                .frame_count
                .checked_add(1)
                .ok_or_else(|| host_error("decode.stream.next", "video frame count overflowed"))?;
            state.decoded_byte_count = state
                .decoded_byte_count
                .checked_add(frame.bgra8.len() as u64)
                .ok_or_else(|| host_error("decode.stream.next", "decoded byte count overflowed"))?;
            if state.frame_count > state.cursor.max_frames
                || state.decoded_byte_count > state.cursor.max_decoded_byte_count
            {
                return Err(host_error(
                    "decode.stream.next",
                    "decoded video stream exceeds its profile-bound budget",
                ));
            }
            // The decoder already owns a validated BGRA8 allocation.  Keep
            // the frame metadata in the small format descriptor and move the
            // pixel Vec directly through the PlatformHost response; encoding
            // the whole frame as postcard here caused a second full-payload
            // allocation on every streaming frame.
            let format = frame.cpu_buffer_format();
            let bytes = frame.bgra8;
            tracing::trace!(
                event = "platform.windows.decode.video_frame.moved",
                sequence = frame.sequence,
                width = frame.width,
                height = frame.height,
                bytes = bytes.len(),
                transfer = "owned_cpu_buffer",
                "WMF streaming frame moved through PlatformHost"
            );
            return Ok(DecodeOutput::CpuBuffer {
                format,
                hash: frame.content_hash.to_string(),
                bytes,
            });
        }
        if state.end_emitted {
            return Err(host_error(
                "decode.stream.next",
                "video stream end was already emitted",
            ));
        }
        state.end_emitted = true;
        let end = astra_media::DecodedVideoStreamCursorEnd {
            schema: astra_media::DECODED_VIDEO_STREAM_CURSOR_END_SCHEMA.into(),
            source_hash: state.cursor.source_hash,
            frame_count: state.frame_count,
            decoded_byte_count: state.decoded_byte_count,
        };
        end.validate_against(&state.cursor)
            .map_err(media_decode_error)?;
        let bytes = end.encode(&state.cursor).map_err(media_decode_error)?;
        Ok(DecodeOutput::CpuBuffer {
            format: format!(
                "postcard:{}",
                astra_media::DECODED_VIDEO_STREAM_CURSOR_END_SCHEMA
            ),
            hash: Hash256::from_sha256(&bytes).to_string(),
            bytes,
        })
    }

    fn next_windows_audio_chunk(
        stream: &mut Option<astra_media::WindowsAudioStreamDecoder>,
    ) -> Result<DecodeOutput, PlatformError> {
        let decoder = stream
            .as_mut()
            .ok_or_else(|| host_error("decode.stream.next", "audio stream has not been started"))?;
        let chunk = decoder
            .next_chunk()
            .map_err(media_decode_error)?
            .ok_or_else(|| {
                host_error("decode.stream.next", "audio stream reached end of stream").with_field(
                    "diagnostic_code",
                    astra_platform::DECODE_STREAM_EOS_DIAGNOSTIC,
                )
            })?;
        Ok(DecodeOutput::CpuBuffer {
            format: format!("pcm_s16le:{}:{}", chunk.sample_rate, chunk.channels),
            hash: Hash256::from_sha256(&chunk.pcm_s16le).to_string(),
            bytes: chunk.pcm_s16le,
        })
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
                        .map_or("WMF decode failed", |value| value.message.as_str()),
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

    fn default_roots() -> Option<super::HostRoots> {
        Some(super::HostRoots {
            save_base: saved_games_root().ok()?,
            bundle_root: std::env::current_exe().ok()?.parent()?.to_path_buf(),
        })
    }

    fn saved_games_root() -> Result<std::path::PathBuf, PlatformError> {
        use std::ffi::c_void;
        use windows::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{FOLDERID_SavedGames, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
        };
        unsafe {
            let path = SHGetKnownFolderPath(&FOLDERID_SavedGames, KF_FLAG_DEFAULT, None)
                .map_err(|_| host_error("save.store.open", "Saved Games folder is unavailable"))?;
            let root = path
                .to_string()
                .map_err(|_| host_error("save.store.open", "Saved Games path is invalid"))?;
            CoTaskMemFree(Some(path.as_ptr() as *const c_void));
            Ok(std::path::PathBuf::from(root))
        }
    }
}
