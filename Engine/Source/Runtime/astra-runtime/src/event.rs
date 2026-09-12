use std::collections::BTreeMap;

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
}

impl EventQueue {
    pub fn push(&mut self, mut event: RuntimeEvent) {
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
