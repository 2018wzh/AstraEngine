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

#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct DelayedEventQueue {
    queued: Vec<ScheduledEvent>,
    next_sequence: u64,
}

impl<'de> Deserialize<'de> for DelayedEventQueue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct SavedQueue {
            queued: Vec<ScheduledEvent>,
            next_sequence: u64,
        }
        let mut saved = SavedQueue::deserialize(deserializer)?;
        // Normalize once at the recovery boundary, never on an idle tick.
        saved.queued.sort_unstable_by_key(event_order);
        Ok(Self {
            queued: saved.queued,
            next_sequence: saved.next_sequence,
        })
    }
}

fn event_order(event: &ScheduledEvent) -> (u64, u64, DelayedEventId) {
    (event.due_tick, event.sequence, event.id)
}

impl DelayedEventQueue {
    pub fn schedule(&mut self, mut event: ScheduledEvent) -> DelayedEventId {
        event.sequence = self.next_sequence;
        self.next_sequence += 1;
        let id = event.id;
        let key = event_order(&event);
        let index = self
            .queued
            .partition_point(|pending| event_order(pending) <= key);
        self.queued.insert(index, event);
        id
    }

    pub fn cancel(&mut self, id: DelayedEventId) -> bool {
        let Some(index) = self.queued.iter().position(|event| event.id == id) else {
            return false;
        };
        self.queued.remove(index);

        true
    }

    pub fn drain_due(&mut self, fixed_step: u64) -> Vec<RuntimeEvent> {
        if self
            .queued
            .first()
            .is_none_or(|event| event.due_tick > fixed_step)
        {
            return Vec::new();
        }
        let due = self
            .queued
            .partition_point(|event| event.due_tick <= fixed_step);
        self.queued
            .drain(..due)
            .map(|event| RuntimeEvent {
                id: EventId(event.id.0),
                source: event.source,
                step: event.due_tick,
                sequence: event.sequence,
                payload: event.payload,
            })
            .collect()
    }

    pub fn queued(&self) -> &[ScheduledEvent] {
        &self.queued
    }
}
