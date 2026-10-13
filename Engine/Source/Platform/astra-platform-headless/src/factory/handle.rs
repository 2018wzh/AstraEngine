use super::*;

impl HostState {
    pub(super) async fn handle(&mut self, command: HostCommand) {
        match command {
            HostCommand::CreateWindow { reply, .. } => {
                let _ = reply.send(self.windows.insert(WindowState { surface_count: 0 }));
            }
            HostCommand::CreateSurface { request, reply } => {
                let gpu_renderer = if self.profile.providers.renderer == "wgpu_offscreen" {
                    let instance = match self.gpu_instance() {
                        Ok(instance) => instance,
                        Err(error) => {
                            let _ = reply.send(Err(error));
                            return;
                        }
                    };
                    let renderer = if self.performance_observer.is_some() {
                        match self.profile.gpu_adapter.as_ref() {
                            Some(policy) => {
                                WgpuOffscreenRenderer::new_for_performance(
                                    &instance,
                                    policy,
                                    self.profile.max_gpu_resource_bytes,
                                    request.width,
                                    request.height,
                                )
                                .await
                            }
                            None => Err(PlatformError::new(
                                PlatformErrorCode::InvalidProfile,
                                "headless.performance.gpu_policy",
                                "performance observer requires an explicit GPU adapter policy",
                            )),
                        }
                    } else if let Some(policy) = &self.profile.gpu_adapter {
                        WgpuOffscreenRenderer::new_with_policy(&instance, policy).await
                    } else {
                        WgpuOffscreenRenderer::new(&instance).await
                    };
                    match renderer {
                        Ok(renderer) => Some(renderer),
                        Err(error) => {
                            let _ = reply.send(Err(error));
                            return;
                        }
                    }
                } else {
                    None
                };
                let result = (|| {
                    if let Some(renderer) = &gpu_renderer {
                        let identity = renderer.identity();
                        self.artifacts
                            .lock()
                            .map_err(|_| {
                                invalid("artifact.renderer", "artifact recorder lock is poisoned")
                            })?
                            .set_renderer_identity(RendererExecutionIdentity {
                                provider: identity.provider.clone(),
                                backend: identity.backend.clone(),
                                device_type: identity.device_type.clone(),
                                vendor_id: identity.vendor_id,
                                device_id: identity.device_id,
                                adapter_name_hash: identity.adapter_name_hash.clone(),
                                driver_identity_hash: identity.driver_identity_hash.clone(),
                            })?;
                    }
                    let window = self.windows.get_mut(request.window)?;
                    let renderer = CpuRendererProvider
                        .create(RendererCreateRequest {
                            width: request.width,
                            height: request.height,
                            format: RenderTargetFormat::Rgba8Srgb,
                            profile: self.profile.id.clone(),
                        })
                        .map_err(media_error)?;
                    let handle = self.surfaces.insert(SurfaceState {
                        window: request.window,
                        renderer,
                        width: request.width,
                        height: request.height,
                        last_sequence: 0,
                        frame: None,
                        pending: None,
                        materialized_sequence: None,
                        gpu_renderer,
                        pending_gpu_profiles: VecDeque::with_capacity(WGPU_TIMESTAMP_RING_SIZE),
                        deferred_gpu_resource_commands: Vec::new(),
                    })?;
                    window.surface_count += 1;
                    Ok(handle)
                })();
                let _ = reply.send(result);
            }
            HostCommand::CaptureSurface { surface, reply } => {
                let result = self.materialize_surface(surface, true);
                let _ = reply.send(result);
            }
            HostCommand::PresentRgba {
                surface,
                frame,
                reply,
            } => {
                let result = (|| {
                    let state = self.surfaces.get(surface)?;
                    ensure_increasing(state.last_sequence, frame.sequence, "surface.present_rgba")?;
                    if frame.width != state.width || frame.height != state.height {
                        return Err(invalid(
                            "surface.present_rgba",
                            "frame dimensions do not match surface",
                        ));
                    }
                    let canonical = canonical_json_digest(&(
                        frame.sequence,
                        frame.width,
                        frame.height,
                        &frame.rgba8,
                    ))
                    .map_err(|_| invalid("surface.present_rgba", "frame serialization failed"))?;
                    {
                        let mut artifacts = self.artifacts.lock().map_err(|_| {
                            invalid("artifact.frame", "artifact recorder lock is poisoned")
                        })?;
                        artifacts.record_submission(frame.sequence, &canonical)?;
                        artifacts.record_rasterized_frame(
                            frame.sequence,
                            frame.width,
                            frame.height,
                            &frame.rgba8,
                        )?;
                    }
                    present_rgba(self.surfaces.get_mut(surface)?, frame)
                })();
                let _ = reply.send(result);
            }
            HostCommand::PresentScene {
                surface,
                frame,
                reply,
            } => {
                let result = (|| {
                    let scene_build_started = Instant::now();
                    let sequence = frame.sequence;
                    let mut commands = Vec::with_capacity(frame.commands.len() + 1);
                    commands.push(SceneCommand::Clear {
                        rgba: frame.clear_rgba,
                    });
                    commands.extend(frame.commands);
                    let scene_validation_started = Instant::now();
                    let (journal, flush_deferred) = {
                        let s = self.surfaces.get(surface)?;
                        ensure_increasing(
                            s.last_sequence,
                            frame.sequence,
                            "surface.present_scene",
                        )?;
                        if frame.width != s.width || frame.height != s.height {
                            return Err(invalid(
                                "surface.present_scene",
                                "frame dimensions do not match surface",
                            ));
                        }
                        let journal = s.renderer.validate_frame(&commands).map_err(media_error)?;
                        let flush_deferred = crate::deferred_resources::crosses_resource_boundary(
                            &s.deferred_gpu_resource_commands,
                            &commands,
                        );
                        (journal, flush_deferred)
                    };
                    let scene_validation_ns = elapsed_ns(
                        scene_validation_started,
                        "surface.present_scene",
                        "scene validation duration overflowed",
                    )?;
                    if flush_deferred {
                        // Validate the incoming frame before submitting anything.
                        // The previous pending scene owns the earlier mutations;
                        // submit it without a checkpoint readback before reusing IDs.
                        self.materialize_surface(surface, false)?;
                        tracing::debug!(
                            event = "platform.headless.gpu.resource_boundary",
                            sequence,
                            "submitted deferred scene before the next resource mutation"
                        );
                    }
                    let (canonical, pending, deferred_resources, materialize) = {
                        let s = self.surfaces.get(surface)?;
                        let scene_digest_started = Instant::now();
                        let canonical = scene_submission_identity(
                            frame.sequence,
                            frame.width,
                            frame.height,
                            frame.clear_rgba,
                            commands.len(),
                            &frame.semantics,
                        )?;
                        let scene_digest_ns = elapsed_ns(
                            scene_digest_started,
                            "surface.present_scene",
                            "scene digest duration overflowed",
                        )?;
                        let scene_pending_started = Instant::now();
                        let deferred_resources: Vec<_> = commands
                            .iter()
                            .filter(|command| {
                                matches!(
                                    command,
                                    SceneCommand::UploadTexture { .. }
                                        | SceneCommand::UpdateTextureRegion { .. }
                                        | SceneCommand::UploadGlyph { .. }
                                        | SceneCommand::ReleaseResource { .. }
                                )
                            })
                            .cloned()
                            .collect();
                        let (pending_renderer, commands, gpu_commands) = if s.gpu_renderer.is_some()
                        {
                            let mut gpu_commands = s.deferred_gpu_resource_commands.clone();
                            gpu_commands.extend(commands.into_iter().skip(1));
                            (None, Vec::new(), gpu_commands)
                        } else {
                            (Some(s.renderer.clone()), commands, Vec::new())
                        };
                        let pending = PendingScene {
                            sequence,
                            width: frame.width,
                            height: frame.height,
                            renderer: pending_renderer,
                            commands,
                            gpu_commands,
                            clear_rgba: frame.clear_rgba,
                            semantics: frame.semantics,
                            scene_build_ns: scene_build_started
                                .elapsed()
                                .as_nanos()
                                .try_into()
                                .map_err(|_| {
                                    invalid(
                                        "surface.present_scene",
                                        "scene build duration overflowed",
                                    )
                                })?,
                            scene_digest_ns,
                            scene_validation_ns,
                            scene_pending_ns: elapsed_ns(
                                scene_pending_started,
                                "surface.present_scene",
                                "scene pending duration overflowed",
                            )?,
                        };
                        (
                            canonical,
                            pending,
                            deferred_resources,
                            self.profile.render_policy == HeadlessRenderPolicy::All
                                || sequence == 1,
                        )
                    };
                    // The canonical submission stream advances only after the full
                    // scene has validated. Invalid skipped frames therefore cannot
                    // alter either retained resources or submitted evidence.
                    self.artifacts
                        .lock()
                        .map_err(|_| {
                            invalid("artifact.scene", "artifact recorder lock is poisoned")
                        })?
                        .record_submission(sequence, &canonical)?;
                    {
                        let s = self.surfaces.get_mut(surface)?;
                        s.renderer.commit_frame(journal);
                        s.pending = Some(pending);
                        s.frame = None;
                        s.materialized_sequence = None;
                        s.last_sequence = sequence;
                        if s.gpu_renderer.is_some() {
                            s.deferred_gpu_resource_commands.extend(deferred_resources);
                        }
                    }
                    if materialize {
                        let capture = self.profile.readback_policy
                            == HeadlessReadbackPolicy::RasterizedFrames;
                        self.materialize_surface(surface, capture)?;
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            HostCommand::InjectSurfaceDeviceLoss { reply, .. } => {
                let _ = reply.send(Err(PlatformError::new(
                    PlatformErrorCode::PlatformNotImplemented,
                    "surface.test.inject_device_loss",
                    "Headless is an E2 test host and does not emulate native device loss",
                )));
            }
            HostCommand::DestroySurface { surface, reply } => {
                let capture =
                    self.profile.readback_policy == HeadlessReadbackPolicy::RasterizedFrames;
                let result = self
                    .surfaces
                    .get(surface)
                    .map(|state| state.pending.is_some() || state.frame.is_some())
                    .and_then(|has_submitted_frame| {
                        if has_submitted_frame {
                            self.materialize_surface(surface, capture).map(|_| ())
                        } else {
                            Ok(())
                        }
                    })
                    .and_then(|_| self.flush_gpu_profile(surface))
                    .and_then(|_| self.surfaces.remove(surface))
                    .and_then(|s| {
                        let window = self.windows.get_mut(s.window)?;
                        window.surface_count =
                            window.surface_count.checked_sub(1).ok_or_else(|| {
                                invalid("surface.destroy", "window surface count underflow")
                            })?;
                        Ok(())
                    });
                let _ = reply.send(result);
            }
            HostCommand::DestroyWindow { window, reply } => {
                let result = self
                    .windows
                    .get(window)
                    .and_then(|w| {
                        if w.surface_count == 0 {
                            Ok(())
                        } else {
                            Err(invalid("window.destroy", "window still owns live surfaces"))
                        }
                    })
                    .and_then(|_| self.windows.remove(window).map(|_| ()));
                let _ = reply.send(result);
            }
            HostCommand::OpenAudioOutput { request, reply } => {
                let result = if request.sample_rate != 48_000
                    || request.channels != 2
                    || request.chunk_frames == 0
                    || request.chunk_frames > request.max_buffered_frames
                {
                    Err(invalid(
                        "audio.open",
                        "headless audio requires 48kHz stereo and a bounded non-zero chunk",
                    ))
                } else {
                    (|| {
                        let artifact_stream = if !request.capture_samples
                            || matches!(
                                self.profile.artifacts.retention,
                                astra_platform::HeadlessArtifactRetention::ManifestOnly
                            ) {
                            None
                        } else {
                            Some(
                                self.artifacts
                                    .lock()
                                    .map_err(|_| {
                                        invalid("audio.open", "artifact recorder lock is poisoned")
                                    })?
                                    .begin_audio_stream()?,
                            )
                        };
                        let paused = Arc::new(AtomicBool::new(request.start_paused));
                        let wake = AudioWakeRegistration::default();
                        let artifact_stream = Arc::new(Mutex::new(artifact_stream));
                        let consumed_samples = Arc::new(AtomicU64::new(0));
                        let capture = request
                            .capture_samples
                            .then(astra_platform::AudioCaptureReader::default);
                        let handle = self.audio.insert(AudioState {
                            paused: Arc::clone(&paused),
                            wake: wake.clone(),
                            artifact_stream: Arc::clone(&artifact_stream),
                        })?;
                        let chunk_samples = request
                            .chunk_frames
                            .checked_mul(usize::from(request.channels))
                            .ok_or_else(|| {
                                invalid("audio.open", "audio chunk sample count overflowed")
                            })?;
                        Ok(OpenedAudioOutput {
                            handle,
                            format: AudioDeviceFormat {
                                sample_rate: request.sample_rate,
                                channels: request.channels,
                            },
                            capture: capture.clone(),
                            lane: Box::new(HeadlessAudioLane {
                                chunk_samples,
                                submitted_samples: 0,
                                consumed_samples,
                                paused,
                                wake,
                                observed_wake: 0,
                                artifact_stream,
                                artifacts: Arc::clone(&self.artifacts),
                                capture,
                            }),
                        })
                    })()
                };
                let _ = reply.send(result);
            }
            HostCommand::PauseAudio { output, reply } => {
                let result = self.audio.get(output).and_then(|audio| {
                    if audio.paused.swap(true, Ordering::AcqRel) {
                        Err(invalid("audio.pause", "audio output is already paused"))
                    } else {
                        audio.wake.notify();
                        Ok(())
                    }
                });
                let _ = reply.send(result);
            }
            HostCommand::ResumeAudio { output, reply } => {
                let result = self.audio.get(output).and_then(|audio| {
                    if !audio.paused.swap(false, Ordering::AcqRel) {
                        Err(invalid("audio.resume", "audio output is not paused"))
                    } else {
                        audio.wake.notify();
                        Ok(())
                    }
                });
                let _ = reply.send(result);
            }
            HostCommand::AbortAudio { output, reply } => {
                let result = self.audio.remove(output).and_then(|state| {
                    state.wake.notify();
                    let stream = state
                        .artifact_stream
                        .lock()
                        .map_err(|_| invalid("audio.abort", "audio stream lock is poisoned"))?
                        .take();
                    if let Some(stream) = stream {
                        self.artifacts
                            .lock()
                            .map_err(|_| {
                                invalid("audio.abort", "artifact recorder lock is poisoned")
                            })?
                            .abort_audio_stream(stream);
                    }
                    Ok(())
                });
                let _ = reply.send(result);
            }
            HostCommand::InjectAudioDeviceLoss { reply, .. } => {
                let _ = reply.send(Err(PlatformError::new(
                    PlatformErrorCode::PlatformNotImplemented,
                    "audio.test.inject_device_loss",
                    "Headless is an E2 test host and does not emulate native device loss",
                )));
            }
            HostCommand::CloseAudio { output, reply } => {
                let result = (|| {
                    let state = self.audio.remove(output)?;
                    state.wake.notify();
                    let stream = state
                        .artifact_stream
                        .lock()
                        .map_err(|_| invalid("audio.close", "audio stream lock is poisoned"))?
                        .take();
                    if let Some(stream) = stream {
                        self.artifacts
                            .lock()
                            .map_err(|_| {
                                invalid("audio.close", "artifact recorder lock is poisoned")
                            })?
                            .finish_audio_stream(stream)?;
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            HostCommand::OpenDecode { kind, reply } => {
                let result = self.decoders.insert(DecodeState {
                    kind,
                    last_sequence: 0,
                    #[cfg(feature = "ffmpeg-vcpkg")]
                    video_stream: None,
                });
                let _ = reply.send(result);
            }
            HostCommand::Decode {
                session,
                request,
                reply,
            } => {
                let result = self.decoders.get_mut(session).and_then(|state| {
                    decode_session(
                        state,
                        request,
                        &self.profile.providers.video_decode,
                        self.profile.max_video_frames,
                        self.profile.max_decode_output_bytes,
                        self.profile.limits.max_frame_bytes,
                    )
                });
                let _ = reply.send(result);
            }
            HostCommand::CloseDecode { session, reply } => {
                let result = self.decoders.remove(session).map(|state| {
                    #[cfg(feature = "ffmpeg-vcpkg")]
                    let had_video_stream = state.video_stream.is_some();
                    #[cfg(not(feature = "ffmpeg-vcpkg"))]
                    let had_video_stream = false;
                    tracing::info!(
                        event = "platform.headless.decode.session.closed",
                        kind = ?state.kind,
                        had_video_stream,
                        "closed Headless decode session and released decoder resources"
                    );
                });
                let _ = reply.send(result);
            }
            HostCommand::BeginSave { slot, reply } => {
                let result = self
                    .save_store
                    .begin(&slot)
                    .and_then(|s| self.saves.insert(s));
                let _ = reply.send(result);
            }
            HostCommand::WriteSave {
                transaction,
                bytes,
                reply,
            } => {
                let result = self
                    .saves
                    .get_mut(transaction)
                    .and_then(|s| s.write(&bytes));
                let _ = reply.send(result);
            }
            HostCommand::CommitSave { transaction, reply } => {
                let result = self
                    .saves
                    .remove(transaction)
                    .and_then(SaveTransaction::commit);
                let _ = reply.send(result);
            }
            HostCommand::AbortSave { transaction, reply } => {
                let result = self
                    .saves
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
                let result = self.open_package(source).await;
                let _ = reply.send(result);
            }
            HostCommand::ReadPackageRange {
                source,
                offset,
                length,
                reply,
            } => {
                let max = self.profile.limits.max_package_read_bytes;
                let result = if length > max {
                    Err(invalid("package.read_range", "range exceeds profile limit"))
                } else {
                    match self.packages.get_mut(source) {
                        Ok(source) => package_range(source, offset, length).await,
                        Err(error) => Err(error),
                    }
                };
                let _ = reply.send(result);
            }
            HostCommand::ClosePackage { source, reply } => {
                let result = self.packages.remove(source).map(|_| ());
                let _ = reply.send(result);
            }
            HostCommand::Shutdown { reply } => {
                let result = self.ensure_empty().and_then(|_| {
                    self.artifacts
                        .lock()
                        .map_err(|_| {
                            invalid("artifact.finish", "artifact recorder lock is poisoned")
                        })?
                        .finish()
                        .map(|_| ())
                });
                let _ = reply.send(result);
            }
        }
    }
}
