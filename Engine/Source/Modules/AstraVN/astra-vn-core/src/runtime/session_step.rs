use super::*;

impl VnRuntime {
    /// Apply in place and advance revision, leaving host await binding to the caller.
    /// An execution error may leave partial state; the owner must end the session
    /// or restore a validated save before executing another command.
    pub fn apply_deferred(
        &mut self,
        command: VnPlayerCommand,
    ) -> Result<PendingVnStepOutput, VnError> {
        self.apply_deferred_with_writes(command, &[])
    }

    /// Host-owned action transaction. Apply its writes before continuing the current fence.
    pub fn apply_deferred_with_writes(
        &mut self,
        command: VnPlayerCommand,
        writes: &[(String, String, i64)],
    ) -> Result<PendingVnStepOutput, VnError> {
        if !writes.is_empty() && !matches!(command, VnPlayerCommand::CompleteWait { .. }) {
            return Err(VnError::diagnostic(
                "ASTRA_VN_ACTION_WRITES_WITHOUT_COMPLETION",
                "action writes require a completed wait",
            ));
        }
        let before = self.state.revision;
        let after = before.checked_add(1).ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_STATE_REVISION_OVERFLOW",
                "VN state revision exhausted its deterministic range",
            )
        })?;
        let pending = self.apply_pending(command, before, writes)?;
        self.state.revision = after;
        Ok(pending)
    }

    /// Advance a host tick without synthesizing input or changing reading mode.
    pub fn poll_deferred(&mut self) -> Result<PendingVnStepOutput, VnError> {
        let before = self.state.revision;
        self.state.revision = before.checked_add(1).ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_STATE_REVISION_OVERFLOW",
                "VN state revision exhausted",
            )
        })?;
        Ok(PendingVnStepOutput(VnStepOutput {
            schema: "astra.vn.step_output.v1".into(),
            next_cursor: self.state.cursor.clone(),
            wait: self.state.pending_wait.clone(),
            awaits: Vec::new(),
            events: Vec::new(),
            presentation: Vec::new(),
            audio: Vec::new(),
            timeline_tasks: Vec::new(),
            mutations: Vec::new(),
            coverage: VnCoverage {
                reached: BTreeSet::new(),
            },
            state_revision_before_advance: before,
            state_revision_after_advance: before,
        }))
    }

    /// Bind only the current wait observed by the host; stale binding is rejected.
    pub fn bind_pending_wait(
        &mut self,
        expected: &VnWaitState,
        await_id: String,
    ) -> Result<VnWaitState, VnError> {
        if await_id.is_empty() || self.state.pending_wait.as_ref() != Some(expected) {
            return Err(VnError::diagnostic(
                "ASTRA_VN_WAIT_BINDING_MISMATCH",
                "host binding does not match the current VN wait",
            ));
        }
        let wait = self
            .state
            .pending_wait
            .as_mut()
            .expect("checked current wait");
        wait.await_id = Some(await_id);
        Ok(wait.clone())
    }
}
