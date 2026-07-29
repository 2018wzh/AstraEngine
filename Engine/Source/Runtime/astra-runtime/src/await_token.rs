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
    #[serde(skip)]
    #[schemars(skip)]
    transaction: Option<AwaitQueueTransaction>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct AwaitQueueTransaction {
    pending_order: Vec<AwaitTokenId>,
    completed_order: Vec<(AwaitTokenId, u64)>,
    removed_pending: BTreeMap<AwaitTokenId, AwaitToken>,
    removed_completed: BTreeMap<(AwaitTokenId, u64), AwaitResult>,
    added_pending: BTreeSet<AwaitTokenId>,
    added_completed: BTreeSet<(AwaitTokenId, u64)>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AwaitDrain {
    pub results: Vec<AwaitResult>,
    pub diagnostics: Vec<Diagnostic>,
}

impl AwaitQueue {
    pub(crate) fn begin_transaction(&mut self) -> Result<(), &'static str> {
        if self.transaction.is_some() {
            return Err("ASTRA_RUNTIME_AWAIT_TRANSACTION_NESTED");
        }
        self.transaction = Some(AwaitQueueTransaction {
            pending_order: self.pending.iter().map(|token| token.token_id).collect(),
            completed_order: self
                .completed
                .iter()
                .map(|result| (result.token_id, result.sequence))
                .collect(),
            diagnostics: self.diagnostics.clone(),
            ..AwaitQueueTransaction::default()
        });
        Ok(())
    }

    pub(crate) fn commit_transaction(&mut self) {
        self.transaction = None;
    }

    pub(crate) fn rollback_transaction(&mut self) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        let mut pending = self
            .pending
            .drain(..)
            .filter(|token| !transaction.added_pending.contains(&token.token_id))
            .map(|token| (token.token_id, token))
            .collect::<BTreeMap<_, _>>();
        pending.extend(transaction.removed_pending);
        self.pending = transaction
            .pending_order
            .into_iter()
            .filter_map(|id| pending.remove(&id))
            .collect();

        let mut completed = self
            .completed
            .drain(..)
            .filter(|result| {
                !transaction
                    .added_completed
                    .contains(&(result.token_id, result.sequence))
            })
            .map(|result| ((result.token_id, result.sequence), result))
            .collect::<BTreeMap<_, _>>();
        completed.extend(transaction.removed_completed);
        self.completed = transaction
            .completed_order
            .into_iter()
            .filter_map(|key| completed.remove(&key))
            .collect();
        self.diagnostics = transaction.diagnostics;
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
        if let Some(transaction) = self.transaction.as_mut() {
            transaction.added_pending.insert(token.token_id);
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
        if let Some(transaction) = self.transaction.as_mut() {
            transaction
                .added_completed
                .insert((result.token_id, result.sequence));
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
                        if let Some(transaction) = self.transaction.as_mut() {
                            if !transaction.added_pending.contains(&token.token_id) {
                                transaction
                                    .removed_pending
                                    .insert(token.token_id, token.clone());
                            }
                        }
                    } else {
                        retained.push(token);
                    }
                }
                self.pending = retained;
                if let Some(transaction) = self.transaction.as_mut() {
                    let key = (result.token_id, result.sequence);
                    if !transaction.added_completed.contains(&key) {
                        transaction.removed_completed.insert(key, result.clone());
                    }
                }
                ready.push(result);
            } else {
                later.push(result);
            }
        }
        self.completed = later;
        let mut timeout_tokens = Vec::new();
        let mut retained = Vec::with_capacity(self.pending.len());
        for token in self.pending.drain(..) {
            if token.replay_policy == AwaitReplayPolicy::DeterministicTimeout
                && token
                    .deterministic_timeout_step
                    .is_some_and(|timeout_step| timeout_step <= step)
            {
                timeout_tokens.push(token.clone());
                if let Some(transaction) = self.transaction.as_mut() {
                    if !transaction.added_pending.contains(&token.token_id) {
                        transaction.removed_pending.insert(token.token_id, token);
                    }
                }
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
use std::collections::{BTreeMap, BTreeSet};
