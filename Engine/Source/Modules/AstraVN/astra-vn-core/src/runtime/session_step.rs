use super::*;

impl VnRuntime {
    /// Apply in place and advance revision, leaving host await binding to the caller.
    /// An execution error may leave partial state; the owner must end the session
    /// or restore a validated save before executing another command.
    pub fn apply_deferred(
        &mut self,
        command: VnPlayerCommand,
    ) -> Result<PendingVnStepOutput, VnError> {
        let before = self.state.revision;
        let after = before.checked_add(1).ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_STATE_REVISION_OVERFLOW",
                "VN state revision exhausted its deterministic range",
            )
        })?;
        let pending = self.apply_pending(command, before)?;
        self.state.revision = after;
        Ok(pending)
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
