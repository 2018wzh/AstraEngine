use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use mlua::{Lua, VmState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{PolicyCommandRecord, PolicyError, PolicyQueryRecord, PolicyTraceRecord, PolicyValue};

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

pub fn create_sandboxed_lua(budget: PolicyExecutionBudget) -> Result<Lua, PolicyError> {
    budget.validate()?;
    let lua = Lua::new();
    lua.set_memory_limit(budget.memory_bytes)
        .map_err(|error| PolicyError::Runtime(error.to_string()))?;
    let interrupt_count = Rc::new(Cell::new(0_u64));
    let counter = Rc::clone(&interrupt_count);
    lua.set_interrupt(move |_| {
        let next = counter.get().saturating_add(1);
        counter.set(next);
        if next > budget.interrupt_limit {
            return Err(mlua::Error::runtime(
                "ASTRA_POLICY_INSTRUCTION_BUDGET: policy exceeded interrupt budget",
            ));
        }
        Ok(VmState::Continue)
    });
    let globals = lua.globals();
    for name in [
        "io", "os", "debug", "package", "require", "loadfile", "dofile",
    ] {
        globals
            .set(name, mlua::Value::Nil)
            .map_err(|error| PolicyError::Runtime(error.to_string()))?;
    }
    drop(globals);
    tracing::debug!(
        event = "policy.sandbox.create",
        interrupt_limit = budget.interrupt_limit,
        memory_bytes = budget.memory_bytes,
        output_limit = budget.output_limit,
        snapshot_depth = budget.snapshot_depth,
        "shared policy sandbox created"
    );
    Ok(lua)
}

/// Host-neutral owner for the shared Luau sandbox and its deterministic limits.
pub struct PolicyVm {
    lua: Lua,
    budget: PolicyExecutionBudget,
    state: BTreeMap<String, PolicyValue>,
    commands: Vec<PolicyCommandRecord>,
    queries: Vec<PolicyQueryRecord>,
    traces: Vec<PolicyTraceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicySnapshot {
    pub schema: String,
    pub state: BTreeMap<String, PolicyValue>,
    pub commands: Vec<PolicyCommandRecord>,
    pub queries: Vec<PolicyQueryRecord>,
    pub traces: Vec<PolicyTraceRecord>,
}

impl PolicyVm {
    pub fn new(budget: PolicyExecutionBudget) -> Result<Self, PolicyError> {
        Ok(Self {
            lua: create_sandboxed_lua(budget)?,
            budget,
            state: BTreeMap::new(),
            commands: Vec::new(),
            queries: Vec::new(),
            traces: Vec::new(),
        })
    }

    pub fn budget(&self) -> PolicyExecutionBudget {
        self.budget
    }

    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    pub fn into_lua(self) -> Lua {
        self.lua
    }

    pub fn eval_bool(&self, source: &str) -> Result<bool, PolicyError> {
        self.lua
            .load(source)
            .eval()
            .map_err(|error| PolicyError::Runtime(error.to_string()))
    }

    pub fn set_state(
        &mut self,
        key: impl Into<String>,
        value: PolicyValue,
    ) -> Result<(), PolicyError> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(PolicyError::diagnostic(
                "ASTRA_POLICY_STATE_KEY",
                "policy state key must be non-empty",
            ));
        }
        value.validate_depth(self.budget.snapshot_depth)?;
        self.state.insert(key, value);
        Ok(())
    }

    pub fn state(&self, key: &str) -> Option<&PolicyValue> {
        self.state.get(key)
    }

    pub fn record_command(&mut self, record: PolicyCommandRecord) -> Result<(), PolicyError> {
        record.payload.validate_depth(self.budget.snapshot_depth)?;
        self.require_output_capacity()?;
        self.commands.push(record);
        Ok(())
    }

    pub fn record_query(&mut self, record: PolicyQueryRecord) -> Result<(), PolicyError> {
        for value in record.args.values() {
            value.validate_depth(self.budget.snapshot_depth)?;
        }
        self.require_output_capacity()?;
        self.queries.push(record);
        Ok(())
    }

    pub fn record_trace(&mut self, record: PolicyTraceRecord) -> Result<(), PolicyError> {
        record.fields.validate_depth(self.budget.snapshot_depth)?;
        self.require_output_capacity()?;
        self.traces.push(record);
        Ok(())
    }

    pub fn snapshot(&self) -> PolicySnapshot {
        PolicySnapshot {
            schema: "astra.policy_snapshot.v1".into(),
            state: self.state.clone(),
            commands: self.commands.clone(),
            queries: self.queries.clone(),
            traces: self.traces.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: PolicySnapshot) -> Result<(), PolicyError> {
        if snapshot.schema != "astra.policy_snapshot.v1" {
            return Err(PolicyError::diagnostic(
                "ASTRA_POLICY_SNAPSHOT_SCHEMA",
                "policy snapshot schema is unsupported",
            ));
        }
        let output_count = snapshot.commands.len() + snapshot.queries.len() + snapshot.traces.len();
        if output_count > self.budget.output_limit {
            return Err(PolicyError::diagnostic(
                "ASTRA_POLICY_OUTPUT_BUDGET",
                "policy snapshot exceeds the configured output budget",
            ));
        }
        for value in snapshot.state.values() {
            value.validate_depth(self.budget.snapshot_depth)?;
        }
        self.state = snapshot.state;
        self.commands = snapshot.commands;
        self.queries = snapshot.queries;
        self.traces = snapshot.traces;
        Ok(())
    }

    fn require_output_capacity(&self) -> Result<(), PolicyError> {
        if self.commands.len() + self.queries.len() + self.traces.len() >= self.budget.output_limit
        {
            return Err(PolicyError::diagnostic(
                "ASTRA_POLICY_OUTPUT_BUDGET",
                "policy output record budget is exhausted",
            ));
        }
        Ok(())
    }
}
