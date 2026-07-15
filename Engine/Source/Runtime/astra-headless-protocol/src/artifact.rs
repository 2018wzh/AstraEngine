use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    is_sha256, validate_symbol, ProtocolError, HEADLESS_PREFLIGHT_LINK_SCHEMA,
    HEADLESS_REVIEW_BUNDLE_SCHEMA, HEADLESS_REVIEW_SCHEMA, PLATFORM_RUN_IDENTITY_SCHEMA,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    pub schema: String,
    pub run_id: String,
    pub build_fingerprint: String,
    pub package_hash: String,
    pub input_sequence_hash: String,
    pub provider_identity_hash: String,
    pub presented_frame_count: u64,
    pub audio_frame_count: u64,
    pub frame_stream_hash: String,
    pub audio_stream_hash: String,
    pub audio_peak_dbfs: Option<f64>,
    pub audio_rms_dbfs: Option<f64>,
    pub silence: bool,
    pub clipping: bool,
    pub artifacts: Vec<ArtifactEntry>,
}

impl ArtifactManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != crate::HEADLESS_ARTIFACT_MANIFEST_SCHEMA
            || [
                &self.build_fingerprint,
                &self.package_hash,
                &self.input_sequence_hash,
                &self.provider_identity_hash,
                &self.frame_stream_hash,
                &self.audio_stream_hash,
            ]
            .iter()
            .any(|hash| !is_sha256(hash))
        {
            return Err(ProtocolError::invalid(
                "artifact_manifest.validate",
                "artifact manifest identity or stream hash is invalid",
            ));
        }
        validate_symbol("artifact_manifest.run", &self.run_id)?;
        if self
            .audio_peak_dbfs
            .is_some_and(|value| !value.is_finite() || value > 0.0)
            || self
                .audio_rms_dbfs
                .is_some_and(|value| !value.is_finite() || value > 0.0)
        {
            return Err(ProtocolError::invalid(
                "artifact_manifest.audio_metrics",
                "artifact audio metrics are not finite dBFS values",
            ));
        }
        let mut paths = std::collections::BTreeSet::new();
        for artifact in &self.artifacts {
            let (path, hash, checkpoint) = match artifact {
                ArtifactEntry::Frame {
                    relative_path,
                    sha256,
                    byte_size,
                    width,
                    height,
                    color_space,
                    sequence,
                    checkpoint,
                } => {
                    if *byte_size == 0
                        || *width == 0
                        || *height == 0
                        || color_space != "rgba8_srgb"
                        || *sequence == 0
                    {
                        return Err(ProtocolError::invalid(
                            "artifact_manifest.frame",
                            "frame artifact metadata is invalid",
                        ));
                    }
                    (relative_path, sha256, checkpoint)
                }
                ArtifactEntry::Audio {
                    relative_path,
                    sha256,
                    byte_size,
                    sample_rate,
                    channels,
                    frame_count,
                    duration_ns,
                    checkpoint,
                } => {
                    if *byte_size == 0
                        || *sample_rate != 48_000
                        || *channels != 2
                        || *frame_count == 0
                        || *duration_ns == 0
                    {
                        return Err(ProtocolError::invalid(
                            "artifact_manifest.audio",
                            "audio artifact metadata is invalid",
                        ));
                    }
                    (relative_path, sha256, checkpoint)
                }
            };
            if !safe_relative_path(path) || !is_sha256(hash) || !paths.insert(path) {
                return Err(ProtocolError::invalid(
                    "artifact_manifest.artifact",
                    "artifact path or hash is invalid or duplicated",
                ));
            }
            if let Some(checkpoint) = checkpoint {
                validate_symbol("artifact_manifest.checkpoint", checkpoint)?;
            }
        }
        Ok(())
    }
}

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
        checkpoint: Option<String>,
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
    pub frame_count: u64,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewRecord {
    pub schema: String,
    pub run_report_hash: String,
    pub reviewer_kind: ReviewerKind,
    pub reviewer_identity: String,
    pub tool_identity_hash: String,
    pub checkpoints: Vec<ReviewVerdict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewBundle {
    pub schema: String,
    pub run_report_hash: String,
    pub manifest_hash: String,
    pub automatic_passed: bool,
    pub selected_frames: Vec<ReviewArtifactSelection>,
    pub selected_audio: Vec<ReviewArtifactSelection>,
    pub required_checkpoints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewArtifactSelection {
    pub role: ReviewArtifactRole,
    pub relative_path: String,
    pub sha256: String,
    pub sequence: Option<u64>,
    pub checkpoint: Option<String>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ReviewArtifactRole {
    FirstFrame,
    LastFrame,
    RequiredCheckpoint,
    MaximumDifference,
    FailureNeighbor,
    FullAudio,
    CheckpointAudio,
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
            ]
            .iter()
            .any(|hash| !is_sha256(hash))
        {
            return Err(ProtocolError::invalid(
                "run_report.validate",
                "run report schema or identity hash is invalid",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerKind {
    Model,
    Human,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewVerdict {
    pub checkpoint: String,
    pub passed: bool,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreflightLink {
    pub schema: String,
    pub headless_run_report_hash: String,
    pub platform_run_report_hash: String,
    pub build_fingerprint: String,
    pub cooked_package_hash: String,
    pub input_sequence_hash: String,
    pub scenario: String,
    pub target: String,
    pub content_identity: String,
    pub headless_profile_id: String,
    pub headless_session_id: String,
    pub platform_profile_id: String,
    pub platform_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlatformRunIdentity {
    pub schema: String,
    pub run_report_hash: String,
    pub build_fingerprint: String,
    pub cooked_package_hash: String,
    pub input_sequence_hash: String,
    pub scenario: String,
    pub target: String,
    pub content_identity: String,
    pub profile_id: String,
    pub session_id: String,
}

impl ReviewRecord {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HEADLESS_REVIEW_SCHEMA
            || !is_sha256(&self.run_report_hash)
            || !is_sha256(&self.tool_identity_hash)
            || self.reviewer_identity.is_empty()
            || self.reviewer_identity.len() > 256
            || self.reviewer_identity.contains('\0')
            || self.checkpoints.is_empty()
        {
            return Err(ProtocolError::invalid(
                "review.validate",
                "review schema, identity, hashes, or verdict set is invalid",
            ));
        }
        let mut checkpoints = std::collections::BTreeSet::new();
        for verdict in &self.checkpoints {
            validate_symbol("review.checkpoint", &verdict.checkpoint)?;
            if !checkpoints.insert(&verdict.checkpoint) {
                return Err(ProtocolError::invalid(
                    "review.checkpoint",
                    "review repeats a checkpoint verdict",
                ));
            }
            for code in &verdict.diagnostic_codes {
                validate_symbol("review.diagnostic", code)?;
            }
        }
        Ok(())
    }
}

impl ReviewBundle {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HEADLESS_REVIEW_BUNDLE_SCHEMA
            || !is_sha256(&self.run_report_hash)
            || !is_sha256(&self.manifest_hash)
            || self.selected_frames.is_empty()
            || self.selected_audio.is_empty()
        {
            return Err(ProtocolError::invalid(
                "review_bundle.validate",
                "review bundle schema, hashes, or required media selection is invalid",
            ));
        }
        let mut identities = std::collections::BTreeSet::new();
        for artifact in self.selected_frames.iter().chain(&self.selected_audio) {
            if !is_sha256(&artifact.sha256)
                || !safe_relative_path(&artifact.relative_path)
                || !identities.insert((artifact.role, artifact.relative_path.as_str()))
            {
                return Err(ProtocolError::invalid(
                    "review_bundle.artifact",
                    "review artifact path, hash, or role identity is invalid",
                ));
            }
            if let Some(checkpoint) = &artifact.checkpoint {
                validate_symbol("review_bundle.checkpoint", checkpoint)?;
            }
        }
        let mut checkpoints = std::collections::BTreeSet::new();
        for checkpoint in &self.required_checkpoints {
            validate_symbol("review_bundle.required_checkpoint", checkpoint)?;
            if !checkpoints.insert(checkpoint) {
                return Err(ProtocolError::invalid(
                    "review_bundle.required_checkpoint",
                    "required checkpoint is duplicated",
                ));
            }
            if !self.selected_frames.iter().any(|artifact| {
                artifact.checkpoint.as_deref() == Some(checkpoint.as_str())
                    && artifact.role == ReviewArtifactRole::RequiredCheckpoint
            }) {
                return Err(ProtocolError::invalid(
                    "review_bundle.required_checkpoint",
                    "required checkpoint has no selected frame",
                ));
            }
        }
        Ok(())
    }
}

impl PreflightLink {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HEADLESS_PREFLIGHT_LINK_SCHEMA
            || [
                &self.headless_run_report_hash,
                &self.platform_run_report_hash,
                &self.build_fingerprint,
                &self.cooked_package_hash,
                &self.input_sequence_hash,
            ]
            .iter()
            .any(|hash| !is_sha256(hash))
        {
            return Err(ProtocolError::invalid(
                "preflight.validate",
                "preflight schema or identity hash is invalid",
            ));
        }
        for (operation, value) in [
            ("preflight.scenario", &self.scenario),
            ("preflight.target", &self.target),
            ("preflight.content", &self.content_identity),
            ("preflight.headless_profile", &self.headless_profile_id),
            ("preflight.headless_session", &self.headless_session_id),
            ("preflight.platform_profile", &self.platform_profile_id),
            ("preflight.platform_session", &self.platform_session_id),
        ] {
            validate_symbol(operation, value)?;
        }
        Ok(())
    }
}

impl PlatformRunIdentity {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != PLATFORM_RUN_IDENTITY_SCHEMA
            || [
                &self.run_report_hash,
                &self.build_fingerprint,
                &self.cooked_package_hash,
                &self.input_sequence_hash,
            ]
            .iter()
            .any(|hash| !is_sha256(hash))
        {
            return Err(ProtocolError::invalid(
                "platform_run_identity.validate",
                "platform run identity schema or hash is invalid",
            ));
        }
        for (operation, value) in [
            ("platform_run_identity.scenario", &self.scenario),
            ("platform_run_identity.target", &self.target),
            ("platform_run_identity.content", &self.content_identity),
            ("platform_run_identity.profile", &self.profile_id),
            ("platform_run_identity.session", &self.session_id),
        ] {
            validate_symbol(operation, value)?;
        }
        Ok(())
    }
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && !value.starts_with('/')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && !value.contains(':')
        && !value.contains('\0')
}
