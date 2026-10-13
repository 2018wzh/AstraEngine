use std::{
    collections::BTreeMap,
    fmt,
    sync::{atomic::AtomicU64, Mutex},
    time::Instant,
};

use astra_core::PerformanceRecorder;
use astra_headless_protocol::ProductPerformanceSample;
use astra_observability::PerfettoTraceWriter;

pub(crate) const FRAME_DEADLINE_NS: u64 = 8_333_333;
pub(crate) const TRACE_COUNTER_HEARTBEAT_FRAMES: u64 = 120;
// The performance report owns every frame's budget samples. The trace keeps
// every physical-input flow and frame timeline, then samples expensive nested
// CPU/GPU phase trees at a deterministic cadence. This prevents a long 120 Hz
// product run from exhausting the bounded writer while preserving an unbiased
// timeline for Perfetto hotspot analysis.
// A 32-frame cadence keeps detail below the p95 frame population. The report
// remains per-frame and is the authority for E2 thresholds; Perfetto retains a
// deterministic, representative timeline plus every physical-input flow.
pub(crate) const TRACE_DETAILED_PHASE_SAMPLE_INTERVAL: u64 = 32;
pub(crate) const TRACE_TRACK_INPUT_FLOW: u32 = 1;
pub(crate) const TRACE_TRACK_GPU_FLOW: u32 = 2;

pub(crate) fn trace_detailed_phase(sequence: u64) -> bool {
    sequence.is_multiple_of(TRACE_DETAILED_PHASE_SAMPLE_INTERVAL)
}

pub(crate) fn measurement_duration_us(
    frame_count: u64,
    presentation_rate_hz: u32,
) -> Result<u64, String> {
    let rate = u64::from(presentation_rate_hz);
    if rate == 0 {
        return Err("ASTRA_PERFORMANCE_PRESENTATION_RATE_INVALID".into());
    }
    frame_count
        .checked_mul(1_000_000)
        .ok_or_else(|| "ASTRA_PERFORMANCE_DURATION_OVERFLOW".into())
        .map(|microseconds| microseconds.div_ceil(rate))
}

pub(crate) fn trace_track(name: &str) -> u32 {
    // Trace Event thread ids are logical lanes, not operating-system thread ids.
    // Assign each phase a deterministic lane so nested phase summaries remain
    // independently queryable by Trace Processor instead of overlapping on one
    // synthetic thread and being discarded during import.
    let mut hash = 2_166_136_261_u32;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    100_u32.wrapping_add(hash)
}

#[derive(Clone, Copy)]
pub(crate) struct TraceCounterSample {
    pub(crate) value: u64,
    pub(crate) paced_frame: u64,
}

pub(crate) struct RecorderState {
    pub(crate) recorder: PerformanceRecorder,
    pub(crate) trace: PerfettoTraceWriter,
    pub(crate) started: Instant,
    pub(crate) last_memory_sample: Option<(u64, u64)>,
    pub(crate) memory_baseline: Option<(u64, u64)>,
    pub(crate) allocation_baseline: astra_observability::AllocationSnapshot,
    pub(crate) active_input_flow: Option<ActiveInputFlow>,
    pub(crate) pending_product_sample: Option<BoundProductSample>,
    pub(crate) product_sample_by_gpu_sequence: BTreeMap<u64, BoundProductSample>,
    pub(crate) warmup_frames_remaining: u64,
    pub(crate) measurement_started: Option<Instant>,
    pub(crate) measurement_stopped: Option<Instant>,
    pub(crate) measurement_frame_count: u64,
    pub(crate) measurement_sample_capacity: u64,
    pub(crate) next_frame_deadline: Option<Instant>,
    pub(crate) paced_frame_count: u64,
    pub(crate) presentation_rate_hz: u32,
    pub(crate) start_sequence: u64,
    pub(crate) armed: bool,
    pub(crate) first_gpu_sequence: Option<u64>,
    pub(crate) last_paced_gpu_sequence: Option<u64>,
    pub(crate) gpu_measurement_cutoff: Option<u64>,
    pub(crate) trace_counters: BTreeMap<(&'static str, &'static str), TraceCounterSample>,
    pub(crate) deadline_miss_count: u64,
    pub(crate) skipped_presentation_samples: u64,
}

pub(crate) struct ActiveInputFlow {
    pub(crate) sequence: u64,
    pub(crate) gpu_seen: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct BoundProductSample {
    pub(crate) cpu_ns: u64,
    pub(crate) phases: ProductPerformanceSample,
}

pub struct ProductPerformanceRecorder {
    pub(crate) state: Mutex<Option<RecorderState>>,
    pub(crate) decoded_cache_bytes: AtomicU64,
}

impl fmt::Debug for ProductPerformanceRecorder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductPerformanceRecorder")
            .finish_non_exhaustive()
    }
}
