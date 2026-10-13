use super::*;

impl MacosHostApp {
    pub(super) fn new(
        backend: PlatformBackendChannels,
        ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
        command_wake: PlatformCommandWakeRegistration,
        gamepad_proxy: EventLoopProxy<()>,
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
        let gamepad_emitter = backend.event_emitter();
        let gamepad_proxy = gamepad_proxy.clone();
        let gamepad_stop = Arc::new(AtomicBool::new(false));
        let gamepad_stop_thread = Arc::clone(&gamepad_stop);
        let gamepad_thread = thread::Builder::new()
            .name("astra-macos-gamepad".to_string())
            .spawn(move || {
                let mut gamepads = gamepads;
                let mut gamepad_mapper = gamepad_mapper;
                while !gamepad_stop_thread.load(Ordering::Acquire) {
                    let Some(event) =
                        gamepads.next_event_blocking(Some(Duration::from_millis(250)))
                    else {
                        continue;
                    };
                    let Some(event) = raw_gamepad_event(event) else {
                        continue;
                    };
                    match gamepad_mapper.apply_checked(event) {
                        Ok(events) => {
                            let emitted = !events.is_empty();
                            for event in events {
                                if let Err(error) = gamepad_emitter.emit(event) {
                                    tracing::warn!(
                                        event = "platform.macos.gamepad.event_dropped",
                                        diagnostic_code = ?error.code,
                                        operation = %error.operation,
                                        "macOS Gaming Input event could not be emitted"
                                    );
                                }
                            }
                            if emitted && gamepad_proxy.send_event(()).is_err() {
                                tracing::debug!(
                                    event = "platform.macos.gamepad.wake.closed",
                                    diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                                    "macOS event loop was already closed after a gamepad batch"
                                );
                                return;
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
            })
            .map_err(|_| {
                host_error(
                    "input.gamepad.worker",
                    "macOS gamepad worker could not start",
                )
            })?;
        let (package_completion_tx, package_completion_rx) = std_mpsc::channel();
        Ok(Self {
            backend,
            command_wake,
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
            gamepad_stop,
            gamepad_thread: Some(gamepad_thread),
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
                event = "platform.macos.event.emit_failed",
                diagnostic_code = ?error.code,
                operation = %error.operation,
                "macOS platform event could not be emitted"
            );
        }
    }
}
