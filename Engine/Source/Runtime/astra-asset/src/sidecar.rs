use std::collections::BTreeMap;

use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{normalize_source_path, AssetError, AssetId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssetSidecar {
    pub schema: String,
    pub id: AssetId,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<Hash256>,
    #[serde(rename = "type")]
    pub asset_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub importer: String,
    pub cook: CookSettings,
    pub review: ReviewStatus,
}

impl AssetSidecar {
    pub fn from_yaml(input: &str) -> Result<Self, AssetError> {
        serde_yaml::from_str(input).map_err(|err| AssetError::message(err.to_string()))
    }

    pub fn to_yaml(&self) -> Result<String, AssetError> {
        serde_yaml::to_string(self).map_err(|err| AssetError::message(err.to_string()))
    }

    pub fn new_test(id: &str, source: &str, asset_type: &str) -> Self {
        Self {
            schema: "astra.asset.v1".to_string(),
            id: AssetId::parse(id).expect("valid test asset id"),
            source: source.to_string(),
            source_hash: Some(Hash256::from_sha256(source.as_bytes())),
            asset_type: asset_type.to_string(),
            license: Some("project-owned".to_string()),
            importer: "astra.import.test".to_string(),
            cook: CookSettings {
                processor: "astra.cook.test".to_string(),
                target_profiles: vec!["desktop-release".to_string()],
                params: BTreeMap::new(),
            },
            review: ReviewStatus::Accepted,
        }
    }

    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        if self.schema != "astra.asset.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_ASSET_SCHEMA",
                "asset sidecar schema must be astra.asset.v1",
            ));
        }
        if let Err(err) = normalize_source_path(&self.source) {
            diagnostics.push(
                Diagnostic::blocking("ASTRA_ASSET_SOURCE_PATH", err.to_string())
                    .with_field("asset_id", self.id.as_str()),
            );
        }
        if self.source_hash.is_none() {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_SOURCE_HASH_MISSING",
                    "asset sidecar must include source_hash",
                )
                .with_field("asset_id", self.id.as_str()),
            );
        }
        if self.license.as_deref().is_none_or(str::is_empty) {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_LICENSE_MISSING",
                    "asset sidecar must include license",
                )
                .with_field("asset_id", self.id.as_str()),
            );
        }
        if self.importer.trim().is_empty() {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_IMPORTER_MISSING",
                    "asset sidecar must include importer id",
                )
                .with_field("asset_id", self.id.as_str()),
            );
        }
        if self.cook.processor.trim().is_empty() {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_COOK_PROCESSOR_MISSING",
                    "asset sidecar must include cook processor",
                )
                .with_field("asset_id", self.id.as_str()),
            );
        }
        if self.cook.target_profiles.is_empty() {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_TARGET_PROFILE_MISSING",
                    "asset sidecar must include target profile",
                )
                .with_field("asset_id", self.id.as_str()),
            );
        }
        diagnostics
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CookSettings {
    pub processor: String,
    #[serde(default)]
    pub target_profiles: Vec<String>,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Draft,
    Accepted,
    Rejected,
}
