use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{is_sha256, validate_symbol, ProtocolError, HEADLESS_CHECKPOINT_CONFIG_SCHEMA};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckpointConfig {
    pub schema: String,
    pub id: String,
    pub input_sequence_hash: String,
    pub renderer_identity_hash: String,
    pub checkpoints: Vec<CheckpointExpectation>,
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
        if !is_sha256(&self.input_sequence_hash) || !is_sha256(&self.renderer_identity_hash) {
            return Err(ProtocolError::invalid(
                "checkpoint.input_hash",
                "input sequence and renderer identity hashes must be sha256",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
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
