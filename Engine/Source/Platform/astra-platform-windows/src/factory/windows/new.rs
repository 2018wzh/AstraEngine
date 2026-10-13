use super::*;

impl WindowsHostApp {
    pub(super) fn new(
        backend: PlatformBackendChannels,
        ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
        save_store: AtomicSaveStore,
        package_cache: VerifiedPackageCache,
        package: PackageHostConfig,
        event_loop_proxy: EventLoopProxy<()>,
        test_null_audio: bool,
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
        let (audio_completion_tx, audio_completion_rx) = std_mpsc::channel();
        Ok(Self {
            test_null_audio,
            backend,
            ready: Some(ready),
            windows: ResourceTable::new("window"),
            window_ids: BTreeMap::new(),
            accessibility: BTreeMap::new(),
            surfaces: ResourceTable::new("surface"),
            surface_windows: BTreeMap::new(),
            audio_outputs: ResourceTable::new("audio_output"),
            audio_completion_tx,
            audio_completion_rx,
            pending_audio_opens: 0,
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

    pub(super) fn next_sequence(&mut self) -> u64 {
        self.event_sequence += 1;
        self.event_sequence
    }

    pub(super) fn emit(&mut self, kind: PlatformEventKind) {
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
}
