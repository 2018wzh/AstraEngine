use astra_asset::AssetSidecar;
use astra_core::{Diagnostic, Hash256};
use astra_package::SectionPayload;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CookError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookRequest {
    pub sidecar: AssetSidecar,
    pub source_bytes: Vec<u8>,
    pub target_profile: String,
    pub processor_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CookArtifact {
    pub schema: String,
    pub asset_id: String,
    pub section_id: String,
    pub target_profile: String,
    pub processor_id: String,
    pub processor_version: String,
    pub source_hash: Hash256,
    pub sidecar_hash: Hash256,
    pub cache_key: Hash256,
    pub payload_hash: Hash256,
    pub payload: Vec<u8>,
}

impl CookArtifact {
    pub fn to_section(&self) -> SectionPayload {
        SectionPayload::raw(
            self.section_id.clone(),
            "astra.cooked_asset.v1",
            self.payload.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct DefaultCookProcessor {
    processor_id: String,
    processor_version: String,
}

impl DefaultCookProcessor {
    pub fn new(processor_id: impl Into<String>, processor_version: impl Into<String>) -> Self {
        Self {
            processor_id: processor_id.into(),
            processor_version: processor_version.into(),
        }
    }

    pub fn cook(&self, request: CookRequest) -> Result<CookArtifact, CookError> {
        tracing::info!(
            event = "cook.asset.start",
            asset_id = %request.sidecar.id,
            profile = %request.target_profile,
            source_byte_size = request.source_bytes.len(),
            "asset cook started"
        );
        let diagnostics = validate_cook_request(&request, &self.processor_id);
        if !diagnostics.is_empty() {
            tracing::error!(
                event = "cook.asset.blocked",
                asset_id = %request.sidecar.id,
                diagnostic_count = diagnostics.len(),
                "asset cook blocked"
            );
            return Err(CookError::Diagnostics(diagnostics));
        }
        let sidecar_yaml = request
            .sidecar
            .to_yaml()
            .map_err(|err| CookError::message(err.to_string()))?;
        let source_hash = Hash256::from_sha256(&request.source_bytes);
        let sidecar_hash = Hash256::from_sha256(sidecar_yaml.as_bytes());
        let cache_key = cook_cache_key(
            &source_hash,
            &sidecar_hash,
            &self.processor_id,
            &request.processor_version,
            &request.target_profile,
        );
        let payload = request.source_bytes.clone();
        let artifact = CookArtifact {
            schema: "astra.cook_artifact.v1".to_string(),
            asset_id: request.sidecar.id.to_string(),
            section_id: section_id_for(&request.sidecar),
            target_profile: request.target_profile,
            processor_id: self.processor_id.clone(),
            processor_version: self.processor_version.clone(),
            source_hash,
            sidecar_hash,
            cache_key,
            payload_hash: Hash256::from_sha256(&payload),
            payload,
        };
        tracing::info!(
            event = "cook.asset.complete",
            asset_id = %artifact.asset_id,
            cache_key = %artifact.cache_key,
            payload_hash = %artifact.payload_hash,
            "asset cook completed"
        );
        Ok(artifact)
    }
}

pub fn cook_cache_key(
    source_hash: &Hash256,
    sidecar_hash: &Hash256,
    processor_id: &str,
    processor_version: &str,
    target_profile: &str,
) -> Hash256 {
    let input = format!(
        "{}|{}|{}|{}|{}",
        source_hash, sidecar_hash, processor_id, processor_version, target_profile
    );
    Hash256::from_sha256(input.as_bytes())
}

pub fn expected_cache_key(request: &CookRequest, processor_id: &str) -> Result<Hash256, CookError> {
    let sidecar_yaml = request
        .sidecar
        .to_yaml()
        .map_err(|err| CookError::message(err.to_string()))?;
    Ok(cook_cache_key(
        &Hash256::from_sha256(&request.source_bytes),
        &Hash256::from_sha256(sidecar_yaml.as_bytes()),
        processor_id,
        &request.processor_version,
        &request.target_profile,
    ))
}

fn validate_cook_request(request: &CookRequest, processor_id: &str) -> Vec<Diagnostic> {
    let mut diagnostics = request.sidecar.validate();
    if request.sidecar.cook.processor != processor_id {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_COOK_PROCESSOR_MISMATCH",
            "cook processor does not match sidecar",
        ));
    }
    if !request
        .sidecar
        .cook
        .target_profiles
        .contains(&request.target_profile)
    {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_COOK_PROFILE_BLOCKED",
            "asset is not enabled for target profile",
        ));
    }
    diagnostics
}

fn section_id_for(sidecar: &AssetSidecar) -> String {
    let normalized = sidecar
        .id
        .as_str()
        .trim_start_matches("asset:/")
        .replace('/', ".");
    format!("asset.{normalized}")
}
