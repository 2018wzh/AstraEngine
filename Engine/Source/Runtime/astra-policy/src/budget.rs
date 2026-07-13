use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::PolicyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyExecutionBudget {
    pub interrupt_limit: u64,
    pub memory_bytes: usize,
    pub output_limit: usize,
    pub snapshot_depth: usize,
}

impl Default for PolicyExecutionBudget {
    fn default() -> Self {
        Self {
            interrupt_limit: 100_000,
            memory_bytes: 16 * 1024 * 1024,
            output_limit: 4096,
            snapshot_depth: 8,
        }
    }
}

impl PolicyExecutionBudget {
    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.interrupt_limit == 0
            || self.memory_bytes == 0
            || self.output_limit == 0
            || self.snapshot_depth == 0
        {
            return Err(PolicyError::diagnostic(
                "ASTRA_POLICY_BUDGET_INVALID",
                "policy execution budgets must be non-zero",
            ));
        }
        Ok(())
    }
}
