use std::{
    collections::VecDeque,
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use astra_observability::{sample_process_memory, PerfettoTraceConfig, PerfettoTraceWriter};
use astra_platform::SceneFrame;
use astra_platform_common::{WgpuOffscreenRenderer, WGPU_TIMESTAMP_RING_SIZE};

use super::*;

pub(crate) fn run_warmup(
    renderer: &mut WgpuOffscreenRenderer,
    frame: &mut SceneFrame,
) -> Result<(), String> {
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

pub(crate) fn validate_profiler_overhead(
    renderer: &mut WgpuOffscreenRenderer,
    frame: &mut SceneFrame,
    trace_path: &Path,
) -> Result<(u64, u64), String> {
    let memory_before = sample_process_memory()
        .map_err(|error| format!("ASTRA_PERFORMANCE_MEMORY_SAMPLE_FAILED: {error}"))?;
    let mut disabled = Vec::with_capacity(PROFILER_OVERHEAD_FRAMES as usize);
    // GPU timestamps are mandatory E2 measurement infrastructure. The disabled
    // side therefore keeps the identical bounded timestamp-query/readback ring
    // and measures the incremental cost of trace serialization, not a different
    // renderer submission path.
    let mut disabled_pending = VecDeque::with_capacity(WGPU_TIMESTAMP_RING_SIZE);
    let disabled_started = Instant::now();
    for index in 0..PROFILER_OVERHEAD_FRAMES {
        let scheduled = disabled_started + Duration::from_nanos(FRAME_PERIOD_NS * index);
        if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        animate(frame, WARMUP_FRAMES + index + 1)?;
        let started = Instant::now();
        if disabled_pending.len() >= WGPU_TIMESTAMP_RING_SIZE - 1 {
            let oldest = *disabled_pending
                .front()
                .expect("bounded timestamp queue is not empty");
            if renderer
                .try_resolve_profiled_submission(oldest)
                .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_BASELINE_FAILED: {error}"))?
                .is_some()
            {
                disabled_pending.pop_front();
            }
        }
        if disabled_pending.len() == WGPU_TIMESTAMP_RING_SIZE {
            renderer
                .resolve_profiled_submission(
                    disabled_pending
                        .pop_front()
                        .expect("bounded timestamp queue is not empty"),
                )
                .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_BASELINE_FAILED: {error}"))?;
        }
        let submission = renderer
            .submit_frame_timestamped(frame)
            .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_BASELINE_FAILED: {error}"))?;
        disabled_pending.push_back(submission);
        disabled.push(duration_ns(started.elapsed())?);
    }
    while let Some(pending) = disabled_pending.pop_front() {
        renderer
            .resolve_profiled_submission(pending)
            .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILER_BASELINE_FAILED: {error}"))?;
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
    let mut trace_write_costs =
        Vec::with_capacity((PROFILER_OVERHEAD_FRAMES / PROFILER_TRACE_SAMPLE_STRIDE + 1) as usize);
    let trace_started = Instant::now();
    let mut pending = VecDeque::with_capacity(WGPU_TIMESTAMP_RING_SIZE);
    for index in 0..PROFILER_OVERHEAD_FRAMES {
        let scheduled = trace_started + Duration::from_nanos(FRAME_PERIOD_NS * index);
        if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        animate(frame, WARMUP_FRAMES + PROFILER_OVERHEAD_FRAMES + index + 1)?;
        let started = Instant::now();
        let mut resolved_gpu_ns = None;
        if pending.len() >= WGPU_TIMESTAMP_RING_SIZE - 1 {
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
        if pending.len() == WGPU_TIMESTAMP_RING_SIZE {
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
        // Mirror the production trace policy: timestamp every submitted frame for the GPU
        // distribution, but serialize detailed CPU slices only once per bounded sample stride.
        // The overhead gate must measure the configuration that the E2 run actually uses.
        if index % PROFILER_TRACE_SAMPLE_STRIDE == 0 {
            let trace_write_started = Instant::now();
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
                    Some(gpu_ns) => {
                        writer.counter("profiler", "gpu.duration_ns", timestamp_ns, gpu_ns)
                    }
                    None => Ok(()),
                })
                .map_err(|error| error.to_string())?;
            trace_write_costs.push(duration_ns(trace_write_started.elapsed())?);
        }
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
    let disabled_p95 = percentile(&disabled, 95).max(1);
    // Renderer submission and timestamp readback are deliberately identical in
    // both samples. Measuring two independent GPU runs as "disabled" and
    // "enabled" therefore reports DX12 scheduling variance as profiler cost.
    // Measure the incremental, bounded trace serialization work in the same
    // enabled submission loop instead, then conservatively amortize a p95
    // detailed sample across its fixed cadence. This is the per-frame cost that
    // the production recorder adds; regular frames do not serialize trace data.
    trace_write_costs.sort_unstable();
    let trace_write_p95 = percentile(&trace_write_costs, 95);
    let amortized_trace_p95 = trace_write_p95.div_ceil(PROFILER_TRACE_SAMPLE_STRIDE);
    let overhead_ppm = amortized_trace_p95
        .checked_mul(1_000_000)
        .ok_or_else(|| "ASTRA_PERFORMANCE_PROFILER_RATIO_OVERFLOW".to_string())?
        .div_ceil(disabled_p95);
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
            "ASTRA_PERFORMANCE_PROFILER_OVERHEAD_BLOCKED: overhead_ppm={overhead_ppm} working_set_bytes={extra_working_set} baseline_p95_ns={disabled_p95} trace_write_p95_ns={trace_write_p95} amortized_trace_p95_ns={amortized_trace_p95}"
        ));
    }
    Ok((overhead_ppm, extra_working_set))
}
