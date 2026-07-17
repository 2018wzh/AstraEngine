use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    is_sha256, ProtocolError, RendererExecutionIdentity, RunStatus,
    HEADLESS_RENDER_PERFORMANCE_SCHEMA,
};

pub const MIN_CPU_SPARSE_SPEEDUP: f64 = 2.0;
pub const MIN_GPU_ALL_SPEEDUP: f64 = 1.25;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderPerformanceReport {
    pub schema: String,
    pub status: RunStatus,
    pub build_fingerprint: String,
    pub scenario_hash: String,
    pub width: u32,
    pub height: u32,
    pub warmup_frames: u32,
    pub measured_frames: u32,
    pub cpu_all_duration_ns: u64,
    pub cpu_all_p50_frame_ns: u64,
    pub cpu_all_p95_frame_ns: u64,
    pub cpu_sparse_duration_ns: u64,
    pub cpu_sparse_p50_frame_ns: u64,
    pub cpu_sparse_p95_frame_ns: u64,
    pub cpu_sparse_speedup: f64,
    pub gpu_all_duration_ns: Option<u64>,
    pub gpu_all_p50_frame_ns: Option<u64>,
    pub gpu_all_p95_frame_ns: Option<u64>,
    pub gpu_all_speedup: Option<f64>,
    pub gpu_identity: Option<RendererExecutionIdentity>,
    pub diagnostics: Vec<String>,
}

impl RenderPerformanceReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HEADLESS_RENDER_PERFORMANCE_SCHEMA
            || !is_sha256(&self.build_fingerprint)
            || !is_sha256(&self.scenario_hash)
            || self.width != 1920
            || self.height != 1080
            || self.warmup_frames != 60
            || self.measured_frames != 600
            || self.cpu_all_duration_ns == 0
            || self.cpu_all_p50_frame_ns == 0
            || self.cpu_all_p95_frame_ns < self.cpu_all_p50_frame_ns
            || self.cpu_sparse_duration_ns == 0
            || self.cpu_sparse_p50_frame_ns == 0
            || self.cpu_sparse_p95_frame_ns < self.cpu_sparse_p50_frame_ns
            || !self.cpu_sparse_speedup.is_finite()
        {
            return Err(ProtocolError::invalid(
                "render_performance.validate",
                "render performance identity or measurement is invalid",
            ));
        }
        if self.gpu_all_duration_ns.is_some() != self.gpu_all_speedup.is_some()
            || self.gpu_all_duration_ns.is_some() != self.gpu_all_p50_frame_ns.is_some()
            || self.gpu_all_duration_ns.is_some() != self.gpu_all_p95_frame_ns.is_some()
            || self.gpu_all_duration_ns.is_some() != self.gpu_identity.is_some()
        {
            return Err(ProtocolError::invalid(
                "render_performance.gpu",
                "GPU measurement and identity must be present together",
            ));
        }
        if let Some(identity) = &self.gpu_identity {
            identity.validate()?;
            if self.gpu_all_p50_frame_ns == Some(0)
                || self.gpu_all_p95_frame_ns < self.gpu_all_p50_frame_ns
            {
                return Err(ProtocolError::invalid(
                    "render_performance.gpu",
                    "GPU frame percentiles are invalid",
                ));
            }
        }
        if self.status == RunStatus::Passed
            && (!self.diagnostics.is_empty()
                || self.cpu_sparse_speedup < MIN_CPU_SPARSE_SPEEDUP
                || self
                    .gpu_all_speedup
                    .is_some_and(|speedup| speedup < MIN_GPU_ALL_SPEEDUP))
        {
            return Err(ProtocolError::invalid(
                "render_performance.threshold",
                "passing performance report does not meet required speedups",
            ));
        }
        Ok(())
    }
}
