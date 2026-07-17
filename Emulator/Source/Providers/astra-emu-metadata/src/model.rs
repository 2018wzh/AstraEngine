use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetadataProviderId {
    Vndb,
    Bangumi,
}

impl MetadataProviderId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vndb => "vndb",
            Self::Bangumi => "bangumi",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MetadataSearchQuery {
    pub title: String,
    pub aliases: Vec<String>,
    pub developer: Option<String>,
    pub release_date: Option<String>,
    pub limit: u8,
}

impl MetadataSearchQuery {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.title.trim().is_empty() || self.title.chars().count() > 256 {
            return Err(MetadataError::InvalidRequest("title"));
        }
        if !(1..=20).contains(&self.limit)
            || self.aliases.len() > 16
            || self.aliases.iter().any(|value| value.chars().count() > 256)
            || self
                .developer
                .as_ref()
                .is_some_and(|value| value.chars().count() > 256)
        {
            return Err(MetadataError::InvalidRequest("bounds"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MetadataRecord {
    pub provider: MetadataProviderId,
    pub remote_id: String,
    pub title: String,
    pub alternate_titles: Vec<String>,
    pub developers: Vec<String>,
    pub release_date: Option<String>,
    pub platforms: Vec<String>,
    pub engine: Option<String>,
    pub cover: Option<RemoteCover>,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemoteCover {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sexual: Option<f32>,
    pub violence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverAsset {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MatchEvidence {
    pub kind: String,
    pub local_value: String,
    pub remote_value: String,
    pub score_millis: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MatchAssessment {
    pub matcher_version: String,
    pub score_millis: u16,
    pub evidence: Vec<MatchEvidence>,
    pub requires_confirmation: bool,
    pub auto_link_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BangumiPlayStatus {
    Wish,
    Doing,
    Collect,
    OnHold,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BangumiPlayUpdate {
    pub subject_id: u32,
    pub status: BangumiPlayStatus,
    pub rating: Option<u8>,
    pub note: Option<String>,
    pub private: bool,
}

impl BangumiPlayUpdate {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.subject_id == 0
            || self
                .rating
                .is_some_and(|rating| !(1..=10).contains(&rating))
            || self
                .note
                .as_ref()
                .is_some_and(|note| note.chars().count() > 1024)
        {
            return Err(MetadataError::InvalidRequest("bangumi_play_update"));
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("ASTRA_EMU_METADATA_INVALID_REQUEST: {0}")]
    InvalidRequest(&'static str),
    #[error("ASTRA_EMU_METADATA_CONSENT_REQUIRED: {0}")]
    ConsentRequired(&'static str),
    #[error("ASTRA_EMU_METADATA_LICENSE_BLOCKED: {0}")]
    LicenseBlocked(&'static str),
    #[error("ASTRA_EMU_METADATA_REMOTE_ID: {0}")]
    InvalidRemoteId(String),
    #[error("ASTRA_EMU_METADATA_NOT_FOUND: {0}")]
    NotFound(String),
    #[error("ASTRA_EMU_METADATA_RATE_LIMITED")]
    RateLimited,
    #[error("ASTRA_EMU_METADATA_UNAUTHORIZED")]
    Unauthorized,
    #[error("ASTRA_EMU_METADATA_SCHEMA_MISMATCH: {0}")]
    SchemaMismatch(&'static str),
    #[error("ASTRA_EMU_METADATA_NETWORK: {0}")]
    Network(String),
    #[error("ASTRA_EMU_METADATA_RESPONSE_BOUNDS: {0}")]
    ResponseBounds(&'static str),
    #[error("ASTRA_EMU_METADATA_COVER_BLOCKED: {0}")]
    CoverBlocked(&'static str),
    #[error("ASTRA_EMU_METADATA_UPSTREAM: {0}")]
    Upstream(String),
}
