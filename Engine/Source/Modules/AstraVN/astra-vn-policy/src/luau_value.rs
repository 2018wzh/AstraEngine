use super::luau::LUAU_MAX_EXACT_INTEGER;
use super::luau_budget::{entry_bytes, value_bytes, SharedBudget};
use crate::{
    PolicyCommandTraceEntry, PolicyQueryTraceEntry, PolicySnapshotValue, PolicyTraceEvent, VnError,
};
use astra_core::Diagnostic;
use mlua::Lua;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub(super) fn record_policy_command(
    command_trace: &Rc<RefCell<Vec<PolicyCommandTraceEntry>>>,
    api: &str,
    name: String,
    payload: PolicySnapshotValue,
) {
    command_trace.borrow_mut().push(PolicyCommandTraceEntry {
        api: api.to_string(),
        name,
        payload,
        replay_event: "vn.policy.command".to_string(),
    });
}

pub(super) fn record_policy_query(
    query_trace: &Rc<RefCell<Vec<PolicyQueryTraceEntry>>>,
    budget: &SharedBudget,
    api: &str,
    target: String,
    args: BTreeMap<String, PolicySnapshotValue>,
    result: &PolicySnapshotValue,
) -> mlua::Result<()> {
    let bytes = args
        .iter()
        .fold(256 + api.len() + target.len(), |sum, (key, value)| {
            sum.saturating_add(entry_bytes(key, value))
        })
        .saturating_add(value_bytes(result));
    budget.borrow_mut().replace(0, bytes)?;
    query_trace.borrow_mut().push(PolicyQueryTraceEntry {
        api: api.to_string(),
        target,
        args,
        result: result.clone(),
        replay_event: "vn.policy.query".to_string(),
    });
    Ok(())
}

pub(super) fn record_policy_trace(
    trace_events: &Rc<RefCell<Vec<PolicyTraceEvent>>>,
    api: &str,
    kind: String,
    fields: PolicySnapshotValue,
) {
    trace_events.borrow_mut().push(PolicyTraceEvent {
        api: api.to_string(),
        kind,
        fields,
        replay_event: "vn.policy.trace".to_string(),
    });
}

pub(super) fn sandbox_error(err: mlua::Error) -> VnError {
    let message = err.to_string();
    for (marker, code) in [
        ("ASTRA_VN_LUAU_AUTHORITY_API", "ASTRA_VN_LUAU_AUTHORITY_API"),
        (
            "ASTRA_VN_LUAU_INSTRUCTION_BUDGET",
            "ASTRA_VN_LUAU_INSTRUCTION_BUDGET",
        ),
        (
            "ASTRA_POLICY_INSTRUCTION_BUDGET",
            "ASTRA_VN_LUAU_INSTRUCTION_BUDGET",
        ),
        (
            "ASTRA_VN_POLICY_QUERY_MISSING",
            "ASTRA_VN_POLICY_QUERY_MISSING",
        ),
    ] {
        if message.contains(marker) {
            return VnError::Diagnostic(Diagnostic::blocking(code, message));
        }
    }
    if message.contains("ASTRA_VN_LUAU_SNAPSHOT_UNSERIALIZABLE") {
        return VnError::Diagnostic(Diagnostic::blocking(
            "ASTRA_VN_LUAU_SNAPSHOT_UNSERIALIZABLE",
            message,
        ));
    }
    VnError::Luau(err.to_string())
}

pub(super) fn charged_snapshot(
    value: mlua::Value,
    budget: &SharedBudget,
    overhead: usize,
    old: usize,
    depth_limit: usize,
) -> mlua::Result<PolicySnapshotValue> {
    let bytes = budget
        .borrow()
        .lua_value_bytes(&value, 0)?
        .saturating_add(overhead);
    budget.borrow_mut().replace(old, bytes)?;
    policy_snapshot_from_lua(value, 0, depth_limit)
}

fn policy_snapshot_from_lua(
    value: mlua::Value,
    depth: usize,
    max_depth: usize,
) -> mlua::Result<PolicySnapshotValue> {
    if depth > max_depth {
        return Err(snapshot_error(
            "snapshot nesting exceeds the supported depth",
        ));
    }
    match value {
        mlua::Value::Nil => Ok(PolicySnapshotValue::Nil),
        mlua::Value::Boolean(value) => Ok(PolicySnapshotValue::Bool(value)),
        mlua::Value::Integer(value) => Ok(PolicySnapshotValue::Integer(i64::from(value))),
        mlua::Value::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && value.abs() <= LUAU_MAX_EXACT_INTEGER as f64 =>
        {
            Ok(PolicySnapshotValue::Integer(value as i64))
        }
        mlua::Value::String(value) => Ok(PolicySnapshotValue::String(value.to_str()?.to_string())),
        mlua::Value::Table(table) => {
            let mut values = BTreeMap::new();
            for pair in table.pairs::<mlua::Value, mlua::Value>() {
                let (key, value) = pair?;
                values.insert(
                    policy_snapshot_key_from_lua(key)?,
                    policy_snapshot_from_lua(value, depth + 1, max_depth)?,
                );
            }
            Ok(PolicySnapshotValue::Object(values))
        }
        _ => Err(snapshot_error(
            "snapshot values may only contain nil, bool, integer, string or object values",
        )),
    }
}

fn policy_snapshot_key_from_lua(value: mlua::Value) -> mlua::Result<String> {
    match value {
        mlua::Value::String(value) => Ok(value.to_str()?.to_string()),
        mlua::Value::Integer(value) => Ok(value.to_string()),
        _ => Err(snapshot_error(
            "snapshot object keys may only be string or integer keys",
        )),
    }
}

pub(super) fn policy_snapshot_to_lua(
    lua: &Lua,
    value: &PolicySnapshotValue,
) -> mlua::Result<mlua::Value> {
    match value {
        PolicySnapshotValue::Nil => Ok(mlua::Value::Nil),
        PolicySnapshotValue::Bool(value) => Ok(mlua::Value::Boolean(*value)),
        PolicySnapshotValue::Integer(value) => {
            if value.unsigned_abs() > LUAU_MAX_EXACT_INTEGER {
                return Err(snapshot_error(
                    "integer snapshot is outside Luau exact integer range",
                ));
            }
            Ok(match i32::try_from(*value) {
                Ok(value) => mlua::Value::Integer(value),
                Err(_) => mlua::Value::Number(*value as f64),
            })
        }
        PolicySnapshotValue::String(value) => Ok(mlua::Value::String(lua.create_string(value)?)),
        PolicySnapshotValue::Object(values) => {
            let table = lua.create_table()?;
            for (key, value) in values {
                table.set(key.as_str(), policy_snapshot_to_lua(lua, value)?)?;
            }
            Ok(mlua::Value::Table(table))
        }
    }
}

fn snapshot_error(message: &str) -> mlua::Error {
    mlua::Error::RuntimeError(format!("ASTRA_VN_LUAU_SNAPSHOT_UNSERIALIZABLE: {message}"))
}
