use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{expected_cache_key, CookArtifact, CookRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Fresh,
    Stale,
    Blocked,
}

pub struct CookAudit;

impl CookAudit {
    pub fn classify(request: &CookRequest, artifact: &CookArtifact) -> ArtifactState {
        match expected_cache_key(request, &artifact.processor_id) {
            Ok(expected) if expected == artifact.cache_key => ArtifactState::Fresh,
            Ok(_) => ArtifactState::Stale,
            Err(_) => ArtifactState::Blocked,
        }
    }
}
