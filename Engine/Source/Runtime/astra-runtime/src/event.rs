use std::collections::{BTreeMap, BTreeSet};

use astra_core::StableId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::BlackboardValue;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EventId(pub StableId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Runtime,
    PlayerInput,
    AwaitResult,
    StateMachine,
    Scenario,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventPayload {
    pub kind: String,
    #[serde(default)]
    pub data: BTreeMap<String, BlackboardValue>,
}

impl EventPayload {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            data: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeEvent {
    pub id: EventId,
    pub source: EventSource,
    pub step: u64,
    pub sequence: u64,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventQueue {
    queued: Vec<RuntimeEvent>,
    trace: Vec<RuntimeEvent>,
    next_sequence: u64,
    #[serde(skip)]
    #[schemars(skip)]
    transaction: Option<EventQueueTransaction>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct EventQueueTransaction {
    queued_order: Vec<EventId>,
    removed: BTreeMap<EventId, RuntimeEvent>,
    added: BTreeSet<EventId>,
    trace_len: usize,
    next_sequence: u64,
}

impl EventQueue {
    pub(crate) fn begin_transaction(&mut self) -> Result<(), &'static str> {
        if self.transaction.is_some() {
            return Err("ASTRA_RUNTIME_EVENT_TRANSACTION_NESTED");
        }
        self.transaction = Some(EventQueueTransaction {
            queued_order: self.queued.iter().map(|event| event.id).collect(),
            removed: BTreeMap::new(),
            added: BTreeSet::new(),
            trace_len: self.trace.len(),
            next_sequence: self.next_sequence,
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
        let mut values = self
            .queued
            .drain(..)
            .filter(|event| !transaction.added.contains(&event.id))
            .map(|event| (event.id, event))
            .collect::<BTreeMap<_, _>>();
        values.extend(transaction.removed);
        self.queued = transaction
            .queued_order
            .into_iter()
            .filter_map(|id| values.remove(&id))
            .collect();
        self.trace.truncate(transaction.trace_len);
        self.next_sequence = transaction.next_sequence;
    }

    pub(crate) fn deterministic_pending_fingerprint(&self) -> astra_core::Hash128 {
        astra_core::Hash128::from_blake3(
            &postcard::to_allocvec(&(&self.queued, self.next_sequence))
                .expect("event queue pending state must serialize for deterministic hashing"),
        )
    }

    pub fn push(&mut self, mut event: RuntimeEvent) {
        if let Some(transaction) = self.transaction.as_mut() {
            transaction.added.insert(event.id);
        }
        event.sequence = self.next_sequence;
        self.next_sequence += 1;
        self.queued.push(event);
    }

    pub fn drain_ordered_for_step(&mut self, step: u64) -> Vec<RuntimeEvent> {
        self.queued
            .sort_by_key(|event| (event.step, event.sequence, event.id));
        let mut ready = Vec::new();
        let mut pending = Vec::new();
        for event in self.queued.drain(..) {
            if event.step <= step {
                if let Some(transaction) = self.transaction.as_mut() {
                    if !transaction.added.contains(&event.id) {
                        transaction.removed.insert(event.id, event.clone());
                    }
                }
                ready.push(event);
            } else {
                pending.push(event);
            }
        }
        self.queued = pending;
        self.trace.extend(ready.clone());
        ready
    }

    pub fn trace(&self) -> &[RuntimeEvent] {
        &self.trace
    }

    pub fn pending(&self) -> &[RuntimeEvent] {
        &self.queued
    }
}
