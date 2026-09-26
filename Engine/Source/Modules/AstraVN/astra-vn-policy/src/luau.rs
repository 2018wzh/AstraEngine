use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use super::luau_budget::{entry_bytes, HostBudget};
use super::luau_value::*;
use astra_policy::{create_sandboxed_lua, PolicyExecutionBudget};
use mlua::Lua;

use crate::{MutationTraceEntry, PolicyQueryContext, PolicySnapshotValue, VnError, VnPolicyState};

// Luau numbers are IEEE-754 doubles; reject the first ambiguous adjacent integer.
pub(crate) const LUAU_MAX_EXACT_INTEGER: u64 = (1_u64 << 53) - 1;

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
        self.eval_bool_with_host(
            source,
            state,
            queries,
            budget,
            crate::PolicyIoContext {
                state: &mut crate::PolicyIoState::default(),
                capabilities: &crate::PolicyIoCapabilities::default(),
            },
        )
    }

    /// Evaluate one explicit continuation. External work is dispatched by the session only
    /// after successful evaluation; completed results are read from serializable state.
    pub fn eval_bool_with_host(
        &mut self,
        source: &str,
        state: &mut VnPolicyState,
        queries: &PolicyQueryContext,
        budget: PolicyExecutionBudget,
        host: crate::PolicyIoContext<'_>,
    ) -> Result<bool, VnError> {
        self.evaluate_with(state, queries, budget, host, |lua, _, _, _| {
            lua.load(source).eval::<bool>()
        })
    }

    pub(crate) fn evaluate_with<T>(
        &mut self,
        state: &mut VnPolicyState,
        queries: &PolicyQueryContext,
        budget: PolicyExecutionBudget,
        host: crate::PolicyIoContext<'_>,
        invoke: impl FnOnce(
            &Lua,
            &mlua::Table,
            &Rc<RefCell<crate::PolicyIoState>>,
            &super::luau_budget::SharedBudget,
        ) -> mlua::Result<T>,
    ) -> Result<T, VnError> {
        host.state.validate()?;
        let host_state = Rc::new(RefCell::new(host.state.clone()));
        let lua = create_sandboxed_lua(budget)
            .map_err(|error| VnError::diagnostic(error.code(), error.to_string()))?;
        let globals = lua.globals();

        let host_budget = HostBudget::new(state, queries, budget).map_err(sandbox_error)?;

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

        let command_register_trace_budget = Rc::clone(&host_budget);
        let command_register_trace = Rc::clone(&command_trace);
        command
            .set(
                "register",
                lua.create_function_mut(
                    move |_, (name, manifest, _handler): (String, mlua::Value, mlua::Value)| {
                        let manifest = charged_snapshot(
                            manifest,
                            &command_register_trace_budget,
                            256 + name.len(),
                            0,
                            budget.snapshot_depth,
                        )?;
                        record_policy_command(
                            &command_register_trace,
                            "astra.command.register",
                            name,
                            manifest,
                        );
                        Ok(())
                    },
                )
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let command_filter_trace_budget = Rc::clone(&host_budget);
        let command_filter_trace = Rc::clone(&command_trace);
        command
            .set(
                "filter",
                lua.create_function_mut(move |_, (name, _handler): (String, mlua::Value)| {
                    command_filter_trace_budget
                        .borrow_mut()
                        .replace(0, 320 + name.len())?;
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

        let command_emit_trace_budget = Rc::clone(&host_budget);
        let command_emit_trace = Rc::clone(&command_trace);
        command
            .set(
                "emit",
                lua.create_function_mut(move |_, (name, payload): (String, mlua::Value)| {
                    let payload = charged_snapshot(
                        payload,
                        &command_emit_trace_budget,
                        256 + name.len(),
                        0,
                        budget.snapshot_depth,
                    )?;
                    record_policy_command(&command_emit_trace, "astra.command.emit", name, payload);
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let command_enqueue_trace_budget = Rc::clone(&host_budget);
        let command_enqueue_trace = Rc::clone(&command_trace);
        command
            .set(
                "enqueue",
                lua.create_function_mut(move |_, (name, payload): (String, mlua::Value)| {
                    let payload = charged_snapshot(
                        payload,
                        &command_enqueue_trace_budget,
                        256 + name.len(),
                        0,
                        budget.snapshot_depth,
                    )?;
                    record_policy_command(
                        &command_enqueue_trace,
                        "astra.command.enqueue",
                        name,
                        payload,
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

        let mutate_vars_budget = Rc::clone(&host_budget);
        let mutate_vars = Rc::clone(&variables);
        let mutate_trace = Rc::clone(&mutation_trace);
        mutate
            .set(
                "set_var",
                lua.create_function_mut(move |_, (scope, key, value): (String, String, i64)| {
                    let variable_bytes = if mutate_vars
                        .borrow()
                        .get(&scope)
                        .is_some_and(|values| values.contains_key(&key))
                    {
                        0
                    } else {
                        128 + scope.len() + key.len()
                    };
                    mutate_vars_budget
                        .borrow_mut()
                        .replace(0, variable_bytes + 256 + scope.len() * 3 + key.len())?;
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

        let query_text_trace_budget = Rc::clone(&host_budget);
        let query_text_trace = Rc::clone(&query_trace);
        let query_text_values = queries.text.clone();
        query
            .set(
                "text",
                lua.create_function(move |lua, (key, locale): (String, String)| {
                    let lookup = format!("{locale}:{key}");
                    let value = query_text_values.get(&lookup).ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "ASTRA_VN_POLICY_QUERY_MISSING: text {lookup}"
                        ))
                    })?;
                    record_policy_query(
                        &query_text_trace,
                        &query_text_trace_budget,
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
                        value,
                    )?;
                    policy_snapshot_to_lua(lua, value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_asset_trace_budget = Rc::clone(&host_budget);
        let query_asset_trace = Rc::clone(&query_trace);
        let query_asset_values = queries.assets.clone();
        query
            .set(
                "asset",
                lua.create_function(move |lua, id: String| {
                    let value = query_asset_values.get(&id).ok_or_else(|| {
                        mlua::Error::runtime(format!("ASTRA_VN_POLICY_QUERY_MISSING: asset {id}"))
                    })?;
                    record_policy_query(
                        &query_asset_trace,
                        &query_asset_trace_budget,
                        "astra.query.asset",
                        id.clone(),
                        [("id".to_string(), PolicySnapshotValue::String(id.clone()))]
                            .into_iter()
                            .collect(),
                        value,
                    )?;
                    policy_snapshot_to_lua(lua, value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_backlog_trace_budget = Rc::clone(&host_budget);
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
                        &query_backlog_trace_budget,
                        "astra.query.backlog",
                        "backlog".to_string(),
                        BTreeMap::new(),
                        &query_backlog_value,
                    )?;
                    policy_snapshot_to_lua(lua, &query_backlog_value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_savepoint_trace_budget = Rc::clone(&host_budget);
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
                        &query_savepoint_trace_budget,
                        "astra.query.savepoint",
                        "savepoint".to_string(),
                        BTreeMap::new(),
                        &query_savepoint_value,
                    )?;
                    policy_snapshot_to_lua(lua, &query_savepoint_value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let query_layout_trace_budget = Rc::clone(&host_budget);
        let query_layout_trace = Rc::clone(&query_trace);
        let query_layout_values = queries.layouts.clone();
        query
            .set(
                "layout",
                lua.create_function(move |lua, target: String| {
                    let value = query_layout_values.get(&target).ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "ASTRA_VN_POLICY_QUERY_MISSING: layout {target}"
                        ))
                    })?;
                    record_policy_query(
                        &query_layout_trace,
                        &query_layout_trace_budget,
                        "astra.query.layout",
                        target.clone(),
                        [(
                            "target".to_string(),
                            PolicySnapshotValue::String(target.clone()),
                        )]
                        .into_iter()
                        .collect(),
                        value,
                    )?;
                    policy_snapshot_to_lua(lua, value)
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        astra.set("query", query).map_err(sandbox_error)?;

        let trace_event_log_budget = Rc::clone(&host_budget);
        let trace_event_log = Rc::clone(&trace_events);
        trace
            .set(
                "event",
                lua.create_function_mut(move |_, (kind, fields): (String, mlua::Value)| {
                    let fields = charged_snapshot(
                        fields,
                        &trace_event_log_budget,
                        256 + kind.len(),
                        0,
                        budget.snapshot_depth,
                    )?;
                    record_policy_trace(&trace_event_log, "astra.trace.event", kind, fields);
                    Ok(())
                })
                .map_err(sandbox_error)?,
            )
            .map_err(sandbox_error)?;

        let trace_scope_log_budget = Rc::clone(&host_budget);
        let trace_scope_log = Rc::clone(&trace_events);
        trace
            .set(
                "performance_scope",
                lua.create_function_mut(move |_, name: String| {
                    trace_scope_log_budget
                        .borrow_mut()
                        .replace(0, 320 + name.len())?;
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

        let snapshot_set_budget = Rc::clone(&host_budget);
        let snapshot_set = Rc::clone(&snapshots);
        snapshot
            .set(
                "set",
                lua.create_function_mut(move |_, (key, value): (String, mlua::Value)| {
                    let old = snapshot_set
                        .borrow()
                        .get(&key)
                        .map_or(0, |value| entry_bytes(&key, value));
                    let value = charged_snapshot(
                        value,
                        &snapshot_set_budget,
                        key.len(),
                        old,
                        budget.snapshot_depth,
                    )?;
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
        super::luau_io::install(
            &lua,
            &astra,
            Rc::clone(&host_state),
            host.capabilities.clone(),
        )
        .map_err(sandbox_error)?;
        globals.set("astra", astra.clone()).map_err(sandbox_error)?;
        let result = invoke(&lua, &astra, &host_state, &host_budget).map_err(sandbox_error)?;
        host_budget.borrow().check().map_err(sandbox_error)?;
        state.variables = variables.borrow().clone();
        state.mutation_trace = mutation_trace.borrow().clone();
        state.command_trace = command_trace.borrow().clone();
        state.query_trace = query_trace.borrow().clone();
        state.trace_events = trace_events.borrow().clone();
        state.snapshots = snapshots.borrow().clone();
        *host.state = host_state.borrow().clone();
        Ok(result)
    }
}
