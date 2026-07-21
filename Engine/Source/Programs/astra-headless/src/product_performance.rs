use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use astra_core::{
    PerformanceBudget, PerformanceRecorder, PerformanceRunIdentity, PerformanceTraceManifest,
    PERFORMANCE_TRACE_MANIFEST_SCHEMA,
};
use astra_observability::{
    sample_process_memory, PerfettoFlowPhase, PerfettoTraceConfig, PerfettoTraceWriter,
};
use astra_platform::PlatformError;
use astra_platform_headless::{HeadlessGpuFrameSample, HeadlessPerformanceObserver};
use astra_product_host::{ProductPerformanceObserver, ProductPerformanceSample};

const FRAME_DEADLINE_NS: u64 = 8_333_333;
const TRACE_COUNTER_HEARTBEAT_FRAMES: u64 = 120;

#[derive(Clone, Copy)]
struct TraceCounterSample {
    value: u64,
    paced_frame: u64,
}

struct RecorderState {
    recorder: PerformanceRecorder,
    trace: PerfettoTraceWriter,
    started: Instant,
    last_memory_sample: Option<(u64, u64)>,
    memory_baseline: Option<(u64, u64)>,
    allocation_baseline: astra_observability::AllocationSnapshot,
    active_input_flow: Option<ActiveInputFlow>,
    pending_product_cpu_ns: u64,
    product_cpu_by_gpu_sequence: BTreeMap<u64, u64>,
    warmup_frames: u64,
    warmup_frames_remaining: u64,
    measurement_started: Option<Instant>,
    measurement_stopped: Option<Instant>,
    pacing_started: Instant,
    paced_frame_count: u64,
    presentation_rate_hz: u32,
    start_sequence: u64,
    armed: bool,
    first_gpu_sequence: Option<u64>,
    last_paced_gpu_sequence: Option<u64>,
    gpu_measurement_cutoff: Option<u64>,
    trace_counters: BTreeMap<(&'static str, &'static str), TraceCounterSample>,
    deadline_miss_count: u64,
}

struct ActiveInputFlow {
    sequence: u64,
    gpu_seen: bool,
}

pub struct ProductPerformanceRecorder {
    state: Mutex<Option<RecorderState>>,
    decoded_cache_bytes: AtomicU64,
}

impl fmt::Debug for ProductPerformanceRecorder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductPerformanceRecorder")
            .finish_non_exhaustive()
    }
}

impl ProductPerformanceObserver for ProductPerformanceRecorder {
    fn record_phase(&self, name: &str) -> Result<(), String> {
        self.record_memory_snapshot(name)
    }

    fn record_sample(&self, sample: ProductPerformanceSample) -> Result<(), String> {
        self.record_product_sample_inner(sample)
    }
}

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
                pending_product_cpu_ns: 0,
                product_cpu_by_gpu_sequence: BTreeMap::new(),
                warmup_frames,
                warmup_frames_remaining: warmup_frames,
                measurement_started: (warmup_frames == 0).then(Instant::now),
                measurement_stopped: None,
                pacing_started: started,
                paced_frame_count: 0,
                presentation_rate_hz,
                start_sequence,
                armed: false,
                first_gpu_sequence: None,
                last_paced_gpu_sequence: None,
                gpu_measurement_cutoff: None,
                trace_counters: BTreeMap::new(),
                deadline_miss_count: 0,
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

    fn record_product_sample_inner(&self, sample: ProductPerformanceSample) -> Result<(), String> {
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
        for (domain, name, duration_ns) in [
            ("runtime.cpu", "runtime.tick_action", sample.runtime_tick_ns),
            ("vn.cpu", "vn.step", sample.vn_step_ns),
            ("ui.cpu", "ui.layout_paint", sample.ui_layout_paint_ns),
            ("ui.cpu", "ui.update_layout", sample.ui_update_layout_ns),
            (
                "ui.cpu",
                "ui.paint_conversion",
                sample.ui_paint_conversion_ns,
            ),
            ("ui.cpu", "ui.host_scene", sample.ui_host_scene_ns),
            ("ui.cpu", "ui.model_binding", sample.ui_model_binding_ns),
            ("ui.cpu", "ui.controller", sample.ui_controller_ns),
            ("ui.cpu", "ui.frame_model", sample.ui_frame_model_ns),
            ("ui.cpu", "ui.text_scene", sample.ui_text_scene_ns),
            ("media.cpu", "media.decode_mix", sample.media_decode_ns),
            ("save.cpu", "save_load", sample.save_load_ns),
        ] {
            if duration_ns == 0 {
                continue;
            }
            state
                .trace
                .complete(domain, name, 1, correlation_id, timestamp_ns, duration_ns)
                .map_err(|error| error.to_string())?;
        }
        let product_cpu_ns = sample
            .runtime_tick_ns
            .checked_add(sample.ui_layout_paint_ns)
            .and_then(|value| value.checked_add(sample.media_decode_ns))
            .and_then(|value| value.checked_add(sample.save_load_ns))
            .ok_or("ASTRA_PERFORMANCE_PRODUCT_CPU_OVERFLOW")?;
        state.pending_product_cpu_ns = state
            .pending_product_cpu_ns
            .checked_add(product_cpu_ns)
            .ok_or("ASTRA_PERFORMANCE_PRODUCT_CPU_OVERFLOW")?;
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
            state.pacing_started = started;
            state.paced_frame_count = 0;
            state.measurement_started = (state.warmup_frames_remaining == 0).then_some(started);
            state.measurement_stopped = None;
            state.allocation_baseline = astra_observability::allocation_snapshot();
            state.first_gpu_sequence = None;
            state.last_paced_gpu_sequence = None;
            state.gpu_measurement_cutoff = None;
            state.trace_counters.clear();
            state.deadline_miss_count = 0;
            state.pending_product_cpu_ns = 0;
            state.product_cpu_by_gpu_sequence.clear();
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
                1,
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
        if !active.gpu_seen {
            let timestamp_ns = elapsed_ns(state.started)?;
            state
                .trace
                .flow(
                    "frame.flow",
                    "physical_input_to_gpu",
                    1,
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
            return Err("ASTRA_PERFORMANCE_GPU_MEASUREMENT_ALREADY_STOPPED".into());
        }
        state.gpu_measurement_cutoff = Some(
            state
                .last_paced_gpu_sequence
                .ok_or("ASTRA_PERFORMANCE_GPU_MEASUREMENT_EMPTY")?,
        );
        let offset = frame_deadline_offset(state.paced_frame_count, state.presentation_rate_hz)?;
        state.measurement_stopped = Some(
            state
                .pacing_started
                .checked_add(offset)
                .ok_or("ASTRA_PERFORMANCE_MEASUREMENT_DEADLINE_OVERFLOW")?,
        );
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
                1,
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
        let timestamp_ns = elapsed_ns(state.started)?;
        state
            .trace
            .begin(domain, name, 1, correlation_id, timestamp_ns)
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
        let timestamp_ns = elapsed_ns(state.started)?;
        state
            .trace
            .end(domain, name, 1, correlation_id, timestamp_ns)
            .map_err(|error| error.to_string())
    }

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
        let duration_us = measurement_stopped
            .checked_duration_since(measurement_started)
            .ok_or("ASTRA_PERFORMANCE_MEASUREMENT_TIME_REVERSED")?
            .as_micros()
            .try_into()
            .map_err(|_| "ASTRA_PERFORMANCE_DURATION_OVERFLOW")?;
        let trace_summary = state.trace.finish().map_err(|error| error.to_string())?;
        let report = state
            .recorder
            .finalize(identity.clone(), duration_us)
            .map_err(|error| error.to_string())?;
        super::write_atomic_json(report_path, &report)?;
        let report_hash = super::hash_file(report_path)?;
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
        super::write_atomic_json(trace_manifest_path, &manifest)?;
        if !matches!(report.status, astra_core::PerformanceStatus::Pass) {
            return Err("ASTRA_PERFORMANCE_BUDGET_BLOCKED".into());
        }
        Ok(())
    }
}

fn validate_product_budget(budget: &PerformanceBudget) -> Result<(), String> {
    let actual = budget
        .metrics
        .iter()
        .map(|metric| metric.id.as_str())
        .collect::<BTreeSet<_>>();
    let required = super::performance_e2::PRODUCT_METRICS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let stress_thresholds = budget
        .metrics
        .iter()
        .all(|metric| metric.thresholds == super::performance_e2::metric_thresholds(&metric.id));
    let route_thresholds = budget.metrics.iter().all(|metric| {
        metric.thresholds == super::performance_e2::route_metric_thresholds(&metric.id)
    });
    if actual != required
        || budget
            .metrics
            .iter()
            .any(|metric| metric.unit != super::performance_e2::metric_unit(&metric.id))
        || (!stress_thresholds && !route_thresholds)
    {
        return Err("ASTRA_PERFORMANCE_PRODUCT_BUDGET_INVALID".into());
    }
    Ok(())
}

impl HeadlessPerformanceObserver for ProductPerformanceRecorder {
    fn pace_gpu_frame(&self, sequence: u64) -> Result<(), PlatformError> {
        let deadline = {
            let mut guard = self
                .state
                .lock()
                .map_err(|_| performance_error("recorder mutex poisoned"))?;
            let state = guard
                .as_mut()
                .ok_or_else(|| performance_error("recorder already finished"))?;
            if !state.armed {
                return Ok(());
            }
            if state.gpu_measurement_cutoff.is_some() {
                return Ok(());
            }
            state.first_gpu_sequence.get_or_insert(sequence);
            state.last_paced_gpu_sequence = Some(sequence);
            state.paced_frame_count = state
                .paced_frame_count
                .checked_add(1)
                .ok_or_else(|| performance_error("paced frame count overflowed"))?;
            let offset = frame_deadline_offset(state.paced_frame_count, state.presentation_rate_hz)
                .map_err(performance_error)?;
            state
                .pacing_started
                .checked_add(offset)
                .ok_or_else(|| performance_error("frame deadline overflowed"))?
        };
        pace_until(deadline);
        Ok(())
    }

    fn bind_gpu_frame(&self, sequence: u64) -> Result<Option<u64>, PlatformError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| performance_error("recorder mutex poisoned"))?;
        let state = guard
            .as_mut()
            .ok_or_else(|| performance_error("recorder already finished"))?;
        if !state.armed
            || state.gpu_measurement_cutoff.is_some()
            || state
                .first_gpu_sequence
                .is_none_or(|first| sequence < first)
        {
            return Ok(None);
        }
        let product_cpu_ns = std::mem::take(&mut state.pending_product_cpu_ns);
        if state
            .product_cpu_by_gpu_sequence
            .insert(sequence, product_cpu_ns)
            .is_some()
        {
            return Err(performance_error("duplicate GPU sequence CPU binding"));
        }
        if state.product_cpu_by_gpu_sequence.len() > 1_024 {
            return Err(performance_error("GPU CPU correlation capacity exceeded"));
        }
        Ok(state
            .active_input_flow
            .as_mut()
            .filter(|flow| !flow.gpu_seen)
            .map(|flow| {
                flow.gpu_seen = true;
                flow.sequence
            }))
    }

    fn record_gpu_frame(&self, sample: HeadlessGpuFrameSample) -> Result<(), PlatformError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| performance_error("recorder mutex poisoned"))?;
        let state = guard
            .as_mut()
            .ok_or_else(|| performance_error("recorder already finished"))?;
        if !state.armed
            || state
                .first_gpu_sequence
                .is_none_or(|first| sample.sequence < first)
            || state
                .gpu_measurement_cutoff
                .is_some_and(|cutoff| sample.sequence > cutoff)
        {
            return Ok(());
        }
        let product_cpu_ns = state
            .product_cpu_by_gpu_sequence
            .remove(&sample.sequence)
            .ok_or_else(|| performance_error("GPU frame has no submit-time CPU binding"))?;
        self.record_gpu_frame_sample(state, sample, product_cpu_ns)
    }
}

impl ProductPerformanceRecorder {
    fn record_gpu_frame_sample(
        &self,
        state: &mut RecorderState,
        sample: HeadlessGpuFrameSample,
        product_cpu_ns: u64,
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
                        2,
                        flow_id,
                        timestamp_ns,
                        PerfettoFlowPhase::End,
                    )
                    .map_err(|error| performance_error(&error.to_string()))?;
            }
            state.warmup_frames_remaining -= 1;
            if state.warmup_frames_remaining == 0 {
                let offset = frame_deadline_offset(state.warmup_frames, state.presentation_rate_hz)
                    .map_err(performance_error)?;
                state.measurement_started = Some(
                    state
                        .pacing_started
                        .checked_add(offset)
                        .ok_or_else(|| performance_error("measurement deadline overflowed"))?,
                );
                state.last_memory_sample = None;
                state.memory_baseline = None;
            }
            return Ok(());
        }
        let timestamp_ns: u64 = state
            .started
            .elapsed()
            .as_nanos()
            .try_into()
            .map_err(|_| performance_error("trace timestamp overflowed"))?;
        let cpu_ns = product_cpu_ns
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
        // Timestamp queries resolve on the next materialization so the CPU never
        // waits for the just-submitted frame. Keep streamed trace timestamps
        // monotonic and carry the measured durations on correlated slices.
        let start_ns = timestamp_ns;
        let gpu_start_ns = timestamp_ns;
        let active_flow = sample.input_flow_id;
        for (domain, name, thread_id, duration_ns) in [
            ("runtime.cpu", "scene.build", 1, sample.scene_build_ns),
            (
                "renderer.cpu",
                "wgpu.prepare_submit",
                1,
                sample.cpu_submit_ns,
            ),
            (
                "renderer.cpu",
                "scene.commands",
                1,
                sample.scene_command_cpu_ns,
            ),
            ("renderer.cpu", "scene.atlas", 1, sample.scene_atlas_cpu_ns),
            (
                "renderer.cpu",
                "scene.geometry",
                1,
                sample.scene_geometry_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.vertex_upload",
                1,
                sample.scene_vertex_upload_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.render_submit",
                1,
                sample.scene_render_submit_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.render_encode",
                1,
                sample.scene_render_encode_cpu_ns,
            ),
            (
                "renderer.cpu",
                "scene.queue_submit",
                1,
                sample.scene_queue_submit_cpu_ns,
            ),
            (
                "renderer.gpu",
                "atlas.upload",
                2,
                sample.atlas_upload_gpu_ns,
            ),
            ("renderer.gpu", "scene.pass", 2, sample.scene_gpu_ns),
            ("renderer.gpu", "filter.pass", 2, sample.filter_gpu_ns),
        ] {
            trace_complete_nonzero(
                &mut state.trace,
                domain,
                name,
                thread_id,
                sample.sequence,
                if thread_id == 1 {
                    start_ns
                } else {
                    gpu_start_ns
                },
                duration_ns,
            )
            .map_err(|error| performance_error(&error))?;
        }
        if let Some(flow_id) = active_flow {
            state
                .trace
                .flow(
                    "frame.flow",
                    "physical_input_to_gpu",
                    2,
                    flow_id,
                    gpu_start_ns,
                    PerfettoFlowPhase::Step,
                )
                .and_then(|_| {
                    state.trace.flow(
                        "frame.flow",
                        "physical_input_to_gpu",
                        2,
                        flow_id,
                        gpu_start_ns,
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

fn trace_complete_nonzero(
    trace: &mut PerfettoTraceWriter,
    domain: &'static str,
    name: &'static str,
    thread_id: u32,
    sequence: u64,
    timestamp_ns: u64,
    duration_ns: u64,
) -> Result<(), String> {
    if duration_ns == 0 {
        return Ok(());
    }
    trace
        .complete(
            domain,
            name,
            thread_id,
            Some(sequence),
            timestamp_ns,
            duration_ns,
        )
        .map_err(|error| error.to_string())
}

fn trace_counter_journaled(
    state: &mut RecorderState,
    domain: &'static str,
    name: &'static str,
    timestamp_ns: u64,
    value: u64,
    emit_on_change: bool,
) -> Result<(), String> {
    let key = (domain, name);
    let should_emit = should_emit_trace_counter(
        state.trace_counters.get(&key).copied(),
        state.paced_frame_count,
        value,
        emit_on_change,
    );
    if !should_emit {
        return Ok(());
    }
    state
        .trace
        .counter(domain, name, timestamp_ns, value)
        .map_err(|error| error.to_string())?;
    state.trace_counters.insert(
        key,
        TraceCounterSample {
            value,
            paced_frame: state.paced_frame_count,
        },
    );
    Ok(())
}

fn should_emit_trace_counter(
    previous: Option<TraceCounterSample>,
    paced_frame: u64,
    value: u64,
    emit_on_change: bool,
) -> bool {
    previous.is_none_or(|previous| {
        (emit_on_change && previous.value != value)
            || paced_frame.saturating_sub(previous.paced_frame) >= TRACE_COUNTER_HEARTBEAT_FRAMES
    })
}

fn frame_deadline_offset(
    frame_count: u64,
    presentation_rate_hz: u32,
) -> Result<Duration, &'static str> {
    if presentation_rate_hz == 0 {
        return Err("presentation rate must be non-zero");
    }
    let nanoseconds = u128::from(frame_count)
        .checked_mul(1_000_000_000)
        .ok_or("frame deadline offset overflowed")?
        / u128::from(presentation_rate_hz);
    let nanoseconds: u64 = nanoseconds
        .try_into()
        .map_err(|_| "frame deadline offset overflowed")?;
    Ok(Duration::from_nanos(nanoseconds))
}

fn pace_until(deadline: Instant) {
    const SPIN_WINDOW: Duration = Duration::from_micros(200);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline.duration_since(now);
        if remaining > SPIN_WINDOW {
            std::thread::sleep(remaining - SPIN_WINDOW);
        } else {
            std::hint::spin_loop();
        }
    }
}

fn performance_error(message: &str) -> PlatformError {
    PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        "headless.performance.record",
        message,
    )
}

fn elapsed_ns(started: Instant) -> Result<u64, String> {
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| "ASTRA_PERFORMANCE_TIMESTAMP_OVERFLOW".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_core::{
        Hash256, PerformanceMetricBudget, PerformanceUnit, PERFORMANCE_BUDGET_SCHEMA,
    };

    #[test]
    fn frame_deadlines_do_not_accumulate_fractional_rate_drift() {
        assert_eq!(
            frame_deadline_offset(120, 120).unwrap(),
            Duration::from_secs(1)
        );
        assert_eq!(
            frame_deadline_offset(72_000, 120).unwrap(),
            Duration::from_secs(600)
        );
        assert!(frame_deadline_offset(1, 0).is_err());
    }

    #[test]
    fn trace_counters_emit_changes_and_bounded_heartbeats() {
        let previous = TraceCounterSample {
            value: 42,
            paced_frame: 10,
        };
        assert!(should_emit_trace_counter(None, 10, 42, true));
        assert!(!should_emit_trace_counter(Some(previous), 11, 42, true));
        assert!(should_emit_trace_counter(Some(previous), 11, 43, true));
        assert!(!should_emit_trace_counter(Some(previous), 11, 43, false));
        assert!(should_emit_trace_counter(
            Some(previous),
            10 + TRACE_COUNTER_HEARTBEAT_FRAMES,
            42,
            false,
        ));
    }

    #[test]
    fn product_recorder_writes_identity_bound_report_and_trace_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let profile_hash = hash(b"profile");
        let metrics = [
            ("frame.cpu_ns", PerformanceUnit::Nanoseconds),
            ("frame.gpu_ns", PerformanceUnit::Nanoseconds),
            ("frame.end_to_end_ns", PerformanceUnit::Nanoseconds),
            ("deadline.miss_count", PerformanceUnit::Count),
            ("memory.working_set_bytes", PerformanceUnit::Bytes),
            ("memory.private_bytes", PerformanceUnit::Bytes),
            ("memory.growth_bytes", PerformanceUnit::Bytes),
            ("gpu.resource_bytes", PerformanceUnit::Bytes),
            ("gpu.atlas_bytes", PerformanceUnit::Bytes),
            ("cache.decoded_bytes", PerformanceUnit::Bytes),
            ("gpu.upload_bytes", PerformanceUnit::Bytes),
            ("gpu.readback_bytes", PerformanceUnit::Bytes),
            ("renderer.draw_calls", PerformanceUnit::Count),
            ("renderer.queue_submissions", PerformanceUnit::Count),
            ("renderer.pipeline_count", PerformanceUnit::Count),
            ("heap.allocation_bytes", PerformanceUnit::Bytes),
            ("heap.allocation_count", PerformanceUnit::Count),
        ]
        .into_iter()
        .map(|(id, unit)| PerformanceMetricBudget {
            id: id.into(),
            unit,
            min_samples: 1,
            max_samples: 1,
            thresholds: crate::performance_e2::metric_thresholds(id),
        })
        .collect();
        let recorder = ProductPerformanceRecorder::create(
            PerformanceBudget {
                schema: PERFORMANCE_BUDGET_SCHEMA.into(),
                budget_id: "product.performance.test".into(),
                target: "nativevn.test".into(),
                profile: "classic".into(),
                profile_hash: profile_hash.clone(),
                min_run_duration_us: 1,
                metrics,
            },
            &temp.path().join("trace.json"),
            1,
            astra_platform::HEADLESS_PERFORMANCE_PRESENTATION_RATE_HZ,
            1,
        )
        .unwrap();
        recorder.begin_input_flow(1).unwrap();
        let mut sample = HeadlessGpuFrameSample {
            sequence: 1,
            input_flow_id: Some(1),
            scene_build_ns: 10,
            cpu_submit_ns: 20,
            gpu_duration_ns: 30,
            scene_cpu_ns: 15,
            filter_cpu_ns: 5,
            scene_command_cpu_ns: 3,
            scene_atlas_cpu_ns: 0,
            scene_geometry_cpu_ns: 4,
            scene_vertex_upload_cpu_ns: 3,
            scene_render_encode_cpu_ns: 2,
            scene_queue_submit_cpu_ns: 3,
            scene_render_submit_cpu_ns: 5,
            atlas_upload_gpu_ns: 0,
            scene_gpu_ns: 20,
            filter_gpu_ns: 10,
            gpu_resource_bytes: 1024,
            atlas_bytes: 512,
            upload_bytes: 0,
            readback_bytes: 0,
            draw_calls: 1,
            queue_submissions: 1,
            pipeline_count: 1,
            heap_allocation_bytes: 0,
            heap_allocation_count: 0,
        };
        let product_sample = ProductPerformanceSample {
            runtime_tick_ns: 5,
            vn_step_ns: 2,
            ui_layout_paint_ns: 40,
            ui_update_layout_ns: 10,
            ui_paint_conversion_ns: 5,
            ui_host_scene_ns: 25,
            ui_model_binding_ns: 4,
            ui_controller_ns: 3,
            ui_frame_model_ns: 2,
            ui_text_scene_ns: 1,
            media_decode_ns: 6,
            save_load_ns: 7,
        };
        recorder.record_product_sample(1, product_sample).unwrap();
        recorder.pace_gpu_frame(1).unwrap();
        assert_eq!(recorder.bind_gpu_frame(1).unwrap(), Some(1));
        recorder.record_gpu_frame(sample).unwrap();
        recorder.end_input_flow(1).unwrap();
        recorder.begin_input_flow(2).unwrap();
        sample.sequence = 2;
        sample.input_flow_id = Some(2);
        recorder.record_product_sample(2, product_sample).unwrap();
        recorder.pace_gpu_frame(2).unwrap();
        assert_eq!(recorder.bind_gpu_frame(2).unwrap(), Some(2));
        recorder.record_gpu_frame(sample).unwrap();
        recorder.end_input_flow(2).unwrap();
        recorder.stop_gpu_measurement().unwrap();
        recorder
            .finish(
                PerformanceRunIdentity {
                    source_revision: "a".repeat(40),
                    dirty: false,
                    target: "nativevn.test".into(),
                    profile: "classic".into(),
                    profile_hash,
                    package_hash: hash(b"package"),
                    build_fingerprint: hash(b"build"),
                    session_id: "product.performance.test".into(),
                },
                "product.performance.test",
                hash(b"adapter"),
                hash(b"driver"),
                &temp.path().join("report.json"),
                &temp.path().join("trace-manifest.json"),
            )
            .unwrap();
        assert!(temp.path().join("trace.json").is_file());
        assert!(temp.path().join("report.json").is_file());
        assert!(temp.path().join("trace-manifest.json").is_file());
        let report: astra_core::PerformanceReport =
            serde_json::from_slice(&std::fs::read(temp.path().join("report.json")).unwrap())
                .unwrap();
        let cpu = report
            .metrics
            .iter()
            .find(|metric| metric.id == "frame.cpu_ns")
            .unwrap();
        assert_eq!(cpu.p95, 88);
    }

    fn hash(bytes: &[u8]) -> String {
        Hash256::from_sha256(bytes).to_string()
    }
}
