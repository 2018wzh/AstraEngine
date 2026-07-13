use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use astra_core::{Diagnostic, Hash128};
use astra_policy::{create_sandboxed_lua, PolicyExecutionBudget};
use mlua::Lua;

use crate::{
    MutationTraceEntry, PolicyCommandTraceEntry, PolicyQueryContext, PolicyQueryTraceEntry,
    PolicySnapshotValue, PolicyTraceEvent, VnError, VnPolicyState,
};

#[derive(Default)]
pub struct LuauPolicy;

impl LuauPolicy {
    pub fn new() -> Result<Self, VnError> {
        tracing::info!(
            event = "vn.policy.runtime.create",
            "AstraVN Luau policy runtime created"
        );
        Ok(Self)
    }

    pub fn eval_bool(&mut self, source: &str, state: &mut VnPolicyState) -> Result<bool, VnError> {
        tracing::trace!(
            event = "vn.policy.eval_bool.start",
            source_byte_size = source.len(),
            "AstraVN policy boolean evaluation started"
        );
        self.eval_bool_with_context(
            source,
            state,
            &PolicyQueryContext::default(),
            PolicyExecutionBudget::default(),
        )
    }

    pub fn eval_bool_with_context(
        &mut self,
        source: &str,
        state: &mut VnPolicyState,
        queries: &PolicyQueryContext,
        budget: PolicyExecutionBudget,
    ) -> Result<bool, VnError> {
        let lua = create_sandboxed_lua(budget)
            .map_err(|error| VnError::diagnostic(error.code(), error.to_string()))?;
        let globals = lua.globals();

        let initial_output_count = state.command_trace.len()
            + state.query_trace.len()
            + state.trace_events.len()
            + state.mutation_trace.len()
            + state.snapshots.len();

        let variables = Rc::new(RefCell::new(state.variables.clone()));
        let mutation_trace = Rc::new(RefCell::new(state.mutation_trace.clone()));
        let command_trace = Rc::new(RefCell::new(state.command_trace.clone()));
        let query_trace = Rc::new(RefCell::new(state.query_trace.clone()));
        let trace_events = Rc::new(RefCell::new(state.trace_events.clone()));
        let snapshots = Rc::new(RefCell::new(state.snapshots.clone()));
        let astra = lua.create_table().map_err(sandbox_error)?;
        let command = lua.create_table().map_err(sandbox_error)?;
        let var = lua.create_table().map_err(sandbox_error)?;
        let mutate = lua.create_table().map_err(sandbox_error)?;
        let query = lua.create_table().map_err(sandbox_error)?;
        let trace = lua.create_table().map_err(sandbox_error)?;
        let snapshot = lua.create_table().map_err(sandbox_error)?;

        let command_register_trace = Rc::clone(&command_trace);
        command
            .set(
                "register",
                lua.create_function_mut(
                    move |_, (name, manifest, _handler): (String, mlua::Value, mlua::Value)| {
                        record_policy_command(
                            &command_register_trace,
                            "astra.command.register",
                            name,
                            policy_snapshot_from_lua(manifest, 0, budget.snapshot_depth)?,
                        );
                        Ok(())
                    },
                )
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let command_filter_trace = Rc::clone(&command_trace);
        command
            .set(
                "filter",
                lua.create_function_mut(move |_, (name, _handler): (String, mlua::Value)| {
                    record_policy_command(
                        &command_filter_trace,
                        "astra.command.filter",
                        name,
                        PolicySnapshotValue::Nil,
                    );
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let command_emit_trace = Rc::clone(&command_trace);
        command
            .set(
                "emit",
                lua.create_function_mut(move |_, (name, payload): (String, mlua::Value)| {
                    record_policy_command(
                        &command_emit_trace,
                        "astra.command.emit",
                        name,
                        policy_snapshot_from_lua(payload, 0, budget.snapshot_depth)?,
                    );
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let command_enqueue_trace = Rc::clone(&command_trace);
        command
            .set(
                "enqueue",
                lua.create_function_mut(move |_, (name, payload): (String, mlua::Value)| {
                    record_policy_command(
                        &command_enqueue_trace,
                        "astra.command.enqueue",
                        name,
                        policy_snapshot_from_lua(payload, 0, budget.snapshot_depth)?,
                    );
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        astra.set("command", command).map_err(sandbox_error)?;

        let get_vars = Rc::clone(&variables);
        var.set(
            "get",
            lua.create_function(move |_, (scope, key): (String, String)| {
                Ok(get_vars
                    .borrow()
                    .get(&scope)
                    .and_then(|scope| scope.get(&key))
                    .copied()
                    .unwrap_or_default())
            })
            .map_err(sandbox_error)?,
        )
        .map_err(sandbox_error)?;

        var.set(
            "set",
            lua.create_function(|_, _: (String, String, i64)| {
                Err::<(), _>(mlua::Error::runtime(
                    "ASTRA_VN_LUAU_AUTHORITY_API: astra.var.set was removed; use astra.mutate.set_var",
                ))
            })
            .map_err(sandbox_error)?,
        )
        .map_err(sandbox_error)?;

        astra.set("var", var).map_err(sandbox_error)?;

        let mutate_vars = Rc::clone(&variables);
        let mutate_trace = Rc::clone(&mutation_trace);
        mutate
            .set(
                "set_var",
                lua.create_function_mut(move |_, (scope, key, value): (String, String, i64)| {
                    let previous_value = mutate_vars
                        .borrow_mut()
                        .entry(scope.clone())
                        .or_default()
                        .insert(key.clone(), value);
                    mutate_trace.borrow_mut().push(MutationTraceEntry {
                        api: "astra.mutate.set_var".to_string(),
                        scope: scope.clone(),
                        key,
                        value,
                        previous_value,
                        dirty_scope: scope.clone(),
                        rollback_scope: scope,
                        replay_event: "vn.mutation.set_var".to_string(),
                    });
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        astra.set("mutate", mutate).map_err(sandbox_error)?;

        let query_text_trace = Rc::clone(&query_trace);
        let query_text_values = queries.text.clone();
        query
            .set(
                "text",
                lua.create_function(move |lua, (key, locale): (String, String)| {
                    let lookup = format!("{locale}:{key}");
                    let value = query_text_values.get(&lookup).cloned().ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "ASTRA_VN_POLICY_QUERY_MISSING: text {lookup}"
                        ))
                    })?;
                    record_policy_query(
                        &query_text_trace,
                        "astra.query.text",
                        key.clone(),
                        [
                            ("key".to_string(), PolicySnapshotValue::String(key.clone())),
                            (
                                "locale".to_string(),
                                PolicySnapshotValue::String(locale.clone()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                        &value,
                    );
                    policy_snapshot_to_lua(lua, &value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_asset_trace = Rc::clone(&query_trace);
        let query_asset_values = queries.assets.clone();
        query
            .set(
                "asset",
                lua.create_function(move |lua, id: String| {
                    let value = query_asset_values.get(&id).cloned().ok_or_else(|| {
                        mlua::Error::runtime(format!("ASTRA_VN_POLICY_QUERY_MISSING: asset {id}"))
                    })?;
                    record_policy_query(
                        &query_asset_trace,
                        "astra.query.asset",
                        id.clone(),
                        [("id".to_string(), PolicySnapshotValue::String(id.clone()))]
                            .into_iter()
                            .collect(),
                        &value,
                    );
                    policy_snapshot_to_lua(lua, &value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_backlog_trace = Rc::clone(&query_trace);
        let query_backlog_value = queries.backlog.clone();
        query
            .set(
                "backlog",
                lua.create_function(move |lua, ()| {
                    if query_backlog_value == PolicySnapshotValue::Nil {
                        return Err(mlua::Error::runtime(
                            "ASTRA_VN_POLICY_QUERY_MISSING: backlog",
                        ));
                    }
                    record_policy_query(
                        &query_backlog_trace,
                        "astra.query.backlog",
                        "backlog".to_string(),
                        BTreeMap::new(),
                        &query_backlog_value,
                    );
                    policy_snapshot_to_lua(lua, &query_backlog_value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_savepoint_trace = Rc::clone(&query_trace);
        let query_savepoint_value = queries.savepoint.clone();
        query
            .set(
                "savepoint",
                lua.create_function(move |lua, ()| {
                    if query_savepoint_value == PolicySnapshotValue::Nil {
                        return Err(mlua::Error::runtime(
                            "ASTRA_VN_POLICY_QUERY_MISSING: savepoint",
                        ));
                    }
                    record_policy_query(
                        &query_savepoint_trace,
                        "astra.query.savepoint",
                        "savepoint".to_string(),
                        BTreeMap::new(),
                        &query_savepoint_value,
                    );
                    policy_snapshot_to_lua(lua, &query_savepoint_value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_layout_trace = Rc::clone(&query_trace);
        let query_layout_values = queries.layouts.clone();
        query
            .set(
                "layout",
                lua.create_function(move |lua, target: String| {
                    let value = query_layout_values.get(&target).cloned().ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "ASTRA_VN_POLICY_QUERY_MISSING: layout {target}"
                        ))
                    })?;
                    record_policy_query(
                        &query_layout_trace,
                        "astra.query.layout",
                        target.clone(),
                        [(
                            "target".to_string(),
                            PolicySnapshotValue::String(target.clone()),
                        )]
                        .into_iter()
                        .collect(),
                        &value,
                    );
                    policy_snapshot_to_lua(lua, &value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        astra.set("query", query).map_err(sandbox_error)?;

        let trace_event_log = Rc::clone(&trace_events);
        trace
            .set(
                "event",
                lua.create_function_mut(move |_, (kind, fields): (String, mlua::Value)| {
                    record_policy_trace(
                        &trace_event_log,
                        "astra.trace.event",
                        kind,
                        policy_snapshot_from_lua(fields, 0, budget.snapshot_depth)?,
                    );
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let trace_scope_log = Rc::clone(&trace_events);
        trace
            .set(
                "performance_scope",
                lua.create_function_mut(move |_, name: String| {
                    record_policy_trace(
                        &trace_scope_log,
                        "astra.trace.performance_scope",
                        name,
                        PolicySnapshotValue::Nil,
                    );
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        astra.set("trace", trace).map_err(sandbox_error)?;

        let snapshot_set = Rc::clone(&snapshots);
        snapshot
            .set(
                "set",
                lua.create_function_mut(move |_, (key, value): (String, mlua::Value)| {
                    let value = policy_snapshot_from_lua(value, 0, budget.snapshot_depth)?;
                    snapshot_set.borrow_mut().insert(key, value);
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let snapshot_get = Rc::clone(&snapshots);
        snapshot
            .set(
                "get",
                lua.create_function(move |lua, key: String| {
                    let Some(value) = snapshot_get.borrow().get(&key).cloned() else {
                        return Ok(mlua::Value::Nil);
                    };
                    policy_snapshot_to_lua(lua, &value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        astra.set("snapshot", snapshot).map_err(sandbox_error)?;
        globals.set("astra", astra).map_err(sandbox_error)?;
        let result = lua.load(source).eval::<bool>().map_err(sandbox_error)?;
        let output_count = command_trace.borrow().len()
            + query_trace.borrow().len()
            + trace_events.borrow().len()
            + mutation_trace.borrow().len()
            + snapshots.borrow().len()
            - initial_output_count;
        if output_count > budget.output_limit {
            return Err(VnError::diagnostic(
                "ASTRA_VN_LUAU_OUTPUT_BUDGET",
                "Luau policy exceeded its recorded output budget",
            ));
        }
        state.variables = variables.borrow().clone();
        state.mutation_trace = mutation_trace.borrow().clone();
        state.command_trace = command_trace.borrow().clone();
        state.query_trace = query_trace.borrow().clone();
        state.trace_events = trace_events.borrow().clone();
        state.snapshots = snapshots.borrow().clone();
        Ok(result)
    }
}

fn record_policy_command(
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

fn record_policy_query(
    query_trace: &Rc<RefCell<Vec<PolicyQueryTraceEntry>>>,
    api: &str,
    target: String,
    args: BTreeMap<String, PolicySnapshotValue>,
    result: &PolicySnapshotValue,
) {
    query_trace.borrow_mut().push(PolicyQueryTraceEntry {
        api: api.to_string(),
        target,
        args,
        result_hash: Hash128::from_blake3(
            &postcard::to_allocvec(result).expect("policy query result must serialize"),
        ),
        replay_event: "vn.policy.query".to_string(),
    });
}

fn record_policy_trace(
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

fn sandbox_error(err: mlua::Error) -> VnError {
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
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64 =>
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

fn policy_snapshot_to_lua(lua: &Lua, value: &PolicySnapshotValue) -> mlua::Result<mlua::Value> {
    match value {
        PolicySnapshotValue::Nil => Ok(mlua::Value::Nil),
        PolicySnapshotValue::Bool(value) => Ok(mlua::Value::Boolean(*value)),
        PolicySnapshotValue::Integer(value) => {
            let value = i32::try_from(*value)
                .map_err(|_| snapshot_error("integer snapshot is outside Luau integer range"))?;
            Ok(mlua::Value::Integer(value))
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
