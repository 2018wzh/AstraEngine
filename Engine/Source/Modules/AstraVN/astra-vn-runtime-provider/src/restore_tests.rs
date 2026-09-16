use super::*;

fn fixture() -> (NativeVnRuntimeProvider, GameRuntimeSessionId) {
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
    let session = NativeVnSession {
        id: GameRuntimeSessionId("restore.test".into()),
        seed: RuntimeConfig::default().seed,
        world,
        owner,
        compiled,
        runtime_index,
        runtime,
        failed: false,
        step_complexity: None,
    };
    let id = GameRuntimeSessionId("restore.test".into());
    let mut provider = NativeVnRuntimeProvider::default();
    provider.sessions.insert(id.0.clone(), session);
    (provider, id)
}

fn section(snapshot: RuntimeSnapshot) -> RuntimeSectionPayload {
    let save = astra_runtime::write_runtime_save(snapshot, SaveRequest::default()).unwrap();
    RuntimeSectionPayload {
        section_id: "runtime.world".into(),
        schema: "astra.runtime.save_blob.v5".into(),
        version: SchemaVersion::new(5, 0, 0),
        codec: RuntimeSectionCodec::Raw,
        hash: astra_core::Hash256::from_sha256(&save.0),
        bytes: save.0,
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
        let (mut provider, id) = fixture();
        let session = provider.session(&id).unwrap();
        let before = session.world.save(SaveRequest::default()).unwrap();
        let state_before = session.runtime.state().clone();
        let scope_before = session.world.task_scope();
        let mut snapshot = materialized_save_snapshot(session).unwrap();
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
        let error = provider.restore(RuntimeRestoreRequest {
            session_id: id.clone(),
            sections: vec![section(snapshot)],
        });
        assert!(error.is_err(), "{case}");
        if case == "legacy_machine" {
            assert!(error
                .unwrap_err()
                .to_string()
                .contains("ASTRA_NATIVE_VN_RESTORE_LEGACY_MACHINE"));
        }
        let session = provider.session(&id).unwrap();
        assert_eq!(
            session.world.save(SaveRequest::default()).unwrap().0,
            before.0,
            "{case}"
        );
        assert_eq!(session.runtime.state(), &state_before, "{case}");
        assert_eq!(session.world.task_scope(), scope_before, "{case}");
        assert!(!scope_before.is_cancelled());
    }
}

#[test]
fn restore_checks_outer_integrity_then_commits_and_cancels_old_scope() {
    let (mut provider, id) = fixture();
    let saved = provider
        .save(RuntimeSaveRequest {
            session_id: id.clone(),
            slot: "test".into(),
        })
        .unwrap();
    let old_scope = provider.session(&id).unwrap().world.task_scope();
    assert_eq!(saved.sections[0].version, SchemaVersion::new(5, 0, 0));
    for wrong_version in [true, false] {
        let mut sections = saved.sections.clone();
        if wrong_version {
            sections[0].version = SchemaVersion::new(4, 0, 0);
        } else {
            sections[0].hash = astra_core::Hash256::from_sha256(b"wrong");
        }
        let error = provider
            .restore(RuntimeRestoreRequest {
                session_id: id.clone(),
                sections,
            })
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("ASTRA_NATIVE_VN_RESTORE_INTEGRITY"));
        assert!(!old_scope.is_cancelled());
    }
    provider
        .session_mut(&id)
        .unwrap()
        .runtime
        .apply_deferred(CoreVnPlayerCommand::SetAuto { enabled: true })
        .unwrap();
    provider
        .session_mut(&id)
        .unwrap()
        .world
        .create_actor("discarded", vec![])
        .unwrap();
    let report = provider
        .restore(RuntimeRestoreRequest {
            session_id: id.clone(),
            sections: saved.sections,
        })
        .unwrap();
    assert_eq!(report.restored_fixed_step, 0);
    let session = provider.session_mut(&id).unwrap();
    assert_eq!(session.runtime.state().revision, 0);
    assert!(old_scope.is_cancelled());
    assert!(!session.world.task_scope().is_cancelled());
    assert_eq!(
        session
            .world
            .snapshot()
            .unwrap()
            .actors
            .actor_snapshots()
            .len(),
        1
    );
    assert!(session
        .world
        .snapshot()
        .unwrap()
        .actors
        .component_ids_for_actor_schema(session.owner, &VN_RUNTIME_STATE_SCHEMA.to_string())
        .is_empty());
    session
        .world
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            vec![],
        ))
        .unwrap();
}
