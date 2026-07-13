use std::path::PathBuf;

use astra_asset::{
    normalize_source_path, AssetError, AssetId, AssetSidecar, CookSettings, FontAssetMetadata,
    ReviewStatus,
};
use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CookError {
    #[error("{0}")]
    Message(String),
    #[error("cook blocked")]
    Diagnostics(Vec<Diagnostic>),
    #[error("ASTRA_COOK_CANCELLED: cook batch was cancelled")]
    Cancelled,
}

impl CookError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Diagnostics(diagnostics) => diagnostics,
            Self::Message(_) | Self::Cancelled => &[],
        }
    }
}

impl From<AssetError> for CookError {
    fn from(value: AssetError) -> Self {
        match value {
            AssetError::Diagnostics(diagnostics) => Self::Diagnostics(diagnostics),
            AssetError::Message(message) => Self::Message(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRequest {
    pub asset_id: AssetId,
    pub source_path: PathBuf,
    pub asset_type: String,
    pub license: String,
    pub font: Option<FontAssetMetadata>,
    pub target_profiles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImportAudit {
    pub schema: String,
    pub importer_id: String,
    pub source_hash: Hash256,
    pub metadata: ImportMetadata,
    pub sidecar: AssetSidecar,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImportMetadata {
    pub kind: String,
    pub codec: String,
    pub byte_len: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct DefaultMetadataImporter {
    importer_id: String,
}

impl DefaultMetadataImporter {
    pub fn new(importer_id: impl Into<String>) -> Self {
        Self {
            importer_id: importer_id.into(),
        }
    }

    pub fn import(&self, request: ImportRequest) -> Result<ImportAudit, CookError> {
        let bytes = std::fs::read(&request.source_path)
            .map_err(|err| CookError::message(format!("read source asset: {err}")))?;
        let source_hash = Hash256::from_sha256(&bytes);
        let metadata = probe_metadata(&request.source_path, &bytes)?;
        let source_name = request
            .source_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| CookError::message("source asset must have UTF-8 file name"))?;
        let source = normalize_source_path(&format!("content/imported/{source_name}"))?;
        let sidecar = AssetSidecar {
            schema: "astra.asset.v1".to_string(),
            id: request.asset_id,
            source,
            source_hash: Some(source_hash),
            asset_type: request.asset_type,
            license: Some(request.license),
            importer: self.importer_id.clone(),
            font: request.font,
            dependencies: Vec::new(),
            cook: CookSettings {
                processor: default_processor_for(&metadata.kind).to_string(),
                target_profiles: request.target_profiles,
                params: Default::default(),
            },
            review: ReviewStatus::Accepted,
        };
        let diagnostics = sidecar.validate();
        if diagnostics
            .iter()
            .any(|diag| diag.severity == astra_core::DiagnosticSeverity::Blocking)
        {
            return Err(CookError::Diagnostics(diagnostics));
        }
        Ok(ImportAudit {
            schema: "astra.import_audit.v1".to_string(),
            importer_id: self.importer_id.clone(),
            source_hash,
            metadata,
            sidecar,
            diagnostics,
        })
    }
}

fn probe_metadata(path: &std::path::Path, bytes: &[u8]) -> Result<ImportMetadata, CookError> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => {
            let image = image::load_from_memory(bytes)
                .map_err(|err| CookError::message(format!("decode image metadata: {err}")))?;
            Ok(ImportMetadata {
                kind: "image".to_string(),
                codec: extension,
                byte_len: bytes.len() as u64,
                width: Some(image.width()),
                height: Some(image.height()),
            })
        }
        "ttf" | "otf" => Ok(ImportMetadata {
            kind: "font".to_string(),
            codec: extension,
            byte_len: bytes.len() as u64,
            width: None,
            height: None,
        }),
        "wav" | "ogg" | "flac" | "mp3" => Ok(ImportMetadata {
            kind: "audio".to_string(),
            codec: extension,
            byte_len: bytes.len() as u64,
            width: None,
            height: None,
        }),
        other => Err(CookError::Diagnostics(vec![Diagnostic::blocking(
            "ASTRA_IMPORT_UNSUPPORTED_FORMAT",
            format!("unsupported source asset format {other}"),
        )])),
    }
}

fn default_processor_for(kind: &str) -> &'static str {
    match kind {
        "image" => "astra.cook.texture2d",
        "font" => "astra.cook.font",
        "audio" => "astra.cook.audio",
        _ => "astra.cook.binary",
    }
}
