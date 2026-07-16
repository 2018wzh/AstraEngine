use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LegacyProviderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyContextState {
    Runnable,
    Waiting,
    Terminal,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LegacyContextCursor {
    pub context_id: u32,
    pub priority: i32,
    pub sequence: u64,
    pub pc: u64,
    pub state: LegacyContextState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeterministicFamilyScheduler {
    contexts: BTreeMap<u32, LegacyContextCursor>,
}

impl DeterministicFamilyScheduler {
    pub fn register(&mut self, cursor: LegacyContextCursor) -> Result<(), LegacyProviderError> {
        if self.contexts.insert(cursor.context_id, cursor).is_some() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_CONTEXT_DUPLICATE",
                "context id is already registered",
            ));
        }
        Ok(())
    }

    pub fn update(&mut self, cursor: LegacyContextCursor) -> Result<(), LegacyProviderError> {
        let existing = self.contexts.get(&cursor.context_id).ok_or_else(|| {
            LegacyProviderError::invalid("ASTRA_EMU_CONTEXT_MISSING", "context is not registered")
        })?;
        if cursor.sequence < existing.sequence {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_CONTEXT_SEQUENCE",
                "context sequence moved backwards",
            ));
        }
        self.contexts.insert(cursor.context_id, cursor);
        Ok(())
    }

    pub fn runnable_order(&self) -> Vec<u32> {
        let mut runnable: Vec<_> = self
            .contexts
            .values()
            .filter(|cursor| cursor.state == LegacyContextState::Runnable)
            .collect();
        runnable.sort_by_key(|cursor| (cursor.priority, cursor.context_id, cursor.sequence));
        runnable
            .into_iter()
            .map(|cursor| cursor.context_id)
            .collect()
    }

    pub fn contexts(&self) -> impl Iterator<Item = &LegacyContextCursor> {
        self.contexts.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_uses_priority_context_and_sequence() {
        let mut scheduler = DeterministicFamilyScheduler::default();
        for cursor in [
            LegacyContextCursor {
                context_id: 8,
                priority: 1,
                sequence: 2,
                pc: 0,
                state: LegacyContextState::Runnable,
            },
            LegacyContextCursor {
                context_id: 2,
                priority: 0,
                sequence: 9,
                pc: 0,
                state: LegacyContextState::Runnable,
            },
            LegacyContextCursor {
                context_id: 1,
                priority: 1,
                sequence: 7,
                pc: 0,
                state: LegacyContextState::Runnable,
            },
        ] {
            scheduler.register(cursor).unwrap();
        }
        assert_eq!(scheduler.runnable_order(), vec![2, 1, 8]);
    }
}
