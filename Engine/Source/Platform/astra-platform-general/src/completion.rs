use std::collections::BTreeMap;

use astra_platform::{PlatformError, PlatformErrorCode};

pub struct OrderedCompletionQueue<T> {
    next_sequence: u64,
    capacity: usize,
    pending: BTreeMap<u64, T>,
}

impl<T> OrderedCompletionQueue<T> {
    pub fn new(next_sequence: u64, capacity: usize) -> Self {
        Self {
            next_sequence,
            capacity,
            pending: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, sequence: u64, value: T) -> Result<(), PlatformError> {
        if sequence < self.next_sequence || self.pending.contains_key(&sequence) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "completion.push",
                "completion sequence is stale or duplicated",
            ));
        }
        if self.capacity == 0
            || sequence.saturating_sub(self.next_sequence) >= self.capacity as u64
            || self.pending.len() >= self.capacity
        {
            return Err(PlatformError::new(
                PlatformErrorCode::QueueOverflow,
                "completion.push",
                "completion exceeds the bounded reorder window",
            ));
        }
        self.pending.insert(sequence, value);
        Ok(())
    }

    pub fn drain_ready(&mut self) -> Vec<(u64, T)> {
        let mut ready = Vec::new();
        while let Some(value) = self.pending.remove(&self.next_sequence) {
            let sequence = self.next_sequence;
            self.next_sequence = self.next_sequence.saturating_add(1);
            ready.push((sequence, value));
        }
        ready
    }
}
