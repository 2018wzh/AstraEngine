use super::*;

impl RuntimeWorld {
    pub(super) fn submit_await_result(&mut self, completion: AwaitCompletion) {
        if !self.tasks.accepts(&completion) {
            self.awaits.reject_stale(completion.result.token_id);
            return;
        }
        let result = completion.result;
        debug!(
            token_id = ?result.token_id,
            sequence = result.sequence,
            completed_at_step = result.completed_at_step,
            kind = %result.payload.kind,
            "runtime.await.submit_result"
        );
        self.awaits.submit_result(result);
    }

    pub fn task_scope(&self) -> TaskScope {
        self.tasks.scope()
    }

    pub fn await_handle(
        &mut self,
        token_id: crate::AwaitTokenId,
        scope: &TaskScope,
    ) -> Result<AwaitCompletionHandle, RuntimeError> {
        self.ensure_active()?;
        let token = self
            .awaits
            .pending()
            .iter()
            .find(|token| token.token_id == token_id)
            .ok_or_else(|| {
                RuntimeError::message("ASTRA_AWAIT_TOKEN_MISSING: token is not pending")
            })?;
        if token.completion_policy != crate::AwaitCompletionPolicy::HostResult {
            return Err(RuntimeError::message(
                "ASTRA_AWAIT_RESULT_POLICY: timeout tokens do not accept worker results",
            ));
        }
        if self.awaits.has_result(token_id) {
            return Err(RuntimeError::message("ASTRA_AWAIT_RESULT_PENDING: token already has a committed result waiting for its tick"));
        }
        self.tasks.handle(token_id, scope)
    }

    pub fn cancel_await(&mut self, token_id: crate::AwaitTokenId) -> Result<bool, RuntimeError> {
        self.ensure_active()?;
        self.tasks.finish(token_id, true);
        let Some(token) = self.awaits.cancel(token_id) else {
            return Ok(false);
        };
        let mut payload = EventPayload::new("await.cancelled");
        payload.data.insert(
            "token_id".into(),
            BlackboardValue::StableId(token.token_id.0),
        );
        self.emit_event(EventSource::AwaitResult, payload)?;
        Ok(true)
    }
}
