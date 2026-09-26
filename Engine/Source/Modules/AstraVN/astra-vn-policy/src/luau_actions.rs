use crate::{
    actions::action_error, ExtensionCommandDescriptor, ExtensionFieldContract, ExtensionValue,
    LuauPolicy, PolicyActionBinding, PolicyActionContinuation, PolicyActionOutcome,
    PolicyExecutionBudget, PolicyIoContext, PolicyIoStatus, PolicyQueryContext, VnError,
    VnPolicyState, POLICY_ACTION_PROVIDER,
};
use mlua::{Function, Lua, LuaSerdeExt, Table};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

struct RegisteredAction {
    descriptor: ExtensionCommandDescriptor,
    handler: Function,
}

/// Execute a registration-only entry. The same VM owns both registration and handler.
fn register(lua: &Lua, source: &str) -> mlua::Result<BTreeMap<String, RegisteredAction>> {
    let registered = Rc::new(RefCell::new(BTreeMap::new()));
    let registrations = registered.clone();
    let command = lua.create_table()?;
    command.set(
        "register",
        lua.create_function(
            move |lua, (name, manifest, handler): (String, Table, Function)| {
                let fields: BTreeMap<String, ExtensionFieldContract> =
                    lua.from_value(mlua::Value::Table(manifest.get("fields")?))?;
                let descriptor = ExtensionCommandDescriptor {
                    command: name.clone(),
                    provider_id: POLICY_ACTION_PROVIDER.into(),
                    schema: manifest.get("schema")?,
                    fields,
                };
                descriptor
                    .validate()
                    .map_err(|d| mlua::Error::runtime(d.code))?;
                let mut registrations = registrations.borrow_mut();
                if registrations.len() >= 64 || registrations.contains_key(&name) {
                    return Err(mlua::Error::runtime(
                        "ASTRA_VN_POLICY_ACTION_DUPLICATE_OR_LIMIT",
                    ));
                }
                registrations.insert(
                    name,
                    RegisteredAction {
                        descriptor,
                        handler,
                    },
                );
                Ok(())
            },
        )?,
    )?;
    let astra = lua.create_table()?;
    astra.set("command", command)?;
    lua.globals().set("astra", astra)?;
    lua.load(source).exec()?;
    let mut actions = registered.borrow_mut();
    Ok(std::mem::take(&mut *actions))
}

impl mlua::UserData for PolicyActionOutcome {}

impl LuauPolicy {
    /// Cook uses the same registration path and budget as runtime invocation.
    pub fn action_descriptors(
        source: &str,
        budget: PolicyExecutionBudget,
    ) -> Result<Vec<ExtensionCommandDescriptor>, VnError> {
        if source.len() > crate::POLICY_IO_MAX_BYTES {
            return Err(action_error("action source too large"));
        }
        let lua = astra_policy::create_sandboxed_lua(budget)
            .map_err(|e| VnError::diagnostic(e.code(), e.to_string()))?;
        let actions =
            register(&lua, source).map_err(|_| action_error("action registration failed"))?;
        if actions.is_empty() {
            return Err(action_error("entry registers no actions"));
        }
        Ok(actions.into_values().map(|a| a.descriptor).collect())
    }

    /// Await commits explicit snapshots and IO only. Gameplay writes commit once on Complete.
    pub fn invoke_action(
        &mut self,
        binding: &PolicyActionBinding,
        continuation: &mut PolicyActionContinuation,
        state: &mut VnPolicyState,
        queries: &PolicyQueryContext,
        budget: PolicyExecutionBudget,
        host: PolicyIoContext<'_>,
    ) -> Result<PolicyActionOutcome, VnError> {
        binding.validate_command(&continuation.command)?;
        continuation.request_key("validate")?;
        let mut staged = state.clone();
        staged.snapshots = continuation.state.clone();
        // Action effects are a fresh transaction, never a replay of prior traces.
        staged.mutation_trace.clear();
        staged.command_trace.clear();
        let capabilities = host.capabilities.restrict(&binding.read_text);
        let host = PolicyIoContext {
            state: host.state,
            capabilities: &capabilities,
        };
        let outcome =
            self.evaluate_with(&mut staged, queries, budget, host, |lua, astra, io| {
                let mut registered = register(lua, &binding.source)?;
                let action = registered
                    .remove(&binding.descriptor.command)
                    .ok_or_else(|| mlua::Error::runtime("ASTRA_VN_POLICY_ACTION_MISSING"))?;
                if action.descriptor != binding.descriptor {
                    return Err(mlua::Error::runtime("ASTRA_VN_POLICY_ACTION_DESCRIPTOR"));
                }
                // No trace-only command effects or dynamic registrations inside a handler.
                astra.set("command", mlua::Value::Nil)?;
                let api = lua.create_table()?;
                api.set(
                    "complete",
                    lua.create_function(|_, ()| Ok(PolicyActionOutcome::Complete))?,
                )?;
                api.set(
                    "await",
                    lua.create_function(|_, key: String| {
                        Ok(PolicyActionOutcome::Await { request_key: key })
                    })?,
                )?;
                astra.set("action", api)?;
                let host: Table = astra.get("host")?;
                for name in ["read_text", "cancel", "release"] {
                    let original: Function = host.get(name)?;
                    let continuation = continuation.clone();
                    host.set(
                        name,
                        lua.create_function(move |_, mut args: mlua::MultiValue| {
                            let key = match args.pop_front() {
                                Some(mlua::Value::String(key)) => key.to_str()?.to_owned(),
                                _ => {
                                    return Err(mlua::Error::runtime("ASTRA_VN_POLICY_ACTION_KEY"))
                                }
                            };
                            let key = continuation
                                .request_key(&key)
                                .map_err(mlua::Error::external)?;
                            // Calling the original function preserves the typed host API and bounds.
                            original.call::<mlua::MultiValue>((key, args))
                        })?,
                    )?;
                }
                lua.globals().set("astra", astra.clone())?;
                let args = lua.create_table()?;
                for (key, value) in &continuation.command.fields {
                    let value = match value {
                        ExtensionValue::String(s)
                        | ExtensionValue::Symbol(s)
                        | ExtensionValue::AssetUri(s) => mlua::Value::String(lua.create_string(s)?),
                        ExtensionValue::Integer(i) => {
                            if i.unsigned_abs() > (1_u64 << 53) {
                                return Err(mlua::Error::runtime(
                                    "ASTRA_VN_POLICY_ACTION_INTEGER_RANGE",
                                ));
                            }
                            mlua::Value::Number(*i as f64)
                        }
                        ExtensionValue::Fixed(f) => {
                            mlua::Value::Number(f.millionths as f64 / 1_000_000.0)
                        }
                        ExtensionValue::Boolean(b) => mlua::Value::Boolean(*b),
                    };
                    args.set(key.as_str(), value)?;
                }
                let result: mlua::AnyUserData = action.handler.call(args)?;
                let outcome = result.borrow::<PolicyActionOutcome>()?.clone();
                if let PolicyActionOutcome::Await { request_key } = &outcome {
                    let key = continuation
                        .request_key(request_key)
                        .map_err(mlua::Error::external)?;
                    if !io
                        .borrow()
                        .requests
                        .get(&key)
                        .is_some_and(|record| record.status == PolicyIoStatus::Pending)
                    {
                        return Err(mlua::Error::runtime(
                            "ASTRA_VN_POLICY_ACTION_AWAIT_NOT_PENDING",
                        ));
                    }
                }
                if outcome == PolicyActionOutcome::Complete {
                    let prefix = format!("a{}.", continuation.instance);
                    if io.borrow().requests.iter().any(|(key, record)| {
                        key.starts_with(&prefix) && record.status == PolicyIoStatus::Pending
                    }) {
                        return Err(mlua::Error::runtime(
                            "ASTRA_VN_POLICY_ACTION_PENDING_ON_COMPLETE",
                        ));
                    }
                }
                Ok(outcome)
            })?;
        continuation.state = staged.snapshots.clone();
        continuation.waiting_key = match &outcome {
            PolicyActionOutcome::Complete => None,
            PolicyActionOutcome::Await { request_key } => Some(request_key.clone()),
        };
        if outcome == PolicyActionOutcome::Complete {
            *state = staged;
        }
        Ok(outcome)
    }
}
