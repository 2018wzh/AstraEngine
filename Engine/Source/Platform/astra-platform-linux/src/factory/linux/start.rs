use super::*;

pub async fn start(
    launch_profile: HostLaunchProfile,
    roots: Option<super::super::HostRoots>,
) -> Result<PlatformHostSession, PlatformError> {
    let profile = launch_profile.require_platform()?.clone();
    if profile.platform != astra_platform::PlatformId::Linux {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "host.start",
            "Linux factory requires a Linux profile",
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
        .name("astra-platform-linux".to_string())
        .spawn(move || {
            run_backend(
                backend,
                ready_tx,
                backend_profile,
                roots,
                instance_guard,
                command_wake,
            )
        })
        .map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.start",
                "Linux platform thread could not be started",
            )
        })?;
    ready_rx.recv().map_err(|_| {
        PlatformError::new(
            PlatformErrorCode::QueueClosed,
            "host.start",
            "Linux platform thread stopped during startup",
        )
    })??;
    Ok(PlatformHostSession {
        client,
        events,
        profile: launch_profile,
    })
}

pub(super) fn run_backend(
    backend: PlatformBackendChannels,
    ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
    profile: PlatformHostProfile,
    roots: Option<super::super::HostRoots>,
    _instance_guard: SingleInstanceGuard,
    command_wake: PlatformCommandWakeRegistration,
) {
    let roots = match roots.or_else(|| default_roots(&profile.package_id)) {
        Some(roots) => roots,
        None => {
            let _ = ready.send(Err(host_error(
                "host.start",
                "Linux save or bundle root is unavailable",
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
    let event_loop = match EventLoop::builder()
        .with_wayland()
        .with_any_thread(true)
        .build()
    {
        Ok(event_loop) => event_loop,
        Err(_) => {
            let _ = ready.send(Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.start",
                "Linux event loop could not be created",
            )));
            return;
        }
    };
    let proxy = event_loop.create_proxy();
    let command_proxy = proxy.clone();
    if let Err(error) = command_wake.bind(move || {
        let _ = command_proxy.send_event(());
    }) {
        let _ = ready.send(Err(error));
        return;
    }
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = match LinuxHostApp::new(
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
        proxy,
    ) {
        Ok(app) => app,
        Err(_) => return,
    };
    if let Err(error) = event_loop.run_app(&mut app) {
        tracing::error!(
            event = "platform.linux.event_loop.failed",
            diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP",
            error = %error,
            "Linux platform event loop failed"
        );
    }
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
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| {
                host_error(
                    "host.instance.acquire",
                    "XDG_RUNTIME_DIR is required for the Linux player lock",
                )
            })?;
        let lock_dir = runtime_dir.join("astra");
        std::fs::create_dir_all(&lock_dir).map_err(|_| {
            host_error(
                "host.instance.acquire",
                "Linux player lock directory could not be created",
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
                    "Linux player lock could not be opened",
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
                "Linux player lock could not be acquired",
            ));
        }
        Ok(Self { _lock: lock })
    }
}

pub(super) struct LinuxHostApp {
    pub(super) backend: PlatformBackendChannels,
    pub(super) ready: Option<std_mpsc::SyncSender<Result<(), PlatformError>>>,
    pub(super) windows: ResourceTable<Arc<Window>, WindowHandle>,
    pub(super) window_ids: BTreeMap<WindowId, WindowHandle>,
    pub(super) surfaces: ResourceTable<SurfaceResource, SurfaceHandle>,
    pub(super) surface_windows: BTreeMap<SurfaceHandle, WindowId>,
    pub(super) audio_outputs: ResourceTable<AudioResource, AudioOutputHandle>,
    pub(super) decode_sessions: ResourceTable<DecodeSession, DecodeSessionHandle>,
    pub(super) save_store: AtomicSaveStore,
    pub(super) package_cache: VerifiedPackageCache,
    pub(super) package_source_policies: Vec<astra_platform::PackageSourcePolicy>,
    pub(super) package_id: String,
    pub(super) package_cache_policy: astra_platform::PackageCachePolicy,
    pub(super) package_completion_tx: std_mpsc::Sender<PackageCompletion>,
    pub(super) package_completion_rx: std_mpsc::Receiver<PackageCompletion>,
    pub(super) pending_package_opens: usize,
    pub(super) package_workers: Vec<PackageWorker>,
    pub(super) event_loop_proxy: EventLoopProxy<()>,
    pub(super) save_transactions: ResourceTable<SaveTransaction, SaveTransactionHandle>,
    pub(super) bundle_root: std::path::PathBuf,
    pub(super) package_sources: ResourceTable<PackageSourceResource, PackageSourceHandle>,
    pub(super) event_sequence: u64,
    pub(super) gamepad_events: std_mpsc::Receiver<Vec<PlatformEventKind>>,
    pub(super) gamepad_stop: Arc<AtomicBool>,
    pub(super) gamepad_thread: Option<thread::JoinHandle<()>>,
}

pub(super) struct PackageHostConfig {
    pub(super) source_policies: Vec<astra_platform::PackageSourcePolicy>,
    pub(super) package_id: String,
    pub(super) cache_policy: astra_platform::PackageCachePolicy,
    pub(super) bundle_root: std::path::PathBuf,
}
