use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) const MANIFEST_SCHEMA: &str = "astra.headless_session_batch.v2";
pub(crate) const REPORT_SCHEMA: &str = "astra.headless_session_batch_report.v2";
pub(crate) const MAX_PARALLEL_SESSIONS: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchManifest {
    pub(crate) schema: String,
    pub(crate) worker_limit: usize,
    pub(crate) jobs: Vec<BatchJob>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchJob {
    pub(crate) session_id: String,
    pub(crate) kind: BatchJobKind,
    pub(crate) gpu: bool,
    pub(crate) profile: PathBuf,
    pub(crate) package: PathBuf,
    pub(crate) input: PathBuf,
    pub(crate) artifact_root: PathBuf,
    pub(crate) serial_artifact_root: PathBuf,
    pub(crate) checkpoint_config: Option<PathBuf>,
    pub(crate) build_identity: PathBuf,
    pub(crate) performance: Option<BatchPerformanceConfig>,
    pub(crate) timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BatchJobKind {
    Route,
    Replay,
    Performance,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchPerformanceConfig {
    pub(crate) budget: PathBuf,
    pub(crate) warmup_frames: u64,
    pub(crate) start_sequence: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct BatchReport {
    pub(crate) schema: &'static str,
    pub(crate) status: &'static str,
    pub(crate) configured_worker_limit: usize,
    pub(crate) available_parallelism: usize,
    pub(crate) selected_concurrency: usize,
    pub(crate) serial_session_worker_limit: usize,
    pub(crate) concurrent_session_worker_limit: usize,
    pub(crate) concurrent_worker_capacity: usize,
    pub(crate) worker_limit: usize,
    pub(crate) peak_workers: usize,
    pub(crate) wall_time_us: u64,
    pub(crate) total_queue_time_us: u64,
    pub(crate) throughput_milli_sessions_per_second: u64,
    pub(crate) session_slot_utilization_permille: u16,
    pub(crate) worker_utilization_permille: u16,
    pub(crate) serial_baseline_wall_time_us: u64,
    pub(crate) jobs: Vec<BatchJobReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BatchJobReport {
    pub(crate) session_id: String,
    pub(crate) kind: BatchJobKind,
    pub(crate) status: &'static str,
    pub(crate) queue_time_us: u64,
    pub(crate) run_time_us: u64,
    pub(crate) serial_baseline_run_time_us: u64,
    pub(crate) worker_limit: usize,
    pub(crate) serial_worker_limit: usize,
    pub(crate) cpu_time_us: Option<u64>,
    pub(crate) serial_cpu_time_us: Option<u64>,
    pub(crate) cpu_utilization_permille: Option<u16>,
    pub(crate) serial_cpu_utilization_permille: Option<u16>,
    pub(crate) output_identity_hash: Option<String>,
    pub(crate) serial_output_identity_hash: Option<String>,
    pub(crate) frame_cpu_p95_ns: Option<u64>,
    pub(crate) frame_cpu_p99_ns: Option<u64>,
    pub(crate) frame_end_to_end_p95_ns: Option<u64>,
    pub(crate) frame_end_to_end_p99_ns: Option<u64>,
    pub(crate) peak_private_memory_bytes: Option<u64>,
    pub(crate) serial_frame_cpu_p95_ns: Option<u64>,
    pub(crate) serial_frame_cpu_p99_ns: Option<u64>,
    pub(crate) serial_frame_end_to_end_p95_ns: Option<u64>,
    pub(crate) serial_frame_end_to_end_p99_ns: Option<u64>,
    pub(crate) serial_peak_private_memory_bytes: Option<u64>,
    pub(crate) profile_hash: Option<String>,
    pub(crate) package_hash: Option<String>,
    pub(crate) input_hash: Option<String>,
    pub(crate) build_identity_hash: Option<String>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct BatchJobIdentity {
    pub(crate) profile_hash: String,
    pub(crate) package_hash: String,
    pub(crate) input_hash: String,
    pub(crate) build_identity_hash: String,
}

pub(crate) struct BatchChildOutcome {
    pub(crate) identity: Option<BatchJobIdentity>,
    pub(crate) output_identity_hash: Option<String>,
    pub(crate) performance: Option<BatchPerformanceSummary>,
    pub(crate) peak_private_memory_bytes: Option<u64>,
    pub(crate) cpu_time_us: Option<u64>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct ChildProcessObservation {
    pub(crate) peak_private_memory_bytes: u64,
    pub(crate) cpu_time_us: u64,
}

#[derive(Clone)]
pub(crate) struct BatchPerformanceSummary {
    pub(crate) frame_cpu_p95_ns: u64,
    pub(crate) frame_cpu_p99_ns: u64,
    pub(crate) frame_end_to_end_p95_ns: u64,
    pub(crate) frame_end_to_end_p99_ns: u64,
    pub(crate) reported_peak_private_memory_bytes: Option<u64>,
}

pub(crate) struct SerialBaseline {
    pub(crate) run_time_us: u64,
    pub(crate) outcome: BatchChildOutcome,
}
