use std::{path::Path, sync::atomic::Ordering, time::Instant};

use astra_core::{
    PerformanceRunIdentity, PerformanceTraceManifest, PERFORMANCE_TRACE_MANIFEST_SCHEMA,
};
use astra_observability::{sample_process_memory, PerfettoFlowPhase};
use astra_platform::PlatformError;
use astra_platform_headless::HeadlessGpuFrameSample;

use super::*;

impl ProductPerformanceRecorder {
    pub(crate) fn record_gpu_frame_sample(
        &self,
        state: &mut RecorderState,
        sample: HeadlessGpuFrameSample,
        product_sample: BoundProductSample,
    ) -> Result<(), PlatformError> {
        if state.warmup_frames_remaining > 0 {
            if let Some(flow_id) = sample.input_flow_id {
                let timestamp_ns: u64 = state
                    .started
                    .elapsed()
                    .as_nanos()
                    .try_into()
                    .map_err(|_| performance_error("trace timestamp overflowed"))?;
                state
                    .trace
                    .flow(
                        "frame.flow",
                        "physical_input_to_gpu",
                        TRACE_TRACK_GPU_FLOW,
                        flow_id,
                        timestamp_ns,
                        PerfettoFlowPhase::End,
                    )
                    .map_err(|error| performance_error(&error.to_string()))?;
            }
            state.warmup_frames_remaining -= 1;
            if state.warmup_frames_remaining == 0 {
                state.measurement_started = Some(Instant::now());
                state.last_memory_sample = None;
                state.memory_baseline = None;
                state.measurement_frame_count = 0;
            }
            return Ok(());
        }
        state.measurement_frame_count = state
            .measurement_frame_count
            .checked_add(1)
            .ok_or_else(|| performance_error("measurement frame count overflowed"))?;
        let timestamp_ns: u64 = state
            .started
            .elapsed()
            .as_nanos()
            .try_into()
            .map_err(|_| performance_error("trace timestamp overflowed"))?;
        let cpu_ns = product_sample
            .cpu_ns
            .checked_add(sample.scene_build_ns)
            .and_then(|value| value.checked_add(sample.cpu_submit_ns))
            .ok_or_else(|| performance_error("CPU frame duration overflowed"))?;
        let end_to_end_ns = cpu_ns
            .checked_add(sample.gpu_duration_ns)
            .ok_or_else(|| performance_error("frame duration overflowed"))?;
        state.deadline_miss_count = state
            .deadline_miss_count
            .checked_add(u64::from(end_to_end_ns > FRAME_DEADLINE_NS))
            .ok_or_else(|| performance_error("deadline miss counter overflowed"))?;
        if sample.sequence == 1
            || sample.sequence.is_multiple_of(60)
            || state.last_memory_sample.is_none()
        {
            let memory = sample_process_memory()
                .map_err(|_| performance_error("process memory sampling failed"))?;
            state.last_memory_sample = Some((memory.working_set_bytes, memory.private_bytes));
            state
                .memory_baseline
                .get_or_insert((memory.working_set_bytes, memory.private_bytes));
        }
        let (working_set, private_bytes) = state.last_memory_sample.unwrap();
        let (baseline_working_set, baseline_private) = state.memory_baseline.unwrap();
        let memory_growth = working_set
            .saturating_sub(baseline_working_set)
            .max(private_bytes.saturating_sub(baseline_private));
        let cache_bytes = self.decoded_cache_bytes.load(Ordering::Relaxed);
        let allocation = astra_observability::allocation_snapshot();
        for (metric, value) in [
            ("frame.cpu_ns", cpu_ns),
            ("frame.gpu_ns", sample.gpu_duration_ns),
            ("frame.end_to_end_ns", end_to_end_ns),
            (
                "deadline.miss_count",
                u64::from(end_to_end_ns > FRAME_DEADLINE_NS),
            ),
            ("memory.working_set_bytes", working_set),
            ("memory.private_bytes", private_bytes),
            ("memory.growth_bytes", memory_growth),
            ("gpu.resource_bytes", sample.gpu_resource_bytes),
            ("gpu.atlas_bytes", sample.atlas_bytes),
            ("cache.decoded_bytes", cache_bytes),
            ("gpu.upload_bytes", sample.upload_bytes),
            ("gpu.readback_bytes", sample.readback_bytes),
            ("renderer.draw_calls", sample.draw_calls),
            ("renderer.queue_submissions", sample.queue_submissions),
            ("renderer.pipeline_count", sample.pipeline_count),
            ("heap.allocation_bytes", sample.heap_allocation_bytes),
            ("heap.allocation_count", sample.heap_allocation_count),
        ] {
            state
                .recorder
                .record(metric, value)
                .map_err(|error| performance_error(&error.to_string()))?;
        }
        if state.measurement_frame_count == state.measurement_sample_capacity {
            // Product input sequences can contain additional physical events
            // after their 72,000-frame steady segment. Stop precisely at the
            // bounded budget instead of recording one extra frame and failing
            // after an otherwise valid ten-minute measurement.
            state.gpu_measurement_cutoff = Some(sample.sequence);
            state.measurement_stopped = Some(Instant::now());
        }
        // Timestamp queries resolve on the next materialization so the CPU never
        // waits for the just-submitted frame. The report retains every measured
        // frame; the trace emits a bounded representative timeline.
        let active_flow = sample.input_flow_id;
        // The report records every frame. Perfetto phase slices are sampled at
        // a fixed cadence so a ten-minute 120 Hz run remains below the bounded
        // trace writer limit while still preserving representative CPU phase
        // timelines and all correlated physical-input flows.
        // A full 73,200-frame product run still records its per-frame budget
        // counters and physical-input flow. One detailed phase sample per
        // thirty-two submitted frames keeps the trace below the hard two-million
        // event contract on the highest-density product workload.
        let trace_detailed_phase = trace_detailed_phase(sample.sequence);
        if trace_detailed_phase {
            state
                .trace
                .complete(
                    "frame.timeline",
                    "frame.end_to_end",
                    trace_track("frame.end_to_end"),
                    Some(sample.sequence),
                    timestamp_ns,
                    end_to_end_ns,
                )
                .map_err(|error| performance_error(&error.to_string()))?;
            for (domain, name, duration_ns) in [
                (
                    "product.bound",
                    "product.runtime",
                    product_sample.phases.runtime_tick_ns,
                ),
                (
                    "product.bound",
                    "product.ui",
                    product_sample.phases.ui_layout_paint_ns,
                ),
                (
                    "product.bound",
                    "product.media",
                    product_sample.phases.media_decode_ns,
                ),
                (
                    "product.bound",
                    "product.save_load",
                    product_sample.phases.save_load_ns,
                ),
                (
                    "product.bound",
                    "product.ui_tree_build",
                    product_sample.phases.ui_tree_build_ns,
                ),
                (
                    "product.bound",
                    "product.ui_action_dispatch",
                    product_sample.phases.ui_action_dispatch_ns,
                ),
                (
                    "product.bound",
                    "product.runtime_host_step",
                    product_sample.phases.ui_runtime_host_step_ns,
                ),
                (
                    "product.bound",
                    "product.runtime_output_decode",
                    product_sample.phases.ui_runtime_output_decode_ns,
                ),
                (
                    "product.bound",
                    "product.runtime_render",
                    product_sample.phases.ui_runtime_render_ns,
                ),
                (
                    "product.bound",
                    "product.stage_prepare",
                    product_sample.phases.vn_stage_prepare_ns,
                ),
                (
                    "product.bound",
                    "product.stage_scene",
                    product_sample.phases.vn_stage_scene_ns,
                ),
                (
                    "product.bound",
                    "product.stage_texture",
                    product_sample.phases.vn_stage_texture_ns,
                ),
                (
                    "product.bound",
                    "product.stage_command",
                    product_sample.phases.vn_stage_command_ns,
                ),
                (
                    "product.bound",
                    "product.stage_lifecycle",
                    product_sample.phases.vn_stage_lifecycle_ns,
                ),
                (
                    "product.bound",
                    "product.scene_compose",
                    product_sample.phases.vn_scene_compose_ns,
                ),
                (
                    "product.bound",
                    "product.ui_model_binding",
                    product_sample.phases.ui_model_binding_ns,
                ),
                (
                    "product.bound",
                    "product.ui_controller",
                    product_sample.phases.ui_controller_ns,
                ),
                (
                    "product.bound",
                    "product.ui_frame_model",
                    product_sample.phases.ui_frame_model_ns,
                ),
                (
                    "product.bound",
                    "product.ui_text_scene",
                    product_sample.phases.ui_text_scene_ns,
                ),
                (
                    "product.bound",
                    "product.ui_text_layout",
                    product_sample.phases.ui_text_layout_ns,
                ),
                (
                    "product.bound",
                    "product.ui_text_resource",
                    product_sample.phases.ui_text_resource_ns,
                ),
                (
                    "product.bound",
                    "product.ui_text_compose",
                    product_sample.phases.ui_text_compose_ns,
                ),
                (
                    "product.bound",
                    "product.ui_present_scene",
                    product_sample.phases.ui_present_scene_ns,
                ),
                (
                    "product.bound",
                    "product.ui_request_validation",
                    product_sample.phases.ui_request_validation_ns,
                ),
                (
                    "product.bound",
                    "product.ui_input_routing",
                    product_sample.phases.ui_input_routing_ns,
                ),
                (
                    "product.bound",
                    "product.ui_layout_finalize",
                    product_sample.phases.ui_layout_finalize_ns,
                ),
                (
                    "product.bound",
                    "product.ui_semantics",
                    product_sample.phases.ui_semantics_ns,
                ),
                (
                    "product.bound",
                    "product.ui_update_layout",
                    product_sample.phases.ui_update_layout_ns,
                ),
                (
                    "product.bound",
                    "product.ui_paint_conversion",
                    product_sample.phases.ui_paint_conversion_ns,
                ),
                (
                    "product.bound",
                    "product.ui_output_validation",
                    product_sample.phases.ui_output_validation_ns,
                ),
                (
                    "product.bound",
                    "product.ui_host_scene",
                    product_sample.phases.ui_host_scene_ns,
                ),
            ] {
                trace_complete_nonzero(
                    &mut state.trace,
                    domain,
                    name,
                    trace_track(name),
                    sample.sequence,
                    timestamp_ns,
                    duration_ns,
                )
                .map_err(|error| performance_error(&error))?;
            }
        }
        let phases = [
            ("frame.cpu", "frame.cpu", cpu_ns),
            ("runtime.cpu", "scene.build", sample.scene_build_ns),
            ("runtime.cpu", "scene.digest", sample.scene_digest_ns),
            (
                "runtime.cpu",
                "scene.validation",
                sample.scene_validation_ns,
            ),
            ("runtime.cpu", "scene.pending", sample.scene_pending_ns),
            ("renderer.cpu", "wgpu.prepare_submit", sample.cpu_submit_ns),
            (
                "renderer.cpu",
                "scene.commands",
                sample.scene_command_cpu_ns,
            ),
            ("renderer.cpu", "scene.atlas", sample.scene_atlas_cpu_ns),
            (
                "renderer.cpu",
                "scene.geometry",
                sample.scene_geometry_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.vertex_upload",
                sample.scene_vertex_upload_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.render_submit",
                sample.scene_render_submit_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.render_encode",
                sample.scene_render_encode_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.queue_submit",
                sample.scene_queue_submit_cpu_ns,
            ),
            ("renderer.gpu", "atlas.upload", sample.atlas_upload_gpu_ns),
            ("renderer.gpu", "scene.pass", sample.scene_gpu_ns),
            ("renderer.gpu", "filter.pass", sample.filter_gpu_ns),
        ];
        if trace_detailed_phase {
            for (domain, name, duration_ns) in phases {
                trace_complete_nonzero(
                    &mut state.trace,
                    domain,
                    name,
                    trace_track(name),
                    sample.sequence,
                    timestamp_ns,
                    duration_ns,
                )
                .map_err(|error| performance_error(&error))?;
            }
        }
        if let Some(flow_id) = active_flow {
            state
                .trace
                .flow(
                    "frame.flow",
                    "physical_input_to_gpu",
                    TRACE_TRACK_GPU_FLOW,
                    flow_id,
                    timestamp_ns,
                    PerfettoFlowPhase::Step,
                )
                .and_then(|_| {
                    state.trace.flow(
                        "frame.flow",
                        "physical_input_to_gpu",
                        TRACE_TRACK_GPU_FLOW,
                        flow_id,
                        timestamp_ns,
                        PerfettoFlowPhase::End,
                    )
                })
                .map_err(|error| performance_error(&error.to_string()))?;
        }
        for (domain, name, value, emit_on_change) in [
            ("memory", "working_set.bytes", working_set, false),
            ("memory", "private.bytes", private_bytes, false),
            ("cache", "decoded.bytes", cache_bytes, true),
            (
                "renderer",
                "gpu_resource.bytes",
                sample.gpu_resource_bytes,
                true,
            ),
            ("renderer", "atlas.bytes", sample.atlas_bytes, true),
            ("renderer", "upload.bytes", sample.upload_bytes, true),
            ("renderer", "readback.bytes", sample.readback_bytes, true),
            ("renderer", "draw_calls.count", sample.draw_calls, true),
            (
                "renderer",
                "queue_submissions.count",
                sample.queue_submissions,
                true,
            ),
            ("renderer", "pipeline.count", sample.pipeline_count, true),
            ("deadline", "miss.count", state.deadline_miss_count, true),
            (
                "allocator",
                "frame.bytes",
                sample.heap_allocation_bytes,
                true,
            ),
            (
                "allocator",
                "scene_command.bytes",
                sample.command_allocation_bytes,
                true,
            ),
            (
                "allocator",
                "scene_atlas.bytes",
                sample.atlas_allocation_bytes,
                true,
            ),
            (
                "allocator",
                "scene_geometry.bytes",
                sample.geometry_allocation_bytes,
                true,
            ),
            ("allocator", "live.bytes", allocation.live_bytes, false),
            (
                "allocator",
                "peak_live.bytes",
                allocation.peak_live_bytes,
                false,
            ),
            (
                "allocator",
                "allocated_since_arm.bytes",
                allocation
                    .allocated_bytes
                    .saturating_sub(state.allocation_baseline.allocated_bytes),
                false,
            ),
            (
                "allocator",
                "allocations_since_arm.count",
                allocation
                    .allocation_count
                    .saturating_sub(state.allocation_baseline.allocation_count),
                false,
            ),
        ] {
            trace_counter_journaled(state, domain, name, timestamp_ns, value, emit_on_change)
                .map_err(|error| performance_error(&error))?;
        }
        Ok(())
    }
}

impl ProductPerformanceRecorder {
    pub fn finish(
        &self,
        identity: PerformanceRunIdentity,
        workload_id: &str,
        adapter_identity_hash: String,
        driver_identity_hash: String,
        report_path: &Path,
        trace_manifest_path: &Path,
    ) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard.take().ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if !state.armed {
            return Err("ASTRA_PERFORMANCE_START_SEQUENCE_NOT_REACHED".into());
        }
        if state.gpu_measurement_cutoff.is_none() {
            return Err("ASTRA_PERFORMANCE_GPU_MEASUREMENT_NOT_STOPPED".into());
        }
        let measurement_started = state
            .measurement_started
            .ok_or("ASTRA_PERFORMANCE_MEASUREMENT_NOT_STARTED")?;
        let measurement_stopped = state
            .measurement_stopped
            .ok_or("ASTRA_PERFORMANCE_MEASUREMENT_NOT_STOPPED")?;
        if measurement_stopped < measurement_started {
            return Err("ASTRA_PERFORMANCE_MEASUREMENT_TIME_REVERSED".into());
        }
        let duration_us =
            measurement_duration_us(state.measurement_frame_count, state.presentation_rate_hz)?;
        let trace_summary = state.trace.finish().map_err(|error| error.to_string())?;
        let report = state
            .recorder
            .finalize(identity.clone(), duration_us)
            .map_err(|error| error.to_string())?;
        crate::write_atomic_json(report_path, &report)?;
        let report_hash = crate::hash_file(report_path)?;
        let manifest = PerformanceTraceManifest {
            schema: PERFORMANCE_TRACE_MANIFEST_SCHEMA.into(),
            identity,
            workload_id: workload_id.into(),
            adapter_identity_hash,
            driver_identity_hash,
            report_hash,
            trace_hash: trace_summary.trace_hash.to_string(),
            event_count: trace_summary.event_count,
            dropped_event_count: trace_summary.dropped_event_count,
            byte_length: trace_summary.byte_length,
            truncated: trace_summary.truncated,
            timestamps_monotonic: trace_summary.timestamps_monotonic,
        };
        manifest.validate().map_err(|error| error.to_string())?;
        crate::write_atomic_json(trace_manifest_path, &manifest)?;
        if !matches!(report.status, astra_core::PerformanceStatus::Pass) {
            return Err("ASTRA_PERFORMANCE_BUDGET_BLOCKED".into());
        }
        Ok(())
    }
}
