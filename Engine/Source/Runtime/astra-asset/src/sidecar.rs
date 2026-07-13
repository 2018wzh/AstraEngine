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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<FontAssetMetadata>,
    #[serde(default)]
    pub dependencies: Vec<AssetId>,
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
            font: None,
            dependencies: Vec::new(),
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
        let is_font = self.asset_type.starts_with("font.");
        match (&self.font, is_font) {
            (None, true) => diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_FONT_METADATA_MISSING",
                    "font assets must declare family, face index, coverage, and optional subset metadata",
                )
                .with_field("asset_id", self.id.as_str()),
            ),
            (Some(_), false) => diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_FONT_METADATA_UNEXPECTED",
                    "non-font assets cannot declare font metadata",
                )
                .with_field("asset_id", self.id.as_str()),
            ),
            (Some(font), true) => diagnostics.extend(font.validate(self.id.as_str())),
            (None, false) => {}
        }
        let mut dependencies = std::collections::BTreeSet::new();
        for dependency in &self.dependencies {
            if dependency == &self.id {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_ASSET_DEPENDENCY_SELF",
                        "asset sidecar cannot depend on itself",
                    )
                    .with_field("asset_id", self.id.as_str()),
                );
            }
            if !dependencies.insert(dependency) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_ASSET_DEPENDENCY_DUPLICATE",
                        "asset sidecar repeats a dependency",
                    )
                    .with_field("asset_id", self.id.as_str())
                    .with_field("dependency", dependency.as_str()),
                );
            }
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
pub struct FontAssetMetadata {
    pub family: String,
    #[serde(default)]
    pub face_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subset: Option<String>,
    pub coverage: Vec<FontCoverageRange>,
}

impl FontAssetMetadata {
    fn validate(&self, asset_id: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        if self.family.trim().is_empty() {
            diagnostics.push(
                Diagnostic::blocking("ASTRA_ASSET_FONT_FAMILY", "font family must be non-empty")
                    .with_field("asset_id", asset_id),
            );
        }
        if self
            .subset
            .as_deref()
            .is_some_and(|subset| subset.trim().is_empty())
        {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_FONT_SUBSET",
                    "font subset must be omitted or non-empty",
                )
                .with_field("asset_id", asset_id),
            );
        }
        if self.coverage.is_empty()
            || self.coverage.iter().any(|range| {
                range.start > range.end
                    || char::from_u32(range.start).is_none()
                    || char::from_u32(range.end).is_none()
            })
            || self
                .coverage
                .windows(2)
                .any(|ranges| ranges[0].end >= ranges[1].start)
        {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_ASSET_FONT_COVERAGE",
                    "font coverage must contain ordered, disjoint Unicode scalar ranges",
                )
                .with_field("asset_id", asset_id),
            );
        }
        diagnostics
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FontCoverageRange {
    pub start: u32,
    pub end: u32,
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
