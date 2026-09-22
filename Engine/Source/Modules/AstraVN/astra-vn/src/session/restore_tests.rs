use super::*;

fn fixture() -> VnSession {
    let compiled: Arc<CoreCompiledStory> = Arc::new(compile_astra_project(
        [AstraSource::story("restore.astra", "story main #@id story.main\n\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:hello speaker:narrator #@id hello\n")],
        Default::default(),
    ).unwrap().into());
    let runtime_index = Arc::new(CoreVnRuntimeIndex::build(&compiled).unwrap());
    let runtime = CoreVnRuntime::new_shared_indexed(
        Arc::clone(&compiled),
        Arc::clone(&runtime_index),
        VnRunConfig::classic("zh-Hans"),
    )
    .unwrap();
    let mut world = RuntimeWorld::create(RuntimeConfig::default())
        .unwrap()
        .with_package(PackageHandle::default())
        .unwrap();
    let owner = world.create_actor("vn", vec![]).unwrap();
    VnSession {
        id: GameRuntimeSessionId("restore.test".into()),
        engine: EngineSession::from_world(world),
        owner,
        compiled,
        runtime_index,
        runtime,
        failed: false,
        step_complexity: None,
    }
}

#[test]
fn typed_restore_rejection_preserves_world_state_and_scope() {
    for case in [
        "missing",
        "duplicate",
        "version",
        "decode",
        "schema",
        "package",
        "seed",
        "legacy_machine",
    ] {
        let mut session = fixture();
        let before = session.engine.world().save(SaveRequest::default()).unwrap();
        let state_before = session.runtime.state().clone();
        let scope_before = session.engine.world().task_scope();
        let mut snapshot = materialized_save_snapshot(&session).unwrap();
        // This mutation would become visible if the world were committed before validation.
        snapshot.step = 99;
        let component = snapshot
            .actors
            .component_ids_for_actor_schema(session.owner, &VN_RUNTIME_STATE_SCHEMA.to_string())[0];
        match case {
            "missing" => {
                snapshot.actors.detach_component(component);
            }
            "duplicate" => {
                let mut extra = snapshot.actors.component(component).unwrap().clone();
                extra.component_id =
                    ComponentId(astra_core::StableId::deterministic_v7(999, 999, 0));
                assert!(snapshot.actors.attach_component(extra));
            }
            "seed" => {
                snapshot.config.seed = snapshot.config.seed.wrapping_add(1);
            }
            "package" => {
                snapshot.package.as_mut().unwrap().package_id = "other".into();
            }
            "version" => {
                snapshot.actors.component_mut(component).unwrap().payload =
                    RuntimeComponentPayload::typed(
                        VN_RUNTIME_STATE_SCHEMA,
                        SchemaVersion::new(999, 0, 0),
                        state_before.clone(),
                    );
            }
            "decode" => {
                snapshot.actors.component_mut(component).unwrap().payload =
                    RuntimeComponentPayload::typed(
                        VN_RUNTIME_STATE_SCHEMA,
                        SchemaVersion::new(VN_RUNTIME_STATE_SCHEMA_MAJOR, 0, 0),
                        42_u64,
                    );
            }
            "schema" => {
                let mut invalid = state_before.clone();
                invalid.schema = "invalid".into();
                snapshot.actors.component_mut(component).unwrap().payload =
                    RuntimeComponentPayload::typed(
                        VN_RUNTIME_STATE_SCHEMA,
                        SchemaVersion::new(VN_RUNTIME_STATE_SCHEMA_MAJOR, 0, 0),
                        invalid,
                    );
            }
            "legacy_machine" => {
                let running = astra_core::StableId::deterministic_v7(0, 1, 0);
                snapshot
                    .machines
                    .add(astra_runtime::StateMachineDefinition {
                        id: astra_core::StableId::deterministic_v7(0, 2, 0),
                        owner: session.owner,
                        states: vec![astra_runtime::StateDefinition {
                            id: running,
                            name: "vn.running".into(),
                            terminal: false,
                        }],
                        transitions: vec![],
                        initial_state: running,
                    })
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let error = session
            .restore(astra_runtime::write_runtime_save(snapshot, SaveRequest::default()).unwrap());
        assert!(error.is_err(), "{case}");
        if case == "legacy_machine" {
            assert!(error
                .unwrap_err()
                .to_string()
                .contains("ASTRA_NATIVE_VN_RESTORE_LEGACY_MACHINE"));
        }
        assert_eq!(
            session
                .engine
                .world()
                .save(SaveRequest::default())
                .unwrap()
                .0,
            before.0,
            "{case}"
        );
        assert_eq!(session.runtime.state(), &state_before, "{case}");
        assert_eq!(session.engine.world().task_scope(), scope_before, "{case}");
        assert!(!scope_before.is_cancelled());
    }
}

#[test]
fn corrupt_container_preserves_scope_and_valid_restore_replaces_it() {
    let mut session = fixture();
    let saved = session.save().unwrap();
    let old_scope = session.engine.world().task_scope();
    let mut corrupt = saved.clone();
    corrupt.0[0] ^= 1;
    assert!(session.restore(corrupt).is_err());
    assert!(!old_scope.is_cancelled());
    session
        .engine
        .world_mut()
        .create_actor("discarded", vec![])
        .unwrap();
    let report = session.restore(saved).unwrap();
    assert_eq!(report.step, 0);
    assert!(old_scope.is_cancelled());
    assert!(!session.engine.world().task_scope().is_cancelled());
    assert_eq!(
        session
            .engine
            .world()
            .snapshot()
            .unwrap()
            .actors
            .actor_snapshots()
            .len(),
        1
    );
    session.restore(session.save().unwrap()).unwrap();
}
