use super::{EdenEdition, EdenHistoryMessage, EdenSaveEncoding};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Export support is acquired only by importing a validated eden checkpoint.
/// Unrepresentable execution permanently invalidates that history until import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub enum EdenExportState {
    #[default]
    Unavailable,
    Ready {
        edition: EdenEdition,
        encoding: EdenSaveEncoding,
        history: Vec<EdenHistoryMessage>,
    },
    Rejected(EdenExportRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EdenExportRejection {
    Variables,
    Presentation,
    Audio,
    MessageBoundary,
    HistoryBound,
}
