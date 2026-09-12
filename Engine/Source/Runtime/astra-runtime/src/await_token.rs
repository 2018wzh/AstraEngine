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
pub enum AwaitCompletionPolicy {
    HostResult,
    TickTimeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AwaitToken {
    pub token_id: AwaitTokenId,
    pub kind: AwaitKind,
    pub requested_at_step: u64,
    pub timeout_step: Option<u64>,
    pub completion_policy: AwaitCompletionPolicy,
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
    pub(crate) fn validate(&self) -> Result<(), Diagnostic> {
        let mut tokens = std::collections::BTreeMap::new();
        for token in &self.pending {
            token.validate()?;
            if tokens.insert(token.token_id, token).is_some() {
                return Err(Diagnostic::blocking(
                    "ASTRA_AWAIT_TOKEN_CONFLICT",
                    "saved await token id is duplicated",
                ));
            }
        }
        let mut results = std::collections::BTreeSet::new();
        for result in &self.completed {
            let valid_token = tokens
                .get(&result.token_id)
                .is_some_and(|token| token.completion_policy == AwaitCompletionPolicy::HostResult);
            if !valid_token || !results.insert(result.token_id) {
                return Err(Diagnostic::blocking(
                    "ASTRA_AWAIT_SAVE_RESULT_INVALID",
                    "saved completion must belong to exactly one pending host-result token",
                ));
            }
        }
        Ok(())
    }

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

    pub(crate) fn reject_stale(&mut self, token: AwaitTokenId) {
        self.diagnostics.push(
            Diagnostic::warning(
                "ASTRA_AWAIT_RESULT_STALE",
                "completion handle is cancelled, finished or belongs to another World generation",
            )
            .with_field("token", token.0),
        );
    }

    pub fn cancel(&mut self, token: AwaitTokenId) -> Option<AwaitToken> {
        let index = self
            .pending
            .iter()
            .position(|pending| pending.token_id == token)?;
        self.completed.retain(|result| result.token_id != token);
        Some(self.pending.remove(index))
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
        if token.completion_policy == AwaitCompletionPolicy::TickTimeout {
            self.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_AWAIT_RESULT_POLICY",
                    "tick-timeout await tokens reject submitted results",
                )
                .with_field("token", result.token_id.0),
            );
            return;
        }
        if self
            .completed
            .iter()
            .any(|queued| queued.token_id == result.token_id)
        {
            self.diagnostics.push(
                Diagnostic::warning(
                    "ASTRA_AWAIT_RESULT_DUPLICATE",
                    "await token already has a queued terminal result",
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
                let mut retained = Vec::with_capacity(self.pending.len());
                for token in self.pending.drain(..) {
                    if token.token_id == result.token_id {
                    } else {
                        retained.push(token);
                    }
                }
                self.pending = retained;

                ready.push(result);
            } else {
                later.push(result);
            }
        }
        self.completed = later;
        let mut timeout_tokens = Vec::new();
        let mut retained = Vec::with_capacity(self.pending.len());
        for token in self.pending.drain(..) {
            if token.completion_policy == AwaitCompletionPolicy::TickTimeout
                && token
                    .timeout_step
                    .is_some_and(|timeout_step| timeout_step <= step)
            {
                timeout_tokens.push(token);
            } else {
                retained.push(token);
            }
        }
        self.pending = retained;
        timeout_tokens.sort_by_key(|token| token.token_id);
        for token in timeout_tokens {
            ready.push(AwaitResult::timeout(token, step));
        }
        AwaitDrain {
            results: ready,
            diagnostics: std::mem::take(&mut self.diagnostics),
        }
    }

    pub(crate) fn has_result(&self, token: AwaitTokenId) -> bool {
        self.completed.iter().any(|result| result.token_id == token)
    }

    pub fn pending(&self) -> &[AwaitToken] {
        &self.pending
    }
}

impl AwaitToken {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        match self.completion_policy {
            AwaitCompletionPolicy::HostResult if self.timeout_step.is_some() => {
                Err(Diagnostic::blocking(
                    "ASTRA_AWAIT_COMPLETION_POLICY",
                    "host-result await token cannot declare a deterministic timeout",
                )
                .with_field("token", self.token_id.0))
            }
            AwaitCompletionPolicy::TickTimeout => {
                let Some(timeout_step) = self.timeout_step else {
                    return Err(Diagnostic::blocking(
                        "ASTRA_AWAIT_COMPLETION_POLICY",
                        "tick-timeout await token requires a timeout step",
                    )
                    .with_field("token", self.token_id.0));
                };
                if timeout_step < self.requested_at_step {
                    return Err(Diagnostic::blocking(
                        "ASTRA_AWAIT_COMPLETION_POLICY",
                        "await timeout step precedes the request step",
                    )
                    .with_field("token", self.token_id.0)
                    .with_field("requested_at_step", self.requested_at_step)
                    .with_field("timeout_step", timeout_step));
                }
                Ok(())
            }
            AwaitCompletionPolicy::HostResult => Ok(()),
        }
    }
}

impl AwaitResult {
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
