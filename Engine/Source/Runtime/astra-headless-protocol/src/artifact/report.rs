use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;
use crate::{is_sha256, validate_symbol, ProtocolError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactEntry {
    Frame {
        relative_path: String,
        sha256: String,
        byte_size: u64,
        width: u32,
        height: u32,
        color_space: String,
        sequence: u64,
        checkpoint_ids: Vec<String>,
    },
    Audio {
        relative_path: String,
        sha256: String,
        byte_size: u64,
        sample_rate: u32,
        channels: u16,
        frame_count: u64,
        duration_ns: u64,
        checkpoint: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunReport {
    pub schema: String,
    pub run_id: String,
    pub build_fingerprint: String,
    pub package_hash: String,
    pub input_sequence_hash: String,
    pub checkpoint_config_hash: String,
    pub profile_id: String,
    pub session_id: String,
    pub scenario: String,
    pub target: String,
    pub content_identity: String,
    pub status: RunStatus,
    pub manifest_hash: String,
    pub renderer_identity_hash: String,
    pub render_policy: String,
    pub submitted_frame_count: u64,
    pub rasterized_frame_count: u64,
    pub audio_frame_count: u64,
    pub duration_ns: u64,
    pub completed_sequence: u64,
    pub checkpoint_results: Vec<CheckpointResult>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckpointResult {
    pub id: String,
    pub passed: bool,
    pub observation_hash: String,
    pub image_metrics: Option<ImageMetrics>,
    pub audio_metrics: Option<AudioMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageMetrics {
    pub changed_pixel_ratio: f64,
    pub max_channel_delta: u8,
    pub ssim: f64,
    pub nonempty_bbox_offset_px: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioMetrics {
    pub duration_delta_ms: f64,
    pub peak_delta_db: f64,
    pub rms_delta_db: f64,
    pub loudness_delta_lufs: f64,
    pub normalized_spectrum_distance: f64,
    pub silence_matches: bool,
    pub clipping_matches: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: String,
    pub operation: String,
    pub message: String,
}

impl RunReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != crate::HEADLESS_RUN_REPORT_SCHEMA
            || [
                &self.build_fingerprint,
                &self.package_hash,
                &self.input_sequence_hash,
                &self.checkpoint_config_hash,
                &self.manifest_hash,
                &self.renderer_identity_hash,
            ]
            .iter()
            .any(|hash| !is_sha256(hash))
        {
            return Err(ProtocolError::invalid(
                "run_report.validate",
                "run report schema or identity hash is invalid",
            ));
        }
        if !matches!(self.render_policy.as_str(), "all" | "checkpoints")
            || self.rasterized_frame_count > self.submitted_frame_count
        {
            return Err(ProtocolError::invalid(
                "run_report.render",
                "run report render policy or frame counts are invalid",
            ));
        }
        for (operation, value) in [
            ("run_report.run", &self.run_id),
            ("run_report.profile", &self.profile_id),
            ("run_report.session", &self.session_id),
            ("run_report.scenario", &self.scenario),
            ("run_report.target", &self.target),
            ("run_report.content", &self.content_identity),
        ] {
            validate_symbol(operation, value)?;
        }
        let mut checkpoints = std::collections::BTreeSet::new();
        for checkpoint in &self.checkpoint_results {
            validate_symbol("run_report.checkpoint", &checkpoint.id)?;
            if !is_sha256(&checkpoint.observation_hash) || !checkpoints.insert(&checkpoint.id) {
                return Err(ProtocolError::invalid(
                    "run_report.checkpoint",
                    "checkpoint identity is invalid or duplicated",
                ));
            }
        }
        if self.status == RunStatus::Passed
            && (self
                .checkpoint_results
                .iter()
                .any(|checkpoint| !checkpoint.passed)
                || !self.diagnostics.is_empty())
        {
            return Err(ProtocolError::invalid(
                "run_report.status",
                "passing report contains failed checkpoints or diagnostics",
            ));
        }
        Ok(())
    }
}

pub(super) fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && !value.starts_with('/')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && !value.contains(':')
        && !value.contains('\0')
}
