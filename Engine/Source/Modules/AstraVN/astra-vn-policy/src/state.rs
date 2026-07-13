use std::collections::BTreeMap;

pub use astra_policy::{
    PolicyCommandRecord as PolicyCommandTraceEntry, PolicyExecutionBudget,
    PolicyQueryRecord as PolicyQueryTraceEntry, PolicyTraceRecord as PolicyTraceEvent,
    PolicyValue as PolicySnapshotValue,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPolicyState {
    pub variables: BTreeMap<String, BTreeMap<String, i64>>,
    #[serde(default)]
    pub mutation_trace: Vec<MutationTraceEntry>,
    #[serde(default)]
    pub command_trace: Vec<PolicyCommandTraceEntry>,
    #[serde(default)]
    pub query_trace: Vec<PolicyQueryTraceEntry>,
    #[serde(default)]
    pub trace_events: Vec<PolicyTraceEvent>,
    #[serde(default)]
    pub snapshots: BTreeMap<String, PolicySnapshotValue>,
}

impl VnPolicyState {
    pub fn var(&self, scope: &str, key: &str) -> Option<i64> {
        self.variables
            .get(scope)
            .and_then(|scope| scope.get(key))
            .copied()
    }

    pub fn snapshot(&self, key: &str) -> Option<&PolicySnapshotValue> {
        self.snapshots.get(key)
    }

    pub fn rollback_scope(&mut self, rollback_scope: &str) -> Vec<MutationTraceEntry> {
        let rolled_back = self
            .mutation_trace
            .iter()
            .filter(|entry| entry.rollback_scope == rollback_scope)
            .cloned()
            .collect::<Vec<_>>();
        for entry in rolled_back.iter().rev() {
            self.restore_previous_mutation_value(entry);
        }
        self.mutation_trace
            .retain(|entry| entry.rollback_scope != rollback_scope);
        rolled_back
    }

    pub fn replay_mutation_trace(&mut self, trace: &[MutationTraceEntry]) {
        for entry in trace {
            self.apply_mutation_value(entry);
            self.mutation_trace.push(entry.clone());
        }
    }

    fn apply_mutation_value(&mut self, entry: &MutationTraceEntry) {
        self.variables
            .entry(entry.scope.clone())
            .or_default()
            .insert(entry.key.clone(), entry.value);
    }

    fn restore_previous_mutation_value(&mut self, entry: &MutationTraceEntry) {
        match entry.previous_value {
            Some(value) => {
                self.variables
                    .entry(entry.scope.clone())
                    .or_default()
                    .insert(entry.key.clone(), value);
            }
            None => {
                if let Some(scope) = self.variables.get_mut(&entry.scope) {
                    scope.remove(&entry.key);
                    if scope.is_empty() {
                        self.variables.remove(&entry.scope);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MutationTraceEntry {
    pub api: String,
    pub scope: String,
    pub key: String,
    pub value: i64,
    pub previous_value: Option<i64>,
    pub dirty_scope: String,
    pub rollback_scope: String,
    pub replay_event: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyQueryContext {
    pub text: BTreeMap<String, PolicySnapshotValue>,
    pub assets: BTreeMap<String, PolicySnapshotValue>,
    pub backlog: PolicySnapshotValue,
    pub savepoint: PolicySnapshotValue,
    pub layouts: BTreeMap<String, PolicySnapshotValue>,
}

impl Default for PolicyQueryContext {
    fn default() -> Self {
        Self {
            text: BTreeMap::new(),
            assets: BTreeMap::new(),
            backlog: PolicySnapshotValue::Nil,
            savepoint: PolicySnapshotValue::Nil,
            layouts: BTreeMap::new(),
        }
    }
}
