use super::*;
use astra_vn_policy::{
    LuauPolicy, PolicyExecutionBudget, PolicyIoCapabilities, PolicyIoContext, PolicyIoHost,
    PolicyIoState, PolicyIoStatus, PolicyQueryContext,
};
mod worker;
pub(super) use worker::Completion;
use worker::PolicyIoWorker;

pub(super) const POLICY_STATE_SCHEMA: &str = "astra.vn.session_policy.v2";
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct SessionPolicyState {
    pub state: VnPolicyState,
    pub io: PolicyIoState,
    pub awaits: BTreeMap<u64, astra_runtime::AwaitTokenId>,
    pub action_serial: u64,
    pub action: Option<astra_vn_policy::PolicyActionContinuation>,
}

#[derive(Default)]
pub(super) struct SessionPolicyHost {
    host: Option<Arc<dyn PolicyIoHost>>,
    pub(super) capabilities: PolicyIoCapabilities,
    pub(super) actions: BTreeMap<String, astra_vn_policy::PolicyActionBinding>,
    worker: Option<PolicyIoWorker>,
}
impl SessionPolicyHost {
    pub(super) fn stop(&mut self) {
        self.worker.take();
    }
}
impl VnSession {
    /// Explicit embedding capability grant. Rebinding cancels outstanding work.
    pub fn bind_policy_io(
        &mut self,
        host: Arc<dyn PolicyIoHost>,
        capabilities: PolicyIoCapabilities,
    ) -> Result<(), CoreVnError> {
        self.ensure_policy_active()?;
        if cfg!(target_arch = "wasm32") {
            return Err(CoreVnError::diagnostic(
                "ASTRA_POLICY_HOST_IO_UNSUPPORTED",
                "native policy IO workers are unavailable on Web",
            ));
        }
        self.policy_host.stop();
        let mut policy = self.policy_state()?;
        policy.io.interrupt_pending();
        self.sync_policy_awaits(&mut policy)?;
        self.write_policy(&policy)?;
        self.policy_host.host = Some(host);
        self.policy_host.capabilities = capabilities;
        Ok(())
    }

    /// Run a fresh VM using explicit saved state. No script stack survives this call.
    /// IO is only scheduled after evaluation commits; results enter on a later VN tick.
    pub fn evaluate_policy(
        &mut self,
        source: &str,
        budget: PolicyExecutionBudget,
    ) -> Result<bool, CoreVnError> {
        self.ensure_policy_active()?;
        let mut policy = self.policy_state()?;
        let result = LuauPolicy::new()?.eval_bool_with_host(
            source,
            &mut policy.state,
            &PolicyQueryContext::default(),
            budget,
            PolicyIoContext {
                state: &mut policy.io,
                capabilities: &self.policy_host.capabilities,
            },
        )?;
        self.sync_policy_awaits(&mut policy)?;
        self.write_policy(&policy)?;
        if let Err(error) = self.dispatch_policy_io(&policy) {
            self.failed = true;
            self.engine.world().task_scope().cancel();
            return Err(error);
        }
        Ok(result)
    }

    pub fn policy_snapshot(&self) -> Result<VnPolicyState, CoreVnError> {
        Ok(self.policy_state()?.state)
    }
    pub fn policy_io_snapshot(&self) -> Result<PolicyIoState, CoreVnError> {
        Ok(self.policy_state()?.io)
    }

    fn ensure_policy_active(&self) -> Result<(), CoreVnError> {
        if self.failed || self.engine.world().task_scope().is_cancelled() {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_FAILED",
                "policy requires an active session",
            ));
        }
        Ok(())
    }
    pub(super) fn policy_state(&self) -> Result<SessionPolicyState, CoreVnError> {
        self.engine
            .world()
            .read_component(self.policy_component)
            .map_err(|error| CoreVnError::message(error.to_string()))
    }
    pub(super) fn write_policy(&mut self, policy: &SessionPolicyState) -> Result<(), CoreVnError> {
        self.engine
            .world_mut()
            .replace_component(self.policy_component, policy)
            .map_err(|error| CoreVnError::message(error.to_string()))
    }
    pub(super) fn dispatch_policy_io(
        &mut self,
        policy: &SessionPolicyState,
    ) -> Result<(), CoreVnError> {
        let state = &policy.io;
        if self.policy_host.worker.as_ref().is_some_and(|worker| {
            !state.requests.values().any(|record| {
                record.request.id == worker.id() && record.status == PolicyIoStatus::Pending
            })
        }) {
            self.policy_host.stop();
        }
        if self.policy_host.worker.is_some() {
            return Ok(());
        }
        let Some(record) = state
            .requests
            .values()
            .filter(|record| record.status == PolicyIoStatus::Pending)
            .min_by_key(|record| record.request.id)
        else {
            return Ok(());
        };
        let host = self.policy_host.host.as_ref().ok_or_else(|| {
            CoreVnError::diagnostic("ASTRA_POLICY_HOST_IO_DENIED", "session has no IO host")
        })?;
        let scope = self.engine.world().task_scope().child();
        let handle = self
            .engine
            .world_mut()
            .await_handle(policy.awaits[&record.request.id], &scope)
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        self.policy_host.worker = Some(PolicyIoWorker::start(
            Arc::clone(host),
            record.request.clone(),
            scope,
            handle,
        )?);
        Ok(())
    }
    pub(super) fn sync_policy_awaits(
        &mut self,
        policy: &mut SessionPolicyState,
    ) -> Result<(), CoreVnError> {
        let pending: std::collections::BTreeSet<_> = policy
            .io
            .requests
            .values()
            .filter(|record| record.status == PolicyIoStatus::Pending)
            .map(|record| record.request.id)
            .collect();
        for id in policy.awaits.keys().copied().collect::<Vec<_>>() {
            if !pending.contains(&id) {
                self.engine
                    .world_mut()
                    .cancel_await(policy.awaits.remove(&id).unwrap())
                    .map_err(|error| CoreVnError::message(error.to_string()))?;
            }
        }
        for id in pending {
            if let std::collections::btree_map::Entry::Vacant(entry) = policy.awaits.entry(id) {
                entry.insert(
                    self.engine
                        .world_mut()
                        .create_host_await(astra_runtime::AwaitKind::Custom(
                            "vn.policy.read_text".into(),
                        ))
                        .map_err(|error| CoreVnError::message(error.to_string()))?,
                );
            }
        }
        Ok(())
    }
    pub(super) fn take_policy_completion(&mut self) -> Option<Completion> {
        self.policy_host.worker.as_mut()?.try_complete()
    }
    /// A worker result is committed only after Runtime accepted its scoped AwaitToken.
    pub(super) fn commit_policy_completion(
        &mut self,
        completion: Completion,
    ) -> Result<(), CoreVnError> {
        let mut policy = self.policy_state()?;
        if completion.handle.status() == astra_runtime::TaskStatus::Completed
            && !completion.scope.is_cancelled()
        {
            if let Some(record) = policy.io.requests.values_mut().find(|record| {
                record.request.id == completion.id && record.status == PolicyIoStatus::Pending
            }) {
                record.status = completion.status;
                policy.awaits.remove(&completion.id);
                self.write_policy(&policy)?;
            }
        }
        self.policy_host.stop();
        self.dispatch_policy_io(&policy)
    }
}

/// Validate before replacing the world. A restored pending operation is interrupted,
/// never redispatched, even when the embedding host still grants that resource.
pub(super) fn prepare_policy(
    snapshot: &mut RuntimeSnapshot,
    owner: ActorId,
    component_id: ComponentId,
) -> Result<(), RuntimeError> {
    if snapshot
        .actors
        .component(component_id)
        .is_some_and(|component| component.payload.schema() != POLICY_STATE_SCHEMA)
    {
        return Err(RuntimeError::message(
            "ASTRA_POLICY_RESTORE_VERSION: unsupported session policy state schema",
        ));
    }
    if snapshot
        .actors
        .component_ids_for_actor_schema(owner, &POLICY_STATE_SCHEMA.to_string())
        != [component_id]
    {
        return Err(RuntimeError::message(
            "ASTRA_POLICY_RESTORE_STATE: exactly one session policy state is required",
        ));
    }
    let component = snapshot
        .actors
        .component_mut(component_id)
        .ok_or_else(|| RuntimeError::message("ASTRA_POLICY_RESTORE_STATE: missing policy state"))?;
    if component.payload.version() != SchemaVersion::default() {
        return Err(RuntimeError::message(
            "ASTRA_POLICY_RESTORE_VERSION: unsupported session policy state version",
        ));
    }
    let mut policy: SessionPolicyState = component.payload.decode()?;
    policy
        .io
        .validate()
        .map_err(|error| RuntimeError::message(error.to_string()))?;
    let pending: std::collections::BTreeSet<_> = policy
        .io
        .requests
        .values()
        .filter(|record| record.status == PolicyIoStatus::Pending)
        .map(|record| record.request.id)
        .collect();
    if pending != policy.awaits.keys().copied().collect() {
        return Err(RuntimeError::message(
            "ASTRA_POLICY_RESTORE_AWAIT: policy await set is inconsistent",
        ));
    }
    let tokens = policy.awaits.values().copied().collect::<Vec<_>>();
    let bound_tokens: std::collections::BTreeSet<_> = tokens.iter().copied().collect();
    let saved_tokens: std::collections::BTreeSet<_> = snapshot
        .awaits
        .pending()
        .iter()
        .filter(|token| {
            token.kind == astra_runtime::AwaitKind::Custom("vn.policy.read_text".into())
        })
        .map(|token| token.token_id)
        .collect();
    if bound_tokens.len() != tokens.len() || bound_tokens != saved_tokens {
        return Err(RuntimeError::message(
            "ASTRA_POLICY_RESTORE_AWAIT: policy token set is inconsistent",
        ));
    }

    policy.io.interrupt_pending();
    policy.awaits.clear();
    if !tokens.is_empty() {
        component.payload = component.payload.replaced(policy)?;
    }
    for token in tokens {
        let saved = snapshot
            .awaits
            .pending()
            .iter()
            .find(|pending| pending.token_id == token)
            .ok_or_else(|| {
                RuntimeError::message("ASTRA_POLICY_RESTORE_AWAIT: missing policy await")
            })?;
        if saved.kind != astra_runtime::AwaitKind::Custom("vn.policy.read_text".into()) {
            return Err(RuntimeError::message(
                "ASTRA_POLICY_RESTORE_AWAIT: unexpected policy await kind",
            ));
        }
        snapshot.awaits.cancel(token);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
