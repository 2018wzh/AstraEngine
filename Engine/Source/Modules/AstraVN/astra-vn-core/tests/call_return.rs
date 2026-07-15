use astra_vn_core::{compile_astra_project, AstraSource, VnPlayerCommand, VnRunConfig, VnRuntime};

const CALL_RETURN: &str = r#"
story main #@id story.main

state prologue #@id state.prologue
  scene room #@id scene.room
    text key:line.start speaker:narrator #@id line.start
    call common #@id call.common
    text key:line.after speaker:narrator #@id line.after
    jump ending.good #@id jump.good

state common #@id state.common
  scene common #@id scene.common
    text key:line.common speaker:narrator #@id line.common
    return #@id return.common
"#;

#[astra_headless_test::test]
fn runtime_supports_call_return_stack_and_resume_cursor() {
    let compiled = compile_astra_project(
        [AstraSource::story("call_return.astra", CALL_RETURN)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();

    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();
    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(
        runtime
            .state()
            .cursor
            .as_ref()
            .map(|cursor| cursor.state_id.as_str()),
        Some("state.common")
    );
    assert_eq!(runtime.state().call_stack.len(), 1);

    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(
        runtime
            .state()
            .cursor
            .as_ref()
            .map(|cursor| cursor.state_id.as_str()),
        Some("state.prologue")
    );
    assert!(runtime.state().call_stack.is_empty());
    assert_eq!(
        runtime
            .state()
            .backlog
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        ["line.start", "line.common", "line.after"]
    );

    let terminal = runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert!(terminal.coverage.reached.contains("ending.good"));
}
