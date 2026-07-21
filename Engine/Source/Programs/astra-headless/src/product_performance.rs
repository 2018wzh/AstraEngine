use std::{
    collections::BTreeSet,
    fmt,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Instant,
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
use astra_product_host::ProductPerformanceSample;

const FRAME_DEADLINE_NS: u64 = 8_333_333;

struct RecorderState {
    recorder: PerformanceRecorder,
    trace: PerfettoTraceWriter,
    started: Instant,
    last_memory_sample: Option<(u64, u64)>,
    memory_baseline: Option<(u64, u64)>,
    active_input_flow: Option<ActiveInputFlow>,
    warmup_frames_remaining: u64,
    measurement_started: Option<Instant>,
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

impl ProductPerformanceRecorder {
    pub fn create(
        budget: PerformanceBudget,
        trace_path: &Path,
        warmup_frames: u64,
    ) -> Result<Self, String> {
        budget.validate().map_err(|error| error.to_string())?;
        validate_product_budget(&budget)?;
        Ok(Self {
            state: Mutex::new(Some(RecorderState {
                recorder: PerformanceRecorder::new(budget).map_err(|error| error.to_string())?,
                trace: PerfettoTraceWriter::create(PerfettoTraceConfig::production(
                    trace_path,
                    "astra-headless-product",
                ))
                .map_err(|error| error.to_string())?,
                started: Instant::now(),
                last_memory_sample: None,
                memory_baseline: None,
                active_input_flow: None,
                warmup_frames_remaining: warmup_frames,
                measurement_started: (warmup_frames == 0).then(Instant::now),
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
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "ASTRA_PERFORMANCE_RECORDER_POISONED")?;
        let state = guard
            .as_mut()
            .ok_or("ASTRA_PERFORMANCE_RECORDER_FINISHED")?;
        let timestamp_ns = elapsed_ns(state.started)?;
        for (domain, name, duration_ns) in [
            ("runtime.cpu", "runtime.tick_action", sample.runtime_tick_ns),
            ("vn.cpu", "vn.step", sample.vn_step_ns),
            ("ui.cpu", "ui.layout_paint", sample.ui_layout_paint_ns),
            ("media.cpu", "media.decode_mix", sample.media_decode_ns),
            ("save.cpu", "save_load", sample.save_load_ns),
        ] {
            if duration_ns == 0 {
                continue;
            }
            state
                .trace
                .complete(domain, name, 1, Some(sequence), timestamp_ns, duration_ns)
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
        let duration_us = state
            .measurement_started
            .ok_or("ASTRA_PERFORMANCE_MEASUREMENT_NOT_STARTED")?
            .elapsed()
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
    fn bind_gpu_frame(&self, _sequence: u64) -> Result<Option<u64>, PlatformError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| performance_error("recorder mutex poisoned"))?;
        let state = guard
            .as_mut()
            .ok_or_else(|| performance_error("recorder already finished"))?;
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
                state.measurement_started = Some(Instant::now());
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
        let cpu_ns = sample
            .scene_build_ns
            .checked_add(sample.cpu_submit_ns)
            .ok_or_else(|| performance_error("CPU frame duration overflowed"))?;
        let end_to_end_ns = cpu_ns
            .checked_add(sample.gpu_duration_ns)
            .ok_or_else(|| performance_error("frame duration overflowed"))?;
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
        state
            .trace
            .complete(
                "runtime.cpu",
                "scene.build",
                1,
                Some(sample.sequence),
                start_ns,
                sample.scene_build_ns,
            )
            .and_then(|_| {
                state.trace.complete(
                    "renderer.cpu",
                    "wgpu.prepare_submit",
                    1,
                    Some(sample.sequence),
                    start_ns,
                    sample.cpu_submit_ns,
                )
            })
            .and_then(|_| match active_flow {
                Some(flow_id) => state.trace.flow(
                    "frame.flow",
                    "physical_input_to_gpu",
                    2,
                    flow_id,
                    gpu_start_ns,
                    PerfettoFlowPhase::Step,
                ),
                None => Ok(()),
            })
            .and_then(|_| {
                state.trace.complete(
                    "renderer.gpu",
                    "atlas.upload",
                    2,
                    Some(sample.sequence),
                    gpu_start_ns,
                    sample.atlas_upload_gpu_ns,
                )
            })
            .and_then(|_| {
                state.trace.complete(
                    "renderer.gpu",
                    "scene.pass",
                    2,
                    Some(sample.sequence),
                    gpu_start_ns,
                    sample.scene_gpu_ns,
                )
            })
            .and_then(|_| {
                state.trace.complete(
                    "renderer.gpu",
                    "filter.pass",
                    2,
                    Some(sample.sequence),
                    gpu_start_ns,
                    sample.filter_gpu_ns,
                )
            })
            .and_then(|_| match active_flow {
                Some(flow_id) => state.trace.flow(
                    "frame.flow",
                    "physical_input_to_gpu",
                    2,
                    flow_id,
                    gpu_start_ns,
                    PerfettoFlowPhase::End,
                ),
                None => Ok(()),
            })
            .and_then(|_| {
                state
                    .trace
                    .counter("memory", "working_set.bytes", timestamp_ns, working_set)
            })
            .and_then(|_| {
                state
                    .trace
                    .counter("memory", "private.bytes", timestamp_ns, private_bytes)
            })
            .and_then(|_| {
                state.trace.counter(
                    "renderer",
                    "gpu_resource.bytes",
                    timestamp_ns,
                    sample.gpu_resource_bytes,
                )
            })
            .and_then(|_| {
                state
                    .trace
                    .counter("renderer", "atlas.bytes", timestamp_ns, sample.atlas_bytes)
            })
            .and_then(|_| {
                state.trace.counter(
                    "renderer",
                    "upload.bytes",
                    timestamp_ns,
                    sample.upload_bytes,
                )
            })
            .and_then(|_| {
                state.trace.counter(
                    "allocator",
                    "frame.bytes",
                    timestamp_ns,
                    sample.heap_allocation_bytes,
                )
            })
            .map_err(|error| performance_error(&error.to_string()))?;
        Ok(())
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
        )
        .unwrap();
        let mut sample = HeadlessGpuFrameSample {
            sequence: 1,
            input_flow_id: None,
            scene_build_ns: 10,
            cpu_submit_ns: 20,
            gpu_duration_ns: 30,
            scene_cpu_ns: 15,
            filter_cpu_ns: 5,
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
        recorder.record_gpu_frame(sample).unwrap();
        sample.sequence = 2;
        recorder.record_gpu_frame(sample).unwrap();
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
    }

    fn hash(bytes: &[u8]) -> String {
        Hash256::from_sha256(bytes).to_string()
    }
}
