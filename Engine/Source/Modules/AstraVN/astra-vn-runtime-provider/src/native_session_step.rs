use super::*;

impl NativeVnSession {
    pub(super) fn apply_command_at_step(
        &mut self,
        command: CoreVnPlayerCommand,
        timing: TickInput,
        mode: astra_runtime::TickMode,
    ) -> Result<NativeVnStepOutput, CoreVnError> {
        let fixed_step = timing.fixed_step;
        let session = self;
        let previous_state = session.runtime.state();
        let pending_wait = previous_state.pending_wait.clone();
        let reading_mode = previous_state.system.reading_mode;
        let previous_backlog_count = previous_state.backlog.len();
        let previous_wait = pending_wait.clone();
        let mut pending_output = session.runtime.apply_deferred(command.clone())?;
        let next_state = session.runtime.state();
        if next_state.backlog.len() < previous_backlog_count {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_HISTORY_TRUNCATION",
                "VN execution attempted to truncate append-only backlog history",
            ));
        }
        let next_revision = next_state.revision;
        let create_wait = if next_state.pending_wait != previous_wait {
            next_state.pending_wait.as_ref().and_then(|wait| {
                let has_runtime_await_id = wait
                    .await_id
                    .as_deref()
                    .is_some_and(|await_id| astra_core::StableId::parse(await_id).is_ok());
                (!has_runtime_await_id)
                    .then(|| astra_runtime::AwaitKind::Custom(format!("vn.{:?}", wait.kind)))
            })
        } else {
            None
        };
        if let Some(wait) = next_state.pending_wait.clone() {
            pending_output.set_wait(wait);
        }
        let mut ingress = Vec::new();
        if command_resolves_wait(
            &command,
            pending_wait.as_ref().map(|wait| wait.kind),
            reading_mode,
            &session.compiled,
        ) {
            let await_id = pending_wait
                .as_ref()
                .and_then(|wait| wait.await_id.as_deref())
                .ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_AWAIT_ID_MISSING",
                        "VN wait does not reference its Runtime AwaitToken",
                    )
                })?;
            let token_id = astra_runtime::AwaitTokenId(
                astra_core::StableId::parse(await_id)
                    .map_err(|err| CoreVnError::message(err.to_string()))?,
            );
            let scope = session.world.task_scope();
            let handle = session
                .world
                .await_handle(token_id, &scope)
                .map_err(|error| CoreVnError::message(error.to_string()))?;
            ingress.push(OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::AwaitCompletion(handle.complete(
                    fixed_step,
                    fixed_step,
                    EventPayload::new("await.resolved"),
                )),
            });
        }
        let request = TickRequest {
            timing,
            mode,
            ingress,
        };
        let tick = session
            .world
            .tick(request)
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        if let Some(diagnostic) = tick.diagnostics.first() {
            return Err(CoreVnError::diagnostic(
                diagnostic.code.clone(),
                diagnostic.message.clone(),
            ));
        }
        for event in pending_output.events() {
            session
                .world
                .emit_event(
                    astra_runtime::EventSource::Runtime,
                    EventPayload {
                        kind: event.kind.clone(),
                        data: [("id".into(), BlackboardValue::String(event.id.clone()))]
                            .into_iter()
                            .collect(),
                    },
                )
                .map_err(|error| CoreVnError::message(error.to_string()))?;
        }
        if let Some(kind) = create_wait {
            let token_id = session
                .world
                .create_host_await(kind)
                .map_err(|error| CoreVnError::message(error.to_string()))?;
            let wait = session
                .runtime
                .state()
                .pending_wait
                .clone()
                .ok_or_else(|| {
                    CoreVnError::diagnostic(
                        "ASTRA_NATIVE_VN_AWAIT_STATE_MISSING",
                        "Runtime created an await token without VN wait state",
                    )
                })?;
            let runtime_await_id = token_id.0.to_string();
            let wait = session
                .runtime
                .bind_pending_wait(&wait, runtime_await_id.clone())?;
            pending_output.set_wait(wait);
            pending_output.push_await(runtime_await_id);
        }
        let next_state = session.runtime.state();
        if next_state.pending_wait != previous_wait
            && next_state.pending_wait.as_ref().is_some_and(|wait| {
                wait.await_id
                    .as_deref()
                    .is_none_or(|id| astra_core::StableId::parse(id).is_err())
            })
        {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_AWAIT_ID_MISSING",
                "VN wait was not bound to a Runtime AwaitToken",
            ));
        }
        let appended_backlog_entries = next_state.backlog.len() - previous_backlog_count;
        let output = pending_output.finalize(next_revision);
        let mutation_journal_entries = output.mutations.len();
        session.step_complexity = Some(VnStepComplexityMetrics {
            schema: "astra.vn.step_complexity_metrics.v3".to_string(),
            previous_backlog_count,
            appended_backlog_entries,
            state_cache_hit: true,
            materialized_history_entries: 0,
            history_component_writes: 0,
            encoded_hot_state_bytes: 0,
            mutation_journal_entries,
        });
        let live_vn_state = NativeVnStateView::project(session.runtime.state());
        Ok(NativeVnStepOutput {
            fixed_step,
            vn_state: live_vn_state,
            presentations: output.presentation,
            audio: output.audio,
            timeline: output.timeline_tasks,
            coverage_reached: output.coverage.reached.into_iter().collect(),
        })
    }
}
