use super::*;

impl WindowsHostApp {
    pub(super) fn process_commands(&mut self, event_loop: &ActiveEventLoop) {
        const MAX_COMMANDS_PER_TURN: usize = 64;
        let mut processed = 0usize;
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
                            crate::window_placement::fit_to_work_area(&window)?;
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
                    let semantics = frame.semantics.clone();
                    let result = self
                        .surfaces
                        .get_mut(surface)
                        .and_then(|surface| surface.present_scene(frame));
                    let result = result.and_then(|()| {
                        let Some(semantics) = semantics.as_ref() else {
                            return Ok(());
                        };
                        let window_id = self.surface_windows.get(&surface).ok_or_else(|| {
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
                    self.start_audio_output_open(request, reply);
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
                    let result = self.audio_outputs.remove(output).map(|_| ());
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
                        .and_then(|_| {
                            if self.pending_audio_opens == 0 {
                                Ok(())
                            } else {
                                Err(PlatformError::new(
                                    PlatformErrorCode::InvalidState,
                                    "host.shutdown",
                                    "audio output requests are still in flight",
                                ))
                            }
                        })
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
                #[cfg(not(feature = "platform-test-driver"))]
                #[allow(unreachable_patterns)]
                command => {
                    let _ = command.reply_error(host_error(
                        operation,
                        "platform test command is unavailable in this host build",
                    ));
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
            processed += 1;
            // Every successful client submission already wakes this event
            // loop through `command_wake`. Yield after presentation so OS
            // messages run, but do not enqueue an additional empty wake
            // per frame. Only a deliberately truncated command batch must
            // reschedule itself because those commands are already queued.
            if operation == "surface.present_scene" {
                break;
            }
            if processed == MAX_COMMANDS_PER_TURN {
                if operation != "host.shutdown" && self.event_loop_proxy.send_event(()).is_err() {
                    tracing::error!(
                        event = "platform.windows.command_reschedule.failed",
                        diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                        "Windows platform host could not reschedule queued commands"
                    );
                    event_loop.exit();
                }
                break;
            }
        }
    }

    pub(super) fn open_user_authorized_package(
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
}
