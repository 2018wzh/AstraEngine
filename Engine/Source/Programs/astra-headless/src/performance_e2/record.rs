use astra_core::PerformanceRecorder;
use astra_observability::{PerfettoFlowPhase, PerfettoTraceWriter, ProcessMemorySample};
use astra_platform_common::{WgpuFramePerformanceCounters, WgpuPendingProfile};

use super::*;

pub(crate) struct PendingMeasuredFrame {
    pub(crate) index: u64,
    pub(crate) pending: WgpuPendingProfile,
    pub(crate) memory: ProcessMemorySample,
    pub(crate) counters: WgpuFramePerformanceCounters,
    pub(crate) submitted_timestamp_ns: u64,
}

pub(crate) fn record_measured_frame(
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
    // WGPU timestamp queries provide durations but do not share the host's
    // monotonic clock epoch. Anchor every correlated slice to the measured
    // frame timestamp instead of inventing an absolute GPU timeline.
    let frame_timestamp_ns = pending.submitted_timestamp_ns;
    // The report above retains all 72,000 measured frames. Perfetto is a
    // bounded hotspot timeline, so serialize one correlated Scene2D sample per
    // fixed stride; this keeps trace I/O outside the p95 frame population while
    // retaining deterministic representative CPU/GPU/flow evidence.
    if !pending.index.is_multiple_of(PROFILER_TRACE_SAMPLE_STRIDE) {
        return Ok(deadline_miss);
    }
    trace
        .flow(
            "frame.flow",
            "scene_to_gpu",
            1,
            pending.index,
            frame_timestamp_ns,
            PerfettoFlowPhase::Start,
        )
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "wgpu.prepare_submit",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.cpu_submit_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.prepare_submit",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "filter.prepare_submit",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.filter_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.commands",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_command_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.atlas",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_atlas_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.geometry",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_geometry_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.vertex_upload",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_vertex_upload_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.render_submit",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_render_submit_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.render_encode",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_render_encode_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.cpu",
                "scene.queue_submit",
                1,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_queue_submit_cpu_ns,
            )
        })
        .and_then(|_| {
            trace.flow(
                "frame.flow",
                "scene_to_gpu",
                2,
                pending.index,
                frame_timestamp_ns,
                PerfettoFlowPhase::Step,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.gpu",
                "atlas.upload",
                2,
                Some(pending.index),
                frame_timestamp_ns,
                submission.atlas_upload_gpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.gpu",
                "scene.pass",
                2,
                Some(pending.index),
                frame_timestamp_ns,
                submission.scene_gpu_ns,
            )
        })
        .and_then(|_| {
            trace.complete(
                "renderer.gpu",
                "filter.pass",
                2,
                Some(pending.index),
                frame_timestamp_ns,
                submission.filter_gpu_ns,
            )
        })
        .and_then(|_| {
            trace.flow(
                "frame.flow",
                "scene_to_gpu",
                2,
                pending.index,
                frame_timestamp_ns,
                PerfettoFlowPhase::End,
            )
        })
        .and_then(|_| {
            trace.counter(
                "memory",
                "working_set.bytes",
                frame_timestamp_ns,
                pending.memory.working_set_bytes,
            )
        })
        .and_then(|_| {
            trace.counter(
                "memory",
                "private.bytes",
                frame_timestamp_ns,
                pending.memory.private_bytes,
            )
        })
        .and_then(|_| {
            trace.counter(
                "renderer",
                "gpu_resource.bytes",
                frame_timestamp_ns,
                pending.counters.gpu_resource_bytes,
            )
        })
        .and_then(|_| trace.counter("allocator", "frame.bytes", frame_timestamp_ns, allocated))
        .map_err(|error| error.to_string())?;
    Ok(deadline_miss)
}
