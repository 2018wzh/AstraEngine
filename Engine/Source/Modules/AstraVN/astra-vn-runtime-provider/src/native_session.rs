use super::*;

/// An owned NativeVN session. Dropping it cancels its world-scoped tasks.
pub struct NativeVnSession {
    pub(super) id: GameRuntimeSessionId,
    pub(super) seed: u64,
    pub(super) world: RuntimeWorld,
    pub(super) owner: ActorId,
    pub(super) compiled: Arc<CoreCompiledStory>,
    pub(super) runtime_index: Arc<CoreVnRuntimeIndex>,
    pub(super) state: VnRuntimeState,
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
    pub fn close(self) -> RuntimeShutdownReport {
        let id = self.id.clone();
        drop(self);
        RuntimeShutdownReport {
            session_id: id,
            status: "shutdown".into(),
            diagnostics: vec![],
        }
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
                session_id: session.id().clone(),
                fixed_step: 1,
                delta_ns: 16_666_667,
                session_seed: 23,
                mode: RuntimeStepMode::Live,
                command: NativeVnStepCommand::LaunchDefault,
            })
            .unwrap();
    }

    #[test]
    fn foreign_step_save_and_restore_do_not_modify_owned_session() {
        let mut session = session("one");
        launch(&mut session);
        let state = session.state.clone();
        let save = session
            .save_abi(RuntimeSaveRequest {
                session_id: session.id().clone(),
                slot: "slot".into(),
            })
            .unwrap();
        let foreign = GameRuntimeSessionId("foreign".into());
        assert!(session
            .step(NativeVnStepInput {
                session_id: foreign.clone(),
                fixed_step: 2,
                delta_ns: 16_666_667,
                session_seed: 23,
                mode: RuntimeStepMode::Live,
                command: NativeVnStepCommand::Execute(CoreVnPlayerCommand::Advance),
            })
            .unwrap_err()
            .to_string()
            .contains("SESSION_MISMATCH"));
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
        assert_eq!(session.state, state);
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
        let first_id = first.id().clone();
        let report = first.close();
        assert_eq!(report.session_id, first_id);
        assert!(first_scope.is_cancelled());
        assert!(!second_scope.is_cancelled());
        launch(&mut second);
        assert!(second.state.pending_wait.is_some());
        drop(second);
        assert!(second_scope.is_cancelled());
    }
}
