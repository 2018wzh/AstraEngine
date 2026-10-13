use super::*;

impl HostState {
    pub(super) fn gpu_instance(&self) -> Result<wgpu::Instance, PlatformError> {
        let mut instance = self
            .gpu_instance
            .lock()
            .map_err(|_| invalid("headless.gpu_instance", "graphics owner lock is poisoned"))?;
        if instance.is_none() {
            *instance = Some(astra_platform_common::native_wgpu_instance()?);
        }
        Ok(instance.as_ref().expect("GPU instance initialized").clone())
    }

    pub(super) fn new(
        factory: HeadlessPlatformFactory,
        profile: HeadlessHostProfile,
        backend: astra_platform::PlatformBackendChannels,
    ) -> Result<Self, PlatformError> {
        let save_store = AtomicSaveStore::new(&factory.run_root, &profile.package_id)?;
        let artifacts = Arc::new(Mutex::new(ArtifactRecorder::new(
            factory.run_root.clone(),
            &profile,
            factory.input_sequence_hash,
        )?));
        Ok(Self {
            profile,
            gpu_instance: factory.gpu_instance,
            backend,
            windows: ResourceTable::new("window"),
            surfaces: ResourceTable::new("surface"),
            audio: ResourceTable::new("audio_output"),
            decoders: ResourceTable::new("decode_session"),
            saves: ResourceTable::new("save_transaction"),
            packages: ResourceTable::new("package_source"),
            save_store,
            package_root: factory.package_root,
            user_authorized_package: factory.user_authorized_package,
            artifacts,
            https_root_certificates: factory.https_root_certificates,
            performance_observer: factory.performance_observer,
        })
    }

    pub(super) async fn run(mut self) {
        while let Some(command) = self.backend.next_command().await {
            let shutdown = matches!(command, HostCommand::Shutdown { .. });
            self.handle(command).await;
            if shutdown && self.is_empty() {
                break;
            }
        }
    }

    pub(super) fn flush_gpu_profile(
        &mut self,
        surface: SurfaceHandle,
    ) -> Result<(), PlatformError> {
        let state = self.surfaces.get_mut(surface)?;
        if state.pending_gpu_profiles.is_empty() {
            return Ok(());
        }
        let renderer = state.gpu_renderer.as_mut().ok_or_else(|| {
            invalid(
                "surface.performance",
                "pending GPU profile has no GPU renderer",
            )
        })?;
        let observer = self.performance_observer.as_ref().ok_or_else(|| {
            invalid(
                "surface.performance",
                "pending GPU profile has no performance observer",
            )
        })?;
        while let Some(profile) = state.pending_gpu_profiles.pop_front() {
            let submission = renderer.resolve_profiled_submission(profile.pending)?;
            observer.record_gpu_frame(complete_gpu_sample(profile.sample, submission))?;
        }
        Ok(())
    }

    pub(super) fn materialize_surface(
        &mut self,
        surface: SurfaceHandle,
        capture: bool,
    ) -> Result<CapturedFrame, PlatformError> {
        if let Some(cached) = {
            let state = self.surfaces.get(surface)?;
            state.frame.as_ref().map(|rgba8| CapturedFrame {
                width: state.width,
                height: state.height,
                rgba8: Arc::clone(rgba8),
            })
        } {
            return Ok(cached);
        }
        if self.surfaces.get(surface)?.pending.is_none() {
            if !capture {
                let state = self.surfaces.get(surface)?;
                return Ok(CapturedFrame {
                    width: state.width,
                    height: state.height,
                    rgba8: Arc::<[u8]>::from([]),
                });
            }
            let state = self.surfaces.get_mut(surface)?;
            let renderer = state.gpu_renderer.as_mut().ok_or_else(|| {
                invalid("surface.capture", "surface has no pending GPU checkpoint")
            })?;
            let captured = renderer.capture_checkpoint()?;
            self.artifacts
                .lock()
                .map_err(|_| invalid("artifact.frame", "artifact recorder lock is poisoned"))?
                .record_rasterized_frame(
                    state.materialized_sequence.ok_or_else(|| {
                        invalid("surface.capture", "surface has no materialized sequence")
                    })?,
                    captured.width,
                    captured.height,
                    &captured.rgba8,
                )?;
            state.frame = Some(Arc::clone(&captured.rgba8));
            return Ok(captured);
        }
        let mut pending = self
            .surfaces
            .get_mut(surface)?
            .pending
            .take()
            .ok_or_else(|| invalid("surface.capture", "surface has not submitted a scene"))?;
        let captured = {
            let state = self.surfaces.get_mut(surface)?;
            if let Some(renderer) = &mut state.gpu_renderer {
                if state.pending_gpu_profiles.len() >= WGPU_TIMESTAMP_RING_SIZE - 1 {
                    if let Some(profile) = state.pending_gpu_profiles.front() {
                        if let Some(submission) =
                            renderer.try_resolve_profiled_submission(profile.pending)?
                        {
                            let profile = state
                                .pending_gpu_profiles
                                .pop_front()
                                .expect("profile queue is not empty");
                            self.performance_observer
                                .as_ref()
                                .ok_or_else(|| {
                                    invalid(
                                        "surface.performance",
                                        "pending GPU profile has no performance observer",
                                    )
                                })?
                                .record_gpu_frame(complete_gpu_sample(
                                    profile.sample,
                                    submission,
                                ))?;
                        }
                    }
                }
                if state.pending_gpu_profiles.len() == WGPU_TIMESTAMP_RING_SIZE {
                    let profile = state
                        .pending_gpu_profiles
                        .pop_front()
                        .expect("profile queue is not empty");
                    let submission = renderer.resolve_profiled_submission(profile.pending)?;
                    self.performance_observer
                        .as_ref()
                        .ok_or_else(|| {
                            invalid(
                                "surface.performance",
                                "pending GPU profile has no performance observer",
                            )
                        })?
                        .record_gpu_frame(complete_gpu_sample(profile.sample, submission))?;
                }
                let frame = astra_platform::SceneFrame {
                    sequence: pending.sequence,
                    width: pending.width,
                    height: pending.height,
                    clear_rgba: pending.clear_rgba,
                    commands: pending.gpu_commands,
                    semantics: pending.semantics,
                };
                if let Some(observer) = &self.performance_observer {
                    observer.pace_gpu_frame(pending.sequence)?;
                    let pending_profile = renderer.submit_frame_timestamped(&frame)?;
                    let captured = if capture {
                        renderer.capture_checkpoint()?
                    } else {
                        CapturedFrame {
                            width: pending.width,
                            height: pending.height,
                            rgba8: Arc::<[u8]>::from([]),
                        }
                    };
                    let counters = renderer.performance_counters();
                    state.pending_gpu_profiles.push_back(PendingGpuProfile {
                        pending: pending_profile,
                        sample: HeadlessGpuFrameSample {
                            sequence: pending.sequence,
                            input_flow_id: observer.bind_gpu_frame(pending.sequence)?,
                            scene_build_ns: pending.scene_build_ns,
                            scene_digest_ns: pending.scene_digest_ns,
                            scene_validation_ns: pending.scene_validation_ns,
                            scene_pending_ns: pending.scene_pending_ns,
                            cpu_submit_ns: 0,
                            gpu_duration_ns: 0,
                            scene_cpu_ns: 0,
                            filter_cpu_ns: 0,
                            scene_command_cpu_ns: 0,
                            scene_atlas_cpu_ns: 0,
                            scene_geometry_cpu_ns: 0,
                            scene_vertex_upload_cpu_ns: 0,
                            scene_render_encode_cpu_ns: 0,
                            scene_queue_submit_cpu_ns: 0,
                            scene_render_submit_cpu_ns: 0,
                            atlas_upload_gpu_ns: 0,
                            scene_gpu_ns: 0,
                            filter_gpu_ns: 0,
                            gpu_resource_bytes: counters.gpu_resource_bytes,
                            atlas_bytes: counters.atlas_bytes,
                            upload_bytes: counters.upload_bytes,
                            readback_bytes: counters.readback_bytes,
                            draw_calls: counters.draw_calls,
                            queue_submissions: counters.queue_submissions,
                            pipeline_count: counters.pipeline_count,
                            heap_allocation_bytes: counters.engine_allocation_bytes,
                            heap_allocation_count: counters.engine_allocation_count,
                            command_allocation_bytes: counters.command_allocation_bytes,
                            atlas_allocation_bytes: counters.atlas_allocation_bytes,
                            geometry_allocation_bytes: counters.geometry_allocation_bytes,
                        },
                    });
                    captured
                } else if capture {
                    renderer.render(&frame)?
                } else {
                    renderer.submit_frame(&frame)?;
                    CapturedFrame {
                        width: pending.width,
                        height: pending.height,
                        rgba8: Arc::<[u8]>::from([]),
                    }
                }
            } else {
                let mut renderer = pending.renderer.take().ok_or_else(|| {
                    invalid("surface.capture", "CPU pending renderer is unavailable")
                })?;
                let output = renderer
                    .capture_frame(&pending.commands)
                    .map_err(media_error)?;
                CapturedFrame {
                    width: output.width,
                    height: output.height,
                    rgba8: output.bytes.into(),
                }
            }
        };
        if let Some(renderer) = &self.surfaces.get(surface)?.gpu_renderer {
            let counters = renderer.performance_counters();
            if counters.gpu_resource_bytes > self.profile.max_gpu_resource_bytes {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "surface.capture",
                    format!(
                        "GPU resources exceed the profile-bound residency budget: allocated_bytes={} atlas_bytes={} budget_bytes={}",
                        counters.gpu_resource_bytes, counters.atlas_bytes, self.profile.max_gpu_resource_bytes,
                    ),
                ));
            }
        }
        if captured.width != pending.width || captured.height != pending.height {
            return Err(invalid(
                "surface.capture",
                "materialized frame dimensions do not match the submitted scene",
            ));
        }
        if capture {
            self.artifacts
                .lock()
                .map_err(|_| invalid("artifact.frame", "artifact recorder lock is poisoned"))?
                .record_rasterized_frame(
                    pending.sequence,
                    captured.width,
                    captured.height,
                    &captured.rgba8,
                )?;
        }
        let state = self.surfaces.get_mut(surface)?;
        state.frame = capture.then(|| Arc::clone(&captured.rgba8));
        state.pending = None;
        state.materialized_sequence = Some(pending.sequence);
        state.deferred_gpu_resource_commands.clear();
        Ok(captured)
    }
}
