use super::*;

/// An owned NativeVN session. Dropping it cancels its world-scoped tasks.
pub struct NativeVnSession {
    pub(super) id: GameRuntimeSessionId,
    pub(super) seed: u64,
    pub(super) world: RuntimeWorld,
    pub(super) owner: ActorId,
    pub(super) compiled: Arc<CoreCompiledStory>,
    pub(super) runtime_index: Arc<CoreVnRuntimeIndex>,
    pub(super) runtime: CoreVnRuntime,
    pub(super) failed: bool,
    pub(super) pending_control: Arc<Mutex<Option<PreparedVnControl>>>,
    pub(super) control_result: Arc<Mutex<Option<astra_runtime::AwaitTokenId>>>,
    pub(super) step_complexity: Option<VnStepComplexityMetrics>,
}

impl NativeVnSession {
    pub fn id(&self) -> &GameRuntimeSessionId {
        &self.id
    }

    pub(super) fn validate_id(&self, id: &GameRuntimeSessionId) -> Result<(), CoreVnError> {
        if id != &self.id {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_MISMATCH",
                "request belongs to a different NativeVN session",
            ));
        }
        Ok(())
    }

    /// Consume the owned session and cancel all world-scoped work before returning.
    pub fn close(self) {
        drop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(target: &str) -> NativeVnSession {
        let compiled = compile_astra_project(
            [AstraSource::story("owned.astra", "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:hello speaker:narrator #@id hello\n")],
            Default::default(),
        ).unwrap();
        NativeVnSession::new(
            Arc::new(compiled.into()),
            VnRunConfig::classic("en"),
            NativeVnSessionConfig {
                target_id: target.into(),
                seed: 23,
                package: None,
                integrity_mode: TickIntegrityMode::Shipping,
                worker_count: 1,
            },
        )
        .unwrap()
    }

    fn launch(session: &mut NativeVnSession) {
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
    fn foreign_abi_save_and_restore_do_not_modify_owned_session() {
        let mut session = session("one");
        launch(&mut session);
        let state = session.runtime.state().clone();
        let save = session
            .save_abi(RuntimeSaveRequest {
                session_id: session.id().clone(),
                slot: "slot".into(),
            })
            .unwrap();
        let foreign = GameRuntimeSessionId("foreign".into());
        assert!(session
            .save_abi(RuntimeSaveRequest {
                session_id: foreign.clone(),
                slot: "slot".into()
            })
            .unwrap_err()
            .to_string()
            .contains("SESSION_MISMATCH"));
        assert!(session
            .restore_abi(RuntimeRestoreRequest {
                session_id: foreign,
                sections: save.sections.clone()
            })
            .unwrap_err()
            .to_string()
            .contains("SESSION_MISMATCH"));
        assert_eq!(session.runtime.state(), &state);
        assert_eq!(
            session
                .save_abi(RuntimeSaveRequest {
                    session_id: session.id().clone(),
                    slot: "slot".into()
                })
                .unwrap(),
            save
        );
        session.close();
    }

    #[test]
    fn closing_one_owned_session_does_not_cancel_or_stop_another() {
        let mut first = session("one");
        let mut second = session("two");
        launch(&mut first);
        let first_scope = first.world.task_scope();
        let second_scope = second.world.task_scope();
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
        let old_scope = session.world.task_scope();
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
        assert!(!session.world.task_scope().is_cancelled());
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
}
