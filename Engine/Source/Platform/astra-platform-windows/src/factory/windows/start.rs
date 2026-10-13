use super::*;

pub async fn start(
    launch_profile: HostLaunchProfile,
    roots: Option<super::super::HostRoots>,
    test_null_audio: bool,
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
                test_null_audio,
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

pub async fn start_services(
    launch_profile: HostLaunchProfile,
) -> Result<PlatformHostSession, PlatformError> {
    let profile = launch_profile.require_platform()?.clone();
    if profile.platform != astra_platform::PlatformId::Windows {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "host.services.start",
            "Windows services require a Windows profile",
        ));
    }
    let command_wake = PlatformCommandWakeRegistration::default();
    let (client, backend, events) = host_channel_with_command_wake(
        HostLaunchProfile::platform(profile.clone()),
        profile.limits.command_queue_capacity,
        profile.limits.event_queue_capacity,
        command_wake.clone(),
    )?;
    let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
    thread::Builder::new()
        .name("astra-platform-windows-services".to_owned())
        .spawn(move || run_service_backend(backend, command_wake, ready_tx))
        .map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.services.start",
                "Windows service thread could not be started",
            )
        })?;
    ready_rx.recv().map_err(|_| {
        PlatformError::new(
            PlatformErrorCode::QueueClosed,
            "host.services.start",
            "Windows service thread stopped during startup",
        )
    })??;
    Ok(PlatformHostSession {
        client,
        events,
        profile: launch_profile,
    })
}

pub(super) fn run_service_backend(
    mut backend: PlatformBackendChannels,
    command_wake: PlatformCommandWakeRegistration,
    ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
) {
    let (wake_tx, wake_rx) = std_mpsc::sync_channel(1);
    let wake_sender = wake_tx.clone();
    if let Err(error) = command_wake.bind(move || {
        let _ = wake_sender.try_send(());
    }) {
        let _ = ready.send(Err(error));
        return;
    }
    let mut audio_outputs = ResourceTable::<AudioResource, AudioOutputHandle>::new("audio_output");
    let mut decode_sessions =
        ResourceTable::<DecodeResource, DecodeSessionHandle>::new("decode_session");
    if backend
        .emit_event(PlatformEvent::new(1, PlatformEventKind::Resumed))
        .is_err()
    {
        let _ = ready.send(Err(host_error(
            "host.services.start",
            "Windows service event channel is unavailable",
        )));
        return;
    }
    if ready.send(Ok(())).is_err() {
        return;
    }

    loop {
        if wake_rx.recv().is_err() {
            return;
        }
        loop {
            let command = match backend.try_next_command() {
                Ok(Some(command)) => command,
                Ok(None) => break,
                Err(_) => return,
            };
            match command {
                HostCommand::OpenAudioOutput { request, reply } => {
                    let result = AudioResource::new(request, backend.audio_wake(), false).and_then(
                        |(resource, lane, format)| {
                            let handle = audio_outputs.insert(resource)?;
                            Ok(OpenedAudioOutput {
                                handle,
                                format,
                                lane: Box::new(lane),
                                capture: None,
                            })
                        },
                    );
                    let _ = reply.send(result);
                }
                HostCommand::PauseAudio { output, reply } => {
                    let _ =
                        reply.send(audio_outputs.get_mut(output).and_then(AudioResource::pause));
                }
                HostCommand::ResumeAudio { output, reply } => {
                    let _ = reply.send(
                        audio_outputs
                            .get_mut(output)
                            .and_then(AudioResource::resume),
                    );
                }
                HostCommand::AbortAudio { output, reply }
                | HostCommand::CloseAudio { output, reply } => {
                    let _ = reply.send(audio_outputs.remove(output).map(|_| ()));
                }
                #[cfg(feature = "platform-test-driver")]
                HostCommand::InjectAudioDeviceLoss { output, reply } => {
                    let _ = reply.send(
                        audio_outputs
                            .get_mut(output)
                            .map(AudioResource::inject_device_loss),
                    );
                }
                HostCommand::OpenDecode { kind, reply } => {
                    let _ = reply.send(
                        DecodeResource::new(kind)
                            .and_then(|resource| decode_sessions.insert(resource)),
                    );
                }
                HostCommand::Decode {
                    session,
                    request,
                    reply,
                } => {
                    if let Ok(resource) = decode_sessions.get(session) {
                        resource.submit(request, reply);
                    } else {
                        let _ = reply.send(Err(host_error(
                            "decode.submit",
                            "decode session handle is stale or unknown",
                        )));
                    }
                }
                HostCommand::CloseDecode { session, reply } => {
                    let _ = reply.send(decode_sessions.remove(session).map(|_| ()));
                }
                HostCommand::Shutdown { reply } => {
                    drop(decode_sessions);
                    drop(audio_outputs);
                    let _ = reply.send(Ok(()));
                    return;
                }
                unsupported => {
                    let operation = unsupported.operation();
                    let _ = unsupported.reply_error(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        operation,
                        "Windows service host only supports audio and decode commands",
                    ));
                }
            }
        }
    }
}

pub(super) fn run_backend(
    backend: PlatformBackendChannels,
    command_wake: PlatformCommandWakeRegistration,
    ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
    profile: PlatformHostProfile,
    roots: Option<super::super::HostRoots>,
    test_null_audio: bool,
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
        test_null_audio,
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

pub(super) struct SingleInstanceGuard(pub(super) windows::Win32::Foundation::HANDLE);

unsafe impl Send for SingleInstanceGuard {}

impl SingleInstanceGuard {
    pub(super) fn acquire(profile: &PlatformHostProfile) -> Result<Self, PlatformError> {
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
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(wide.as_ptr())) }.map_err(|_| {
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

pub(super) struct WindowsHostApp {
    pub(super) test_null_audio: bool,
    pub(super) backend: PlatformBackendChannels,
    pub(super) ready: Option<std_mpsc::SyncSender<Result<(), PlatformError>>>,
    pub(super) windows: ResourceTable<Arc<Window>, WindowHandle>,
    pub(super) window_ids: BTreeMap<WindowId, WindowHandle>,
    pub(super) accessibility: BTreeMap<WindowId, WindowsAccessibilityBridge>,
    pub(super) surfaces: ResourceTable<SurfaceResource, SurfaceHandle>,
    pub(super) surface_windows: BTreeMap<SurfaceHandle, WindowId>,
    pub(super) audio_outputs: ResourceTable<AudioResource, AudioOutputHandle>,
    pub(super) audio_completion_tx: std_mpsc::Sender<AudioCompletion>,
    pub(super) audio_completion_rx: std_mpsc::Receiver<AudioCompletion>,
    pub(super) pending_audio_opens: usize,
    pub(super) decode_sessions: ResourceTable<DecodeResource, DecodeSessionHandle>,
    pub(super) save_store: AtomicSaveStore,
    pub(super) package_cache: VerifiedPackageCache,
    pub(super) package_source_policies: Vec<astra_platform::PackageSourcePolicy>,
    pub(super) package_id: String,
    pub(super) package_cache_policy: astra_platform::PackageCachePolicy,
    pub(super) package_completion_tx: std_mpsc::Sender<PackageCompletion>,
    pub(super) package_completion_rx: std_mpsc::Receiver<PackageCompletion>,
    pub(super) pending_package_opens: usize,
    pub(super) save_transactions: ResourceTable<SaveTransaction, SaveTransactionHandle>,
    pub(super) bundle_root: std::path::PathBuf,
    pub(super) package_sources: ResourceTable<PackageSourceResource, PackageSourceHandle>,
    pub(super) event_sequence: u64,
    pub(super) gamepad_events: std_mpsc::Receiver<Vec<PlatformEventKind>>,
    pub(super) gamepad_stop: Arc<AtomicBool>,
    pub(super) gamepad_thread: Option<thread::JoinHandle<()>>,
    pub(super) event_loop_proxy: EventLoopProxy<()>,
}

pub(super) struct PackageHostConfig {
    pub(super) source_policies: Vec<astra_platform::PackageSourcePolicy>,
    pub(super) package_id: String,
    pub(super) cache_policy: astra_platform::PackageCachePolicy,
    pub(super) bundle_root: std::path::PathBuf,
}
