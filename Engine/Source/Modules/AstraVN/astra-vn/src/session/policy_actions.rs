use super::*;
use astra_vn_policy::{
    LuauPolicy, PolicyActionBinding, PolicyActionContinuation, PolicyActionOutcome,
    PolicyExecutionBudget, PolicyIoContext, PolicyIoStatus, PolicyQueryContext,
};

type VariableWrites = Vec<(String, String, i64)>;

impl VnSession {
    /// Bind validated, immutable package entries before launching the story.
    pub fn bind_policy_actions(
        &mut self,
        bindings: Vec<PolicyActionBinding>,
    ) -> Result<(), CoreVnError> {
        if self.runtime.state().cursor.is_some() || !self.policy_host.actions.is_empty() {
            return Err(action_error("actions must be bound once before launch"));
        }
        let mut actions = BTreeMap::new();
        for binding in bindings {
            binding.validate()?;
            if actions
                .insert(binding.descriptor.command.clone(), binding)
                .is_some()
            {
                return Err(action_error("duplicate action binding"));
            }
        }
        for state in self.compiled.states.values() {
            for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
                if let CompiledCommand::Presentation {
                    command: PresentationCommand::Extension(extension),
                    ..
                } = command
                {
                    if extension.provider_id == POLICY_ACTION_PROVIDER {
                        actions
                            .get(&extension.command)
                            .ok_or_else(|| action_error("compiled action has no package binding"))?
                            .validate_command(extension)?;
                    }
                }
            }
        }
        self.policy_host.actions = actions;
        Ok(())
    }

    /// True only while gameplay owns the action fence. System-page navigation pauses it.
    pub fn policy_action_pending(&self) -> Result<bool, CoreVnError> {
        let state = self.runtime.state();
        if self.policy_host.actions.is_empty()
            || !state.system_stack.is_empty()
            || !state.pending_wait.as_ref().is_some_and(|wait| {
                wait.kind == VnWaitKind::Fence
                    && wait.fence.strip_prefix("policy.") == Some(wait.command_id.as_str())
            })
        {
            return Ok(false);
        }
        let policy = self.policy_state()?;
        Ok(policy.action.as_ref().is_some_and(|action| {
            self.runtime
                .state()
                .pending_wait
                .as_ref()
                .is_some_and(|wait| wait.command_id == action.source_id)
                && self.runtime.state().system_stack.is_empty()
        }))
    }

    pub(super) fn policy_action_wait_id(&self) -> Result<Option<String>, CoreVnError> {
        if self.policy_host.actions.is_empty() {
            return Ok(None);
        }
        let policy = self.policy_state()?;
        let Some(action) = policy.action else {
            return Ok(None);
        };
        let state = self.runtime.state();
        Ok(state
            .pending_wait
            .iter()
            .chain(
                state
                    .system_stack
                    .iter()
                    .filter_map(|frame| frame.return_wait.as_ref()),
            )
            .find(|wait| wait.command_id == action.source_id)
            .and_then(|wait| wait.await_id.clone()))
    }

    pub(super) fn cancel_abandoned_policy_action(
        &mut self,
        prior_wait: Option<String>,
    ) -> Result<(), CoreVnError> {
        let mut policy = self.policy_state()?;
        let Some(action) = &policy.action else {
            return Ok(());
        };
        let state = self.runtime.state();
        let still_owned = state
            .pending_wait
            .iter()
            .chain(
                state
                    .system_stack
                    .iter()
                    .filter_map(|frame| frame.return_wait.as_ref()),
            )
            .any(|wait| wait.command_id == action.source_id && wait.await_id.is_some());
        if still_owned {
            return Ok(());
        }
        if let Some(id) = prior_wait {
            let token = astra_runtime::AwaitTokenId(
                astra_core::StableId::parse(&id)
                    .map_err(|_| action_error("invalid abandoned action wait"))?,
            );
            self.engine
                .world_mut()
                .cancel_await(token)
                .map_err(|error| CoreVnError::message(error.to_string()))?;
        }
        let prefix = format!("a{}.", action.instance);
        policy
            .io
            .requests
            .retain(|key, _| !key.starts_with(&prefix));
        policy.action = None;
        self.sync_policy_awaits(&mut policy)?;
        self.write_policy(&policy)?;
        self.dispatch_policy_io(&policy)
    }

    pub(super) fn queue_policy_action(
        &mut self,
        command: ExtensionPresentationCommand,
    ) -> Result<(), CoreVnError> {
        self.policy_host
            .actions
            .get(&command.command)
            .ok_or_else(|| action_error("action has no package binding"))?
            .validate_command(&command)?;
        let mut policy = self.policy_state()?;
        if policy.action.is_some() {
            return Err(action_error("another action is still active"));
        }
        policy.action_serial = policy
            .action_serial
            .checked_add(1)
            .ok_or_else(|| action_error("action identity exhausted"))?;
        let source_id = self
            .runtime
            .state()
            .pending_wait
            .as_ref()
            .ok_or_else(|| action_error("action requires a VN wait"))?
            .command_id
            .clone();
        policy.action = Some(PolicyActionContinuation {
            source_id,
            instance: policy.action_serial,
            command,
            waiting_key: None,
            state: BTreeMap::new(),
        });
        self.write_policy(&policy)
    }

    pub(super) fn poll_policy_action(
        &mut self,
    ) -> Result<(Option<CoreVnPlayerCommand>, VariableWrites), CoreVnError> {
        if !self.policy_action_pending()? {
            return Err(action_error(
                "policy tick requires its current action fence",
            ));
        }
        let mut policy = self.policy_state()?;
        let mut action = policy
            .action
            .take()
            .ok_or_else(|| action_error("missing action"))?;
        if let Some(key) = &action.waiting_key {
            let key = action.request_key(key)?;
            let record = policy
                .io
                .requests
                .get(&key)
                .ok_or_else(|| action_error("waiting action request is missing"))?;
            if record.status == PolicyIoStatus::Pending {
                return Ok((None, Vec::new()));
            }
        }
        let binding = self
            .policy_host
            .actions
            .get(&action.command.command)
            .ok_or_else(|| action_error("missing action binding"))?;
        policy.state.variables = self.runtime.state().variables.clone();
        let outcome = LuauPolicy::new()?.invoke_action(
            binding,
            &mut action,
            &mut policy.state,
            &PolicyQueryContext::default(),
            PolicyExecutionBudget::default(),
            PolicyIoContext {
                state: &mut policy.io,
                capabilities: &self.policy_host.capabilities,
            },
        )?;
        let result = match outcome {
            PolicyActionOutcome::Await { .. } => {
                policy.action = Some(action);
                (None, Vec::new())
            }
            PolicyActionOutcome::Complete => {
                let prefix = format!("a{}.", action.instance);
                policy
                    .io
                    .requests
                    .retain(|key, _| !key.starts_with(&prefix));
                for mutation in &policy.state.mutation_trace {
                    if !self
                        .compiled
                        .variable_manifest
                        .scopes
                        .get(&mutation.scope)
                        .is_some_and(|scope| scope.keys.contains(&mutation.key))
                    {
                        return Err(action_error("action writes an undeclared variable"));
                    }
                }
                let writes = policy
                    .state
                    .mutation_trace
                    .iter()
                    .map(|m| (m.scope.clone(), m.key.clone(), m.value))
                    .collect();
                let fence = self
                    .runtime
                    .state()
                    .pending_wait
                    .as_ref()
                    .unwrap()
                    .fence
                    .clone();
                (Some(CoreVnPlayerCommand::CompleteWait { fence }), writes)
            }
        };
        self.sync_policy_awaits(&mut policy)?;
        self.write_policy(&policy)?;
        self.dispatch_policy_io(&policy)?;
        Ok(result)
    }
}

fn action_error(message: &str) -> CoreVnError {
    CoreVnError::diagnostic("ASTRA_VN_POLICY_ACTION", message)
}

/// Validate the candidate against the immutable package and its saved VN fence.
pub(super) fn validate_saved_action(
    policy: &super::policy_io::SessionPolicyState,
    runtime: &VnRuntimeState,
    compiled: &CompiledStory,
    bindings: &BTreeMap<String, PolicyActionBinding>,
) -> Result<(), RuntimeError> {
    let fail = || {
        RuntimeError::message(
            "ASTRA_VN_POLICY_ACTION_RESTORE: continuation does not match the saved story",
        )
    };
    let waits = runtime.pending_wait.iter().chain(
        runtime
            .system_stack
            .iter()
            .filter_map(|frame| frame.return_wait.as_ref()),
    );
    let action_sources: std::collections::BTreeSet<_> = compiled
        .states
        .values()
        .flat_map(|state| &state.scenes)
        .flat_map(|scene| &scene.commands)
        .filter_map(|command| {
            if let CompiledCommand::Presentation {
                id,
                command: PresentationCommand::Extension(extension),
            } = command
            {
                if extension.provider_id == POLICY_ACTION_PROVIDER {
                    return Some(id.as_str());
                }
            }
            None
        })
        .collect();
    let policy_waits: Vec<_> = waits
        .filter(|wait| action_sources.contains(wait.command_id.as_str()))
        .collect();
    let Some(action) = &policy.action else {
        return if policy_waits.is_empty() {
            Ok(())
        } else {
            Err(fail())
        };
    };
    action.validate().map_err(|_| fail())?;
    if action.instance == 0
        || action.instance != policy.action_serial
        || policy_waits.len() != 1
        || policy_waits[0].command_id != action.source_id
        || policy_waits[0].fence != format!("policy.{}", action.source_id)
    {
        return Err(fail());
    }
    let found = compiled.states.values().flat_map(|state| &state.scenes).flat_map(|scene| &scene.commands)
        .any(|command| matches!(command, CompiledCommand::Presentation { id, command: PresentationCommand::Extension(extension) } if id == &action.source_id && extension == &action.command));
    if !found {
        return Err(fail());
    }
    let binding = bindings.get(&action.command.command).ok_or_else(fail)?;
    binding
        .validate_command(&action.command)
        .map_err(|_| fail())?;
    let prefix = format!("a{}.", action.instance);
    for (key, record) in &policy.io.requests {
        if let Some(local) = key.strip_prefix(&prefix) {
            action.request_key(local).map_err(|_| fail())?;
            if !binding.read_text.contains(&record.request.resource) {
                return Err(fail());
            }
        }
    }
    if let Some(key) = &action.waiting_key {
        let key = action.request_key(key).map_err(|_| fail())?;
        if !policy.io.requests.contains_key(&key) {
            return Err(fail());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
