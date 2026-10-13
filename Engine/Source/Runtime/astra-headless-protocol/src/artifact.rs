use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{is_sha256, validate_symbol, ProtocolError};

mod report;
use report::safe_relative_path;
pub use report::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RendererExecutionIdentity {
    pub provider: String,
    pub backend: String,
    pub device_type: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub adapter_name_hash: String,
    pub driver_identity_hash: String,
}

impl RendererExecutionIdentity {
    pub fn cpu_reference() -> Self {
        Self {
            provider: "cpu_reference".into(),
            backend: "cpu".into(),
            device_type: "cpu".into(),
            vendor_id: 0,
            device_id: 0,
            adapter_name_hash: hash_bytes(b"cpu_reference"),
            driver_identity_hash: hash_bytes(b"deterministic_cpu"),
        }
    }

    pub fn hash(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| {
            ProtocolError::invalid("renderer.identity", "renderer identity encoding failed")
        })?;
        Ok(hash_bytes(&encoded))
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_symbol("renderer.provider", &self.provider)?;
        validate_symbol("renderer.backend", &self.backend)?;
        validate_symbol("renderer.device_type", &self.device_type)?;
        if !is_sha256(&self.adapter_name_hash) || !is_sha256(&self.driver_identity_hash) {
            return Err(ProtocolError::invalid(
                "renderer.identity",
                "renderer identity hashes are invalid",
            ));
        }
        Ok(())
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
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
    pub renderer_identity_hash: String,
    pub renderer_identity: RendererExecutionIdentity,
    pub render_policy: String,
    pub submitted_frame_count: u64,
    pub rasterized_frame_count: u64,
    pub audio_frame_count: u64,
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
                &self.renderer_identity_hash,
            ]
            .iter()
            .any(|hash| !is_sha256(hash))
        {
            return Err(ProtocolError::invalid(
                "artifact_manifest.validate",
                "artifact manifest identity or stream hash is invalid",
            ));
        }
        self.renderer_identity.validate()?;
        if self.renderer_identity_hash != self.renderer_identity.hash()? {
            return Err(ProtocolError::invalid(
                "artifact_manifest.renderer_identity",
                "renderer identity hash does not match its canonical identity",
            ));
        }
        if !matches!(self.render_policy.as_str(), "all" | "checkpoints") {
            return Err(ProtocolError::invalid(
                "artifact_manifest.render_policy",
                "artifact manifest render policy is invalid",
            ));
        }
        if self.rasterized_frame_count > self.submitted_frame_count {
            return Err(ProtocolError::invalid(
                "artifact_manifest.frame_count",
                "rasterized frame count exceeds submitted frame count",
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
                    checkpoint_ids,
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
                    let mut ids = std::collections::BTreeSet::new();
                    for checkpoint in checkpoint_ids {
                        validate_symbol("artifact_manifest.checkpoint", checkpoint)?;
                        if !ids.insert(checkpoint) {
                            return Err(ProtocolError::invalid(
                                "artifact_manifest.checkpoint",
                                "frame checkpoint identity is duplicated",
                            ));
                        }
                    }
                    (relative_path, sha256, None)
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
                            format!(
                                "audio artifact metadata is invalid: byte_size={byte_size}, sample_rate={sample_rate}, channels={channels}, frame_count={frame_count}, duration_ns={duration_ns}"
                            ),
                        ));
                    }
                    (relative_path, sha256, checkpoint.as_ref())
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
