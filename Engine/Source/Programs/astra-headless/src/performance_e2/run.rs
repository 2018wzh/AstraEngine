use std::{
    collections::VecDeque,
    thread,
    time::{Duration, Instant},
};

use astra_core::{
    Hash256, PerformanceBudget, PerformanceRecorder, PerformanceRunIdentity,
    PerformanceTraceManifest, PERFORMANCE_TRACE_MANIFEST_SCHEMA,
};
use astra_observability::{sample_process_memory, PerfettoTraceConfig, PerfettoTraceWriter};
use astra_package::AstraContainerReader;
use astra_platform::{validate_headless_performance_profile, HeadlessHostProfile};
use astra_platform_common::{WgpuOffscreenRenderer, WGPU_TIMESTAMP_RING_SIZE};

use super::*;

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
    let performance_scheduling = astra_platform_common::PerformanceSchedulingGuard::activate()?;

    let gpu_instance =
        astra_platform_common::native_wgpu_instance().map_err(|error| error.to_string())?;
    let mut renderer = WgpuOffscreenRenderer::new_for_performance(
        &gpu_instance,
        gpu_policy,
        profile.max_gpu_resource_bytes,
        profile.viewport_width,
        profile.viewport_height,
    )
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
    let (profiler_overhead_ppm, profiler_working_set_bytes) =
        validate_profiler_overhead(&mut renderer, &mut frame, request.trace)?;
    let memory_baseline = sample_process_memory()
        .map_err(|error| format!("ASTRA_PERFORMANCE_MEMORY_SAMPLE_FAILED: {error}"))?;
    let allocation_baseline = crate::ASTRA_ALLOCATOR.snapshot();

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
    let mut pending_frames: VecDeque<PendingMeasuredFrame> =
        VecDeque::with_capacity(WGPU_TIMESTAMP_RING_SIZE);
    for index in 0..MEASURED_FRAMES {
        let scheduled = measured_started + Duration::from_nanos(FRAME_PERIOD_NS * index);
        if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        if pending_frames.len() >= WGPU_TIMESTAMP_RING_SIZE - 1 {
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
        if pending_frames.len() == WGPU_TIMESTAMP_RING_SIZE {
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
    let required_duration = Duration::from_nanos(FRAME_PERIOD_NS * MEASURED_FRAMES);
    if let Some(remaining) = required_duration.checked_sub(measured_started.elapsed()) {
        thread::sleep(remaining);
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
    let final_allocation = crate::ASTRA_ALLOCATOR.snapshot();
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
    performance_scheduling.restore()?;
    if !matches!(report.status, astra_core::PerformanceStatus::Pass) {
        return Err("ASTRA_PERFORMANCE_BUDGET_BLOCKED".into());
    }
    Ok(())
}
