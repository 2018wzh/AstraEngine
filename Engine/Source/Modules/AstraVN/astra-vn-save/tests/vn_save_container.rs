use astra_runtime::{
    write_runtime_save_with_sections, PackageHandle, RuntimeConfig, RuntimeWorld, SaveRequest,
};
use astra_vn_policy::{LuauPolicy, PolicySnapshotValue, VnPolicyState};
use astra_vn_save::{
    compile_astra_project, policy_state_save_section, read_runtime_save_policy_state,
    read_runtime_save_vn_state, runtime_state_save_section, AstraSource, VnPlayerCommand,
    VnRunConfig, VnRuntime,
};

const STORY: &str = r#"
story main #@id story.main

state prologue #@id state.prologue
  scene room #@id scene.room
    text key:prologue.hello speaker:hero voice:voice.hero.0001 #@id line.hello
    choice key:prologue.where #@id choice.where
      option key:choice.library -> library #@id choice.library

state library #@id state.library
  scene library #@id scene.library
    text key:library.followup speaker:hero voice:voice.hero.0002 #@id line.library
"#;

#[astra_headless_test::test]
fn vn_state_roundtrips_inside_runtime_save_container() {
    let compiled = compile_astra_project(
        [AstraSource::story("main.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let mut vn = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();
    vn.apply(VnPlayerCommand::Launch {
        story_id: "story.main".to_string(),
        state_id: "state.prologue".to_string(),
    })
    .unwrap();
    vn.apply(VnPlayerCommand::Advance).unwrap();
    vn.apply(VnPlayerCommand::Choose {
        option_id: "choice.library".to_string(),
    })
    .unwrap();

    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    world.create_actor("vn-host", vec!["nativevn".to_string()]);
    let runtime_hash = world.state_hash();
    let vn_hash = vn.state_hash();
    let mut policy = LuauPolicy::new().unwrap();
    let mut policy_state = VnPolicyState::default();
    assert!(policy
        .eval_bool(
            r#"
            astra.mutate.set_var("project", "affinity", 7)
            astra.snapshot.set("ui.save", { page = "save", slot = 3 })
            return true
            "#,
            &mut policy_state,
        )
        .unwrap());
    let state_payload = postcard::to_allocvec(vn.state()).unwrap();
    postcard::from_bytes::<astra_vn_save::VnRuntimeState>(&state_payload)
        .expect("vn runtime state must be postcard-decodable");
    let vn_section = runtime_state_save_section(vn.state()).unwrap();
    postcard::from_bytes::<astra_vn_save::VnRuntimeStateSave>(&vn_section.payload)
        .expect("vn runtime save section must be directly postcard-decodable");

    let save = write_runtime_save_with_sections(
        world.snapshot(),
        SaveRequest::default(),
        vec![
            vn_section,
            policy_state_save_section(&policy_state).unwrap(),
        ],
    )
    .unwrap();

    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    loaded.load(save.clone()).unwrap();
    assert_eq!(loaded.state_hash(), runtime_hash);

    let loaded_vn = read_runtime_save_vn_state(&save).unwrap();
    assert_eq!(loaded_vn.state_hash, vn_hash);
    assert_eq!(loaded_vn.state.backlog.len(), 2);
    assert!(loaded_vn.state.voice_replay.contains_key("voice.hero.0002"));
    assert!(loaded_vn
        .state
        .route_flags
        .contains_key("choice:choice.where:choice.library:state.library"));
    let loaded_policy = read_runtime_save_policy_state(&save).unwrap();
    assert_eq!(loaded_policy.state.var("project", "affinity"), Some(7));
    assert!(matches!(
        loaded_policy.state.snapshot("ui.save"),
        Some(PolicySnapshotValue::Object(values))
            if values.get("page") == Some(&PolicySnapshotValue::String("save".to_string()))
                && values.get("slot") == Some(&PolicySnapshotValue::Integer(3))
    ));
    assert_eq!(
        loaded_policy.state.mutation_trace[0].replay_event,
        "vn.mutation.set_var"
    );
}
