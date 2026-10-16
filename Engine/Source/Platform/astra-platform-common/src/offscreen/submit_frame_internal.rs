use super::*;

impl WgpuOffscreenRenderer {
    pub(super) fn submit_frame_internal(
        &mut self,
        frame: &SceneFrame,
        profile_gpu: bool,
    ) -> Result<Option<WgpuPendingProfile>, PlatformError> {
        self.last_readback_bytes = 0;
        self.last_queue_submissions = 0;
        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.resolve_submissions_since_frame = 0;
        }
        let cpu_started = Instant::now();
        if self.device_lost.load(Ordering::Acquire) {
            return Err(PlatformError::new(
                PlatformErrorCode::DeviceLost,
                "offscreen.render",
                "GPU device is lost",
            ));
        }
        let filter_stage = SceneFilters::validate(frame)?;
        let profile_atlas_upload = profile_gpu && self.scene_renderer.requires_atlas_update(frame);
        let query_count = 2 + u32::from(profile_atlas_upload) * 2 + filter_stage as u32 * 2;
        if profile_gpu && query_count > MAX_GPU_TIMESTAMP_QUERIES {
            return Err(invalid(
                "offscreen.timestamp_query",
                "frame exceeds the bounded GPU timestamp query count",
            ));
        }
        let timer_slot = if profile_gpu {
            let timer = self.gpu_timer.as_ref().ok_or_else(|| {
                unavailable(
                    "offscreen.timestamp_query",
                    "profiled submission requires timestamp-query support",
                )
            })?;
            if timer.slots[timer.next_slot].pending.is_some() {
                return Err(unavailable(
                    "offscreen.timestamp_query",
                    "GPU timestamp ring is full; resolve an older submission before continuing",
                ));
            }
            Some(timer.next_slot)
        } else {
            None
        };
        let scene_started = Instant::now();
        let prepared = if profile_gpu {
            let timer = self.gpu_timer.as_ref().ok_or_else(|| {
                unavailable(
                    "offscreen.timestamp_query",
                    "profiled submission requires timestamp-query support",
                )
            })?;
            let scene_begin = u32::from(profile_atlas_upload) * 2;
            self.scene_renderer.render_profiled(
                &self.device,
                &self.queue,
                frame,
                GlyphProfileQueries {
                    query_set: &timer.slots[timer_slot.expect("profiled timer slot")].query_set,
                    atlas_upload: profile_atlas_upload.then_some((0, 1)),
                    scene: (scene_begin, scene_begin + 1),
                },
            )?
        } else {
            self.scene_renderer
                .render(&self.device, &self.queue, frame)?
        };
        let scene_cpu_profile = prepared.cpu_profile;
        let texture = self.scene_renderer.commit(prepared);
        let scene_cpu_ns = elapsed_ns(scene_started, "offscreen.scene.cpu")?;
        let filter_started = Instant::now();
        let timestamps = timer_slot.map(|slot| {
            (
                &self
                    .gpu_timer
                    .as_ref()
                    .expect("profiled timer checked")
                    .slots[slot]
                    .query_set,
                2 + u32::from(profile_atlas_upload) * 2,
            )
        });
        let texture =
            self.filters
                .render(&self.device, &self.queue, frame, &texture, timestamps)?;
        self.pending_output = Some(PendingOutput {
            texture,
            width: frame.width,
            height: frame.height,
        });
        if profile_gpu {
            let filter_cpu_ns = elapsed_ns(filter_started, "offscreen.filter.cpu")?;
            let cpu_submit_ns = cpu_started.elapsed().as_nanos().try_into().map_err(|_| {
                invalid(
                    "offscreen.timestamp_query",
                    "CPU submit duration overflowed",
                )
            })?;
            let timer_slot = timer_slot.expect("profiled timer slot");
            let timer = self.gpu_timer.as_mut().ok_or_else(|| {
                unavailable("offscreen.timestamp_query", "GPU timer is unavailable")
            })?;
            mark_gpu_timestamp_recorded(timer, timer_slot, query_count)?;
            self.last_queue_submissions = 1 + u64::from(profile_atlas_upload) + filter_stage as u64;
            timer.next_slot = (timer_slot + 1) % timer.slots.len();
            Ok(Some(WgpuPendingProfile {
                cpu_submit_ns,
                scene_cpu_ns,
                filter_cpu_ns,
                scene_command_cpu_ns: scene_cpu_profile.command_ns,
                scene_atlas_cpu_ns: scene_cpu_profile.atlas_ns,
                scene_geometry_cpu_ns: scene_cpu_profile.geometry_ns,
                scene_vertex_upload_cpu_ns: scene_cpu_profile.vertex_upload_ns,
                scene_render_encode_cpu_ns: scene_cpu_profile.render_encode_ns,
                scene_queue_submit_cpu_ns: scene_cpu_profile.queue_submit_ns,
                scene_render_submit_cpu_ns: scene_cpu_profile.render_submit_ns,
                query_count,
                timer_slot,
                profiled_atlas_upload: profile_atlas_upload,
            }))
        } else {
            self.last_queue_submissions = 1 + filter_stage as u64;
            Ok(None)
        }
    }

    pub fn capture_checkpoint(&mut self) -> Result<CapturedFrame, PlatformError> {
        let output = self.pending_output.as_ref().ok_or_else(|| {
            invalid(
                "offscreen.readback",
                "checkpoint capture requires a submitted frame",
            )
        })?;
        let frame = readback(
            &self.device,
            &self.queue,
            &output.texture,
            output.width,
            output.height,
            &mut self.readback_ring,
        )?;
        self.last_readback_bytes = u64::from(output.width) * u64::from(output.height) * 4;
        Ok(frame)
    }
}

pub(super) async fn select_adapter(
    instance: &wgpu::Instance,
    policy: &GpuAdapterPolicy,
) -> Result<wgpu::Adapter, PlatformError> {
    let mut adapters = instance.enumerate_adapters(native_backend_mask()).await;
    adapters.retain(|adapter| {
        let info = adapter.get_info();
        backend_matches(info.backend, policy.backend)
            && device_type_matches(info.device_type, policy.device_type)
            && matches!(
                info.device_type,
                wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu
            )
            && (!policy.require_timestamp_query
                || adapter.features().contains(
                    wgpu::Features::TIMESTAMP_QUERY
                        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
                ))
            && policy
                .adapter_identity_hash
                .as_deref()
                .is_none_or(|expected| expected == adapter_policy_identity(&info))
    });
    adapters.sort_by_key(|adapter| {
        let info = adapter.get_info();
        (info.vendor, info.device, adapter_policy_identity(&info))
    });
    match adapters.len() {
        1 => Ok(adapters.remove(0)),
        0 => Err(unavailable(
            "offscreen.adapter.policy",
            "no hardware GPU adapter matches the profile-bound policy",
        )),
        _ => Err(unavailable(
            "offscreen.adapter.policy",
            "GPU adapter policy is ambiguous; bind adapter_identity_hash",
        )),
    }
}

pub(super) fn adapter_policy_identity(info: &wgpu::AdapterInfo) -> String {
    renderer_execution_identity(info).hash()
}

pub(super) fn renderer_execution_identity(info: &wgpu::AdapterInfo) -> RendererExecutionIdentity {
    RendererExecutionIdentity {
        provider: "wgpu_offscreen".into(),
        backend: backend_name(info.backend).into(),
        device_type: device_type_name(info.device_type).into(),
        vendor_id: info.vendor,
        device_id: info.device,
        adapter_name_hash: hash(info.name.as_bytes()),
        driver_identity_hash: hash(format!("{}:{}", info.driver, info.driver_info).as_bytes()),
    }
}

pub(super) fn device_descriptor(
    require_timestamp_query: bool,
    timestamp_features: wgpu::Features,
) -> wgpu::DeviceDescriptor<'static> {
    let mut descriptor = wgpu::DeviceDescriptor {
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..wgpu::DeviceDescriptor::default()
    };
    if require_timestamp_query {
        descriptor.required_features |= timestamp_features;
    }
    descriptor
}

pub(super) fn backend_matches(backend: wgpu::Backend, policy: GpuBackendPolicy) -> bool {
    matches!(
        (backend, policy),
        (wgpu::Backend::Dx12, GpuBackendPolicy::Dx12)
            | (wgpu::Backend::Vulkan, GpuBackendPolicy::Vulkan)
            | (wgpu::Backend::Metal, GpuBackendPolicy::Metal)
    )
}

pub(super) fn device_type_matches(
    device_type: wgpu::DeviceType,
    policy: GpuDeviceTypePolicy,
) -> bool {
    matches!(
        (device_type, policy),
        (
            wgpu::DeviceType::IntegratedGpu,
            GpuDeviceTypePolicy::Integrated
        ) | (wgpu::DeviceType::DiscreteGpu, GpuDeviceTypePolicy::Discrete)
            | (
                wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu,
                GpuDeviceTypePolicy::AnyHardware
            )
    )
}
