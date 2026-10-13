use super::*;

impl AndroidHostApp {
    pub(super) fn process_command(&mut self, event_loop: &ActiveEventLoop, command: HostCommand) {
        match command {
            HostCommand::CreateWindow { request, reply } => {
                let result = if !self.resumed {
                    Err(host_error(
                        "window.create",
                        "Android Activity must be resumed before window creation",
                    ))
                } else if !self.window_ids.is_empty() {
                    Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "window.create",
                        "Android Activity owns exactly one main window",
                    ))
                } else {
                    event_loop
                        .create_window(
                            WindowAttributes::default()
                                .with_title(request.title)
                                .with_visible(request.visible)
                                .with_inner_size(winit::dpi::PhysicalSize::new(
                                    request.width,
                                    request.height,
                                )),
                        )
                        .map_err(|_| host_error("window.create", "Android window creation failed"))
                        .and_then(|window| {
                            let window = Arc::new(window);
                            window.set_ime_allowed(false);
                            let id = window.id();
                            let handle = self.windows.insert(window)?;
                            self.window_ids.insert(id, handle);
                            match AndroidAccessibilityBridge::new(
                                &self.app,
                                handle,
                                self.accessibility_action_capacity,
                            ) {
                                Ok(bridge) => {
                                    self.accessibility = Some(bridge);
                                    Ok(handle)
                                }
                                Err(error) => {
                                    self.window_ids.remove(&id);
                                    let _ = self.windows.remove(handle);
                                    Err(error)
                                }
                            }
                        })
                };
                let _ = reply.send(result);
            }
            HostCommand::CreateSurface { request, reply } => {
                let result = (|| {
                    if request.width == 0 || request.height == 0 {
                        return Err(host_error(
                            "surface.create",
                            "Android surface dimensions must be non-zero",
                        ));
                    }
                    let mut slot = SurfaceSlot {
                        window: request.window,
                        width: request.width,
                        height: request.height,
                        core: None,
                    };
                    slot.core = Some(self.create_surface_core(&slot)?);
                    self.surfaces.insert(slot)
                })();
                let _ = reply.send(result);
            }
            HostCommand::PresentRgba {
                surface,
                frame,
                reply,
            } => {
                let result = self
                    .surface_core_mut(surface)
                    .and_then(|core| core.present(frame));
                self.recover_surface_error(surface, &result);
                let _ = reply.send(result);
            }
            HostCommand::PresentScene {
                surface,
                frame,
                reply,
            } => {
                let semantics = frame.semantics.clone();
                let result = self
                    .surface_core_mut(surface)
                    .and_then(|core| core.present_scene(frame))
                    .and_then(|()| {
                        let Some(semantics) = semantics.as_ref() else {
                            return Ok(());
                        };
                        self.accessibility
                            .as_mut()
                            .ok_or_else(|| {
                                host_error(
                                    "accessibility.android.update",
                                    "Android accessibility bridge is unavailable",
                                )
                            })?
                            .update(semantics)
                    });
                self.recover_surface_error(surface, &result);
                if result.is_ok() && self.first_frame {
                    self.first_frame = false;
                    super::super::set_loading_state(&self.app, "", false);
                }
                let _ = reply.send(result);
            }
            HostCommand::CaptureSurface { surface, reply } => {
                let _ = reply.send(self.surface_core_mut(surface).and_then(capture_surface));
            }
            #[cfg(feature = "platform-test-driver")]
            HostCommand::InjectSurfaceDeviceLoss { surface, reply } => {
                let result = self.surface_core_mut(surface).map(|core| {
                    core.inject_device_loss_for_test();
                });
                let _ = reply.send(result);
            }
            HostCommand::DestroySurface { surface, reply } => {
                let _ = reply.send(self.surfaces.remove(surface).map(|_| ()));
            }
            HostCommand::DestroyWindow { window, reply } => {
                let result = self.windows.remove(window).map(|window| {
                    self.window_ids.remove(&window.id());
                    self.accessibility = None;
                });
                let _ = reply.send(result);
            }
            HostCommand::OpenAudioOutput { request, reply } => {
                let result =
                    AndroidAudioResource::new(request, self.backend.audio_wake(), !self.resumed)
                        .and_then(|(resource, lane, format)| {
                            self.audio_outputs
                                .insert(resource)
                                .map(|handle| OpenedAudioOutput {
                                    handle,
                                    format,
                                    lane: Box::new(lane),
                                    capture: None,
                                })
                        });
                let _ = reply.send(result);
            }
            HostCommand::PauseAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .get_mut(output)
                    .and_then(AndroidAudioResource::pause);
                let _ = reply.send(result);
            }
            HostCommand::ResumeAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .get_mut(output)
                    .and_then(AndroidAudioResource::resume);
                let _ = reply.send(result);
            }
            HostCommand::AbortAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .remove(output)
                    .and_then(|mut audio| audio.stop());
                let _ = reply.send(result);
            }
            #[cfg(feature = "platform-test-driver")]
            HostCommand::InjectAudioDeviceLoss { output: _, reply } => {
                let _ = reply.send(Err(PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "audio.test.inject_device_loss",
                    "Android test device loss requires live route disconnection",
                )));
            }
            HostCommand::CloseAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .remove(output)
                    .and_then(|mut audio| audio.stop());
                let _ = reply.send(result);
            }
            HostCommand::OpenDecode { kind, reply } => {
                let result = AndroidDecodeWorker::new(
                    kind,
                    self.decode_root.clone(),
                    self.max_decode_output_bytes,
                )
                .and_then(|worker| self.decoders.insert(worker));
                let _ = reply.send(result);
            }
            HostCommand::Decode {
                session,
                request,
                reply,
            } => match self.decoders.get(session) {
                Ok(worker) => worker.submit(request, reply),
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            HostCommand::CloseDecode { session, reply } => match self.decoders.remove(session) {
                Ok(worker) => worker.close(reply),
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
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
                    } if relative_path == "game.astrapkg" => {
                        if self.bundled_package_hash != expected_hash {
                            Err(PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "package.open",
                                "bundled Android package hash does not match",
                            ))
                        } else {
                            self.package_sources.insert(PackageSourceResource::Memory(
                                MemoryPackageSource {
                                    bytes: Arc::clone(&self.bundled_package),
                                },
                            ))
                        }
                    }
                    PackageSourceRequest::Bundled { .. } => Err(host_error(
                        "package.open",
                        "Android bundled package path must be game.astrapkg",
                    )),
                    PackageSourceRequest::UserAuthorized { expected_hash } => {
                        self.open_saf_import(&expected_hash)
                    }
                    PackageSourceRequest::HttpsRange { url, expected_hash } => {
                        self.start_https_package_open(url, expected_hash, reply);
                        return;
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
                let _ = reply.send(self.package_sources.remove(source).map(|_| ()));
            }
            #[cfg(not(feature = "platform-test-driver"))]
            command @ (HostCommand::InjectSurfaceDeviceLoss { .. }
            | HostCommand::InjectAudioDeviceLoss { .. }) => {
                let operation = command.operation();
                let _ = command.reply_error(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    operation,
                    "device fault injection requires platform-test-driver",
                ));
            }
            HostCommand::Shutdown { reply } => {
                let result = self
                    .surfaces
                    .ensure_empty()
                    .and_then(|_| self.windows.ensure_empty())
                    .and_then(|_| self.audio_outputs.ensure_empty())
                    .and_then(|_| self.decoders.ensure_empty())
                    .and_then(|_| {
                        if self.accessibility.is_none() {
                            Ok(())
                        } else {
                            Err(host_error(
                                "host.shutdown",
                                "Android accessibility bridge is still live",
                            ))
                        }
                    })
                    .and_then(|_| self.save_transactions.ensure_empty())
                    .and_then(|_| self.package_sources.ensure_empty())
                    .and_then(|_| {
                        if self.pending_package_opens == 0 {
                            Ok(())
                        } else {
                            Err(host_error(
                                "host.shutdown",
                                "Android HTTPS package opens are still pending",
                            ))
                        }
                    });
                let exit = result.is_ok();
                let _ = reply.send(result);
                if exit {
                    event_loop.exit();
                }
            }
        }
    }

    pub(super) fn open_saf_import(
        &mut self,
        expected_hash: &str,
    ) -> Result<PackageSourceHandle, PlatformError> {
        let import = super::super::take_saf_import().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::Cancelled,
                "package.open_user_authorized",
                "no completed SAF import is available",
            )
        })?;
        if !import.permission_persisted {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "package.open_user_authorized",
                "SAF read permission was not persisted",
            ));
        }
        if import.sha256 != expected_hash || import.size > self.max_package_entry_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open_user_authorized",
                "SAF package identity or size violates the active profile",
            ));
        }
        if import.token.is_empty()
            || !import.token.ends_with(".astrapkg")
            || !import
                .token
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_' | b'.'))
        {
            return Err(host_error(
                "package.open_user_authorized",
                "SAF package token is invalid",
            ));
        }
        let path = self.imports_root.join(&import.token);
        let metadata = std::fs::metadata(&path).map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::Io,
                "package.open_user_authorized",
                "SAF imported package is unavailable",
            )
        })?;
        if metadata.len() != import.size || !metadata.is_file() {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open_user_authorized",
                "SAF imported package size changed",
            ));
        }
        let bytes = std::fs::read(path).map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::Io,
                "package.open_user_authorized",
                "SAF imported package could not be read",
            )
        })?;
        let actual = Hash256::from_sha256(&bytes).to_string();
        if actual != expected_hash {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open_user_authorized",
                "SAF imported package hash changed",
            ));
        }
        self.package_sources
            .insert(PackageSourceResource::Memory(MemoryPackageSource {
                bytes: Arc::new(bytes),
            }))
    }

    pub(super) fn start_https_package_open(
        &mut self,
        url: String,
        expected_hash: String,
        reply: tokio::sync::oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    ) {
        let policies = self.package_source_policies.clone();
        let policy = self.package_cache_policy.clone();
        let cache_root = self.package_cache_root.clone();
        let completion_tx = self.package_completion_tx.clone();
        self.pending_package_opens += 1;
        let spawn = thread::Builder::new()
            .name("astra-android-package".to_string())
            .spawn(move || {
                let result = (|| {
                    let mut cache = VerifiedPackageCache::open(cache_root, policy)?;
                    let client = astra_platform_common::HttpRangeClient::from_policies(&policies)?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| {
                            host_error(
                                "package.https.open",
                                "Android HTTPS runtime could not start",
                            )
                        })?;
                    runtime.block_on(client.fetch_into_cache(&url, &expected_hash, &mut cache))?;
                    cache.open_source(&expected_hash)
                })();
                let _ = completion_tx.send(PackageCompletion { reply, result });
            });
        if spawn.is_err() {
            self.pending_package_opens = self.pending_package_opens.saturating_sub(1);
        }
    }
}

impl AndroidHostApp {
    pub(super) fn process_package_completions(&mut self) {
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
