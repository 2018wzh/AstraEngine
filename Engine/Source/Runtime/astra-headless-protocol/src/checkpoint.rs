use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    is_sha256, validate_symbol, ProtocolError, HEADLESS_CHECKPOINT_CONFIG_SCHEMA,
    HEADLESS_TOLERANCE_APPROVAL_SCHEMA,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckpointConfig {
    pub schema: String,
    pub id: String,
    pub input_sequence_hash: String,
    pub checkpoints: Vec<CheckpointExpectation>,
    pub tolerance_approval: Option<ToleranceApprovalBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToleranceApprovalBinding {
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToleranceApproval {
    pub schema: String,
    pub approval_id: String,
    pub approver_kind: ToleranceApproverKind,
    pub approver_identity: String,
    pub approved_tolerance_hash: String,
    pub previous_config_hash: Option<String>,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToleranceApproverKind {
    Human,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckpointExpectation {
    pub id: String,
    pub required: bool,
    pub observation_hash: Option<String>,
    pub image_baseline_path: Option<String>,
    pub image_baseline_hash: Option<String>,
    pub audio_baseline_path: Option<String>,
    pub audio_baseline_hash: Option<String>,
    #[serde(default)]
    pub image_tolerance: ImageTolerance,
    #[serde(default)]
    pub audio_tolerance: AudioTolerance,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageTolerance {
    pub changed_pixel_ratio: f64,
    pub max_channel_delta: u8,
    pub min_ssim: f64,
    pub max_nonempty_bbox_offset_px: u32,
}

impl Default for ImageTolerance {
    fn default() -> Self {
        Self {
            changed_pixel_ratio: 0.001,
            max_channel_delta: 4,
            min_ssim: 0.995,
            max_nonempty_bbox_offset_px: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioTolerance {
    pub max_duration_delta_ms: f64,
    pub max_peak_delta_db: f64,
    pub max_rms_delta_db: f64,
    pub max_loudness_delta_lufs: f64,
    pub max_normalized_spectrum_distance: f64,
}

impl Default for AudioTolerance {
    fn default() -> Self {
        Self {
            max_duration_delta_ms: 5.0,
            max_peak_delta_db: 0.5,
            max_rms_delta_db: 0.5,
            max_loudness_delta_lufs: 0.5,
            max_normalized_spectrum_distance: 0.05,
        }
    }
}

impl CheckpointConfig {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HEADLESS_CHECKPOINT_CONFIG_SCHEMA {
            return Err(ProtocolError::invalid(
                "checkpoint.schema",
                "unsupported checkpoint schema",
            ));
        }
        validate_symbol("checkpoint.id", &self.id)?;
        if !is_sha256(&self.input_sequence_hash) {
            return Err(ProtocolError::invalid(
                "checkpoint.input_hash",
                "input sequence hash must be sha256",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut customized_tolerance = false;
        for checkpoint in &self.checkpoints {
            validate_symbol("checkpoint.entry.id", &checkpoint.id)?;
            if !ids.insert(&checkpoint.id) {
                return Err(ProtocolError::invalid(
                    "checkpoint.entry.id",
                    "checkpoint id is duplicated",
                ));
            }
            for hash in [
                checkpoint.observation_hash.as_deref(),
                checkpoint.image_baseline_hash.as_deref(),
                checkpoint.audio_baseline_hash.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                if !is_sha256(hash) {
                    return Err(ProtocolError::invalid(
                        "checkpoint.hash",
                        "checkpoint hash must be sha256",
                    ));
                }
            }
            validate_baseline_pair(
                "checkpoint.image_baseline",
                checkpoint.image_baseline_path.as_deref(),
                checkpoint.image_baseline_hash.as_deref(),
            )?;
            validate_baseline_pair(
                "checkpoint.audio_baseline",
                checkpoint.audio_baseline_path.as_deref(),
                checkpoint.audio_baseline_hash.as_deref(),
            )?;
            validate_image_tolerance(checkpoint.image_tolerance)?;
            validate_audio_tolerance(checkpoint.audio_tolerance)?;
            customized_tolerance |= checkpoint.image_tolerance != ImageTolerance::default()
                || checkpoint.audio_tolerance != AudioTolerance::default();
        }
        if customized_tolerance && self.tolerance_approval.is_none() {
            return Err(ProtocolError::invalid(
                "checkpoint.tolerance_approval",
                "custom tolerance requires a hash-bound human approval record",
            ));
        }
        if let Some(approval) = &self.tolerance_approval {
            validate_baseline_pair(
                "checkpoint.tolerance_approval",
                Some(&approval.relative_path),
                Some(&approval.sha256),
            )?;
        }
        Ok(())
    }

    pub fn tolerance_hash(&self) -> Result<String, ProtocolError> {
        let values = self
            .checkpoints
            .iter()
            .map(|checkpoint| {
                (
                    checkpoint.id.as_str(),
                    checkpoint.image_tolerance,
                    checkpoint.audio_tolerance,
                )
            })
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&values).map_err(|_| {
            ProtocolError::invalid(
                "checkpoint.tolerance_hash",
                "tolerance set cannot be canonicalized",
            )
        })?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }
}

impl ToleranceApproval {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HEADLESS_TOLERANCE_APPROVAL_SCHEMA
            || !is_sha256(&self.approved_tolerance_hash)
            || self
                .previous_config_hash
                .as_ref()
                .is_some_and(|hash| !is_sha256(hash))
        {
            return Err(ProtocolError::invalid(
                "tolerance_approval.validate",
                "approval schema or configuration hashes are invalid",
            ));
        }
        validate_symbol("tolerance_approval.id", &self.approval_id)?;
        validate_symbol("tolerance_approval.approver", &self.approver_identity)?;
        if self.reason_codes.is_empty() {
            return Err(ProtocolError::invalid(
                "tolerance_approval.reason",
                "approval must record at least one reason code",
            ));
        }
        for reason in &self.reason_codes {
            validate_symbol("tolerance_approval.reason", reason)?;
        }
        Ok(())
    }
}

fn validate_baseline_pair(
    operation: &'static str,
    path: Option<&str>,
    hash: Option<&str>,
) -> Result<(), ProtocolError> {
    if path.is_some() != hash.is_some() {
        return Err(ProtocolError::invalid(
            operation,
            "baseline path and hash must be declared together",
        ));
    }
    if let Some(path) = path {
        let path = std::path::Path::new(path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(ProtocolError::invalid(
                operation,
                "baseline path must be a safe relative path",
            ));
        }
    }
    Ok(())
}

fn validate_image_tolerance(value: ImageTolerance) -> Result<(), ProtocolError> {
    if !(0.0..=1.0).contains(&value.changed_pixel_ratio) || !(0.0..=1.0).contains(&value.min_ssim) {
        return Err(ProtocolError::invalid(
            "checkpoint.image_tolerance",
            "image ratios must be finite values in 0..=1",
        ));
    }
    Ok(())
}

fn validate_audio_tolerance(value: AudioTolerance) -> Result<(), ProtocolError> {
    let values = [
        value.max_duration_delta_ms,
        value.max_peak_delta_db,
        value.max_rms_delta_db,
        value.max_loudness_delta_lufs,
        value.max_normalized_spectrum_distance,
    ];
    if values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err(ProtocolError::invalid(
            "checkpoint.audio_tolerance",
            "audio tolerances must be finite and non-negative",
        ));
    }
    Ok(())
}
