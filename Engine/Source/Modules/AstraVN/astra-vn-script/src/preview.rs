//! Parent-owned document revisions, bound to the project actually opened by Player.
use astra_core::Hash256;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PREVIEW_PROTOCOL: &str = "astra.vn.preview.v1";
pub const PREVIEW_MAX_MESSAGE_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewDocumentRevision {
    pub version: u64,
    pub content_hash: Hash256,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewIdentity {
    pub project_hash: Hash256,
    pub documents: BTreeMap<String, PreviewDocumentRevision>,
    pub generation: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub protocol: String,
    pub sequence: u64,
    pub identity: PreviewIdentity,
    pub command: PreviewCommand,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreviewCommand {
    Attach,
    Pause,
    Resume,
    SeekWithinFragment { source_id: String, checkpoint: u64 },
    Stop,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewCheckpointInfo {
    pub id: u64,
    pub presentation_time_ns: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewStatus {
    pub identity: PreviewIdentity,
    pub paused: bool,
    pub presentation_time_ns: u64,
    pub source_id: Option<String>,
    pub checkpoints: Vec<PreviewCheckpointInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewResponse {
    Ready {
        protocol: String,
        sequence: u64,
        status: PreviewStatus,
    },
    State {
        sequence: u64,
        status: PreviewStatus,
    },
    Rejected {
        sequence: u64,
        code: PreviewRejectCode,
    },
    Failure {
        code: PreviewRejectCode,
    },
    Stopped,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum PreviewRejectCode {
    #[error("preview protocol or message is invalid")]
    InvalidRequest,
    #[error("preview document, project or session identity is stale")]
    StaleIdentity,
    #[error("preview request sequence has already been consumed")]
    StaleRequest,
    #[error("preview has not attached")]
    NotAttached,
    #[error("pause preview before seeking")]
    NotPaused,
    #[error("checkpoint belongs to another source fragment")]
    CrossFragment,
    #[error("the exact checkpoint is no longer retained")]
    CheckpointUnavailable,
    #[error("current performance cannot be restored")]
    NotRecoverable,
    #[error("checkpoint restore failed")]
    RestoreFailed,
    #[error("preview control connection closed")]
    Disconnected,
}
