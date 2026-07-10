use astra_core::{Diagnostic, StableId};
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
    #[serde(default)]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AwaitDrain {
    pub results: Vec<AwaitResult>,
    pub diagnostics: Vec<Diagnostic>,
}

impl AwaitQueue {
    pub fn insert(&mut self, token: AwaitToken) -> Result<(), Diagnostic> {
        token.validate()?;
        if self
            .pending
            .iter()
            .any(|pending| pending.token_id == token.token_id)
        {
            return Err(Diagnostic::blocking(
                "ASTRA_AWAIT_TOKEN_CONFLICT",
                "await token id is already pending",
            )
            .with_field("token", token.token_id.0));
        }
        self.pending.push(token);
        Ok(())
    }

    pub fn submit_result(&mut self, result: AwaitResult) {
        let token = self
            .pending
            .iter()
            .find(|token| token.token_id == result.token_id);
        let Some(token) = token else {
            self.diagnostics.push(
                Diagnostic::warning(
                    "ASTRA_AWAIT_RESULT_UNKNOWN",
                    "await result was submitted for an unknown token",
                )
                .with_field("token", result.token_id.0),
            );
            return;
        };
        if token.replay_policy == AwaitReplayPolicy::DeterministicTimeout {
            self.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_AWAIT_RESULT_POLICY",
                    "deterministic-timeout await tokens reject submitted results",
                )
                .with_field("token", result.token_id.0),
            );
            return;
        }
        if self
            .completed
            .iter()
            .any(|queued| queued.token_id == result.token_id && queued.sequence == result.sequence)
        {
            self.diagnostics.push(
                Diagnostic::warning(
                    "ASTRA_AWAIT_RESULT_DUPLICATE",
                    "duplicate await result sequence was ignored",
                )
                .with_field("token", result.token_id.0)
                .with_field("sequence", result.sequence),
            );
            return;
        }
        self.completed.push(result);
    }

    pub fn drain_ordered_results(&mut self, step: u64) -> AwaitDrain {
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
        let mut timeout_tokens = Vec::new();
        self.pending.retain(|token| {
            if token.replay_policy == AwaitReplayPolicy::DeterministicTimeout
                && token
                    .deterministic_timeout_step
                    .is_some_and(|timeout_step| timeout_step <= step)
            {
                timeout_tokens.push(token.clone());
                false
            } else {
                true
            }
        });
        timeout_tokens.sort_by_key(|token| token.token_id);
        for token in timeout_tokens {
            ready.push(AwaitResult::timeout(token, step));
        }
        AwaitDrain {
            results: ready,
            diagnostics: self.diagnostics.drain(..).collect(),
        }
    }

    pub fn pending(&self) -> &[AwaitToken] {
        &self.pending
    }
}

impl AwaitToken {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        match self.replay_policy {
            AwaitReplayPolicy::RecordedResult if self.deterministic_timeout_step.is_some() => {
                Err(Diagnostic::blocking(
                    "ASTRA_AWAIT_REPLAY_POLICY",
                    "recorded-result await token cannot declare a deterministic timeout",
                )
                .with_field("token", self.token_id.0))
            }
            AwaitReplayPolicy::DeterministicTimeout => {
                let Some(timeout_step) = self.deterministic_timeout_step else {
                    return Err(Diagnostic::blocking(
                        "ASTRA_AWAIT_REPLAY_POLICY",
                        "deterministic-timeout await token requires a timeout step",
                    )
                    .with_field("token", self.token_id.0));
                };
                if timeout_step < self.requested_at_step {
                    return Err(Diagnostic::blocking(
                        "ASTRA_AWAIT_REPLAY_POLICY",
                        "await timeout step precedes the request step",
                    )
                    .with_field("token", self.token_id.0)
                    .with_field("requested_at_step", self.requested_at_step)
                    .with_field("timeout_step", timeout_step));
                }
                Ok(())
            }
            AwaitReplayPolicy::RecordedResult => Ok(()),
        }
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

    pub fn timeout(token: AwaitToken, step: u64) -> Self {
        let mut payload = EventPayload::new("await.timeout");
        payload.data.insert(
            "kind".to_string(),
            BlackboardValue::String(format!("{:?}", token.kind)),
        );
        Self {
            token_id: token.token_id,
            sequence: u64::MAX,
            completed_at_step: step,
            payload,
        }
    }
}
