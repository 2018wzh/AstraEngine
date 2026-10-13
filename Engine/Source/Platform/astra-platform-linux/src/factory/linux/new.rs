use super::*;

impl LinuxHostApp {
    pub(super) fn new(
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
                "Linux Gaming Input initialization failed",
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
            .name("astra-linux-gamepad".to_owned())
            .spawn(move || {
                linux_gamepad_worker(
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
                    "Linux Gaming Input worker could not start",
                )
            })?;
        let (package_completion_tx, package_completion_rx) = std_mpsc::channel();
        Ok(Self {
            backend,
            ready: Some(ready),
            windows: ResourceTable::new("window"),
            window_ids: BTreeMap::new(),
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
            package_workers: Vec::new(),
            event_loop_proxy,
            save_transactions: ResourceTable::new("save_transaction"),
            bundle_root: package.bundle_root,
            package_sources: ResourceTable::new("package_source"),
            event_sequence: 0,
            gamepad_events,
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
                event = "platform.linux.event.emit_failed",
                diagnostic_code = ?error.code,
                operation = %error.operation,
                "Linux platform event could not be emitted"
            );
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
                        .and_then(|window| create_surface(window, request.width, request.height))
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
                        for event in
                            astra_platform_common::wgpu_recovery_events("wgpu_hardware", recovered)
                        {
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
                    let result = if frame.semantics.is_some() {
                        Err(host_error(
                            "accessibility.linux.update",
                            "Linux accessibility is not available in this profile",
                        ))
                    } else {
                        self.surfaces
                            .get_mut(surface)
                            .and_then(|surface| surface.present_scene(frame))
                    };
                    if result
                        .as_ref()
                        .is_err_and(|error| error.code == PlatformErrorCode::ContextLost)
                    {
                        let recovered = self
                            .surfaces
                            .get_mut(surface)
                            .and_then(|surface| surface.reconfigure_after_loss())
                            .is_ok();
                        for event in
                            astra_platform_common::wgpu_recovery_events("wgpu_hardware", recovered)
                        {
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
                    });
                    let _ = reply.send(result);
                }
                HostCommand::OpenAudioOutput { request, reply } => {
                    let result = AudioResource::new(request, self.backend.audio_wake()).and_then(
                        |(resource, lane, format)| {
                            self.audio_outputs
                                .insert(resource)
                                .map(|handle| OpenedAudioOutput {
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
                HostCommand::InjectAudioDeviceLoss { output, reply } => {
                    #[cfg(feature = "platform-test-driver")]
                    let result = self
                        .audio_outputs
                        .get_mut(output)
                        .map(AudioResource::inject_device_loss);
                    #[cfg(not(feature = "platform-test-driver"))]
                    let result = self.audio_outputs.get(output).and_then(|_| {
                        Err(PlatformError::new(
                            PlatformErrorCode::InvalidState,
                            "audio.test.inject_device_loss",
                            "audio fault injection requires platform-test-driver",
                        ))
                    });
                    let _ = reply.send(result);
                }
                HostCommand::CloseAudio { output, reply } => {
                    let result = self.audio_outputs.remove(output).map(|_| ());
                    let _ = reply.send(result);
                }
                HostCommand::OpenDecode { kind, reply } => {
                    let result = DecodeSession::new(kind)
                        .and_then(|resource| self.decode_sessions.insert(resource));
                    if let Err(Ok(handle)) = reply.send(result) {
                        // Cancelled open must not retain a worker or a live handle.
                        let _ = self.decode_sessions.remove(handle);
                    }
                }
                HostCommand::Decode {
                    session,
                    request,
                    reply,
                } => match self.decode_sessions.get(session) {
                    Ok(resource) => resource.submit(request, reply),
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                },
                HostCommand::CloseDecode { session, reply } => {
                    let result = self
                        .decode_sessions
                        .remove(session)
                        .and_then(DecodeSession::close);
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
                    deliver_package_result(reply, result, &mut self.package_sources);
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
}
