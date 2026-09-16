use super::*;

/// Native session construction; package identity is optional for embedding.
#[derive(Debug, Clone)]
pub struct NativeVnSessionConfig {
    pub target_id: String,
    pub seed: u64,
    pub package: Option<PackageHandle>,
    pub integrity_mode: TickIntegrityMode,
    pub worker_count: usize,
}

impl NativeVnRuntimeProvider {
    pub fn open_native(
        &mut self,
        compiled: Arc<CoreCompiledStory>,
        config: VnRunConfig,
        options: NativeVnSessionConfig,
    ) -> Result<GameRuntimeSessionId, CoreVnError> {
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
        if self.sessions.contains_key(&session_id.0) {
            return Err(CoreVnError::diagnostic(
                "ASTRA_NATIVE_VN_SESSION_DUPLICATE",
                "runtime session id is already open",
            ));
        }
        let initial_runtime = CoreVnRuntime::new_shared_indexed(
            Arc::clone(&compiled),
            Arc::clone(&runtime_index),
            config,
        )?;
        let mut world = RuntimeWorld::create_with_integrity(
            RuntimeConfig {
                seed: options.seed,
                required_slots: Vec::new(),
            },
            options.integrity_mode,
        )
        .map_err(|err| CoreVnError::message(err.to_string()))?;
        if let Some(package) = options.package {
            world = world
                .with_package(package)
                .map_err(|error| CoreVnError::message(error.to_string()))?;
        }
        world
            .set_machine_worker_count(options.worker_count)
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        let owner = world
            .create_actor("astra.vn.runtime", vec!["gameplay_runtime".to_string()])
            .map_err(|error| CoreVnError::message(error.to_string()))?;
        let initial_state = initial_runtime.state().clone();
        world
            .attach_component(owner, "astra.vn.policy_state.v1", &VnPolicyState::default())
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let pending_control = Arc::new(Mutex::new(None));
        let control_result = Arc::new(Mutex::new(None));
        world
            .register_action(
                NATIVE_VN_PROVIDER_ID,
                VnStepAction {
                    pending_control: Arc::clone(&pending_control),
                    control_result: Arc::clone(&control_result),
                },
            )
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        let running = astra_core::StableId::deterministic_v7(0, 1, options.seed);
        world
            .add_state_machine(StateMachineDefinition {
                id: astra_core::StableId::deterministic_v7(0, 2, options.seed),
                owner,
                states: vec![StateDefinition {
                    id: running,
                    name: "vn.running".to_string(),
                    terminal: false,
                }],
                transitions: vec![TransitionDefinition {
                    from: running,
                    to: running,
                    guard: GuardExpr::Or {
                        terms: vn_runtime_event_kinds()
                            .into_iter()
                            .map(|kind| GuardExpr::EventIs {
                                kind: kind.to_string(),
                            })
                            .collect(),
                    },
                    actions: vec![ActionInvocation {
                        action_id: "astra.vn.step".to_string(),
                        input: BTreeMap::new(),
                    }],
                    priority: 0,
                    source_ref: None,
                }],
                initial_state: running,
            })
            .map_err(|err| CoreVnError::message(err.to_string()))?;
        self.sessions.insert(
            session_id.0.clone(),
            NativeVnSession {
                world,
                owner,
                compiled,
                runtime_index,
                state: initial_state,
                pending_control,
                control_result,
                step_complexity: None,
            },
        );
        Ok(session_id)
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

    fn options() -> NativeVnSessionConfig {
        NativeVnSessionConfig {
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
        let mut provider = NativeVnRuntimeProvider::default();
        let session_id = provider
            .open_native(Arc::clone(&compiled), VnRunConfig::classic("en"), options())
            .unwrap();
        assert!(Arc::ptr_eq(
            &compiled,
            &provider.session(&session_id).unwrap().compiled
        ));
        assert!(provider
            .runtime_snapshot(&session_id)
            .unwrap()
            .package
            .is_none());
        let step = |fixed_step, command, mode| NativeVnStepInput {
            session_id: session_id.clone(),
            fixed_step,
            delta_ns: 16_666_667,
            session_seed: 17,
            mode,
            command,
        };
        provider
            .step_native(step(
                1,
                NativeVnStepCommand::LaunchDefault,
                RuntimeStepMode::Live,
            ))
            .unwrap();
        let before = provider.state(&session_id).unwrap();
        let saved = provider
            .save(RuntimeSaveRequest {
                session_id: session_id.clone(),
                slot: "slot".into(),
            })
            .unwrap();
        provider
            .step_native(step(
                2,
                NativeVnStepCommand::Execute(CoreVnPlayerCommand::Advance),
                RuntimeStepMode::Live,
            ))
            .unwrap();
        let restored = provider
            .restore(RuntimeRestoreRequest {
                session_id: session_id.clone(),
                sections: saved.sections,
            })
            .unwrap();
        assert_eq!(restored.restored_fixed_step, 1);
        assert_eq!(provider.state(&session_id).unwrap(), before);
        provider
            .step_native(step(
                2,
                NativeVnStepCommand::Execute(CoreVnPlayerCommand::Advance),
                RuntimeStepMode::RestoreContinuation,
            ))
            .unwrap();
        let scope = provider.session(&session_id).unwrap().world.task_scope();
        provider.shutdown(session_id).unwrap();
        assert_eq!(provider.session_count(), 0);
        assert!(scope.is_cancelled());
    }

    #[test]
    fn rejected_native_open_does_not_publish_or_replace_a_session() {
        let compiled = story();
        let mut provider = NativeVnRuntimeProvider::default();
        for worker_count in [0, 9] {
            let mut config = options();
            config.worker_count = worker_count;
            assert!(provider
                .open_native(Arc::clone(&compiled), VnRunConfig::classic("en"), config)
                .is_err());
            assert_eq!(provider.session_count(), 0);
        }
        let id = provider
            .open_native(Arc::clone(&compiled), VnRunConfig::classic("en"), options())
            .unwrap();
        assert!(provider
            .open_native(story(), VnRunConfig::classic("en"), options())
            .unwrap_err()
            .to_string()
            .contains("SESSION_DUPLICATE"));
        assert_eq!(provider.session_count(), 1);
        assert!(Arc::ptr_eq(
            &compiled,
            &provider.session(&id).unwrap().compiled
        ));
        provider.shutdown(id).unwrap();
    }
}
