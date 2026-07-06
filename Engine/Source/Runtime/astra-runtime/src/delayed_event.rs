use astra_core::StableId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{EventId, EventPayload, EventSource, RuntimeEvent};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct DelayedEventId(pub StableId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScheduledEvent {
    pub id: DelayedEventId,
    pub due_tick: u64,
    pub sequence: u64,
    pub source: EventSource,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelayedEventQueue {
    queued: Vec<ScheduledEvent>,
    next_sequence: u64,
}

impl DelayedEventQueue {
    pub fn schedule(&mut self, mut event: ScheduledEvent) -> DelayedEventId {
        event.sequence = self.next_sequence;
        self.next_sequence += 1;
        let id = event.id;
        self.queued.push(event);
        id
    }

    pub fn cancel(&mut self, id: DelayedEventId) -> bool {
        let before = self.queued.len();
        self.queued.retain(|event| event.id != id);
        self.queued.len() != before
    }

    pub fn drain_due(&mut self, fixed_step: u64) -> Vec<RuntimeEvent> {
        self.queued
            .sort_by_key(|event| (event.due_tick, event.sequence, event.id));
        let mut ready = Vec::new();
        let mut pending = Vec::new();
        for event in self.queued.drain(..) {
            if event.due_tick <= fixed_step {
                ready.push(RuntimeEvent {
                    id: EventId(event.id.0),
                    source: event.source,
                    step: event.due_tick,
                    sequence: event.sequence,
                    payload: event.payload,
                });
            } else {
                pending.push(event);
            }
        }
        self.queued = pending;
        ready
    }

    pub fn queued(&self) -> &[ScheduledEvent] {
        &self.queued
    }
}
