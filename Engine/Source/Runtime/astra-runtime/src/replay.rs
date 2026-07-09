use astra_core::{Hash128, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AwaitResult, PlayerInput, RuntimeEvent, RuntimeSnapshot, TickInput, TickReport};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeReplayTranscript {
    pub schema: String,
    pub checkpoint: RuntimeSnapshot,
    pub ticks: Vec<ReplayTick>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayTick {
    pub tick: TickInput,
    #[serde(default)]
    pub player_inputs: Vec<PlayerInput>,
    #[serde(default)]
    pub await_results: Vec<AwaitResult>,
    #[serde(default)]
    pub provider_outputs: Vec<ProviderReplayOutput>,
    pub expected: ReplayHashCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderReplayOutput {
    pub provider_id: String,
    pub session_id: String,
    pub schema: String,
    pub payload_hash: Hash256,
    #[serde(default)]
    pub events: Vec<RuntimeEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayHashCheckpoint {
    pub step: u64,
    pub state_hash: Hash128,
    pub event_hash: Hash128,
    pub presentation_hash: Hash128,
}

impl From<&TickReport> for ReplayHashCheckpoint {
    fn from(report: &TickReport) -> Self {
        Self {
            step: report.step,
            state_hash: report.state_hash,
            event_hash: report.event_hash,
            presentation_hash: report.presentation_hash,
        }
    }
}
