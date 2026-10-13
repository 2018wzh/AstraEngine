use super::*;

pub(in super::super) struct BackendSetup {
    pub(super) backend: PlatformBackendChannels,
    pub(super) profile: PlatformHostProfile,
    pub(super) roots: Option<super::super::HostRoots>,
    pub(super) command_wake: PlatformCommandWakeRegistration,
    pub(super) _instance_guard: SingleInstanceGuard,
}

pub struct MacosHostRunner {
    pub(super) event_loop: EventLoop<()>,
    pub(super) app: MacosRunnerApp,
}

impl MacosHostRunner {
    pub(in super::super) fn new(
    ) -> Result<(Self, super::super::MacosPlatformFactory), PlatformError> {
        let pending = Arc::new(std::sync::Mutex::new(None));
        let event_loop = EventLoop::builder().build().map_err(|_| {
            host_error(
                "host.runner.create",
                "macOS event loop could not be created",
            )
        })?;
        event_loop.set_control_flow(ControlFlow::Wait);
        let start_wake = event_loop.create_proxy();
        Ok((
            Self {
                event_loop,
                app: MacosRunnerApp {
                    pending: pending.clone(),
                    wake: start_wake.clone(),
                    host: None,
                },
            },
            super::super::MacosPlatformFactory {
                roots: None,
                pending,
                start_wake: Some(start_wake),
            },
        ))
    }

    pub fn run<F>(&mut self, future: F) -> Result<F::Output, PlatformError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        // Tokio and winit cannot both own the macOS main-thread event
        // loop. Run the async player on a worker and let winit block in
        // `pump_app_events(None)`. Host commands and completion are
        // explicit EventLoopProxy user events; there is no fixed-period
        // bridge timer keeping an idle player hot.
        let proxy = self.event_loop.create_proxy();
        let (result_tx, result_rx) = std_mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("astra-macos-player-async".to_owned())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| {
                            host_error("host.runner.run", "Tokio runtime could not be created")
                        })?;
                    Ok::<_, PlatformError>(runtime.block_on(future))
                }))
                .unwrap_or_else(|_| {
                    Err(host_error(
                        "host.runner.run",
                        "macOS async player worker panicked",
                    ))
                });
                let _ = result_tx.send(result);
                let _ = proxy.send_event(());
            })
            .map_err(|_| host_error("host.runner.run", "Tokio player worker could not start"))?;

        let result = loop {
            self.event_loop.pump_app_events(None, &mut self.app);
            match result_rx.try_recv() {
                Ok(result) => break result,
                Err(std_mpsc::TryRecvError::Empty) => continue,
                Err(std_mpsc::TryRecvError::Disconnected) => {
                    break Err(host_error(
                        "host.runner.run",
                        "Tokio player worker exited without a result",
                    ));
                }
            }
        };
        let _ = worker.join();
        result
    }
}

pub(super) struct MacosRunnerApp {
    pub(super) wake: EventLoopProxy<()>,
    pub(super) pending: Arc<std::sync::Mutex<Option<BackendSetup>>>,
    pub(super) host: Option<MacosHostApp>,
}

impl MacosRunnerApp {
    pub(super) fn initialize(&mut self) -> Result<(), PlatformError> {
        let Some(setup) = self
            .pending
            .lock()
            .map_err(|_| host_error("host.start", "macOS pending host state is poisoned"))?
            .take()
        else {
            return Ok(());
        };
        let proxy = self.wake.clone();
        let gamepad_proxy = proxy.clone();
        setup.command_wake.bind(move || {
            if proxy.send_event(()).is_err() {
                tracing::debug!(
                    event = "platform.macos.command_wake.closed",
                    "macOS event loop was already closed when a host wake arrived"
                );
            }
        })?;
        let roots = setup
            .roots
            .or_else(|| default_roots(&setup.profile.package_id))
            .ok_or_else(|| host_error("host.start", "macOS save or bundle root is unavailable"))?;
        let save_store = AtomicSaveStore::new(&roots.save_base, &setup.profile.package_id)?;
        let package_cache = VerifiedPackageCache::platform_cache_root(&setup.profile.package_id)
            .and_then(|root| {
                VerifiedPackageCache::open(root, setup.profile.package_cache.clone())
            })?;
        let (ready, _ready_rx) = std_mpsc::sync_channel(1);
        self.host = Some(MacosHostApp::new(
            setup.backend,
            ready,
            setup.command_wake,
            gamepad_proxy,
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: ()) {
        if self.initialize().is_err() {
            event_loop.exit();
            return;
        }
        if let Some(host) = self.host.as_mut() {
            host.process_package_completions();
            host.process_commands(event_loop);
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
    roots: Option<super::super::HostRoots>,
    pending: Arc<std::sync::Mutex<Option<BackendSetup>>>,
    start_wake: Option<EventLoopProxy<()>>,
) -> Result<PlatformHostSession, PlatformError> {
    let Some(start_wake) = start_wake else {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidState,
            "host.start",
            "macOS host requires main_thread_host and MacosHostRunner",
        ));
    };
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
    let command_wake = PlatformCommandWakeRegistration::default();
    let (client, backend, events) = host_channel_with_command_wake(
        HostLaunchProfile::platform(profile.clone()),
        command_capacity,
        event_capacity,
        command_wake.clone(),
    )?;
    let mut pending_slot = pending
        .lock()
        .map_err(|_| host_error("host.start", "macOS pending host state is poisoned"))?;
    if pending_slot.is_some() {
        return Err(PlatformError::new(
            PlatformErrorCode::AlreadyInUse,
            "host.start",
            "macOS factory already has a pending host",
        ));
    }
    *pending_slot = Some(BackendSetup {
        backend,
        profile,
        roots,
        command_wake,
        _instance_guard: instance_guard,
    });
    drop(pending_slot);
    start_wake.send_event(()).map_err(|_| {
        host_error(
            "host.start",
            "macOS event loop closed before host initialization",
        )
    })?;
    Ok(PlatformHostSession {
        client,
        events,
        profile: launch_profile,
    })
}

pub(super) struct SingleInstanceGuard {
    pub(super) _lock: std::fs::File,
}

impl SingleInstanceGuard {
    pub(super) fn acquire(profile: &PlatformHostProfile) -> Result<Self, PlatformError> {
        use std::{fs::OpenOptions, os::fd::AsRawFd};

        use astra_core::Hash256;

        let identity = format!("{}\n{}\n{}", profile.package_id, profile.target, profile.id);
        let hash = Hash256::from_sha256(identity.as_bytes()).to_string();
        let lock_dir = directories::ProjectDirs::from("com", "AstraEngine", &profile.package_id)
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

pub(super) struct MacosHostApp {
    pub(super) backend: PlatformBackendChannels,
    pub(super) command_wake: PlatformCommandWakeRegistration,
    pub(super) ready: Option<std_mpsc::SyncSender<Result<(), PlatformError>>>,
    pub(super) windows: ResourceTable<Arc<Window>, WindowHandle>,
    pub(super) window_ids: BTreeMap<WindowId, WindowHandle>,
    pub(super) accessibility: BTreeMap<WindowId, MacosAccessibilityBridge>,
    pub(super) surfaces: ResourceTable<SurfaceResource, SurfaceHandle>,
    pub(super) surface_windows: BTreeMap<SurfaceHandle, WindowId>,
    pub(super) audio_outputs: ResourceTable<AudioResource, AudioOutputHandle>,
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
    pub(super) gamepad_stop: Arc<AtomicBool>,
    pub(super) gamepad_thread: Option<thread::JoinHandle<()>>,
}

pub(super) struct PackageHostConfig {
    pub(super) source_policies: Vec<astra_platform::PackageSourcePolicy>,
    pub(super) package_id: String,
    pub(super) cache_policy: astra_platform::PackageCachePolicy,
    pub(super) bundle_root: std::path::PathBuf,
}
