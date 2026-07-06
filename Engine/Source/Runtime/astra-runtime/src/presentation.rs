use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::BlackboardValue;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum PresentationCommand {
    Dialogue {
        speaker: String,
        text: String,
    },
    Choice {
        prompt: String,
        options: Vec<String>,
    },
    TextEvent {
        key: String,
    },
    Marker {
        name: String,
    },
    Custom {
        kind: String,
        #[serde(default)]
        data: BTreeMap<String, BlackboardValue>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationRecord {
    pub step: u64,
    pub sequence: u64,
    pub command: PresentationCommand,
}
