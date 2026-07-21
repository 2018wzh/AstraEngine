use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use astra_core::{
    Hash256, PerformanceBudget, PerformanceMetricBudget, PerformanceRecorder,
    PerformanceRunIdentity, PerformanceThresholds, PerformanceTraceManifest, PerformanceUnit,
    PERFORMANCE_BUDGET_SCHEMA, PERFORMANCE_TRACE_MANIFEST_SCHEMA,
};
use astra_media_core::{
    BlendMode, FilterGraph, FilterNode, FilterParam, FilterTarget, GlyphBitmap, GlyphBitmapFormat,
    GlyphInstance, MeshMaterial2D, MeshVertex2D, RectI, SceneCommand, TextureFrame, Transform2D,
};
use astra_observability::{
    sample_process_memory, PerfettoFlowPhase, PerfettoTraceConfig, PerfettoTraceWriter,
    ProcessMemorySample,
};
use astra_package::AstraContainerReader;
use astra_platform::{validate_headless_performance_profile, HeadlessHostProfile, SceneFrame};
use astra_platform_common::{
    WgpuFramePerformanceCounters, WgpuOffscreenRenderer, WgpuPendingProfile,
};
use clap::ValueEnum;
use serde::Deserialize;

const FRAME_PERIOD_NS: u64 = 8_333_333;
const WARMUP_FRAMES: u64 = 1_200;
const MEASURED_FRAMES: u64 = 72_000;
const PROFILER_OVERHEAD_FRAMES: u64 = 600;
const MAX_PROFILER_OVERHEAD_PPM: u64 = 1_030_000;
const MAX_PROFILER_WORKING_SET_BYTES: u64 = 32 * 1024 * 1024;

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
const REQUIRED_METRICS: &[&str] = &[
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
    profile.providers.renderer = "wgpu_offscreen".into();
    profile.render_policy = astra_platform::HeadlessRenderPolicy::All;
    profile.readback_policy = astra_platform::HeadlessReadbackPolicy::CheckpointsOnly;
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
    fn id(self) -> &'static str {
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
struct BuildIdentity {
    schema: String,
    identity_hash: String,
    checkout_id: String,
    dirty: bool,
}

pub async fn run(request: PerformanceE2Request<'_>) -> Result<(), String> {
    let profile: HeadlessHostProfile = read_json(request.profile, "PROFILE")?;
    let gpu_policy = validate_headless_performance_profile(&profile)
        .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILE_INVALID: {error}"))?;
    if profile.viewport_width != 1920 || profile.viewport_height != 1080 {
        return Err("ASTRA_PERFORMANCE_WORKLOAD_VIEWPORT_MISMATCH".into());
    }
    let budget: PerformanceBudget = read_json(request.budget, "BUDGET")?;
    budget
        .validate()
        .map_err(|error| format!("ASTRA_PERFORMANCE_BUDGET_INVALID: {error}"))?;
    validate_metric_set(&budget)?;
    let build: BuildIdentity = read_json(request.build_identity, "BUILD_IDENTITY")?;
    if build.schema != "astra.build_identity.v1"
        || build.dirty
        || build.identity_hash != profile.build_fingerprint
        || build.checkout_id.len() != 40
    {
        return Err("ASTRA_PERFORMANCE_BUILD_IDENTITY_INVALID".into());
    }
    let profile_hash = profile.hash().map_err(|error| error.to_string())?;
    if budget.target != profile.target
        || budget.profile != profile.product_profile
        || budget.profile_hash != profile_hash
        || budget.min_run_duration_us < 600_000_000
    {
        return Err("ASTRA_PERFORMANCE_BUDGET_PROFILE_MISMATCH".into());
    }
    let identity = PerformanceRunIdentity {
        source_revision: build.checkout_id,
        dirty: false,
        target: profile.target.clone(),
        profile: profile.product_profile.clone(),
        profile_hash,
        package_hash: profile.package_hash.clone(),
        build_fingerprint: build.identity_hash,
        session_id: format!("perf.{}.{}", request.workload.id(), request.run_index),
    };
    identity
        .validate()
        .map_err(|error| format!("ASTRA_PERFORMANCE_IDENTITY_INVALID: {error}"))?;

    let package_hash = profile
        .package_hash
        .parse::<Hash256>()
        .map_err(|_| "ASTRA_PERFORMANCE_PACKAGE_HASH_INVALID".to_string())?;
    let package_source = astra_byte_source::FileByteSource::open(request.package)
        .map_err(|error| format!("ASTRA_PERFORMANCE_PACKAGE_OPEN_FAILED: {error}"))?;
    AstraContainerReader::open_storage_verified_source(
        std::sync::Arc::new(package_source),
        package_hash,
    )
    .map_err(|error| format!("ASTRA_PERFORMANCE_PACKAGE_VERIFY_FAILED: {error}"))?;

    let mut renderer = WgpuOffscreenRenderer::new_with_policy(gpu_policy)
        .await
        .map_err(|error| format!("ASTRA_PERFORMANCE_GPU_REQUIRED: {error}"))?;
    if renderer.identity().device_type != "integrated_gpu" {
        return Err("ASTRA_PERFORMANCE_INTEGRATED_GPU_REQUIRED".into());
    }
    let adapter_identity_hash = Hash256::from_sha256(
        &serde_json::to_vec(renderer.identity())
            .map_err(|error| format!("ASTRA_PERFORMANCE_ADAPTER_ENCODE_FAILED: {error}"))?,
    )
    .to_string();
    if gpu_policy
        .adapter_identity_hash
        .as_deref()
        .is_some_and(|expected| expected != adapter_identity_hash)
    {
        return Err("ASTRA_PERFORMANCE_ADAPTER_IDENTITY_MISMATCH".into());
    }

    let (initialization, mut frame) = scene2d_workload();
    renderer
        .submit_frame_profiled(&initialization)
        .map_err(|error| format!("ASTRA_PERFORMANCE_INITIALIZATION_FAILED: {error}"))?;
    let initialized = renderer.performance_counters();
    if initialized.upload_bytes == 0 {
        return Err("ASTRA_PERFORMANCE_INITIAL_UPLOAD_MISSING".into());
    }
    run_warmup(&mut renderer, &mut frame)?;
    if renderer.performance_counters().upload_bytes != 0 {
        return Err("ASTRA_PERFORMANCE_STABLE_FRAME_REUPLOAD".into());
    }
    let memory_baseline = sample_process_memory()
        .map_err(|error| format!("ASTRA_PERFORMANCE_MEMORY_SAMPLE_FAILED: {error}"))?;
    let allocation_baseline = super::ASTRA_ALLOCATOR.snapshot();

    let (profiler_overhead_ppm, profiler_working_set_bytes) =
        validate_profiler_overhead(&mut renderer, &mut frame, request.trace)?;

    let mut trace = PerfettoTraceWriter::create(PerfettoTraceConfig::production(
        request.trace,
        "astra-headless",
    ))
    .map_err(|error| error.to_string())?;
    let mut recorder =
        PerformanceRecorder::new(budget.clone()).map_err(|error| error.to_string())?;
    recorder
        .record("profiler.overhead_ppm", profiler_overhead_ppm)
        .and_then(|_| recorder.record("profiler.working_set_bytes", profiler_working_set_bytes))
        .map_err(|error| error.to_string())?;
    let measured_started = Instant::now();
    let mut deadline_misses = 0u64;
    let mut last_memory = memory_baseline;
    let mut pending_frames: VecDeque<PendingMeasuredFrame> = VecDeque::with_capacity(4);
    for index in 0..MEASURED_FRAMES {
        let scheduled = measured_started + Duration::from_nanos(FRAME_PERIOD_NS * index);
        if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        if pending_frames.len() >= 3 {
            let oldest = pending_frames
                .front()
                .expect("bounded performance queue is not empty");
            if let Some(submission) = renderer
                .try_resolve_profiled_submission(oldest.pending)
                .map_err(|error| format!("ASTRA_PERFORMANCE_TIMESTAMP_RESOLVE_FAILED: {error}"))?
            {
                deadline_misses = deadline_misses.saturating_add(record_measured_frame(
                    pending_frames
                        .pop_front()
                        .expect("bounded performance queue is not empty"),
                    submission,
                    &memory_baseline,
                    &mut recorder,
                    &mut trace,
                )?);
            }
        }
        if pending_frames.len() == 4 {
            let pending = pending_frames
                .pop_front()
                .expect("bounded performance queue is not empty");
            let submission = renderer
                .resolve_profiled_submission(pending.pending)
                .map_err(|error| format!("ASTRA_PERFORMANCE_TIMESTAMP_RESOLVE_FAILED: {error}"))?;
            deadline_misses = deadline_misses.saturating_add(record_measured_frame(
                pending,
                submission,
                &memory_baseline,
                &mut recorder,
                &mut trace,
            )?);
        }
        if index % 60 == 0 {
            last_memory = sample_process_memory()
                .map_err(|error| format!("ASTRA_PERFORMANCE_MEMORY_SAMPLE_FAILED: {error}"))?;
        }
        animate(&mut frame, index + WARMUP_FRAMES + 1)?;
        let pending = renderer
            .submit_frame_timestamped(&frame)
            .map_err(|error| format!("ASTRA_PERFORMANCE_FRAME_FAILED: {error}"))?;
        let counters = renderer.performance_counters();
        if counters.upload_bytes != 0 {
            return Err("ASTRA_PERFORMANCE_STABLE_FRAME_REUPLOAD".into());
        }
        pending_frames.push_back(PendingMeasuredFrame {
            index,
            pending,
            memory: last_memory,
            counters,
            submitted_timestamp_ns: duration_ns(measured_started.elapsed())?,
        });
    }
    while let Some(pending) = pending_frames.pop_front() {
        let submission = renderer
            .resolve_profiled_submission(pending.pending)
            .map_err(|error| format!("ASTRA_PERFORMANCE_TIMESTAMP_RESOLVE_FAILED: {error}"))?;
        deadline_misses = deadline_misses.saturating_add(record_measured_frame(
            pending,
            submission,
            &memory_baseline,
            &mut recorder,
            &mut trace,
        )?);
    }
    let measured_duration_ns = duration_ns(measured_started.elapsed())?;
    if deadline_misses != 0 {
        tracing::error!(
            event = "headless.performance.deadline_missed",
            deadline_misses,
            "performance workload missed one or more frame deadlines"
        );
    }
    let final_memory = sample_process_memory()
        .map_err(|error| format!("ASTRA_PERFORMANCE_MEMORY_SAMPLE_FAILED: {error}"))?;
    let final_allocation = super::ASTRA_ALLOCATOR.snapshot();
    if final_allocation
        .live_bytes
        .saturating_sub(allocation_baseline.live_bytes)
        > 64 * 1024 * 1024
        || final_memory
            .working_set_bytes
            .saturating_sub(memory_baseline.working_set_bytes)
            > 64 * 1024 * 1024
    {
        return Err("ASTRA_PERFORMANCE_STEADY_MEMORY_GROWTH".into());
    }

    let trace_summary = trace.finish().map_err(|error| error.to_string())?;
    let report = recorder
        .finalize(identity.clone(), measured_duration_ns / 1_000)
        .map_err(|error| error.to_string())?;
    write_json(request.report, &report)?;
    let report_hash = hash_file(request.report)?;
    let manifest = PerformanceTraceManifest {
        schema: PERFORMANCE_TRACE_MANIFEST_SCHEMA.into(),
        identity,
        workload_id: request.workload.id().into(),
        adapter_identity_hash,
        driver_identity_hash: renderer.identity().driver_identity_hash.clone(),
        report_hash,
        trace_hash: trace_summary.trace_hash.to_string(),
        event_count: trace_summary.event_count,
        dropped_event_count: trace_summary.dropped_event_count,
        byte_length: trace_summary.byte_length,
        truncated: trace_summary.truncated,
        timestamps_monotonic: trace_summary.timestamps_monotonic,
    };
    manifest.validate().map_err(|error| error.to_string())?;
    write_json(request.trace_manifest, &manifest)?;
    if !matches!(report.status, astra_core::PerformanceStatus::Pass) {
        return Err("ASTRA_PERFORMANCE_BUDGET_BLOCKED".into());
    }
    Ok(())
}

struct PendingMeasuredFrame {
    index: u64,
    pending: WgpuPendingProfile,
    memory: ProcessMemorySample,
    counters: WgpuFramePerformanceCounters,
    submitted_timestamp_ns: u64,
}

fn record_measured_frame(
    pending: PendingMeasuredFrame,
    submission: astra_platform_common::WgpuProfiledSubmission,
    memory_baseline: &ProcessMemorySample,
    recorder: &mut PerformanceRecorder,
    trace: &mut PerfettoTraceWriter,
) -> Result<u64, String> {
    let end_to_end_ns = submission
        .cpu_submit_ns
        .checked_add(submission.gpu_duration_ns)
        .ok_or_else(|| "ASTRA_PERFORMANCE_FRAME_DURATION_OVERFLOW".to_string())?;
    let deadline_miss = u64::from(end_to_end_ns > FRAME_PERIOD_NS);
    let growth = pending
        .memory
        .working_set_bytes
        .saturating_sub(memory_baseline.working_set_bytes)
        .max(
            pending
                .memory
                .private_bytes
                .saturating_sub(memory_baseline.private_bytes),
        );
    let allocated = pending.counters.engine_allocation_bytes;
    for (metric, value) in [
        ("frame.cpu_ns", submission.cpu_submit_ns),
        ("frame.gpu_ns", submission.gpu_duration_ns),
        ("frame.end_to_end_ns", end_to_end_ns),
        ("deadline.miss_count", deadline_miss),
        ("memory.working_set_bytes", pending.memory.working_set_bytes),
        ("memory.private_bytes", pending.memory.private_bytes),
        ("memory.growth_bytes", growth),
        ("gpu.resource_bytes", pending.counters.gpu_resource_bytes),
        ("gpu.atlas_bytes", pending.counters.atlas_bytes),
        ("cache.decoded_bytes", 0),
        ("gpu.upload_bytes", pending.counters.upload_bytes),
        ("gpu.readback_bytes", pending.counters.readback_bytes),
        ("renderer.draw_calls", pending.counters.draw_calls),
        (
            "renderer.queue_submissions",
            pending.counters.queue_submissions,
        ),
        ("renderer.pipeline_count", pending.counters.pipeline_count),
        ("heap.allocation_bytes", allocated),
        (
            "heap.allocation_count",
            pending.counters.engine_allocation_count,
        ),
    ] {
        recorder
            .record(metric, value)
            .map_err(|error| error.to_string())?;
    }
    let frame_start_ns = pending
        .submitted_timestamp_ns
        .saturating_sub(submission.cpu_submit_ns);
    let cpu_end_ns = frame_start_ns.saturating_add(submission.cpu_submit_ns);
    let gpu_upload_end_ns = cpu_end_ns.saturating_add(submission.atlas_upload_gpu_ns);
    let gpu_scene_end_ns = gpu_upload_end_ns.saturating_add(submission.scene_gpu_ns);
    let trace_event_end_ns = gpu_scene_end_ns
        .saturating_add(submission.filter_gpu_ns)
        .max(pending.submitted_timestamp_ns);
    trace
        .flow(
            "frame.flow",
            "scene_to_gpu",
            1,
            pending.index,
            frame_start_ns,
            PerfettoFlowPhase::Start,
        )
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.prepare_submit",
                1,
                Some(pending.index),
                frame_start_ns,
                submission.scene_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "filter.prepare_submit",
                1,
                Some(pending.index),
                frame_start_ns.saturating_add(submission.scene_cpu_ns),
                submission.filter_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.flow(
                "frame.flow",
                "scene_to_gpu",
                2,
                pending.index,
                cpu_end_ns,
                PerfettoFlowPhase::Step,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.gpu",
                "atlas.upload",
                2,
                Some(pending.index),
                cpu_end_ns,
                submission.atlas_upload_gpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.gpu",
                "scene.pass",
                2,
                Some(pending.index),
                gpu_upload_end_ns,
                submission.scene_gpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.gpu",
                "filter.pass",
                2,
                Some(pending.index),
                gpu_scene_end_ns,
                submission.filter_gpu_ns,
            )
        })
        .and_then(|_| {
            trace.flow(
                "frame.flow",
                "scene_to_gpu",
                2,
                pending.index,
                gpu_scene_end_ns.saturating_add(submission.filter_gpu_ns),
                PerfettoFlowPhase::End,
            )
        })
        .and_then(|_| {
            trace.counter(
                "memory",
                "working_set.bytes",
                trace_event_end_ns,
                pending.memory.working_set_bytes,
            )
        })
        .and_then(|_| {
            trace.counter(
                "memory",
                "private.bytes",
                trace_event_end_ns,
                pending.memory.private_bytes,
            )
        })
        .and_then(|_| {
            trace.counter(
                "renderer",
                "gpu_resource.bytes",
                trace_event_end_ns,
                pending.counters.gpu_resource_bytes,
            )
        })
        .and_then(|_| trace.counter("allocator", "frame.bytes", trace_event_end_ns, allocated))
        .map_err(|error| error.to_string())?;
    Ok(deadline_miss)
}

fn run_warmup(renderer: &mut WgpuOffscreenRenderer, frame: &mut SceneFrame) -> Result<(), String> {
    let started = Instant::now();
    for index in 0..WARMUP_FRAMES {
        let scheduled = started + Duration::from_nanos(FRAME_PERIOD_NS * index);
        if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        animate(frame, index + 1)?;
        renderer
            .submit_frame_profiled(frame)
            .map_err(|error| format!("ASTRA_PERFORMANCE_WARMUP_FAILED: {error}"))?;
    }
    let minimum = Duration::from_nanos(FRAME_PERIOD_NS * WARMUP_FRAMES);
    if let Some(remaining) = minimum.checked_sub(started.elapsed()) {
        thread::sleep(remaining);
    }
    Ok(())
}

fn validate_profiler_overhead(
    renderer: &mut WgpuOffscreenRenderer,
    frame: &mut SceneFrame,
    trace_path: &Path,
) -> Result<(u64, u64), String> {
    let memory_before = sample_process_memory()
        .map_err(|error| format!("ASTRA_PERFORMANCE_MEMORY_SAMPLE_FAILED: {error}"))?;
    let mut disabled = Vec::with_capacity(PROFILER_OVERHEAD_FRAMES as usize);
    let disabled_started = Instant::now();
    for index in 0..PROFILER_OVERHEAD_FRAMES {
        let scheduled = disabled_started + Duration::from_nanos(FRAME_PERIOD_NS * index);
        if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        animate(frame, WARMUP_FRAMES + index + 1)?;
        let started = Instant::now();
        renderer
            .submit_frame(frame)
            .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_BASELINE_FAILED: {error}"))?;
        disabled.push(duration_ns(started.elapsed())?);
    }
    let name = trace_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "ASTRA_PERFORMANCE_TRACE_PATH_INVALID".to_string())?;
    let overhead_path = trace_path.with_file_name(format!(".{name}.overhead.json"));
    let mut writer = PerfettoTraceWriter::create(PerfettoTraceConfig::production(
        &overhead_path,
        "astra-headless",
    ))
    .map_err(|error| error.to_string())?;
    let mut enabled = Vec::with_capacity(PROFILER_OVERHEAD_FRAMES as usize);
    let trace_started = Instant::now();
    let mut pending = VecDeque::with_capacity(3);
    for index in 0..PROFILER_OVERHEAD_FRAMES {
        let scheduled = trace_started + Duration::from_nanos(FRAME_PERIOD_NS * index);
        if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        animate(frame, WARMUP_FRAMES + PROFILER_OVERHEAD_FRAMES + index + 1)?;
        let started = Instant::now();
        let mut resolved_gpu_ns = None;
        if pending.len() >= 3 {
            let oldest = *pending
                .front()
                .expect("bounded timestamp queue is not empty");
            if let Some(previous) = renderer
                .try_resolve_profiled_submission(oldest)
                .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_RESOLVE_FAILED: {error}"))?
            {
                pending.pop_front();
                resolved_gpu_ns = Some(previous.gpu_duration_ns);
            }
        }
        if pending.len() == 4 {
            let previous = renderer
                .resolve_profiled_submission(
                    pending
                        .pop_front()
                        .expect("bounded timestamp queue is not empty"),
                )
                .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_RESOLVE_FAILED: {error}"))?;
            resolved_gpu_ns = Some(previous.gpu_duration_ns);
        }
        let submission = renderer
            .submit_frame_timestamped(frame)
            .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_ENABLED_FAILED: {error}"))?;
        pending.push_back(submission);
        let timestamp_ns = duration_ns(trace_started.elapsed())?;
        let frame_cpu_ns = duration_ns(started.elapsed())?;
        writer
            .complete(
                "profiler",
                "overhead.sample",
                1,
                Some(index),
                timestamp_ns.saturating_sub(frame_cpu_ns),
                frame_cpu_ns,
            )
            .and_then(|_| match resolved_gpu_ns {
                Some(gpu_ns) => writer.counter("profiler", "gpu.duration_ns", timestamp_ns, gpu_ns),
                None => Ok(()),
            })
            .map_err(|error| error.to_string())?;
        enabled.push(duration_ns(started.elapsed())?);
    }
    while let Some(pending) = pending.pop_front() {
        renderer
            .resolve_profiled_submission(pending)
            .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_DRAIN_FAILED: {error}"))?;
    }
    writer.finish().map_err(|error| error.to_string())?;
    let memory_after = sample_process_memory()
        .map_err(|error| format!("ASTRA_PERFORMANCE_MEMORY_SAMPLE_FAILED: {error}"))?;
    fs::remove_file(&overhead_path)
        .map_err(|error| format!("ASTRA_PERFORMANCE_OVERHEAD_TRACE_CLEANUP_FAILED: {error}"))?;
    disabled.sort_unstable();
    enabled.sort_unstable();
    let disabled_p95 = percentile(&disabled, 95).max(1);
    let enabled_p95 = percentile(&enabled, 95);
    let overhead_ppm = enabled_p95
        .saturating_sub(disabled_p95)
        .checked_mul(1_000_000)
        .ok_or_else(|| "ASTRA_PERFORMANCE_PROFILER_RATIO_OVERFLOW".to_string())?
        .div_ceil(FRAME_PERIOD_NS)
        .saturating_add(1_000_000);
    let extra_working_set = memory_after
        .working_set_bytes
        .saturating_sub(memory_before.working_set_bytes)
        .max(
            memory_after
                .private_bytes
                .saturating_sub(memory_before.private_bytes),
        );
    if overhead_ppm > MAX_PROFILER_OVERHEAD_PPM
        || extra_working_set > MAX_PROFILER_WORKING_SET_BYTES
    {
        return Err(format!(
            "ASTRA_PERFORMANCE_PROFILER_OVERHEAD_BLOCKED: overhead_ppm={overhead_ppm} working_set_bytes={extra_working_set} disabled_p95_ns={disabled_p95} enabled_p95_ns={enabled_p95}"
        ));
    }
    Ok((overhead_ppm, extra_working_set))
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

pub(crate) fn scene2d_workload() -> (SceneFrame, SceneFrame) {
    let texture_bytes = [64_u8, 128, 224, 255].repeat(512 * 512);
    let glyph_bytes = vec![192_u8; 64 * 64];
    let texture = TextureFrame {
        width: 512,
        height: 512,
        hash: Hash256::from_sha256(&texture_bytes),
        rgba8: texture_bytes.into(),
    };
    let glyph = GlyphBitmap {
        width: 64,
        height: 64,
        format: GlyphBitmapFormat::Alpha8,
        hash: Hash256::from_sha256(&glyph_bytes),
        pixels: glyph_bytes.into(),
    };
    let mut frame = SceneFrame {
        sequence: 2,
        width: 1920,
        height: 1080,
        clear_rgba: [4, 8, 16, 255],
        commands: draw_commands(),
        semantics: None,
    };
    let mut initialization_commands = Vec::with_capacity(frame.commands.len() + 2);
    initialization_commands.push(SceneCommand::UploadTexture {
        resource_id: "performance.texture.0".into(),
        frame: texture,
    });
    initialization_commands.push(SceneCommand::UploadGlyph {
        resource_id: "performance.glyph.0".into(),
        glyph,
    });
    initialization_commands.extend(frame.commands.clone());
    let initialization = SceneFrame {
        sequence: 1,
        width: frame.width,
        height: frame.height,
        clear_rgba: frame.clear_rgba,
        commands: initialization_commands,
        semantics: None,
    };
    frame.sequence = 2;
    (initialization, frame)
}

fn draw_commands() -> Vec<SceneCommand> {
    let mut fade_params = BTreeMap::new();
    fade_params.insert("amount".into(), FilterParam::Float(0.92));
    vec![
        SceneCommand::SetCamera {
            transform: Transform2D::IDENTITY,
        },
        SceneCommand::Sprite {
            id: "performance.sprite.0".into(),
            texture_id: "performance.texture.0".into(),
            source: None,
            destination: RectI::new(64, 64, 1024, 768),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        SceneCommand::GlyphRun {
            id: "performance.glyph_run.0".into(),
            glyphs: vec![GlyphInstance {
                resource_id: "performance.glyph.0".into(),
                x: 128,
                y: 128,
                rotation_quadrants: 0,
            }],
            rgba: [255, 240, 220, 255],
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        SceneCommand::Mesh2D {
            id: "performance.mesh.0".into(),
            vertices: vec![
                MeshVertex2D {
                    position: [960.0, 128.0],
                    uv: [0.0, 0.0],
                    premultiplied_rgba: [240, 80, 80, 255],
                },
                MeshVertex2D {
                    position: [1760.0, 900.0],
                    uv: [1.0, 1.0],
                    premultiplied_rgba: [80, 240, 80, 255],
                },
                MeshVertex2D {
                    position: [720.0, 900.0],
                    uv: [0.0, 1.0],
                    premultiplied_rgba: [80, 80, 240, 255],
                },
            ],
            indices: vec![0, 1, 2],
            material: MeshMaterial2D::Solid,
            texture_id: None,
            opacity: 0.9,
            blend: BlendMode::Alpha,
        },
        SceneCommand::PushClip {
            rect: RectI::new(32, 32, 1856, 1016),
        },
        SceneCommand::PushOpacity { opacity: 0.98 },
        SceneCommand::Rect {
            id: "performance.rect.0".into(),
            x: 1400,
            y: 120,
            width: 320,
            height: 180,
            rgba: [32, 48, 80, 220],
        },
        SceneCommand::PopOpacity,
        SceneCommand::PopClip,
        SceneCommand::FilterGraph {
            graph: FilterGraph {
                schema: "astra.filter_graph.v1".into(),
                nodes: vec![FilterNode {
                    id: "performance.fade.0".into(),
                    kind: "astra.filter.fade".into(),
                    input: FilterTarget::Final,
                    output: FilterTarget::Final,
                    params: fade_params,
                    deterministic: true,
                    allow_cpu_fallback: false,
                }],
            },
        },
    ]
}

fn animate(frame: &mut SceneFrame, sequence: u64) -> Result<(), String> {
    frame.sequence = sequence + 1;
    let phase = (sequence % 240) as f32 / 240.0;
    let transform = frame
        .commands
        .iter_mut()
        .find_map(|command| match command {
            SceneCommand::SetCamera { transform } => Some(transform),
            _ => None,
        })
        .ok_or_else(|| "ASTRA_PERFORMANCE_CAMERA_COMMAND_MISSING".to_string())?;
    transform.tx = phase * 2.0;
    transform.ty = (1.0 - phase) * 2.0;
    Ok(())
}

fn validate_metric_set(budget: &PerformanceBudget) -> Result<(), String> {
    let actual = budget
        .metrics
        .iter()
        .map(|metric| metric.id.as_str())
        .collect::<BTreeSet<_>>();
    let required = REQUIRED_METRICS.iter().copied().collect::<BTreeSet<_>>();
    if actual != required
        || budget.metrics.iter().any(|metric| {
            let expected = if metric.id.starts_with("profiler.") {
                1
            } else {
                MEASURED_FRAMES as usize
            };
            metric.min_samples != expected || metric.max_samples != expected
        })
    {
        return Err("ASTRA_PERFORMANCE_BUDGET_METRIC_SET_INVALID".into());
    }
    Ok(())
}

fn duration_ns(duration: Duration) -> Result<u64, String> {
    duration
        .as_nanos()
        .try_into()
        .map_err(|_| "ASTRA_PERFORMANCE_DURATION_OVERFLOW".into())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, role: &str) -> Result<T, String> {
    serde_json::from_slice(
        &fs::read(path)
            .map_err(|error| format!("ASTRA_PERFORMANCE_{role}_READ_FAILED: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_PERFORMANCE_{role}_INVALID: {error}"))
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "ASTRA_PERFORMANCE_OUTPUT_PATH_INVALID".to_string())?;
    let temporary = path.with_file_name(format!(".{name}.partial-{}", std::process::id()));
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn hash_file(path: &Path) -> Result<String, String> {
    Ok(Hash256::from_sha256(&fs::read(path).map_err(|error| error.to_string())?).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_platform::{
        GpuAdapterPolicy, GpuBackendPolicy, GpuDeviceTypePolicy, HeadlessReadbackPolicy,
        HeadlessRenderPolicy,
    };

    #[test]
    fn prepared_budget_uses_fixed_production_thresholds() {
        let hash = Hash256::from_sha256(b"fixture").to_string();
        let mut profile = HeadlessHostProfile::reference(
            "windows-x64",
            "performance.fixture",
            hash.clone(),
            hash,
        );
        profile.providers.renderer = "wgpu_offscreen".into();
        profile.gpu_adapter = Some(GpuAdapterPolicy {
            backend: GpuBackendPolicy::Dx12,
            device_type: GpuDeviceTypePolicy::Integrated,
            require_timestamp_query: true,
            adapter_identity_hash: None,
        });
        profile.render_policy = HeadlessRenderPolicy::All;
        profile.readback_policy = HeadlessReadbackPolicy::CheckpointsOnly;
        let temp = tempfile::tempdir().unwrap();
        let profile_path = temp.path().join("profile.json");
        let budget_path = temp.path().join("budget.json");
        write_json(&profile_path, &profile).unwrap();
        prepare_budget(
            &profile_path,
            &budget_path,
            "headless.performance.fixture".into(),
            72_000,
            72_000,
            600_000_000,
            PerformanceBudgetKind::RendererStress,
        )
        .unwrap();
        let budget: PerformanceBudget = read_json(&budget_path, "BUDGET").unwrap();
        assert_eq!(budget.metrics.len(), REQUIRED_METRICS.len());
        assert_eq!(
            budget
                .metrics
                .iter()
                .find(|metric| metric.id == "frame.end_to_end_ns")
                .unwrap()
                .thresholds
                .max_p99,
            Some(FRAME_PERIOD_NS)
        );
    }

    #[test]
    fn product_budget_uses_product_frame_metrics_without_synthetic_overhead_samples() {
        let hash = Hash256::from_sha256(b"fixture").to_string();
        let mut profile = HeadlessHostProfile::reference(
            "windows-x64",
            "performance.fixture",
            hash.clone(),
            hash,
        );
        profile.providers.renderer = "wgpu_offscreen".into();
        profile.gpu_adapter = Some(GpuAdapterPolicy {
            backend: GpuBackendPolicy::Dx12,
            device_type: GpuDeviceTypePolicy::Integrated,
            require_timestamp_query: true,
            adapter_identity_hash: None,
        });
        profile.render_policy = HeadlessRenderPolicy::All;
        profile.readback_policy = HeadlessReadbackPolicy::CheckpointsOnly;
        let temp = tempfile::tempdir().unwrap();
        let profile_path = temp.path().join("profile.json");
        let budget_path = temp.path().join("budget.json");
        write_json(&profile_path, &profile).unwrap();
        prepare_budget(
            &profile_path,
            &budget_path,
            "headless.performance.product.fixture".into(),
            445,
            445,
            1,
            PerformanceBudgetKind::ProductRoute,
        )
        .unwrap();
        let budget: PerformanceBudget = read_json(&budget_path, "BUDGET").unwrap();
        assert!(budget
            .metrics
            .iter()
            .all(|metric| !metric.id.starts_with("profiler.")));
        assert!(budget
            .metrics
            .iter()
            .all(|metric| metric.min_samples == 445));
    }

    #[test]
    fn prepared_performance_profile_requires_timestamped_hardware_gpu() {
        let hash = Hash256::from_sha256(b"fixture").to_string();
        let profile = HeadlessHostProfile::reference(
            "windows-x64",
            "performance.fixture",
            hash.clone(),
            hash,
        );
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input.json");
        let output = temp.path().join("output.json");
        write_json(&input, &profile).unwrap();
        prepare_profile(
            &input,
            &output,
            PerformanceGpuBackend::Dx12,
            PerformanceGpuDeviceType::Integrated,
            None,
        )
        .unwrap();
        let prepared: HeadlessHostProfile = read_json(&output, "PROFILE").unwrap();
        let policy = validate_headless_performance_profile(&prepared).unwrap();
        assert!(policy.require_timestamp_query);
        assert!(matches!(
            policy.device_type,
            GpuDeviceTypePolicy::Integrated
        ));
    }

    #[tokio::test]
    #[ignore = "requires the integrated DX12 performance adapter"]
    async fn stable_scene_submit_has_zero_heap_allocation_p95() {
        let policy = GpuAdapterPolicy {
            backend: GpuBackendPolicy::Dx12,
            device_type: GpuDeviceTypePolicy::Integrated,
            require_timestamp_query: true,
            adapter_identity_hash: None,
        };
        let mut renderer = WgpuOffscreenRenderer::new_with_policy(&policy)
            .await
            .unwrap();
        let (initialization, mut frame) = scene2d_workload();
        renderer.submit_frame_profiled(&initialization).unwrap();
        let mut allocations = Vec::with_capacity(240);
        for sequence in 0..240 {
            animate(&mut frame, sequence + 1).unwrap();
            let pending = renderer.submit_frame_timestamped(&frame).unwrap();
            renderer.resolve_profiled_submission(pending).unwrap();
            allocations.push(renderer.performance_counters().engine_allocation_bytes);
        }
        allocations.sort_unstable();
        assert_eq!(percentile(&allocations, 95), 0, "{allocations:?}");
    }

    #[tokio::test]
    #[ignore = "requires the integrated DX12 performance adapter"]
    async fn timestamp_and_trace_profiling_stays_within_overhead_budget() {
        let policy = GpuAdapterPolicy {
            backend: GpuBackendPolicy::Dx12,
            device_type: GpuDeviceTypePolicy::Integrated,
            require_timestamp_query: true,
            adapter_identity_hash: None,
        };
        let mut renderer = WgpuOffscreenRenderer::new_with_policy(&policy)
            .await
            .unwrap();
        let (initialization, mut frame) = scene2d_workload();
        renderer.submit_frame_profiled(&initialization).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (overhead_ppm, working_set_bytes) = validate_profiler_overhead(
            &mut renderer,
            &mut frame,
            &temp.path().join("overhead.json"),
        )
        .unwrap();
        assert!(overhead_ppm <= MAX_PROFILER_OVERHEAD_PPM, "{overhead_ppm}");
        assert!(working_set_bytes <= MAX_PROFILER_WORKING_SET_BYTES);
    }
}
