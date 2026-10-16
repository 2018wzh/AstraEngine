use super::*;

/// Session-local surface-free renderer used by native platform hosts and Headless.
/// The caller retains the backend instance across sessions; each renderer creates
/// and releases its own device, queue and scene resources.
pub struct WgpuOffscreenRenderer {
    pub(super) _instance: wgpu::Instance,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) scene_renderer: WgpuGlyphAtlasRenderer,
    pub(super) identity: RendererExecutionIdentity,
    pub(super) device_lost: Arc<AtomicBool>,
    pub(super) pending_output: Option<PendingOutput>,
    pub(super) filters: SceneFilters,
    pub(super) gpu_timer: Option<GpuTimer>,
    pub(super) readback_ring: ReadbackRing,
    pub(super) last_readback_bytes: u64,
    pub(super) last_queue_submissions: u64,
}

pub(super) struct PendingOutput {
    pub(super) texture: wgpu::Texture,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) const READBACK_RING_SIZE: usize = 3;

pub(super) const READBACK_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) const PERFORMANCE_ATLAS_SIDE: u32 = 4096;

pub(super) const PERFORMANCE_ATLAS_BYTES: u64 =
    PERFORMANCE_ATLAS_SIDE as u64 * PERFORMANCE_ATLAS_SIDE as u64 * 4;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RendererExecutionIdentity {
    pub provider: String,
    pub backend: String,
    pub device_type: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub adapter_name_hash: String,
    pub driver_identity_hash: String,
}

impl RendererExecutionIdentity {
    pub(super) fn hash(&self) -> String {
        let encoded = serde_json::to_vec(self)
            .expect("the fixed renderer identity fields always serialize to JSON");
        hash(&encoded)
    }

    pub(super) fn is_valid(&self) -> bool {
        !self.provider.is_empty()
            && !self.backend.is_empty()
            && !self.device_type.is_empty()
            && self
                .adapter_name_hash
                .parse::<astra_core::Hash256>()
                .is_ok()
            && self
                .driver_identity_hash
                .parse::<astra_core::Hash256>()
                .is_ok()
    }
}

#[derive(Default)]
pub(super) struct ReadbackRing {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) padded_row_bytes: u32,
    pub(super) slots: Vec<wgpu::Buffer>,
    pub(super) next_slot: usize,
}

pub(super) const MAX_GPU_TIMESTAMP_QUERIES: u32 = 64;

// Keep resolve/copy batches below the profiler's p95 sampling frequency while
// retaining a fixed upper bound on mapped timestamp buffers.
pub const WGPU_TIMESTAMP_RING_SIZE: usize = 64;

pub(super) struct GpuTimer {
    pub(super) slots: Vec<GpuTimerSlot>,
    pub(super) next_slot: usize,
    pub(super) timestamp_period_ns: f32,
    pub(super) resolve_submissions_since_frame: u64,
}

pub(super) struct GpuTimerSlot {
    pub(super) query_set: wgpu::QuerySet,
    pub(super) resolve_buffer: wgpu::Buffer,
    pub(super) read_buffer: wgpu::Buffer,
    pub(super) map_status: Arc<AtomicU8>,
    pub(super) pending: Option<PendingTimestampRead>,
}

pub(super) enum PendingTimestampRead {
    Recorded {
        query_count: u32,
    },
    Submitted {
        submission: wgpu::SubmissionIndex,
        query_count: u32,
    },
}

pub(super) const GPU_MAP_PENDING: u8 = 0;

pub(super) const GPU_MAP_SUCCEEDED: u8 = 1;

pub(super) const GPU_MAP_FAILED: u8 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WgpuFramePerformanceCounters {
    pub gpu_resource_bytes: u64,
    pub atlas_bytes: u64,
    pub upload_bytes: u64,
    pub readback_bytes: u64,
    pub draw_calls: u64,
    /// Scene/filter submissions plus timestamp resolve batches issued since
    /// the latest frame submission. Polling an existing batch adds nothing.
    pub queue_submissions: u64,
    pub pipeline_count: u64,
    pub engine_allocation_bytes: u64,
    pub engine_allocation_count: u64,
    pub command_allocation_bytes: u64,
    pub atlas_allocation_bytes: u64,
    pub geometry_allocation_bytes: u64,
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
    pub(super) cpu_submit_ns: u64,
    pub(super) scene_cpu_ns: u64,
    pub(super) filter_cpu_ns: u64,
    pub(super) scene_command_cpu_ns: u64,
    pub(super) scene_atlas_cpu_ns: u64,
    pub(super) scene_geometry_cpu_ns: u64,
    pub(super) scene_vertex_upload_cpu_ns: u64,
    pub(super) scene_render_encode_cpu_ns: u64,
    pub(super) scene_queue_submit_cpu_ns: u64,
    pub(super) scene_render_submit_cpu_ns: u64,
    pub(super) query_count: u32,
    pub(super) timer_slot: usize,
    pub(super) profiled_atlas_upload: bool,
}
