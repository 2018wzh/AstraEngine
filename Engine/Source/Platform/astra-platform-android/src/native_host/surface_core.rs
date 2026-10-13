use super::*;

pub(super) type SurfaceCore = astra_platform_common::WgpuPresentationCore;

pub(in super::super) fn create_host(
    app: AndroidApp,
    launch_profile: HostLaunchProfile,
    bundled_package: Arc<Vec<u8>>,
    bundled_package_hash: String,
    player: super::super::AndroidPlayerEntry,
    proxy: winit::event_loop::EventLoopProxy<()>,
) -> Result<AndroidHostApp, PlatformError> {
    let profile = launch_profile.require_platform()?.clone();
    if profile.platform != astra_platform::PlatformId::Android {
        return Err(host_error(
            "host.start",
            "Android activity requires an Android profile",
        ));
    }
    astra_platform::validate_host_profile(&profile)?;
    let data_root = app.internal_data_path().ok_or_else(|| {
        host_error(
            "host.start",
            "Android app-private storage directory is unavailable",
        )
    })?;
    let save_store = AtomicSaveStore::new(data_root.join("saves"), &profile.package_id)?;
    let imports_root = data_root.join("files").join("package-imports");
    let package_cache_root = data_root.join("cache").join("packages");
    let decode_root = data_root.join("cache").join("mediacodec");
    crate::decode::prepare_scratch_root(&decode_root)?;
    let max_decode_output_bytes = profile
        .limits
        .max_frame_bytes
        .checked_mul(256)
        .ok_or_else(|| host_error("host.start", "Android decode output budget overflows"))?;
    let command_wake = PlatformCommandWakeRegistration::default();
    let (client, backend, events) = host_channel_with_command_wake(
        launch_profile.clone(),
        profile.limits.command_queue_capacity,
        profile.limits.event_queue_capacity,
        command_wake.clone(),
    )?;
    let session = PlatformHostSession {
        client,
        events,
        profile: launch_profile,
    };
    command_wake.bind(move || {
        let _ = proxy.send_event(());
    })?;
    let (player_result_tx, player_result_rx) = std_mpsc::sync_channel(1);
    let (package_completion_tx, package_completion_rx) = std_mpsc::channel();
    Ok(AndroidHostApp {
        backend,
        player: Some(Box::new(move || {
            let result = player(session);
            let _ = player_result_tx.send(result);
        })),
        player_thread: None,
        player_result_rx,
        player_result: None,
        app,
        resumed: false,
        windows: ResourceTable::new("window"),
        window_ids: BTreeMap::new(),
        surfaces: ResourceTable::new("surface"),
        audio_outputs: ResourceTable::new("audio_output"),
        decoders: ResourceTable::new("decode_session"),
        decode_root,
        max_decode_output_bytes,
        accessibility: None,
        accessibility_action_capacity: profile.limits.event_queue_capacity,
        save_store,
        save_transactions: ResourceTable::new("save_transaction"),
        package_sources: ResourceTable::new("package_source"),
        package_source_policies: profile.package_sources.clone(),
        package_cache_policy: profile.package_cache.clone(),
        package_cache_root,
        package_completion_tx,
        package_completion_rx,
        pending_package_opens: 0,
        bundled_package,
        bundled_package_hash,
        imports_root,
        max_package_entry_bytes: profile.package_cache.max_entry_bytes,
        event_sequence: 0,
        first_frame: true,
    })
}

pub(super) type PlayerStart = Box<dyn FnOnce() + Send>;

pub(in super::super) struct AndroidHostApp {
    pub(super) backend: PlatformBackendChannels,
    pub(super) player: Option<PlayerStart>,
    pub(super) player_thread: Option<thread::JoinHandle<()>>,
    pub(super) player_result_rx: std_mpsc::Receiver<Result<(), PlatformError>>,
    pub(in super::super) player_result: Option<Result<(), PlatformError>>,
    pub(super) app: AndroidApp,
    pub(super) resumed: bool,
    pub(super) windows: ResourceTable<Arc<Window>, WindowHandle>,
    pub(super) window_ids: BTreeMap<WindowId, WindowHandle>,
    pub(super) surfaces: ResourceTable<SurfaceSlot, SurfaceHandle>,
    pub(super) audio_outputs: ResourceTable<AndroidAudioResource, AudioOutputHandle>,
    pub(super) decoders: ResourceTable<AndroidDecodeWorker, DecodeSessionHandle>,
    pub(super) decode_root: std::path::PathBuf,
    pub(super) max_decode_output_bytes: usize,
    pub(super) accessibility: Option<AndroidAccessibilityBridge>,
    pub(super) accessibility_action_capacity: usize,
    pub(super) save_store: AtomicSaveStore,
    pub(super) save_transactions: ResourceTable<SaveTransaction, SaveTransactionHandle>,
    pub(super) package_sources: ResourceTable<PackageSourceResource, PackageSourceHandle>,
    pub(super) package_source_policies: Vec<PackageSourcePolicy>,
    pub(super) package_cache_policy: PackageCachePolicy,
    pub(super) package_cache_root: std::path::PathBuf,
    pub(super) package_completion_tx: std_mpsc::Sender<PackageCompletion>,
    pub(super) package_completion_rx: std_mpsc::Receiver<PackageCompletion>,
    pub(super) pending_package_opens: usize,
    pub(super) bundled_package: Arc<Vec<u8>>,
    pub(super) bundled_package_hash: String,
    pub(super) imports_root: std::path::PathBuf,
    pub(super) max_package_entry_bytes: u64,
    pub(super) event_sequence: u64,
    pub(super) first_frame: bool,
}

pub(super) struct SurfaceSlot {
    pub(super) window: WindowHandle,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) core: Option<SurfaceCore>,
}

pub(super) struct MemoryPackageSource {
    pub(super) bytes: Arc<Vec<u8>>,
}

pub(super) enum PackageSourceResource {
    Memory(MemoryPackageSource),
    Cached(CachedPackageSource),
}

impl PackageSourceResource {
    pub(super) fn read_range(
        &mut self,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, PlatformError> {
        match self {
            Self::Memory(source) => source.read_range(offset, length),
            Self::Cached(source) => source.read_range(offset, length),
        }
    }
}

pub(super) struct PackageCompletion {
    pub(super) reply: tokio::sync::oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    pub(super) result: Result<CachedPackageSource, PlatformError>,
}

impl MemoryPackageSource {
    pub(super) fn read_range(&self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        let offset = usize::try_from(offset)
            .map_err(|_| host_error("package.read_range", "package offset is too large"))?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| host_error("package.read_range", "package range overflows"))?;
        let range = self.bytes.get(offset..end).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.read_range",
                "package range is outside the verified source",
            )
        })?;
        Ok(range.to_vec())
    }
}

impl AndroidHostApp {
    pub(in super::super) fn finish(self) -> Result<(), PlatformError> {
        let Self {
            backend,
            player_thread,
            player_result,
            player_result_rx,
            ..
        } = self;
        // Closing the receiver releases outstanding client awaits before joining.
        drop(backend);
        if let Some(thread) = player_thread {
            thread
                .join()
                .map_err(|_| host_error("player.shutdown", "Android Player worker panicked"))?;
        }
        player_result
            .or_else(|| player_result_rx.try_recv().ok())
            .unwrap_or(Ok(()))
    }

    pub(super) fn next_sequence(&mut self) -> u64 {
        self.event_sequence = self.event_sequence.saturating_add(1);
        self.event_sequence
    }

    pub(super) fn emit(&mut self, kind: PlatformEventKind) {
        let event = PlatformEvent::new(self.next_sequence(), kind);
        if let Err(error) = self.backend.emit_event(event) {
            tracing::error!(
                event = "platform.android.event.emit_failed",
                diagnostic_code = ?error.code,
                operation = %error.operation,
                "Android platform event could not be emitted"
            );
        }
    }

    pub(super) fn create_surface_core(
        &self,
        slot: &SurfaceSlot,
    ) -> Result<SurfaceCore, PlatformError> {
        let window = self.windows.get(slot.window)?.clone();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|_| host_error("surface.create", "Android Vulkan surface creation failed"))?;
        pollster::block_on(SurfaceCore::new(
            instance,
            surface,
            slot.width,
            slot.height,
            true,
        ))
    }

    pub(super) fn restore_surfaces(&mut self) -> Result<(), PlatformError> {
        let handles = self.surfaces.handles().collect::<Vec<_>>();
        for handle in handles {
            let window = self.windows.get(self.surfaces.get(handle)?.window)?.clone();
            if let Some(core) = self.surfaces.get_mut(handle)?.core.as_mut() {
                core.attach_surface(window)?;
            }
        }
        Ok(())
    }

    pub(super) fn suspend_surfaces(&mut self) {
        let handles = self.surfaces.handles().collect::<Vec<_>>();
        for handle in handles {
            if let Ok(slot) = self.surfaces.get_mut(handle) {
                if let Some(core) = slot.core.as_mut() {
                    core.detach_surface();
                }
            }
        }
    }

    pub(super) fn surface_core_mut(
        &mut self,
        handle: SurfaceHandle,
    ) -> Result<&mut SurfaceCore, PlatformError> {
        self.surfaces.get_mut(handle)?.core.as_mut().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::ContextLost,
                "surface.access",
                "Android surface is suspended",
            )
        })
    }

    pub(super) fn recover_surface_error(
        &mut self,
        surface: SurfaceHandle,
        result: &Result<(), PlatformError>,
    ) {
        if result
            .as_ref()
            .is_err_and(|error| error.code == PlatformErrorCode::ContextLost)
        {
            let recovered = self
                .surface_core_mut(surface)
                .and_then(SurfaceCore::reconfigure_after_loss)
                .is_ok();
            for event in astra_platform_common::wgpu_recovery_events("wgpu_vulkan", recovered) {
                self.emit(event);
            }
        } else if result
            .as_ref()
            .is_err_and(|error| error.code == PlatformErrorCode::DeviceLost)
        {
            let recovered = self
                .surface_core_mut(surface)
                .and_then(|core| pollster::block_on(core.recover_device()).map(|_| ()))
                .is_ok();
            for event in
                astra_platform_common::wgpu_device_recovery_events("wgpu_vulkan", recovered)
            {
                self.emit(event);
            }
        }
    }

    pub(super) fn process_commands(&mut self, event_loop: &ActiveEventLoop) {
        loop {
            let command = match self.backend.try_next_command() {
                Ok(Some(command)) => command,
                Ok(None) => break,
                Err(_) => {
                    event_loop.exit();
                    break;
                }
            };
            self.process_command(event_loop, command);
        }
    }
}
