use astra_core::StableId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BlackboardValue, EventPayload};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct AwaitTokenId(pub StableId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AwaitKind {
    Timer,
    PresentationFence,
    AudioFence,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AwaitReplayPolicy {
    RecordedResult,
    DeterministicTimeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AwaitToken {
    pub token_id: AwaitTokenId,
    pub kind: AwaitKind,
    pub requested_at_step: u64,
    pub deterministic_timeout_step: Option<u64>,
    pub replay_policy: AwaitReplayPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AwaitResult {
    pub token_id: AwaitTokenId,
    pub sequence: u64,
    pub completed_at_step: u64,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AwaitQueue {
    pending: Vec<AwaitToken>,
    completed: Vec<AwaitResult>,
}

impl AwaitQueue {
    pub fn insert(&mut self, token: AwaitToken) {
        self.pending.push(token);
    }

    pub fn submit_result(&mut self, result: AwaitResult) {
        self.completed.push(result);
    }

    pub fn drain_ordered_results(&mut self, step: u64) -> Vec<AwaitResult> {
        self.completed
            .sort_by_key(|result| (result.token_id, result.sequence));
        let mut ready = Vec::new();
        let mut later = Vec::new();
        for result in self.completed.drain(..) {
            if result.completed_at_step <= step {
                self.pending
                    .retain(|token| token.token_id != result.token_id);
                ready.push(result);
            } else {
                later.push(result);
            }
        }
        self.completed = later;
        ready
    }

    pub fn pending(&self) -> &[AwaitToken] {
        &self.pending
    }
}

impl AwaitResult {
    pub fn custom(
        token_id: AwaitTokenId,
        sequence: u64,
        step: u64,
        value: impl Into<String>,
    ) -> Self {
        let mut payload = EventPayload::new("await.completed");
        payload
            .data
            .insert("value".to_string(), BlackboardValue::String(value.into()));
        Self {
            token_id,
            sequence,
            completed_at_step: step,
            payload,
        }
    }
}
