use super::*;

/// An owned NativeVN session. Dropping it cancels its world-scoped tasks.
pub struct VnSession {
    pub(super) id: GameRuntimeSessionId,
    pub(super) engine: EngineSession,
    pub(super) owner: ActorId,
    pub(super) compiled: Arc<CoreCompiledStory>,
    pub(super) runtime_index: Arc<CoreVnRuntimeIndex>,
    pub(super) runtime: CoreVnRuntime,
    pub(super) failed: bool,
    pub(super) step_complexity: Option<VnStepComplexityMetrics>,
}

impl VnSession {
    pub fn id(&self) -> &GameRuntimeSessionId {
        &self.id
    }

    /// Consume the owned session and cancel all world-scoped work before returning.
    pub fn close(self) {
        drop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(target: &str) -> VnSession {
        let compiled = compile_astra_project(
            [AstraSource::story("owned.astra", "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:hello speaker:narrator #@id hello\n")],
            Default::default(),
        ).unwrap();
        VnSession::new(
            Arc::new(compiled.into()),
            VnRunConfig::classic("en"),
            VnSessionConfig {
                target_id: target.into(),
                seed: 23,
                package: None,
                integrity_mode: TickIntegrityMode::Shipping,
                worker_count: 1,
            },
        )
        .unwrap()
    }

    fn launch(session: &mut VnSession) {
        session
            .step(NativeVnStepInput {
                timing: TickInput {
                    fixed_step: 1,
                    delta_ns: 16_666_667,
                    seed: 23,
                },
                mode: astra_runtime::TickMode::Live,
                command: NativeVnStepCommand::LaunchDefault,
            })
            .unwrap();
    }

    #[test]
    fn invalid_engine_step_does_not_mutate_story_state() {
        for (step, seed, mode) in [
            (2, 23, astra_runtime::TickMode::Live),
            (1, 99, astra_runtime::TickMode::Live),
            (1, 23, astra_runtime::TickMode::RestoreContinuation),
        ] {
            let mut session = session("invalid-step");
            let before = session.runtime.state().clone();
            assert!(session
                .step(NativeVnStepInput {
                    timing: TickInput {
                        fixed_step: step,
                        delta_ns: 16_666_667,
                        seed
                    },
                    mode,
                    command: NativeVnStepCommand::LaunchDefault,
                })
                .is_err());
            assert_eq!(session.runtime.state(), &before);
            assert!(session.engine.world().task_scope().is_cancelled());
        }
    }

    #[test]
    fn closing_one_owned_session_does_not_cancel_or_stop_another() {
        let mut first = session("one");
        let mut second = session("two");
        launch(&mut first);
        let first_scope = first.engine.world().task_scope();
        let second_scope = second.engine.world().task_scope();
        first.close();
        assert!(first_scope.is_cancelled());
        assert!(!second_scope.is_cancelled());
        launch(&mut second);
        assert!(second.runtime.state().pending_wait.is_some());
        drop(second);
        assert!(second_scope.is_cancelled());
    }
    #[test]
    fn ordinary_step_preserves_large_history_allocations() {
        let mut session = session("history");
        launch(&mut session);
        let mut state = session.runtime.state().clone();
        let entry = state.backlog[0].clone();
        state.backlog.resize(4096, entry);
        session.runtime = CoreVnRuntime::from_shared_state_indexed(
            Arc::clone(&session.compiled),
            Arc::clone(&session.runtime_index),
            state,
        )
        .unwrap();
        let history = session.runtime.state().backlog.as_ptr();
        let first_key = session.runtime.state().backlog[0].key.as_ptr();
        for fixed_step in 2..=20 {
            session
                .step(NativeVnStepInput {
                    timing: TickInput {
                        fixed_step,
                        delta_ns: 16_666_667,
                        seed: 23,
                    },
                    mode: astra_runtime::TickMode::Live,
                    command: NativeVnStepCommand::Execute(CoreVnPlayerCommand::SetAuto {
                        enabled: true,
                    }),
                })
                .unwrap();
            assert_eq!(session.runtime.state().backlog.len(), 4096);
            assert_eq!(session.runtime.state().backlog.as_ptr(), history);
            assert_eq!(session.runtime.state().backlog[0].key.as_ptr(), first_key);
        }
    }

    #[test]
    fn failed_execution_blocks_step_and_save_until_successful_restore() {
        let mut session = session("failed");
        launch(&mut session);
        let saved = session.save().unwrap();
        let old_scope = session.engine.world().task_scope();
        let input = |command, mode| NativeVnStepInput {
            timing: TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 23,
            },
            mode,
            command: NativeVnStepCommand::Execute(command),
        };
        assert!(session
            .step(input(
                CoreVnPlayerCommand::ReturnSystem,
                astra_runtime::TickMode::Live
            ))
            .is_err());
        assert!(old_scope.is_cancelled());
        assert!(session
            .save()
            .unwrap_err()
            .to_string()
            .contains("SESSION_FAILED"));
        assert!(session.restore(SaveBlob(vec![0])).is_err());
        assert!(session
            .step(input(
                CoreVnPlayerCommand::SetAuto { enabled: true },
                astra_runtime::TickMode::Live
            ))
            .unwrap_err()
            .to_string()
            .contains("SESSION_FAILED"));
        session.restore(saved).unwrap();
        assert!(!session.engine.world().task_scope().is_cancelled());
        session
            .step(input(
                CoreVnPlayerCommand::SetAuto { enabled: true },
                astra_runtime::TickMode::RestoreContinuation,
            ))
            .unwrap();
        assert!(session.runtime.state().system.auto_enabled);
        assert!(session.save().is_ok());
    }
    #[test]
    fn wait_binding_rejects_stale_and_empty_replacements() {
        let mut session = session("wait");
        launch(&mut session);
        let original = session.runtime.state().pending_wait.clone().unwrap();
        assert!(session
            .runtime
            .bind_pending_wait(&original, String::new())
            .is_err());
        let bound = session
            .runtime
            .bind_pending_wait(&original, "host.await.new".into())
            .unwrap();
        assert!(session
            .runtime
            .bind_pending_wait(&original, "host.await.stale".into())
            .is_err());
        assert_eq!(session.runtime.state().pending_wait.as_ref(), Some(&bound));
    }
    #[test]
    fn direct_wait_survives_save_and_completes_without_a_state_machine() {
        let mut session = session("direct");
        launch(&mut session);
        let snapshot = session.engine.world().snapshot().unwrap();
        assert_eq!(
            snapshot.machines,
            astra_runtime::StateMachineStore::default()
        );
        assert_eq!(snapshot.awaits.pending().len(), 1);
        let token = &snapshot.awaits.pending()[0];
        assert_eq!(token.requested_at_step, 1);
        assert_eq!(
            session
                .runtime
                .state()
                .pending_wait
                .as_ref()
                .unwrap()
                .await_id
                .as_deref(),
            Some(token.token_id.0.to_string().as_str())
        );
        assert!(!snapshot.events.pending().is_empty());
        assert!(snapshot
            .events
            .pending()
            .iter()
            .all(|event| event.source == astra_runtime::EventSource::Runtime && event.step == 1));
        let saved = session.save().unwrap();
        session.restore(saved).unwrap();
        session
            .step(NativeVnStepInput {
                timing: TickInput {
                    fixed_step: 2,
                    delta_ns: 16_666_667,
                    seed: 23,
                },
                mode: astra_runtime::TickMode::RestoreContinuation,
                command: NativeVnStepCommand::Execute(CoreVnPlayerCommand::Advance),
            })
            .unwrap();
        let after = session.engine.world().snapshot().unwrap();
        assert!(after.awaits.pending().is_empty());
        assert_eq!(after.machines, astra_runtime::StateMachineStore::default());
        assert!(session.runtime.state().pending_wait.is_none());
    }
}
