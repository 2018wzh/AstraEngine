use super::*;

/// Native session construction; package identity is optional for embedding.
#[derive(Debug, Clone)]
pub struct VnSessionConfig {
    pub target_id: String,
    pub seed: u64,
    pub package: Option<PackageHandle>,
    pub integrity_mode: TickIntegrityMode,
    pub worker_count: usize,
}

impl VnSession {
    pub fn new(
        compiled: Arc<CoreCompiledStory>,
        config: VnRunConfig,
        options: VnSessionConfig,
    ) -> Result<Self, CoreVnError> {
        let runtime_index = Arc::new(CoreVnRuntimeIndex::build(&compiled)?);
        tracing::info!(
            event = "vn.provider.session.open.start",
            target_id = %options.target_id,
            seed = options.seed,
            "AstraVN runtime session open started"
        );
        let session_id = GameRuntimeSessionId(format!(
            "{}:{}:{}",
            NATIVE_VN_RUNTIME_ID, options.target_id, options.seed
        ));
        let initial_runtime = CoreVnRuntime::new_shared_indexed(
            Arc::clone(&compiled),
            Arc::clone(&runtime_index),
            config,
        )?;
        let mut engine = EngineSession::new(
            RuntimeConfig {
                seed: options.seed,
                required_slots: Vec::new(),
            },
            options.integrity_mode,
            options.package,
            options.worker_count,
        )
        .map_err(|error| CoreVnError::message(error.to_string()))?;
        let world = engine.world_mut();
        let owner = world
            .create_actor("astra.vn.runtime", vec!["gameplay_runtime".to_string()])
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        world
            .attach_component(owner, "astra.vn.policy_state.v1", &VnPolicyState::default())
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        Ok(Self {
            id: session_id,
            engine,
            owner,
            compiled,
            runtime_index,
            runtime: initial_runtime,
            failed: false,
            step_complexity: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story() -> Arc<CoreCompiledStory> {
        Arc::new(compile_astra_project(
            [AstraSource::story("native.astra", "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:hello speaker:narrator #@id hello\n")],
            Default::default(),
        ).unwrap().into())
    }

    fn options() -> VnSessionConfig {
        VnSessionConfig {
            target_id: "embedded".into(),
            seed: 17,
            package: None,
            integrity_mode: TickIntegrityMode::Shipping,
            worker_count: 1,
        }
    }

    #[test]
    fn native_open_shares_story_and_supports_package_free_save_restore_and_shutdown() {
        let compiled = story();
        let mut session =
            VnSession::new(Arc::clone(&compiled), VnRunConfig::classic("en"), options()).unwrap();
        assert!(Arc::ptr_eq(&compiled, &session.compiled));
        assert!(session.engine.world().snapshot().unwrap().package.is_none());
        let step = |fixed_step, command, mode| NativeVnStepInput {
            timing: TickInput {
                fixed_step,
                delta_ns: 16_666_667,
                seed: 17,
            },
            mode,
            command,
        };
        session
            .step(step(
                1,
                NativeVnStepCommand::LaunchDefault,
                astra_runtime::TickMode::Live,
            ))
            .unwrap();
        let before = session.runtime.state().clone();
        let saved = session.save().unwrap();
        session
            .step(step(
                2,
                NativeVnStepCommand::Execute(CoreVnPlayerCommand::Advance),
                astra_runtime::TickMode::Live,
            ))
            .unwrap();
        let restored = session.restore(saved).unwrap();
        assert_eq!(restored.step, 1);
        assert_eq!(session.runtime.state().clone(), before);
        session
            .step(step(
                2,
                NativeVnStepCommand::Execute(CoreVnPlayerCommand::Advance),
                astra_runtime::TickMode::RestoreContinuation,
            ))
            .unwrap();
        let scope = session.engine.world().task_scope();
        session.close();
        assert!(scope.is_cancelled());
    }

    #[test]
    fn rejects_invalid_worker_count() {
        for worker_count in [0, 9] {
            let mut config = options();
            config.worker_count = worker_count;
            assert!(VnSession::new(story(), VnRunConfig::classic("en"), config).is_err());
        }
    }
}
