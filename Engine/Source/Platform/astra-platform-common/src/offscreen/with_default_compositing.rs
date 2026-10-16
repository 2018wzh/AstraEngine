use super::*;

impl WgpuOffscreenRenderer {
    /// Choose the color space of commands without an explicit compositing mode.
    /// Mesh batches retain their own declaration; mixed spaces remain invalid.
    pub fn with_default_compositing(
        mut self,
        compositing: astra_media_core::SceneCompositing2D,
    ) -> Self {
        self.scene_renderer.set_default_compositing(compositing);
        self
    }

    pub async fn new(instance: &wgpu::Instance) -> Result<Self, PlatformError> {
        Self::new_internal(instance, None, None).await
    }

    pub async fn new_with_policy(
        instance: &wgpu::Instance,
        policy: &GpuAdapterPolicy,
    ) -> Result<Self, PlatformError> {
        Self::new_internal(instance, Some(policy), None).await
    }

    pub async fn new_for_performance(
        instance: &wgpu::Instance,
        policy: &GpuAdapterPolicy,
        max_gpu_resource_bytes: u64,
        width: u32,
        height: u32,
    ) -> Result<Self, PlatformError> {
        if max_gpu_resource_bytes < PERFORMANCE_ATLAS_BYTES {
            return Err(unavailable(
                "offscreen.performance.atlas_reservation",
                "GPU resource budget is too small for the persistent performance atlas",
            ));
        }
        let mut renderer =
            Self::new_internal(instance, Some(policy), Some(PERFORMANCE_ATLAS_SIDE)).await?;
        renderer.prewarm(width, height)?;
        Ok(renderer)
    }

    pub(super) async fn new_internal(
        instance: &wgpu::Instance,
        policy: Option<&GpuAdapterPolicy>,
        atlas_reservation: Option<u32>,
    ) -> Result<Self, PlatformError> {
        let adapter = if let Some(policy) = policy {
            select_adapter(instance, policy).await?
        } else {
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .map_err(|_| {
                    unavailable("offscreen.adapter", "hardware GPU adapter is unavailable")
                })?
        };
        let info = adapter.get_info();
        if !matches!(
            info.device_type,
            wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
        ) {
            return Err(unavailable(
                "offscreen.adapter",
                "only integrated or discrete hardware adapters are allowed for --gpu",
            ));
        }
        validate_native_backend(info.backend)?;
        let required_timestamp_features =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if policy.is_some_and(|policy| policy.require_timestamp_query)
            && !adapter.features().contains(required_timestamp_features)
        {
            return Err(unavailable(
                "offscreen.adapter.timestamp_query",
                "selected GPU adapter does not support timestamp queries",
            ));
        }
        let descriptor = device_descriptor(
            policy.is_some_and(|policy| policy.require_timestamp_query),
            required_timestamp_features,
        );
        let (device, queue) = adapter.request_device(&descriptor).await.map_err(|error| {
            tracing::error!(
                event = "platform.offscreen.device_create.failed",
                backend = backend_name(info.backend),
                device_type = device_type_name(info.device_type),
                diagnostic = %error,
                "offscreen GPU device creation failed"
            );
            unavailable("offscreen.device", "GPU device creation failed")
        })?;
        let identity = renderer_execution_identity(&info);
        if !identity.is_valid() {
            return Err(unavailable("offscreen.identity", "GPU identity is invalid"));
        }
        let scene_renderer = match atlas_reservation {
            Some(side) => WgpuGlyphAtlasRenderer::new_reserved(&device, &queue, side)?,
            None => WgpuGlyphAtlasRenderer::new(&device),
        };
        let gpu_timer = if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            let byte_length = u64::from(MAX_GPU_TIMESTAMP_QUERIES) * 8;
            Some(GpuTimer {
                slots: (0..WGPU_TIMESTAMP_RING_SIZE)
                    .map(|_| GpuTimerSlot {
                        query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                            label: Some("astra-offscreen-performance-timestamps"),
                            ty: wgpu::QueryType::Timestamp,
                            count: MAX_GPU_TIMESTAMP_QUERIES,
                        }),
                        resolve_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("astra-offscreen-performance-resolve"),
                            size: byte_length,
                            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                            mapped_at_creation: false,
                        }),
                        read_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("astra-offscreen-performance-read"),
                            size: byte_length,
                            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                            mapped_at_creation: false,
                        }),
                        map_status: Arc::new(AtomicU8::new(GPU_MAP_PENDING)),
                        pending: None,
                    })
                    .collect(),
                next_slot: 0,
                timestamp_period_ns: queue.get_timestamp_period(),
                resolve_submissions_since_frame: 0,
            })
        } else {
            None
        };
        let device_lost = Arc::new(AtomicBool::new(false));
        install_device_lost_callback(&device, Arc::clone(&device_lost));
        Ok(Self {
            _instance: instance.clone(),
            device,
            queue,
            scene_renderer,
            identity,
            device_lost,
            pending_output: None,
            filters: SceneFilters::default(),
            gpu_timer,
            readback_ring: ReadbackRing::default(),
            last_readback_bytes: 0,
            last_queue_submissions: 0,
        })
    }

    pub fn identity(&self) -> &RendererExecutionIdentity {
        &self.identity
    }

    /// Validate a product raster against the selected hardware device before
    /// scene allocation. Callers still apply their own bounded readback or
    /// cache budgets; this check only reports the device's actual texture
    /// dimension capability.
    pub fn validate_output_extent(&self, width: u32, height: u32) -> Result<(), PlatformError> {
        let max = self.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > max || height > max {
            return Err(unavailable(
                "offscreen.output_extent",
                "requested raster exceeds the selected GPU texture dimension limit",
            )
            .with_field("width", width.to_string())
            .with_field("height", height.to_string())
            .with_field("max_texture_dimension_2d", max.to_string()));
        }
        Ok(())
    }

    pub fn performance_counters(&self) -> WgpuFramePerformanceCounters {
        let filter_bytes = self.filters.resource_bytes();
        let readback_bytes = u64::from(self.readback_ring.padded_row_bytes)
            .saturating_mul(u64::from(self.readback_ring.height))
            .saturating_mul(self.readback_ring.slots.len() as u64);
        let timestamp_bytes = self.gpu_timer.as_ref().map_or(0, |timer| {
            u64::from(MAX_GPU_TIMESTAMP_QUERIES) * 8 * 2 * timer.slots.len() as u64
        });
        WgpuFramePerformanceCounters {
            gpu_resource_bytes: self
                .scene_renderer
                .resource_bytes()
                .saturating_add(filter_bytes)
                .saturating_add(readback_bytes)
                .saturating_add(timestamp_bytes),
            atlas_bytes: self.scene_renderer.atlas_bytes(),
            upload_bytes: self.scene_renderer.last_upload_bytes(),
            readback_bytes: self.last_readback_bytes,
            draw_calls: self.scene_renderer.last_draw_calls() + self.filters.draw_calls(),
            queue_submissions: self.last_queue_submissions
                + self
                    .gpu_timer
                    .as_ref()
                    .map_or(0, |timer| timer.resolve_submissions_since_frame),
            pipeline_count: 1 + self.filters.pipeline_count(),
            engine_allocation_bytes: self.scene_renderer.last_engine_allocation_bytes(),
            engine_allocation_count: self.scene_renderer.last_engine_allocation_count(),
            command_allocation_bytes: self.scene_renderer.last_command_allocation_bytes(),
            atlas_allocation_bytes: self.scene_renderer.last_atlas_allocation_bytes(),
            geometry_allocation_bytes: self.scene_renderer.last_geometry_allocation_bytes(),
        }
    }

    pub(super) fn prewarm(&mut self, width: u32, height: u32) -> Result<(), PlatformError> {
        if width == 0 || height == 0 {
            return Err(invalid(
                "offscreen.performance.prewarm",
                "performance surface dimensions must be non-zero",
            ));
        }
        let started = Instant::now();
        self.submit_frame(&SceneFrame {
            sequence: 0,
            width,
            height,
            clear_rgba: [0, 0, 0, 255],
            // An empty scene only allocates the output texture.  Submit one real
            // primitive so DX12 validates the bind group, vertex path and render
            // pipeline before any measured product frame can reach that lazy path.
            commands: vec![SceneCommand::Clear {
                rgba: [0, 0, 0, 255],
            }],
            semantics: None,
        })?;
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("astra-offscreen-performance-prewarm-fence"),
            });
        let submission = self.queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|_| {
                unavailable(
                    "offscreen.performance.prewarm",
                    "GPU prewarm exceeded its bounded timeout",
                )
            })?;
        self.pending_output = None;
        self.last_readback_bytes = 0;
        self.last_queue_submissions = 0;
        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.resolve_submissions_since_frame = 0;
        }
        tracing::info!(
            event = "platform.offscreen.performance.prewarm.complete",
            width,
            height,
            duration_us = started.elapsed().as_micros() as u64,
            "prewarmed the profile-bound WGPU pipeline before measured frames"
        );
        Ok(())
    }

    pub fn render(&mut self, frame: &SceneFrame) -> Result<CapturedFrame, PlatformError> {
        self.submit_frame(frame)?;
        self.capture_checkpoint()
    }

    pub fn submit_frame(&mut self, frame: &SceneFrame) -> Result<(), PlatformError> {
        self.submit_frame_internal(frame, false).map(|_| ())
    }

    pub fn submit_frame_profiled(
        &mut self,
        frame: &SceneFrame,
    ) -> Result<WgpuProfiledSubmission, PlatformError> {
        let pending = self.submit_frame_timestamped(frame)?;
        self.resolve_profiled_submission(pending)
    }

    pub fn submit_frame_timestamped(
        &mut self,
        frame: &SceneFrame,
    ) -> Result<WgpuPendingProfile, PlatformError> {
        self.submit_frame_internal(frame, true)?.ok_or_else(|| {
            unavailable(
                "offscreen.timestamp_query",
                "profiled submission requires timestamp-query support",
            )
        })
    }

    pub fn resolve_profiled_submission(
        &mut self,
        pending: WgpuPendingProfile,
    ) -> Result<WgpuProfiledSubmission, PlatformError> {
        self.resolve_profiled_submission_internal(pending, true)?
            .ok_or_else(|| {
                unavailable(
                    "offscreen.timestamp_query",
                    "blocking GPU timestamp resolution returned no result",
                )
            })
    }

    pub fn try_resolve_profiled_submission(
        &mut self,
        pending: WgpuPendingProfile,
    ) -> Result<Option<WgpuProfiledSubmission>, PlatformError> {
        self.resolve_profiled_submission_internal(pending, false)
    }

    pub(super) fn resolve_profiled_submission_internal(
        &mut self,
        pending: WgpuPendingProfile,
        wait: bool,
    ) -> Result<Option<WgpuProfiledSubmission>, PlatformError> {
        let timer = self
            .gpu_timer
            .as_mut()
            .ok_or_else(|| unavailable("offscreen.timestamp_query", "GPU timer is unavailable"))?;
        let Some((atlas_upload_gpu_ns, scene_gpu_ns, filter_gpu_ns)) = resolve_gpu_timestamps(
            &self.device,
            &self.queue,
            timer,
            pending.timer_slot,
            pending.query_count,
            pending.profiled_atlas_upload,
            wait,
        )?
        else {
            return Ok(None);
        };
        let gpu_duration_ns = atlas_upload_gpu_ns
            .checked_add(scene_gpu_ns)
            .and_then(|duration| duration.checked_add(filter_gpu_ns))
            .ok_or_else(|| invalid("offscreen.timestamp_query", "GPU duration overflowed"))?;
        Ok(Some(WgpuProfiledSubmission {
            cpu_submit_ns: pending.cpu_submit_ns,
            gpu_duration_ns,
            scene_cpu_ns: pending.scene_cpu_ns,
            filter_cpu_ns: pending.filter_cpu_ns,
            scene_command_cpu_ns: pending.scene_command_cpu_ns,
            scene_atlas_cpu_ns: pending.scene_atlas_cpu_ns,
            scene_geometry_cpu_ns: pending.scene_geometry_cpu_ns,
            scene_vertex_upload_cpu_ns: pending.scene_vertex_upload_cpu_ns,
            scene_render_encode_cpu_ns: pending.scene_render_encode_cpu_ns,
            scene_queue_submit_cpu_ns: pending.scene_queue_submit_cpu_ns,
            scene_render_submit_cpu_ns: pending.scene_render_submit_cpu_ns,
            atlas_upload_gpu_ns,
            scene_gpu_ns,
            filter_gpu_ns,
        }))
    }
}
