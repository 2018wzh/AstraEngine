use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use astra_headless_protocol::RendererExecutionIdentity;
use astra_media_core::{FilterParam, FilterValidator, SceneCommand};
use astra_platform::{
    CapturedFrame, GpuAdapterPolicy, GpuBackendPolicy, GpuDeviceTypePolicy, PlatformError,
    PlatformErrorCode, SceneFrame,
};

use crate::{
    glyph_atlas::{GlyphProfileQueries, WgpuGlyphAtlasRenderer},
    presentation::install_device_lost_callback,
};

/// Surface-free WGPU owner shared by the native platform hosts and Headless.
pub struct WgpuOffscreenRenderer {
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    scene_renderer: WgpuGlyphAtlasRenderer,
    identity: RendererExecutionIdentity,
    device_lost: Arc<AtomicBool>,
    pending_output: Option<PendingOutput>,
    filter_pipelines: BTreeMap<String, CachedFilterPipeline>,
    filter_outputs: Vec<CachedFilterOutput>,
    gpu_timer: Option<GpuTimer>,
    readback_ring: ReadbackRing,
    last_readback_bytes: u64,
    last_queue_submissions: u64,
}

struct PendingOutput {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

struct CachedFilterPipeline {
    bind_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

struct CachedFilterOutput {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    binding_signature: Option<String>,
    bind_group: Option<wgpu::BindGroup>,
}

struct FilterPassContext<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    pipelines: &'a mut BTreeMap<String, CachedFilterPipeline>,
    outputs: &'a mut Vec<CachedFilterOutput>,
    width: u32,
    height: u32,
}

const READBACK_RING_SIZE: usize = 3;
const READBACK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct ReadbackRing {
    width: u32,
    height: u32,
    padded_row_bytes: u32,
    slots: Vec<wgpu::Buffer>,
    next_slot: usize,
}

const MAX_GPU_TIMESTAMP_QUERIES: u32 = 64;
// Keep resolve/copy batches below the profiler's p95 sampling frequency while
// retaining a fixed upper bound on mapped timestamp buffers.
pub const WGPU_TIMESTAMP_RING_SIZE: usize = 64;

struct GpuTimer {
    slots: Vec<GpuTimerSlot>,
    next_slot: usize,
    timestamp_period_ns: f32,
}

struct GpuTimerSlot {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    read_buffer: wgpu::Buffer,
    map_status: Arc<AtomicU8>,
    pending: Option<PendingTimestampRead>,
}

enum PendingTimestampRead {
    Recorded {
        query_count: u32,
    },
    Submitted {
        submission: wgpu::SubmissionIndex,
        query_count: u32,
    },
}

const GPU_MAP_PENDING: u8 = 0;
const GPU_MAP_SUCCEEDED: u8 = 1;
const GPU_MAP_FAILED: u8 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WgpuFramePerformanceCounters {
    pub gpu_resource_bytes: u64,
    pub atlas_bytes: u64,
    pub upload_bytes: u64,
    pub readback_bytes: u64,
    pub draw_calls: u64,
    pub queue_submissions: u64,
    pub pipeline_count: u64,
    pub engine_allocation_bytes: u64,
    pub engine_allocation_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WgpuProfiledSubmission {
    pub cpu_submit_ns: u64,
    pub gpu_duration_ns: u64,
    pub scene_cpu_ns: u64,
    pub filter_cpu_ns: u64,
    pub scene_command_cpu_ns: u64,
    pub scene_atlas_cpu_ns: u64,
    pub scene_geometry_cpu_ns: u64,
    pub scene_vertex_upload_cpu_ns: u64,
    pub scene_render_encode_cpu_ns: u64,
    pub scene_queue_submit_cpu_ns: u64,
    pub scene_render_submit_cpu_ns: u64,
    pub atlas_upload_gpu_ns: u64,
    pub scene_gpu_ns: u64,
    pub filter_gpu_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WgpuPendingProfile {
    cpu_submit_ns: u64,
    scene_cpu_ns: u64,
    filter_cpu_ns: u64,
    scene_command_cpu_ns: u64,
    scene_atlas_cpu_ns: u64,
    scene_geometry_cpu_ns: u64,
    scene_vertex_upload_cpu_ns: u64,
    scene_render_encode_cpu_ns: u64,
    scene_queue_submit_cpu_ns: u64,
    scene_render_submit_cpu_ns: u64,
    query_count: u32,
    timer_slot: usize,
    profiled_atlas_upload: bool,
}

impl WgpuOffscreenRenderer {
    pub async fn new() -> Result<Self, PlatformError> {
        Self::new_internal(None).await
    }

    pub async fn new_with_policy(policy: &GpuAdapterPolicy) -> Result<Self, PlatformError> {
        Self::new_internal(Some(policy)).await
    }

    async fn new_internal(policy: Option<&GpuAdapterPolicy>) -> Result<Self, PlatformError> {
        let instance = native_wgpu_instance()?;
        let adapter = if let Some(policy) = policy {
            select_adapter(&instance, policy).await?
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
        identity
            .validate()
            .map_err(|_| unavailable("offscreen.identity", "GPU identity is invalid"))?;
        let scene_renderer = WgpuGlyphAtlasRenderer::new(&device);
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
            })
        } else {
            None
        };
        let device_lost = Arc::new(AtomicBool::new(false));
        install_device_lost_callback(&device, Arc::clone(&device_lost));
        Ok(Self {
            _instance: instance,
            device,
            queue,
            scene_renderer,
            identity,
            device_lost,
            pending_output: None,
            filter_pipelines: BTreeMap::new(),
            filter_outputs: Vec::new(),
            gpu_timer,
            readback_ring: ReadbackRing::default(),
            last_readback_bytes: 0,
            last_queue_submissions: 0,
        })
    }

    pub fn identity(&self) -> &RendererExecutionIdentity {
        &self.identity
    }

    pub fn performance_counters(&self) -> WgpuFramePerformanceCounters {
        let filter_bytes = self.filter_outputs.iter().fold(0u64, |total, output| {
            total + u64::from(output.width) * u64::from(output.height) * 4
        });
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
            draw_calls: self.scene_renderer.last_draw_calls() + self.filter_outputs.len() as u64,
            queue_submissions: self.last_queue_submissions,
            pipeline_count: 1 + self.filter_pipelines.len() as u64,
            engine_allocation_bytes: self.scene_renderer.last_engine_allocation_bytes(),
            engine_allocation_count: self.scene_renderer.last_engine_allocation_count(),
        }
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

    fn resolve_profiled_submission_internal(
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

    fn submit_frame_internal(
        &mut self,
        frame: &SceneFrame,
        profile_gpu: bool,
    ) -> Result<Option<WgpuPendingProfile>, PlatformError> {
        self.last_readback_bytes = 0;
        self.last_queue_submissions = 0;
        let cpu_started = Instant::now();
        if self.device_lost.load(Ordering::Acquire) {
            return Err(PlatformError::new(
                PlatformErrorCode::DeviceLost,
                "offscreen.render",
                "GPU device is lost",
            ));
        }
        let mut saw_filter = false;
        for command in &frame.commands {
            match command {
                SceneCommand::FilterGraph { graph } => {
                    saw_filter = true;
                    let validation = FilterValidator.validate(graph);
                    if !validation.blocking_diagnostics().is_empty() {
                        return Err(invalid(
                            "offscreen.filter",
                            "filter graph validation failed",
                        ));
                    }
                }
                _ if saw_filter => {
                    return Err(invalid(
                        "offscreen.filter",
                        "draw commands after a filter graph are not supported",
                    ));
                }
                _ => {}
            }
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
        let profile_atlas_upload = profile_gpu && self.scene_renderer.requires_atlas_update(frame);
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
        let mut texture = self.scene_renderer.commit(prepared);
        let scene_cpu_ns = elapsed_ns(scene_started, "offscreen.scene.cpu")?;
        let filter_started = Instant::now();
        let mut filter_stage = 0usize;
        let mut filter_context = FilterPassContext {
            device: &self.device,
            queue: &self.queue,
            pipelines: &mut self.filter_pipelines,
            outputs: &mut self.filter_outputs,
            width: frame.width,
            height: frame.height,
        };
        for command in &frame.commands {
            let SceneCommand::FilterGraph { graph } = command else {
                continue;
            };
            for node in &graph.nodes {
                texture = apply_filter(
                    &mut filter_context,
                    filter_stage,
                    &texture,
                    &node.kind,
                    &node.params,
                    if let Some(timer_slot) = timer_slot {
                        let begin =
                            2 + u32::from(profile_atlas_upload) * 2 + filter_stage as u32 * 2;
                        Some((
                            &self
                                .gpu_timer
                                .as_ref()
                                .ok_or_else(|| {
                                    unavailable(
                                        "offscreen.timestamp_query",
                                        "GPU timer is unavailable",
                                    )
                                })?
                                .slots[timer_slot]
                                .query_set,
                            begin,
                            begin + 1,
                        ))
                    } else {
                        None
                    },
                )?;
                filter_stage += 1;
            }
        }
        self.pending_output = Some(PendingOutput {
            texture,
            width: frame.width,
            height: frame.height,
        });
        if profile_gpu {
            let filter_cpu_ns = elapsed_ns(filter_started, "offscreen.filter.cpu")?;
            let query_count = 2 + u32::from(profile_atlas_upload) * 2 + filter_stage as u32 * 2;
            if query_count > MAX_GPU_TIMESTAMP_QUERIES {
                return Err(invalid(
                    "offscreen.timestamp_query",
                    "frame exceeds the bounded GPU timestamp query count",
                ));
            }
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

async fn select_adapter(
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

fn adapter_policy_identity(info: &wgpu::AdapterInfo) -> String {
    renderer_execution_identity(info)
        .hash()
        .expect("a WGPU renderer identity is canonical and hashable")
}

fn renderer_execution_identity(info: &wgpu::AdapterInfo) -> RendererExecutionIdentity {
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

fn device_descriptor(
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

fn backend_matches(backend: wgpu::Backend, policy: GpuBackendPolicy) -> bool {
    matches!(
        (backend, policy),
        (wgpu::Backend::Dx12, GpuBackendPolicy::Dx12)
            | (wgpu::Backend::Vulkan, GpuBackendPolicy::Vulkan)
            | (wgpu::Backend::Metal, GpuBackendPolicy::Metal)
    )
}

fn device_type_matches(device_type: wgpu::DeviceType, policy: GpuDeviceTypePolicy) -> bool {
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

fn apply_filter(
    context: &mut FilterPassContext<'_>,
    stage: usize,
    input: &wgpu::Texture,
    kind: &str,
    params: &BTreeMap<String, FilterParam>,
    timestamp_query: Option<(&wgpu::QuerySet, u32, u32)>,
) -> Result<wgpu::Texture, PlatformError> {
    let FilterPassContext {
        device,
        queue,
        pipelines,
        outputs,
        width,
        height,
    } = context;
    let width = *width;
    let height = *height;
    let expression = match kind {
        "astra.filter.bloom" => format!(
            "vec4<f32>(min(source.rgb + vec3<f32>({}), vec3<f32>(1.0)), source.a)",
            float_param(params, "intensity")?
        ),
        "astra.filter.fade" => format!(
            "vec4<f32>(source.rgb * {}, source.a)",
            float_param(params, "amount")?
        ),
        "astra.filter.color_matrix" => format!(
            "source * vec4<f32>({}, {}, {}, {})",
            float_param(params, "r")?,
            float_param(params, "g")?,
            float_param(params, "b")?,
            float_param(params, "a")?
        ),
        _ => {
            return Err(invalid(
                "offscreen.filter",
                "GPU filter kind is unsupported",
            ))
        }
    };
    let signature = hash(format!("{kind}:{expression}").as_bytes());
    if !pipelines.contains_key(&signature) {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("astra-offscreen-filter"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "@group(0) @binding(0) var input_texture: texture_2d<f32>;\n\
                 @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{\n\
                   let p = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));\n\
                   return vec4<f32>(p[i], 0.0, 1.0);\n\
                 }}\n\
                 @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{\n\
                   let source = textureLoad(input_texture, vec2<i32>(p.xy), 0);\n\
                   return {expression};\n\
                 }}"
                )
                .into(),
            ),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("astra-offscreen-filter-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("astra-offscreen-filter-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("astra-offscreen-filter-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        pipelines.insert(
            signature.clone(),
            CachedFilterPipeline {
                bind_layout,
                pipeline,
            },
        );
    }
    let cached = pipelines
        .get(&signature)
        .ok_or_else(|| invalid("offscreen.filter", "filter pipeline cache is unavailable"))?;
    while outputs.len() <= stage {
        outputs.push(CachedFilterOutput {
            width: 0,
            height: 0,
            texture: create_filter_output(device, 1, 1),
            binding_signature: None,
            bind_group: None,
        });
    }
    if outputs[stage].width != width || outputs[stage].height != height {
        outputs[stage] = CachedFilterOutput {
            width,
            height,
            texture: create_filter_output(device, width, height),
            binding_signature: None,
            bind_group: None,
        };
        for downstream in &mut outputs[stage + 1..] {
            downstream.binding_signature = None;
            downstream.bind_group = None;
        }
    }
    let output = outputs[stage].texture.clone();
    if outputs[stage].binding_signature.as_deref() != Some(signature.as_str()) {
        let input_view = input.create_view(&Default::default());
        outputs[stage].bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("astra-offscreen-filter-bind-group"),
            layout: &cached.bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input_view),
            }],
        }));
        outputs[stage].binding_signature = Some(signature.clone());
    }
    let output_view = output.create_view(&Default::default());
    let bind_group = outputs[stage]
        .bind_group
        .as_ref()
        .ok_or_else(|| invalid("offscreen.filter", "filter bind group cache is unavailable"))?;
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("astra-offscreen-filter-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamp_query.map(|(query_set, beginning, end)| {
                wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(beginning),
                    end_of_pass_write_index: Some(end),
                }
            }),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&cached.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
    Ok(output)
}

fn mark_gpu_timestamp_recorded(
    timer: &mut GpuTimer,
    timer_slot: usize,
    query_count: u32,
) -> Result<(), PlatformError> {
    let slot = timer.slots.get_mut(timer_slot).ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp slot is outside the bounded ring",
        )
    })?;
    if slot.pending.is_some() {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp slot was reused before resolution",
        ));
    }
    slot.pending = Some(PendingTimestampRead::Recorded { query_count });
    Ok(())
}

fn submit_recorded_timestamp_reads(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    timer: &mut GpuTimer,
) -> Result<(), PlatformError> {
    let recorded = timer
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| match slot.pending.as_ref() {
            Some(PendingTimestampRead::Recorded { query_count }) => Some((index, *query_count)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if recorded.is_empty() {
        return Err(invalid(
            "offscreen.timestamp_query",
            "timestamp resolve batch contains no recorded slots",
        ));
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("astra-offscreen-performance-resolve-encoder"),
    });
    for (index, query_count) in &recorded {
        let slot = &timer.slots[*index];
        let byte_length = u64::from(*query_count) * 8;
        encoder.resolve_query_set(&slot.query_set, 0..*query_count, &slot.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve_buffer, 0, &slot.read_buffer, 0, byte_length);
    }
    let submission = queue.submit([encoder.finish()]);
    for (index, query_count) in recorded {
        let slot = &mut timer.slots[index];
        let byte_length = u64::from(query_count) * 8;
        let slice = slot.read_buffer.slice(..byte_length);
        slot.map_status.store(GPU_MAP_PENDING, Ordering::Release);
        let map_status = Arc::clone(&slot.map_status);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            map_status.store(
                if result.is_ok() {
                    GPU_MAP_SUCCEEDED
                } else {
                    GPU_MAP_FAILED
                },
                Ordering::Release,
            );
        });
        slot.pending = Some(PendingTimestampRead::Submitted {
            submission: submission.clone(),
            query_count,
        });
    }
    Ok(())
}

fn resolve_gpu_timestamps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    timer: &mut GpuTimer,
    timer_slot: usize,
    query_count: u32,
    profiled_atlas_upload: bool,
    wait: bool,
) -> Result<Option<(u64, u64, u64)>, PlatformError> {
    let timestamp_period_ns = timer.timestamp_period_ns;
    let recorded_query_count =
        timer
            .slots
            .get(timer_slot)
            .and_then(|slot| match slot.pending.as_ref() {
                Some(PendingTimestampRead::Recorded { query_count }) => Some(*query_count),
                _ => None,
            });
    if let Some(recorded_query_count) = recorded_query_count {
        if recorded_query_count != query_count {
            return Err(invalid(
                "offscreen.timestamp_query",
                "GPU timestamp query count changed before resolution",
            ));
        }
        submit_recorded_timestamp_reads(device, queue, timer)?;
    }
    let slot = timer.slots.get_mut(timer_slot).ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp slot is outside the bounded ring",
        )
    })?;
    let pending = slot.pending.as_ref().ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp submission was already resolved",
        )
    })?;
    let PendingTimestampRead::Submitted {
        submission,
        query_count: pending_query_count,
    } = pending
    else {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp resolve batch was not submitted",
        ));
    };
    if *pending_query_count != query_count {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp query count changed before resolution",
        ));
    }
    if wait {
        let callback_deadline = Instant::now() + READBACK_TIMEOUT;
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission.clone()),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|_| {
                unavailable(
                    "offscreen.timestamp_query",
                    "GPU timestamp read exceeded its bounded timeout",
                )
            })?;
        loop {
            match slot.map_status.load(Ordering::Acquire) {
                GPU_MAP_SUCCEEDED => break,
                GPU_MAP_FAILED => {
                    return Err(unavailable(
                        "offscreen.timestamp_query",
                        "GPU timestamp map failed",
                    ));
                }
                GPU_MAP_PENDING if Instant::now() < callback_deadline => {
                    device.poll(wgpu::PollType::Poll).map_err(|_| {
                        unavailable(
                            "offscreen.timestamp_query",
                            "GPU timestamp callback polling failed",
                        )
                    })?;
                    thread::yield_now();
                }
                GPU_MAP_PENDING => {
                    return Err(unavailable(
                        "offscreen.timestamp_query",
                        "GPU timestamp map callback exceeded its bounded timeout",
                    ));
                }
                _ => unreachable!("GPU timestamp callback uses a closed status set"),
            }
        }
    } else {
        match slot.map_status.load(Ordering::Acquire) {
            GPU_MAP_SUCCEEDED => {}
            GPU_MAP_PENDING => return Ok(None),
            GPU_MAP_FAILED => {
                return Err(unavailable(
                    "offscreen.timestamp_query",
                    "GPU timestamp map failed",
                ));
            }
            _ => unreachable!("GPU timestamp callback uses a closed status set"),
        }
    }
    slot.pending.take().ok_or_else(|| {
        invalid(
            "offscreen.timestamp_query",
            "GPU timestamp submission was already resolved",
        )
    })?;
    let byte_length = u64::from(query_count) * 8;
    let slice = slot.read_buffer.slice(..byte_length);
    let mapped = slice.get_mapped_range();
    let base_query_count = 2 + u32::from(profiled_atlas_upload) * 2;
    if query_count < base_query_count || !(query_count - base_query_count).is_multiple_of(2) {
        return Err(invalid(
            "offscreen.timestamp_query",
            "GPU timestamp query layout is invalid",
        ));
    }
    let timestamp = |index: usize| {
        let offset = index * 8;
        u64::from_le_bytes(
            mapped[offset..offset + 8]
                .try_into()
                .expect("timestamp bytes"),
        )
    };
    let duration = |begin: u64, end: u64| -> Result<u64, PlatformError> {
        let ticks = end
            .checked_sub(begin)
            .ok_or_else(|| invalid("offscreen.timestamp_query", "GPU timestamp moved backwards"))?;
        let duration_ns = ticks as f64 * f64::from(timestamp_period_ns);
        if !duration_ns.is_finite() || duration_ns < 0.0 || duration_ns > u64::MAX as f64 {
            return Err(invalid(
                "offscreen.timestamp_query",
                "GPU timestamp duration is invalid",
            ));
        }
        Ok(duration_ns.round() as u64)
    };
    let atlas_upload_gpu_ns = if profiled_atlas_upload {
        duration(timestamp(0), timestamp(1))?
    } else {
        0
    };
    let scene_begin = usize::from(profiled_atlas_upload) * 2;
    let scene_gpu_ns = duration(timestamp(scene_begin), timestamp(scene_begin + 1))?;
    let mut filter_gpu_ns = 0u64;
    let filter_base = scene_begin + 2;
    for stage in 0..(query_count as usize - filter_base) / 2 {
        let begin = timestamp(filter_base + stage * 2);
        let end = timestamp(filter_base + stage * 2 + 1);
        filter_gpu_ns = filter_gpu_ns
            .checked_add(duration(begin, end)?)
            .ok_or_else(|| invalid("offscreen.timestamp_query", "GPU duration overflowed"))?;
    }
    drop(mapped);
    slot.read_buffer.unmap();
    Ok(Some((atlas_upload_gpu_ns, scene_gpu_ns, filter_gpu_ns)))
}

fn elapsed_ns(started: Instant, operation: &'static str) -> Result<u64, PlatformError> {
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| invalid(operation, "duration overflowed"))
}

fn create_filter_output(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("astra-offscreen-filter-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn float_param(
    params: &BTreeMap<String, FilterParam>,
    name: &str,
) -> Result<String, PlatformError> {
    match params.get(name) {
        Some(FilterParam::Float(value)) if value.is_finite() => Ok(format!("{value:.9}")),
        _ => Err(invalid(
            "offscreen.filter",
            "validated filter parameter is unavailable",
        )),
    }
}

/// Reuses the backend policy of the corresponding native platform host.
pub fn native_wgpu_instance() -> Result<wgpu::Instance, PlatformError> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    #[cfg(target_os = "windows")]
    {
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::StaticDxc;
    }
    #[cfg(target_os = "linux")]
    {
        descriptor.backends = wgpu::Backends::VULKAN;
    }
    #[cfg(target_os = "macos")]
    {
        descriptor.backends = wgpu::Backends::METAL;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        return Err(unavailable(
            "offscreen.platform",
            "offscreen GPU is implemented only for Windows, Linux, and macOS",
        ));
    }
    Ok(wgpu::Instance::new(descriptor))
}

fn native_backend_mask() -> wgpu::Backends {
    #[cfg(target_os = "windows")]
    return wgpu::Backends::DX12;
    #[cfg(target_os = "linux")]
    return wgpu::Backends::VULKAN;
    #[cfg(target_os = "macos")]
    return wgpu::Backends::METAL;
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    return wgpu::Backends::empty();
}

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    ring: &mut ReadbackRing,
) -> Result<CapturedFrame, PlatformError> {
    let row = width
        .checked_mul(4)
        .ok_or_else(|| invalid("offscreen.readback", "frame row overflows"))?;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    if ring.width != width
        || ring.height != height
        || ring.padded_row_bytes != padded
        || ring.slots.len() != READBACK_RING_SIZE
    {
        ring.width = width;
        ring.height = height;
        ring.padded_row_bytes = padded;
        ring.next_slot = 0;
        ring.slots = (0..READBACK_RING_SIZE)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("astra-offscreen-readback-ring"),
                    size: u64::from(padded) * u64::from(height),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .collect();
    }
    let slot = ring.next_slot;
    ring.next_slot = (ring.next_slot + 1) % READBACK_RING_SIZE;
    let buffer = ring
        .slots
        .get(slot)
        .ok_or_else(|| invalid("offscreen.readback", "readback ring slot is unavailable"))?;
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(READBACK_TIMEOUT),
        })
        .map_err(|_| {
            unavailable(
                "offscreen.readback",
                "GPU readback exceeded its bounded timeout",
            )
        })?;
    rx.recv()
        .map_err(|_| unavailable("offscreen.readback", "GPU readback callback was lost"))?
        .map_err(|_| unavailable("offscreen.readback", "GPU readback mapping failed"))?;
    let mapped = slice.get_mapped_range();
    let mut rgba8 = Vec::with_capacity(row as usize * height as usize);
    for source in mapped.chunks_exact(padded as usize).take(height as usize) {
        rgba8.extend_from_slice(&source[..row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(CapturedFrame {
        width,
        height,
        rgba8: rgba8.into(),
    })
}

fn validate_native_backend(backend: wgpu::Backend) -> Result<(), PlatformError> {
    let valid = cfg!(target_os = "windows") && backend == wgpu::Backend::Dx12
        || cfg!(target_os = "linux") && backend == wgpu::Backend::Vulkan
        || cfg!(target_os = "macos") && backend == wgpu::Backend::Metal;
    if valid {
        Ok(())
    } else {
        Err(unavailable(
            "offscreen.backend",
            "adapter backend does not match the native platform policy",
        ))
    }
}

fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Dx12 => "dx12",
        wgpu::Backend::Vulkan => "vulkan",
        wgpu::Backend::Metal => "metal",
        _ => "unsupported",
    }
}

fn device_type_name(device: wgpu::DeviceType) -> &'static str {
    match device {
        wgpu::DeviceType::DiscreteGpu => "discrete_gpu",
        wgpu::DeviceType::IntegratedGpu => "integrated_gpu",
        wgpu::DeviceType::VirtualGpu => "virtual_gpu",
        wgpu::DeviceType::Cpu => "cpu",
        _ => "other",
    }
}

fn hash(bytes: &[u8]) -> String {
    astra_core::Hash256::from_sha256(bytes).to_string()
}

fn invalid(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

fn unavailable(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}

#[cfg(test)]
mod adapter_identity_tests {
    use super::*;

    fn adapter_info(driver_info: &str) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "bounded-test-adapter".into(),
            vendor: 0x1002,
            device: 0x150e,
            device_type: wgpu::DeviceType::IntegratedGpu,
            device_pci_bus_id: String::new(),
            driver: "test-driver".into(),
            driver_info: driver_info.into(),
            backend: wgpu::Backend::Dx12,
            subgroup_min_size: 32,
            subgroup_max_size: 64,
            transient_saves_memory: false,
        }
    }

    #[test]
    fn policy_identity_is_the_reported_renderer_identity_hash() {
        let info = adapter_info("1.2.3");
        assert_eq!(
            adapter_policy_identity(&info),
            renderer_execution_identity(&info).hash().unwrap()
        );
        assert_ne!(
            adapter_policy_identity(&info),
            adapter_policy_identity(&adapter_info("1.2.4"))
        );
    }

    #[test]
    fn offscreen_device_uses_bounded_memory_allocation_hints() {
        let timestamp_features =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let descriptor = device_descriptor(true, timestamp_features);
        assert!(matches!(
            descriptor.memory_hints,
            wgpu::MemoryHints::MemoryUsage
        ));
        assert!(descriptor.required_features.contains(timestamp_features));
    }
}
