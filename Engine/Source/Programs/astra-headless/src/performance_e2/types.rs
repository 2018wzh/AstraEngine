use std::path::Path;

use astra_core::{
    PerformanceBudget, PerformanceMetricBudget, PerformanceThresholds, PerformanceUnit,
    PERFORMANCE_BUDGET_SCHEMA,
};
use astra_platform::{validate_headless_performance_profile, HeadlessHostProfile};
use clap::ValueEnum;
use serde::Deserialize;

use super::*;

pub(crate) const FRAME_PERIOD_NS: u64 = 8_333_333;
pub(crate) const WARMUP_FRAMES: u64 = 1_200;
pub(crate) const MEASURED_FRAMES: u64 = 72_000;
// Ten seconds at 120 Hz makes the p95 comparison resilient to the occasional
// DX12 scheduling outlier while keeping the self-check bounded and independent
// of the ten-minute acceptance measurement.
pub(crate) const PROFILER_OVERHEAD_FRAMES: u64 = 1_200;
pub(crate) const PROFILER_TRACE_SAMPLE_STRIDE: u64 = 32;
pub(crate) const MAX_PROFILER_OVERHEAD_PPM: u64 = 30_000;
pub(crate) const MAX_PROFILER_WORKING_SET_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) const PRODUCT_METRICS: &[&str] = &[
    "frame.cpu_ns",
    "frame.gpu_ns",
    "frame.end_to_end_ns",
    "deadline.miss_count",
    "memory.working_set_bytes",
    "memory.private_bytes",
    "memory.growth_bytes",
    "gpu.resource_bytes",
    "gpu.atlas_bytes",
    "cache.decoded_bytes",
    "gpu.upload_bytes",
    "gpu.readback_bytes",
    "renderer.draw_calls",
    "renderer.queue_submissions",
    "renderer.pipeline_count",
    "heap.allocation_bytes",
    "heap.allocation_count",
];
pub(crate) const REQUIRED_METRICS: &[&str] = &[
    "frame.cpu_ns",
    "frame.gpu_ns",
    "frame.end_to_end_ns",
    "deadline.miss_count",
    "memory.working_set_bytes",
    "memory.private_bytes",
    "memory.growth_bytes",
    "gpu.resource_bytes",
    "gpu.atlas_bytes",
    "cache.decoded_bytes",
    "gpu.upload_bytes",
    "gpu.readback_bytes",
    "renderer.draw_calls",
    "renderer.queue_submissions",
    "renderer.pipeline_count",
    "heap.allocation_bytes",
    "heap.allocation_count",
    "profiler.overhead_ppm",
    "profiler.working_set_bytes",
];

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PerformanceWorkload {
    Scene2d1080p,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PerformanceBudgetKind {
    RendererStress,
    ProductStress,
    ProductRoute,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PerformanceGpuBackend {
    Dx12,
    Vulkan,
    Metal,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PerformanceGpuDeviceType {
    Integrated,
    Discrete,
}

pub fn prepare_profile(
    input: &Path,
    output: &Path,
    backend: PerformanceGpuBackend,
    device_type: PerformanceGpuDeviceType,
    adapter_identity_hash: Option<String>,
) -> Result<(), String> {
    let mut profile: HeadlessHostProfile = read_json(input, "PROFILE")?;
    if profile.schema != astra_platform::HEADLESS_HOST_PROFILE_SCHEMA {
        return Err(
            "ASTRA_HEADLESS_PERFORMANCE_PROFILE_SCHEMA: only headless v3 is accepted".into(),
        );
    }
    profile.providers.renderer = "wgpu_offscreen".into();
    profile.presentation_rate_hz = astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ;
    profile.render_policy = astra_platform::HeadlessRenderPolicy::All;
    profile.readback_policy = astra_platform::HeadlessReadbackPolicy::CheckpointsOnly;
    profile.artifacts.retention = astra_platform::HeadlessArtifactRetention::ManifestOnly;
    profile.gpu_adapter = Some(astra_platform::GpuAdapterPolicy {
        backend: match backend {
            PerformanceGpuBackend::Dx12 => astra_platform::GpuBackendPolicy::Dx12,
            PerformanceGpuBackend::Vulkan => astra_platform::GpuBackendPolicy::Vulkan,
            PerformanceGpuBackend::Metal => astra_platform::GpuBackendPolicy::Metal,
        },
        device_type: match device_type {
            PerformanceGpuDeviceType::Integrated => astra_platform::GpuDeviceTypePolicy::Integrated,
            PerformanceGpuDeviceType::Discrete => astra_platform::GpuDeviceTypePolicy::Discrete,
        },
        require_timestamp_query: true,
        adapter_identity_hash,
    });
    validate_headless_performance_profile(&profile)
        .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILE_INVALID: {error}"))?;
    write_json(output, &profile)
}

impl PerformanceWorkload {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Scene2d1080p => "scene2d.1920x1080.120hz",
        }
    }
}

pub struct PerformanceE2Request<'a> {
    pub profile: &'a Path,
    pub package: &'a Path,
    pub budget: &'a Path,
    pub report: &'a Path,
    pub trace: &'a Path,
    pub trace_manifest: &'a Path,
    pub build_identity: &'a Path,
    pub workload: PerformanceWorkload,
    pub run_index: u8,
}

pub fn prepare_budget(
    profile_path: &Path,
    output: &Path,
    budget_id: String,
    min_samples: usize,
    max_samples: usize,
    min_run_duration_us: u64,
    kind: PerformanceBudgetKind,
) -> Result<(), String> {
    let profile: HeadlessHostProfile = read_json(profile_path, "PROFILE")?;
    validate_headless_performance_profile(&profile)
        .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILE_INVALID: {error}"))?;
    if min_samples == 0 || max_samples < min_samples || min_run_duration_us == 0 {
        return Err("ASTRA_PERFORMANCE_BUDGET_SAMPLE_RANGE_INVALID".into());
    }
    let profile_hash = profile.hash().map_err(|error| error.to_string())?;
    let metrics = REQUIRED_METRICS
        .iter()
        .filter(|id| {
            matches!(kind, PerformanceBudgetKind::RendererStress) || !id.starts_with("profiler.")
        })
        .map(|id| PerformanceMetricBudget {
            id: (*id).to_string(),
            unit: metric_unit(id),
            min_samples: if id.starts_with("profiler.") {
                1
            } else {
                min_samples
            },
            max_samples: if id.starts_with("profiler.") {
                1
            } else {
                max_samples
            },
            thresholds: match kind {
                PerformanceBudgetKind::ProductRoute => route_metric_thresholds(id),
                PerformanceBudgetKind::RendererStress | PerformanceBudgetKind::ProductStress => {
                    metric_thresholds(id)
                }
            },
        })
        .collect();
    let budget = PerformanceBudget {
        schema: PERFORMANCE_BUDGET_SCHEMA.into(),
        budget_id,
        target: profile.target,
        profile: profile.product_profile,
        profile_hash,
        min_run_duration_us,
        metrics,
    };
    budget
        .validate()
        .map_err(|error| format!("ASTRA_PERFORMANCE_BUDGET_INVALID: {error}"))?;
    write_json(output, &budget)
}

pub(crate) fn metric_unit(id: &str) -> PerformanceUnit {
    if id.ends_with("_ns") {
        PerformanceUnit::Nanoseconds
    } else if id.ends_with("_bytes") || id.ends_with(".bytes") {
        PerformanceUnit::Bytes
    } else {
        PerformanceUnit::Count
    }
}

pub(crate) fn metric_thresholds(id: &str) -> PerformanceThresholds {
    let mut thresholds = PerformanceThresholds {
        min_p50: None,
        min_p95: None,
        max_p50: None,
        max_p95: None,
        max_p99: None,
        max: None,
    };
    match id {
        "frame.cpu_ns" | "frame.gpu_ns" => thresholds.max_p95 = Some(4_000_000),
        "frame.end_to_end_ns" => thresholds.max_p99 = Some(FRAME_PERIOD_NS),
        "deadline.miss_count" => thresholds.max = Some(0),
        "gpu.upload_bytes" | "gpu.readback_bytes" => thresholds.max_p95 = Some(0),
        "memory.working_set_bytes" | "memory.private_bytes" => {
            thresholds.max = Some(768 * 1024 * 1024)
        }
        "memory.growth_bytes" => thresholds.max = Some(64 * 1024 * 1024),
        "gpu.resource_bytes" | "gpu.atlas_bytes" => thresholds.max = Some(256 * 1024 * 1024),
        "cache.decoded_bytes" => thresholds.max = Some(192 * 1024 * 1024),
        "heap.allocation_bytes" | "heap.allocation_count" => thresholds.max_p95 = Some(0),
        "profiler.overhead_ppm" => thresholds.max = Some(MAX_PROFILER_OVERHEAD_PPM),
        "profiler.working_set_bytes" => thresholds.max = Some(MAX_PROFILER_WORKING_SET_BYTES),
        "renderer.draw_calls" => thresholds.max = Some(1_000_000),
        "renderer.queue_submissions" => thresholds.max = Some(64),
        "renderer.pipeline_count" => thresholds.max = Some(1_024),
        _ => thresholds.max = Some(u64::MAX),
    }
    thresholds
}

pub(crate) fn route_metric_thresholds(id: &str) -> PerformanceThresholds {
    let mut thresholds = metric_thresholds(id);
    match id {
        "gpu.upload_bytes" => {
            thresholds.max_p95 = None;
            thresholds.max = Some(256 * 1024 * 1024);
        }
        "gpu.readback_bytes" => {
            thresholds.max_p95 = None;
            thresholds.max = Some(8 * 1024 * 1024);
        }
        "heap.allocation_bytes" => {
            thresholds.max_p95 = None;
            thresholds.max = Some(1024 * 1024);
        }
        "heap.allocation_count" => {
            thresholds.max_p95 = None;
            thresholds.max = Some(16_384);
        }
        _ => {}
    }
    thresholds
}

#[derive(Deserialize)]
pub(crate) struct BuildIdentity {
    pub(crate) schema: String,
    pub(crate) identity_hash: String,
    pub(crate) checkout_id: String,
    pub(crate) dirty: bool,
}
