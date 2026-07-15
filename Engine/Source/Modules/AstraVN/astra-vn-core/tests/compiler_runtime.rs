use astra_vn_core::{
    compile_astra_project, reduce_vn_step, AstraSource, PresentationCommand, SystemPageKind,
    VnPlayerCommand, VnRunConfig, VnRuntime, VnWaitKind,
};

const MAIN: &str = r#"
story main #@id story.main

state prologue #@id state.prologue
  scene room #@id scene.room
    text key:prologue.hello speaker:hero voice:voice.hero.0001 #@id line.hello
    mutate project.affinity += 1 reason:"greeted hero" #@id var.affinity
    choice key:prologue.where #@id choice.where
      option key:choice.library -> library #@id choice.library
      option key:choice.rooftop -> rooftop #@id choice.rooftop

state library #@id state.library
  scene library #@id scene.library
    text key:library.followup speaker:hero voice:voice.hero.0002 #@id line.library
    jump ending.good #@id jump.good

state rooftop #@id state.rooftop
  scene rooftop #@id scene.rooftop
    text key:rooftop.line speaker:hero voice:voice.hero.0003 #@id line.rooftop
    jump ending.normal #@id jump.normal
"#;

const SYSTEM: &str = r#"
story system.title #@id system.title
  scene title_menu #@id system.title.menu
    system_page kind:title policy:astra.policy.standard #@id page.title
    option key:system.start -> story.main:state.prologue #@id system.start
"#;

#[astra_headless_test::test]
fn compiles_route_graph_source_map_and_stable_hash() {
    let compiled = compile_astra_project(
        [
            AstraSource::story("main.astra", MAIN),
            AstraSource::story("system.astra", SYSTEM),
        ],
        Default::default(),
    )
    .unwrap();

    assert_eq!(compiled.schema, "astra.vn.story");
    assert_eq!(compiled.stories.len(), 2);
    assert_eq!(compiled.route_graph.nodes.len(), 5);
    assert!(compiled
        .route_graph
        .edges
        .iter()
        .any(|edge| edge.from == "state.prologue" && edge.to == "state.library"));
    assert_eq!(compiled.source_map.get("line.hello").unwrap().line, 6);
    assert_eq!(
        compiled.debug_symbols.get("choice.library").unwrap(),
        "option"
    );
    assert_eq!(compiled.story_hash.to_hex().len(), 32);
}

#[astra_headless_test::test]
fn compiled_story_exposes_story_variable_and_command_manifests() {
    let compiled =
        compile_astra_project([AstraSource::story("main.astra", MAIN)], Default::default())
            .unwrap();

    assert_eq!(compiled.story_manifest.schema, "astra.vn.story_manifest.v1");
    assert!(compiled
        .story_manifest
        .stories
        .iter()
        .any(|story| story.id == "story.main"
            && story.states == ["state.prologue", "state.library", "state.rooftop"]));
    assert!(compiled
        .variable_manifest
        .scopes
        .get("project")
        .unwrap()
        .keys
        .contains("affinity"));
    assert!(compiled
        .command_manifest
        .commands
        .iter()
        .any(|command| command.id == "line.hello"
            && command.kind == "dialogue"
            && command.state_id == "state.prologue"
            && command.scene_id == "scene.room"
            && command.source.as_ref().unwrap().line == 6));
}

#[astra_headless_test::test]
fn compiled_story_exposes_system_story_manifest() {
    let compiled = compile_astra_project(
        [
            AstraSource::story("main.astra", MAIN),
            AstraSource::story("system.astra", SYSTEM),
        ],
        Default::default(),
    )
    .unwrap();

    assert_eq!(
        compiled.system_story_manifest.schema,
        "astra.vn.system_story_manifest.v1"
    );
    let title = compiled
        .system_story_manifest
        .entries
        .get(&SystemPageKind::Title)
        .unwrap();
    assert_eq!(title.story_id, "system.title");
    assert_eq!(title.state_id, "system.title");
    assert_eq!(title.source_id, "page.title");
    assert_eq!(title.policy.as_deref(), Some("astra.policy.standard"));
}

#[astra_headless_test::test]
fn runtime_drives_dialogue_choice_backlog_read_state_and_save_load() {
    let compiled =
        compile_astra_project([AstraSource::story("main.astra", MAIN)], Default::default())
            .unwrap();
    let mut runtime = VnRuntime::new(compiled.clone(), VnRunConfig::classic("zh-Hans")).unwrap();

    let first = runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();
    assert!(matches!(
        first.presentation.first(),
        Some(PresentationCommand::Dialogue { key, speaker, .. })
            if key == "prologue.hello" && speaker.as_deref() == Some("hero")
    ));
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Dialogue)
    );

    let choice = runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert!(matches!(
        choice.presentation.last(),
        Some(PresentationCommand::Choice { key, options })
            if key == "prologue.where" && options.len() == 2
    ));
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Choice)
    );

    let saved_hash = runtime.state_hash();
    let save = runtime.save_slot("slot.auto").unwrap();
    let selected = runtime
        .apply(VnPlayerCommand::Choose {
            option_id: "choice.library".to_string(),
        })
        .unwrap();
    assert!(selected.coverage.reached.contains("state.library"));
    let choice_flag = runtime
        .state()
        .route_flags
        .get("choice:choice.where:choice.library:state.library")
        .unwrap();
    assert_eq!(choice_flag.source, "choice.where:choice.library");
    assert_eq!(choice_flag.target, "state.library");
    assert_eq!(choice_flag.count, 1);
    assert!(runtime
        .state()
        .backlog
        .iter()
        .any(|entry| entry.key == "library.followup"));
    assert!(runtime.state().read_state.contains("line.library"));
    assert!(runtime.state().voice_replay.contains_key("voice.hero.0002"));
    let ending = runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert!(ending.coverage.reached.contains("ending.good"));
    assert!(runtime
        .state()
        .route_flags
        .contains_key("jump:jump.good:ending.good"));

    let mut loaded = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();
    loaded.load_slot(save).unwrap();
    assert_eq!(loaded.state_hash(), saved_hash);
}

#[astra_headless_test::test]
fn reducer_advances_from_an_explicit_runtime_state_without_hidden_session_state() {
    let compiled =
        compile_astra_project([AstraSource::story("main.astra", MAIN)], Default::default())
            .unwrap();
    let mut runtime = VnRuntime::new(compiled.clone(), VnRunConfig::classic("zh-Hans")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();

    let (state, output) =
        reduce_vn_step(&compiled, runtime.state(), VnPlayerCommand::Advance).unwrap();

    assert!(matches!(
        output.presentation.last(),
        Some(PresentationCommand::Choice { .. })
    ));
    assert!(state.pending_choice.is_some());
    assert_eq!(runtime.state().pending_choice, None);
}

#[astra_headless_test::test]
fn system_story_uses_a_separate_stack_and_explicit_return() {
    let compiled = compile_astra_project(
        [
            AstraSource::story("main.astra", MAIN),
            AstraSource::story("system.astra", SYSTEM),
        ],
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
    let return_to = runtime.state().cursor.clone().unwrap();

    let opened = runtime
        .apply(VnPlayerCommand::OpenSystem {
            page: SystemPageKind::Title,
        })
        .unwrap();
    assert_eq!(runtime.state().system_stack.len(), 1);
    assert_eq!(
        runtime.state().cursor.as_ref().unwrap().state_id,
        "system.title"
    );
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::SystemPage)
    );
    assert!(matches!(
        opened.presentation.last(),
        Some(PresentationCommand::SystemPage {
            page: SystemPageKind::Title
        })
    ));

    runtime.apply(VnPlayerCommand::ReturnSystem).unwrap();
    assert_eq!(runtime.state().system_stack.len(), 0);
    assert_eq!(runtime.state().cursor.as_ref(), Some(&return_to));
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Dialogue)
    );
}
