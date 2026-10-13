use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Instant,
};

use astra_core::{PerformanceBudget, PerformanceRecorder};
use astra_headless_protocol::ProductPerformanceSample;
use astra_observability::{
    sample_process_memory, PerfettoFlowPhase, PerfettoTraceConfig, PerfettoTraceWriter,
};

use super::*;

impl ProductPerformanceRecorder {
    pub fn create(
        budget: PerformanceBudget,
        trace_path: &Path,
        warmup_frames: u64,
        presentation_rate_hz: u32,
        start_sequence: u64,
    ) -> Result<Self, String> {
        budget.validate().map_err(|error| error.to_string())?;
        validate_product_budget(&budget)?;
        let measurement_sample_capacity = budget
            .metrics
            .first()
            .map(|metric| metric.max_samples)
            .ok_or("ASTRA_PERFORMANCE_PRODUCT_BUDGET_EMPTY")?
            .try_into()
            .map_err(|_| "ASTRA_PERFORMANCE_PRODUCT_SAMPLE_CAPACITY_OVERFLOW")?;
        if budget.metrics.iter().any(|metric| {
            u64::try_from(metric.max_samples).ok() != Some(measurement_sample_capacity)
        }) {
            return Err("ASTRA_PERFORMANCE_PRODUCT_SAMPLE_CAPACITY_MISMATCH".into());
        }
        if presentation_rate_hz != astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ {
            return Err("ASTRA_PERFORMANCE_PRESENTATION_RATE_INVALID".into());
        }
        if start_sequence == 0 {
            return Err("ASTRA_PERFORMANCE_START_SEQUENCE_INVALID".into());
        }
        let started = Instant::now();
        Ok(Self {
            state: Mutex::new(Some(RecorderState {
                recorder: PerformanceRecorder::new(budget).map_err(|error| error.to_string())?,
                trace: PerfettoTraceWriter::create(PerfettoTraceConfig::production(
                    trace_path,
                    "astra-headless-product",
                ))
                .map_err(|error| error.to_string())?,
                started,
                last_memory_sample: None,
                memory_baseline: None,
                allocation_baseline: astra_observability::allocation_snapshot(),
                active_input_flow: None,
                pending_product_sample: None,
                product_sample_by_gpu_sequence: BTreeMap::new(),
                warmup_frames_remaining: warmup_frames,
                measurement_started: (warmup_frames == 0).then(Instant::now),
                measurement_stopped: None,
                measurement_frame_count: 0,
                measurement_sample_capacity,
                next_frame_deadline: None,
                paced_frame_count: 0,
                presentation_rate_hz,
                start_sequence,
                armed: false,
                first_gpu_sequence: None,
                last_paced_gpu_sequence: None,
                gpu_measurement_cutoff: None,
                trace_counters: BTreeMap::new(),
                deadline_miss_count: 0,
                skipped_presentation_samples: 0,
            })),
            decoded_cache_bytes: AtomicU64::new(0),
        })
    }

    pub fn set_decoded_cache_bytes(&self, bytes: u64) {
        self.decoded_cache_bytes.store(bytes, Ordering::Relaxed);
    }

    pub fn record_product_sample(
        &self,
        sequence: u64,
        sample: ProductPerformanceSample,
    ) -> Result<(), String> {
        {
            let guard = self
                .state
                .lock()
                .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
            let state = guard
                .as_ref()
                .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
            if state.armed
                && state
                    .active_input_flow
                    .as_ref()
                    .is_none_or(|flow| flow.sequence != sequence)
            {
                return Err("ASTRA_PERFORMANCE_PRODUCT_FLOW_SEQUENCE_MISMATCH".into());
            }
        }
        self.record_product_sample_inner(sample)
    }

    pub(crate) fn record_product_sample_inner(
        &self,
        sample: ProductPerformanceSample,
    ) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if !state.armed {
            return Ok(());
        }
        let correlation_id = state.active_input_flow.as_ref().map(|flow| flow.sequence);
        let timestamp_ns = elapsed_ns(state.started)?;
        let phases = [
            ("runtime.cpu", "runtime.tick_action", sample.runtime_tick_ns),
            ("vn.cpu", "vn.step", sample.vn_step_ns),
            ("ui.cpu", "ui.layout_paint", sample.ui_layout_paint_ns),
            (
                "ui.cpu",
                "ui.request_validation",
                sample.ui_request_validation_ns,
            ),
            ("ui.cpu", "ui.input_routing", sample.ui_input_routing_ns),
            ("ui.cpu", "ui.tree_build", sample.ui_tree_build_ns),
            ("ui.cpu", "ui.layout_finalize", sample.ui_layout_finalize_ns),
            ("ui.cpu", "ui.semantics", sample.ui_semantics_ns),
            ("ui.cpu", "ui.update_layout", sample.ui_update_layout_ns),
            (
                "ui.cpu",
                "ui.paint_conversion",
                sample.ui_paint_conversion_ns,
            ),
            (
                "ui.cpu",
                "ui.output_validation",
                sample.ui_output_validation_ns,
            ),
            ("ui.cpu", "ui.host_scene", sample.ui_host_scene_ns),
            ("ui.cpu", "ui.model_binding", sample.ui_model_binding_ns),
            ("ui.cpu", "ui.controller", sample.ui_controller_ns),
            ("ui.cpu", "ui.frame_model", sample.ui_frame_model_ns),
            ("ui.cpu", "ui.text_scene", sample.ui_text_scene_ns),
            ("ui.cpu", "ui.text_layout", sample.ui_text_layout_ns),
            ("ui.cpu", "ui.text_resource", sample.ui_text_resource_ns),
            ("ui.cpu", "ui.text_compose", sample.ui_text_compose_ns),
            ("ui.cpu", "ui.action_dispatch", sample.ui_action_dispatch_ns),
            ("ui.cpu", "ui.present_scene", sample.ui_present_scene_ns),
            (
                "vn.cpu",
                "vn.runtime_host_step",
                sample.ui_runtime_host_step_ns,
            ),
            (
                "vn.cpu",
                "vn.runtime_output_decode",
                sample.ui_runtime_output_decode_ns,
            ),
            ("vn.cpu", "vn.stage_prepare", sample.vn_stage_prepare_ns),
            ("vn.cpu", "vn.stage_scene", sample.vn_stage_scene_ns),
            ("vn.cpu", "vn.stage_texture", sample.vn_stage_texture_ns),
            ("vn.cpu", "vn.stage_command", sample.vn_stage_command_ns),
            ("vn.cpu", "vn.stage_lifecycle", sample.vn_stage_lifecycle_ns),
            ("vn.cpu", "vn.scene_compose", sample.vn_scene_compose_ns),
            ("vn.cpu", "vn.runtime_render", sample.ui_runtime_render_ns),
            ("media.cpu", "media.decode_mix", sample.media_decode_ns),
            ("media.cpu", "media.prewarm", sample.media_prewarm_ns),
            (
                "media.cpu",
                "media.provider_decode",
                sample.media_provider_decode_ns,
            ),
            (
                "media.cpu",
                "media.parse_convert",
                sample.media_parse_convert_ns,
            ),
            ("media.cpu", "media.mixer", sample.media_mixer_ns),
            (
                "media.audio",
                "media.audio_query",
                sample.media_audio_query_ns,
            ),
            (
                "media.audio",
                "media.audio_render",
                sample.media_audio_render_ns,
            ),
            (
                "media.audio",
                "media.audio_submit",
                sample.media_audio_submit_ns,
            ),
            (
                "media.audio",
                "media.audio_completion",
                sample.media_audio_completion_ns,
            ),
            ("save.cpu", "save_load", sample.save_load_ns),
        ];
        if correlation_id.is_none_or(trace_detailed_phase) {
            for (domain, name, duration_ns) in phases {
                if duration_ns == 0 {
                    continue;
                }
                state
                    .trace
                    .complete(
                        domain,
                        name,
                        trace_track(name),
                        correlation_id,
                        timestamp_ns,
                        duration_ns,
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        let product_cpu_ns = sample
            .runtime_tick_ns
            .checked_add(sample.ui_layout_paint_ns)
            .and_then(|value| value.checked_add(sample.media_decode_ns))
            .and_then(|value| value.checked_add(sample.save_load_ns))
            .ok_or("ASTRA_PERFORMANCE_PRODUCT_CPU_OVERFLOW")?;
        if sample.bind_to_next_presentation {
            if correlation_id.is_none() {
                return Err("ASTRA_PERFORMANCE_PRESENTATION_SAMPLE_OUTSIDE_INPUT_FLOW".into());
            }
            if state
                .pending_product_sample
                .replace(BoundProductSample {
                    cpu_ns: product_cpu_ns,
                    phases: sample,
                })
                .is_some()
            {
                return Err("ASTRA_PERFORMANCE_PRESENTATION_SAMPLE_NESTED".into());
            }
        }
        Ok(())
    }

    pub(crate) fn finish_presentation_scope(&self) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if !state.armed {
            return Ok(());
        }
        if state.pending_product_sample.take().is_some() {
            state.skipped_presentation_samples = state
                .skipped_presentation_samples
                .checked_add(1)
                .ok_or("ASTRA_PERFORMANCE_SKIPPED_PRESENTATION_COUNTER_OVERFLOW")?;
            let timestamp_ns = elapsed_ns(state.started)?;
            state
                .trace
                .counter(
                    "presentation",
                    "skipped_sample.count",
                    timestamp_ns,
                    state.skipped_presentation_samples,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn begin_input_flow(&self, sequence: u64) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if !state.armed {
            if sequence < state.start_sequence {
                return Ok(());
            }
            if sequence > state.start_sequence {
                return Err("ASTRA_PERFORMANCE_START_SEQUENCE_MISSED".into());
            }
            let started = Instant::now();
            state.next_frame_deadline = None;
            state.paced_frame_count = 0;
            state.measurement_started = (state.warmup_frames_remaining == 0).then_some(started);
            state.measurement_stopped = None;
            state.measurement_frame_count = 0;
            state.allocation_baseline = astra_observability::allocation_snapshot();
            state.first_gpu_sequence = None;
            state.last_paced_gpu_sequence = None;
            state.gpu_measurement_cutoff = None;
            state.trace_counters.clear();
            state.deadline_miss_count = 0;
            state.skipped_presentation_samples = 0;
            state.pending_product_sample = None;
            state.product_sample_by_gpu_sequence.clear();
            state.armed = true;
        }
        if state.active_input_flow.is_some() {
            return Err("ASTRA_PERFORMANCE_INPUT_FLOW_ALREADY_ACTIVE".into());
        }
        let timestamp_ns = elapsed_ns(state.started)?;
        state
            .trace
            .flow(
                "frame.flow",
                "physical_input_to_gpu",
                TRACE_TRACK_INPUT_FLOW,
                sequence,
                timestamp_ns,
                PerfettoFlowPhase::Start,
            )
            .map_err(|error| error.to_string())?;
        state.active_input_flow = Some(ActiveInputFlow {
            sequence,
            gpu_seen: false,
        });
        Ok(())
    }

    pub fn end_input_flow(&self, sequence: u64) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if !state.armed {
            return Ok(());
        }
        let active = state
            .active_input_flow
            .take()
            .ok_or("ASTRA_PERFORMANCE_INPUT_FLOW_NOT_ACTIVE")?;
        if active.sequence != sequence {
            return Err("ASTRA_PERFORMANCE_INPUT_FLOW_SEQUENCE_MISMATCH".into());
        }
        if state.pending_product_sample.is_some() {
            return Err("ASTRA_PERFORMANCE_PRESENTATION_SAMPLE_SCOPE_UNCLOSED".into());
        }
        if !active.gpu_seen {
            let timestamp_ns = elapsed_ns(state.started)?;
            state
                .trace
                .flow(
                    "frame.flow",
                    "physical_input_to_gpu",
                    TRACE_TRACK_INPUT_FLOW,
                    sequence,
                    timestamp_ns,
                    PerfettoFlowPhase::End,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn stop_gpu_measurement(&self) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if !state.armed {
            return Err("ASTRA_PERFORMANCE_START_SEQUENCE_NOT_REACHED".into());
        }
        if state.gpu_measurement_cutoff.is_some() {
            // The recorder may have stopped itself exactly at the declared
            // sample capacity while the product input sequence continues.
            // Explicit shutdown is then a no-op, not a second measurement.
            return Ok(());
        }
        state.gpu_measurement_cutoff = Some(
            state
                .last_paced_gpu_sequence
                .ok_or("ASTRA_PERFORMANCE_GPU_MEASUREMENT_EMPTY")?,
        );
        state.measurement_stopped = Some(Instant::now());
        Ok(())
    }

    pub fn record_cpu_slice(
        &self,
        domain: &str,
        name: &str,
        correlation_id: Option<u64>,
        started: Instant,
    ) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        let end_ns = elapsed_ns(state.started)?;
        let duration_ns: u64 = started
            .elapsed()
            .as_nanos()
            .try_into()
            .map_err(|_| "ASTRA_PERFORMANCE_DURATION_OVERFLOW")?;
        state
            .trace
            .complete(
                domain,
                name,
                trace_track(name),
                correlation_id,
                end_ns.saturating_sub(duration_ns),
                duration_ns,
            )
            .map_err(|error| error.to_string())
    }

    pub fn record_memory_snapshot(&self, name: &str) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        let timestamp_ns = elapsed_ns(state.started)?;
        let memory = sample_process_memory()
            .map_err(|_| "ASTRA_PERFORMANCE_PROCESS_MEMORY_SAMPLE_FAILED")?;
        let allocation = astra_observability::allocation_snapshot();
        state
            .trace
            .complete("memory.phase", name, 1, None, timestamp_ns, 0)
            .and_then(|_| {
                state.trace.counter(
                    "memory",
                    "working_set.bytes",
                    timestamp_ns,
                    memory.working_set_bytes,
                )
            })
            .and_then(|_| {
                state.trace.counter(
                    "memory",
                    "private.bytes",
                    timestamp_ns,
                    memory.private_bytes,
                )
            })
            .and_then(|_| {
                state.trace.counter(
                    "allocator",
                    "live.bytes",
                    timestamp_ns,
                    allocation.live_bytes,
                )
            })
            .and_then(|_| {
                state.trace.counter(
                    "allocator",
                    "peak_live.bytes",
                    timestamp_ns,
                    allocation.peak_live_bytes,
                )
            })
            .map_err(|error| error.to_string())
    }

    pub fn begin_cpu_scope(
        &self,
        domain: &str,
        name: &str,
        correlation_id: Option<u64>,
    ) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if correlation_id.is_some_and(|sequence| !trace_detailed_phase(sequence)) {
            return Ok(());
        }
        let timestamp_ns = elapsed_ns(state.started)?;
        state
            .trace
            .begin(
                domain,
                name,
                trace_track(name),
                correlation_id,
                timestamp_ns,
            )
            .map_err(|error| error.to_string())
    }

    pub fn end_cpu_scope(
        &self,
        domain: &str,
        name: &str,
        correlation_id: Option<u64>,
    ) -> Result<(), String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        if correlation_id.is_some_and(|sequence| !trace_detailed_phase(sequence)) {
            return Ok(());
        }
        let timestamp_ns = elapsed_ns(state.started)?;
        state
            .trace
            .end(
                domain,
                name,
                trace_track(name),
                correlation_id,
                timestamp_ns,
            )
            .map_err(|error| error.to_string())
    }
}
