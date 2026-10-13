use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use astra_media_core::{FilterParam, FilterValidator, SceneCommand};

use astra_platform::{
    CapturedFrame, GpuAdapterPolicy, GpuBackendPolicy, GpuDeviceTypePolicy, PlatformError,
    PlatformErrorCode, SceneFrame,
};

use crate::{
    glyph_atlas::{GlyphProfileQueries, WgpuGlyphAtlasRenderer},
    presentation::install_device_lost_callback,
};

#[cfg(test)]
mod adapter_identity_tests {
    use super::submit_frame_internal::adapter_policy_identity;
    use super::{device_descriptor, renderer_execution_identity};

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
            renderer_execution_identity(&info).hash()
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

mod wgpu_offscreen_renderer;
use wgpu_offscreen_renderer::CachedFilterOutput;
use wgpu_offscreen_renderer::CachedFilterPipeline;
use wgpu_offscreen_renderer::FilterPassContext;
use wgpu_offscreen_renderer::GpuTimer;
use wgpu_offscreen_renderer::GpuTimerSlot;
use wgpu_offscreen_renderer::PendingOutput;
use wgpu_offscreen_renderer::PendingTimestampRead;
use wgpu_offscreen_renderer::ReadbackRing;
pub use wgpu_offscreen_renderer::RendererExecutionIdentity;
pub use wgpu_offscreen_renderer::WgpuFramePerformanceCounters;
pub use wgpu_offscreen_renderer::WgpuOffscreenRenderer;
pub use wgpu_offscreen_renderer::WgpuPendingProfile;
pub use wgpu_offscreen_renderer::WgpuProfiledSubmission;
use wgpu_offscreen_renderer::GPU_MAP_FAILED;
use wgpu_offscreen_renderer::GPU_MAP_PENDING;
use wgpu_offscreen_renderer::GPU_MAP_SUCCEEDED;
use wgpu_offscreen_renderer::MAX_GPU_TIMESTAMP_QUERIES;
use wgpu_offscreen_renderer::PERFORMANCE_ATLAS_BYTES;
use wgpu_offscreen_renderer::PERFORMANCE_ATLAS_SIDE;
use wgpu_offscreen_renderer::READBACK_RING_SIZE;
use wgpu_offscreen_renderer::READBACK_TIMEOUT;
pub use wgpu_offscreen_renderer::WGPU_TIMESTAMP_RING_SIZE;
mod with_default_compositing;

mod submit_frame_internal;
use submit_frame_internal::device_descriptor;
use submit_frame_internal::renderer_execution_identity;
use submit_frame_internal::select_adapter;
mod apply_filter;
use apply_filter::apply_filter;
use apply_filter::elapsed_ns;
use apply_filter::mark_gpu_timestamp_recorded;
use apply_filter::resolve_gpu_timestamps;
mod float_param;
use float_param::backend_name;
use float_param::device_type_name;
use float_param::float_param;
use float_param::hash;
use float_param::invalid;
use float_param::native_backend_mask;
pub use float_param::native_wgpu_instance;
use float_param::readback;
use float_param::unavailable;
use float_param::validate_native_backend;
