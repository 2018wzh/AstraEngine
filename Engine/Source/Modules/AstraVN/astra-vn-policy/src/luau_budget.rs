//! Account host-owned copies before Lua values are deep-copied into transaction state.
use crate::{PolicyExecutionBudget, PolicyQueryContext, PolicySnapshotValue, VnPolicyState};
use std::{cell::RefCell, rc::Rc};

pub(super) type SharedBudget = Rc<RefCell<HostBudget>>;
pub(super) struct HostBudget {
    bytes: usize,
    outputs: usize,
    limits: PolicyExecutionBudget,
    failed: bool,
}
impl HostBudget {
    pub(super) fn new(
        state: &VnPolicyState,
        queries: &PolicyQueryContext,
        limits: PolicyExecutionBudget,
    ) -> mlua::Result<SharedBudget> {
        let mut bytes = 0usize;
        for (scope, values) in &state.variables {
            bytes = bytes.saturating_add(64 + scope.len());
            for key in values.keys() {
                bytes = bytes.saturating_add(64 + key.len());
            }
        }
        for (key, value) in &state.snapshots {
            bytes = bytes.saturating_add(entry_bytes(key, value));
        }
        for record in &state.mutation_trace {
            bytes = bytes.saturating_add(
                128 + record.api.len()
                    + record.scope.len()
                    + record.key.len()
                    + record.dirty_scope.len()
                    + record.rollback_scope.len()
                    + record.replay_event.len(),
            );
        }
        for record in &state.command_trace {
            bytes = bytes
                .saturating_add(
                    128 + record.api.len() + record.name.len() + record.replay_event.len(),
                )
                .saturating_add(value_bytes(&record.payload));
        }
        for record in &state.trace_events {
            bytes = bytes
                .saturating_add(
                    128 + record.api.len() + record.kind.len() + record.replay_event.len(),
                )
                .saturating_add(value_bytes(&record.fields));
        }
        for record in &state.query_trace {
            bytes = bytes
                .saturating_add(
                    128 + record.api.len() + record.target.len() + record.replay_event.len(),
                )
                .saturating_add(value_bytes(&record.result));
            for (key, value) in &record.args {
                bytes = bytes.saturating_add(entry_bytes(key, value));
            }
        }
        for (key, value) in queries
            .text
            .iter()
            .chain(&queries.assets)
            .chain(&queries.layouts)
        {
            bytes = bytes.saturating_add(entry_bytes(key, value));
        }
        bytes = bytes
            .saturating_add(value_bytes(&queries.backlog))
            .saturating_add(value_bytes(&queries.savepoint));
        if bytes > limits.memory_bytes {
            return Err(memory_error());
        }
        Ok(Rc::new(RefCell::new(Self {
            bytes,
            outputs: 0,
            limits,
            failed: false,
        })))
    }
    pub(super) fn replace(&mut self, old: usize, new: usize) -> mlua::Result<()> {
        if self.failed {
            return Err(memory_error());
        }
        if self.outputs >= self.limits.output_limit {
            self.failed = true;
            return Err(mlua::Error::runtime("ASTRA_VN_LUAU_OUTPUT_BUDGET"));
        }
        let bytes = self.bytes.saturating_sub(old).saturating_add(new);
        if bytes > self.limits.memory_bytes {
            self.failed = true;
            return Err(memory_error());
        }
        self.bytes = bytes;
        self.outputs += 1;
        Ok(())
    }
    pub(super) fn check(&self) -> mlua::Result<()> {
        if self.failed {
            Err(memory_error())
        } else {
            Ok(())
        }
    }
    pub(super) fn lua_value_bytes(&self, value: &mlua::Value, depth: usize) -> mlua::Result<usize> {
        if depth > self.limits.snapshot_depth {
            return Err(mlua::Error::runtime(
                "ASTRA_VN_LUAU_SNAPSHOT_UNSERIALIZABLE: snapshot depth",
            ));
        }
        let mut bytes = 64usize;
        match value {
            mlua::Value::String(s) => bytes = bytes.saturating_add(s.as_bytes().len()),
            mlua::Value::Table(table) => {
                for pair in table.clone().pairs::<mlua::Value, mlua::Value>() {
                    let (key, value) = pair?;
                    bytes = bytes.saturating_add(match key {
                        mlua::Value::String(s) => s.as_bytes().len(),
                        _ => 32,
                    });
                    bytes = bytes.saturating_add(self.lua_value_bytes(&value, depth + 1)?);
                    if bytes > self.limits.memory_bytes {
                        break;
                    }
                }
            }
            _ => (),
        }
        Ok(bytes)
    }
}

pub(super) fn value_bytes(value: &PolicySnapshotValue) -> usize {
    match value {
        PolicySnapshotValue::String(s) => 64usize.saturating_add(s.len()),
        PolicySnapshotValue::Object(values) => values.iter().fold(64usize, |sum, (key, value)| {
            sum.saturating_add(entry_bytes(key, value))
        }),
        _ => 64,
    }
}
pub(super) fn entry_bytes(key: &str, value: &PolicySnapshotValue) -> usize {
    key.len().saturating_add(value_bytes(value))
}
fn memory_error() -> mlua::Error {
    mlua::Error::runtime("ASTRA_VN_LUAU_HOST_MEMORY_BUDGET")
}
