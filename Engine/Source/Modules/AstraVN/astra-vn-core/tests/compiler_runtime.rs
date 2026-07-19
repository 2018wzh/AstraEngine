use astra_vn_core::{
    compile_astra_project, reduce_vn_step, AstraSource, PresentationCommand, ReadingMode,
    StageCommand, SystemPageKind, VnPlayerCommand, VnRunConfig, VnRuntime, VnWaitKind,
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

    assert_eq!(compiled.schema, "astra.vn.compiled_project.v3");
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
fn input_wait_is_distinct_from_dialogue_and_advances_without_backlog() {
    const INPUT_WAIT: &str = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    input_wait #@id wait.input
    text key:line.after #@id line.after
"#;
    let compiled = compile_astra_project(
        [AstraSource::story("input-wait.astra", INPUT_WAIT)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("en")).unwrap();

    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.start".to_string(),
        })
        .unwrap();
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Input)
    );
    assert!(runtime.state().backlog.is_empty());

    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Dialogue)
    );
    assert_eq!(runtime.state().backlog.last().unwrap().key, "line.after");
}

#[astra_headless_test::test]
fn branch_is_typed_in_route_graph_and_selects_from_runtime_variables() {
    const BRANCHING: &str = r#"
story main #@id story.main

state start #@id state.start
  scene start #@id scene.start
    mutate project.day = 2 #@id set.day
    branch path:project.day op:greater_eq value:2 then:reached else:missed #@id branch.day

state reached #@id state.reached
  scene reached #@id scene.reached
    text key:reached #@id line.reached

state missed #@id state.missed
  scene missed #@id scene.missed
    text key:missed #@id line.missed
"#;
    let compiled = compile_astra_project(
        [AstraSource::story("branching.astra", BRANCHING)],
        Default::default(),
    )
    .unwrap();

    assert!(compiled
        .route_graph
        .edges
        .iter()
        .any(|edge| edge.from == "state.start"
            && edge.to == "state.reached"
            && edge.trigger == "branch.day.then"));
    assert!(compiled
        .route_graph
        .edges
        .iter()
        .any(|edge| edge.from == "state.start"
            && edge.to == "state.missed"
            && edge.trigger == "branch.day.else"));
    assert!(compiled
        .variable_manifest
        .scopes
        .get("project")
        .unwrap()
        .keys
        .contains("day"));

    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();
    let output = runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.start".to_string(),
        })
        .unwrap();
    assert!(matches!(
        output.presentation.last(),
        Some(PresentationCommand::Dialogue { key, .. }) if key == "reached"
    ));
    assert!(runtime
        .state()
        .route_flags
        .values()
        .any(|flag| flag.source == "branch.day" && flag.target == "state.reached"));
}

#[astra_headless_test::test]
fn branch_rejects_an_uninitialized_variable() {
    const BRANCHING: &str = r#"
story main #@id story.main

state start #@id state.start
  scene start #@id scene.start
    branch path:project.day op:eq value:0 then:reached else:missed #@id branch.day

state reached #@id state.reached
  scene reached #@id scene.reached
    text key:reached #@id line.reached

state missed #@id state.missed
  scene missed #@id scene.missed
    text key:missed #@id line.missed
"#;
    let compiled = compile_astra_project(
        [AstraSource::story("branching.astra", BRANCHING)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();
    let error = runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.start".to_string(),
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_VN_BRANCH_VARIABLE_MISSING"));
}

#[astra_headless_test::test]
fn choice_availability_is_evaluated_from_authoritative_variables() {
    const GUARDED: &str = r#"
story main #@id story.main

state start #@id state.start
  scene start #@id scene.start
    mutate project.selected = 1 #@id set.selected
    choice key:prompt #@id choice.prompt
      option key:first target:first when:project.selected,not_eq,1 #@id option.first
      option key:second target:second when:project.selected,eq,1 #@id option.second

state first #@id state.first
  scene first #@id scene.first
    text key:first #@id line.first

state second #@id state.second
  scene second #@id scene.second
    text key:second #@id line.second
"#;
    let compiled = compile_astra_project(
        [AstraSource::story("guarded.astra", GUARDED)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.start".to_string(),
        })
        .unwrap();
    let pending = runtime.state().pending_choice.as_ref().unwrap();
    assert!(!pending.enabled_option_ids.contains("option.first"));
    assert!(pending.enabled_option_ids.contains("option.second"));

    let error = runtime
        .apply(VnPlayerCommand::Choose {
            option_id: "option.first".to_string(),
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_VN_CHOICE_OPTION_DISABLED"));
    assert!(runtime.state().pending_choice.is_some());

    let output = runtime
        .apply(VnPlayerCommand::Choose {
            option_id: "option.second".to_string(),
        })
        .unwrap();
    assert!(matches!(
        output.presentation.last(),
        Some(PresentationCommand::Dialogue { key, .. }) if key == "second"
    ));
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
        "astra.vn.system_story_manifest.v2"
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
    let dialogue_wait_id = runtime
        .state()
        .pending_wait
        .as_ref()
        .and_then(|wait| wait.await_id.clone())
        .expect("dialogue wait occurrence id");

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
    let choice_wait_id = runtime
        .state()
        .pending_wait
        .as_ref()
        .and_then(|wait| wait.await_id.clone())
        .expect("choice wait occurrence id");
    assert_ne!(dialogue_wait_id, choice_wait_id);
    assert_eq!(runtime.state().wait_sequence, 2);

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
    assert_eq!(loaded.state().wait_sequence, 2);
    assert_eq!(
        loaded
            .state()
            .pending_wait
            .as_ref()
            .and_then(|wait| wait.await_id.as_deref()),
        Some(choice_wait_id.as_str())
    );
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

#[astra_headless_test::test]
fn system_page_switch_reuses_the_original_return_frame() {
    const SWITCH_SYSTEM: &str = r#"
story system #@id story.system
state title #@id system.title
  scene title #@id scene.system.title
    system_page kind:title #@id page.title
state save #@id system.save
  scene save #@id scene.system.save
    system_page kind:save #@id page.save
"#;
    let compiled = compile_astra_project(
        [
            AstraSource::story("main.astra", MAIN),
            AstraSource::story("system.astra", SWITCH_SYSTEM),
        ],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.prologue".into(),
        })
        .unwrap();
    let original_cursor = runtime.state().cursor.clone();
    let original_wait = runtime.state().pending_wait.clone();
    runtime
        .apply(VnPlayerCommand::OpenSystem {
            page: SystemPageKind::Title,
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::SwitchSystemPage {
            page: SystemPageKind::Save,
        })
        .unwrap();
    assert_eq!(runtime.state().system_stack.len(), 1);
    assert_eq!(runtime.state().system_stack[0].page, SystemPageKind::Save);
    runtime.apply(VnPlayerCommand::ReturnSystem).unwrap();
    assert_eq!(runtime.state().cursor, original_cursor);
    assert_eq!(runtime.state().pending_wait, original_wait);
}

#[astra_headless_test::test]
fn reading_modes_and_audio_bus_state_are_serialized_authority() {
    const FAST_FORWARD: &str = r#"
story main #@id story.main
state start #@id state.start
  scene start #@id scene.start
    text key:first #@id line.first
    input_wait #@id wait.input
    choice key:route #@id choice.route
      option key:route.a -> end #@id option.a
state end #@id state.end
  scene end #@id scene.end
    text key:end #@id line.end
"#;
    let compiled = compile_astra_project(
        [AstraSource::story("reading.astra", FAST_FORWARD)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled.clone(), VnRunConfig::classic("ja")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.start".into(),
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::SetReadingMode {
            mode: ReadingMode::Hidden,
        })
        .unwrap();
    let cursor_before_reveal = runtime.state().cursor.clone();
    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(runtime.state().system.reading_mode, ReadingMode::Manual);
    assert_eq!(runtime.state().cursor, cursor_before_reveal);
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Dialogue)
    );
    let fast_forward = runtime
        .apply(VnPlayerCommand::SetReadingMode {
            mode: ReadingMode::FastForward,
        })
        .unwrap();
    assert!(
        fast_forward.presentation.len() <= 2,
        "skipped reading surfaces must not accumulate into an unbounded provider output"
    );
    assert!(runtime.state().pending_choice.is_some());
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Choice)
    );
    let audio = runtime
        .apply(VnPlayerCommand::SetAudioEnabled { enabled: false })
        .unwrap();
    assert_eq!(
        audio
            .presentation
            .iter()
            .filter(|command| matches!(
                command,
                PresentationCommand::Stage(StageCommand::SetAudioBusEnabled { enabled: false, .. })
            ))
            .count(),
        2
    );
    let expected_hash = runtime.state_hash();
    let save = runtime.save_slot("slot.01").unwrap();
    let mut restored = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();
    restored.load_slot(save).unwrap();
    assert_eq!(restored.state_hash(), expected_hash);
    assert_eq!(
        restored.state().system.reading_mode,
        ReadingMode::FastForward
    );
    assert!(!restored.state().system.audio_enabled);
}

#[astra_headless_test::test]
fn fast_forward_respects_the_authoritative_skip_allowed_stage_state() {
    const PROTECTED: &str = r#"
story main #@id story.main
state start #@id state.start
  scene start #@id scene.start
    skip_allowed allowed:false #@id skip.protect
    text key:protected #@id line.protected
    skip_allowed allowed:true #@id skip.resume
    text key:skippable #@id line.skippable
    choice key:route #@id choice.route
      option key:route.a -> end #@id option.a
state end #@id state.end
  scene end #@id scene.end
    input_wait #@id wait.end
"#;
    let compiled = compile_astra_project(
        [AstraSource::story("protected-reading.astra", PROTECTED)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.start".into(),
        })
        .unwrap();
    assert!(!runtime.state().system.skip_allowed);
    assert_eq!(
        runtime
            .state()
            .pending_wait
            .as_ref()
            .map(|wait| wait.command_id.as_str()),
        Some("line.protected")
    );

    runtime
        .apply(VnPlayerCommand::SetReadingMode {
            mode: ReadingMode::FastForward,
        })
        .unwrap();
    assert_eq!(
        runtime
            .state()
            .pending_wait
            .as_ref()
            .map(|wait| wait.command_id.as_str()),
        Some("line.protected")
    );

    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert!(runtime.state().system.skip_allowed);
    assert!(runtime.state().pending_choice.is_some());
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Choice)
    );
}

#[astra_headless_test::test]
fn system_actions_are_declared_bounded_and_restorable() {
    const ACTION_SYSTEM: &str = r#"
system_action mutations:"project,test_flag,set,1;project,test_count,add,2" #@id action.test.set
story system #@id story.system
state custom #@id system.custom
  scene custom #@id scene.system.custom
    system_page kind:custom #@id page.custom
"#;
    let compiled = compile_astra_project(
        [
            AstraSource::story("main.astra", MAIN),
            AstraSource::story("system.astra", ACTION_SYSTEM),
        ],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled.clone(), VnRunConfig::classic("ja")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.prologue".into(),
        })
        .unwrap();
    assert!(runtime
        .apply(VnPlayerCommand::InvokeSystemAction {
            action_id: "action.undeclared".into(),
        })
        .is_err());
    runtime
        .apply(VnPlayerCommand::InvokeSystemAction {
            action_id: "action.test.set".into(),
        })
        .unwrap();
    assert_eq!(runtime.state().variables["project"]["test_flag"], 1);
    assert_eq!(runtime.state().variables["project"]["test_count"], 2);
    let expected_hash = runtime.state_hash();
    let save = runtime.save_slot("slot.01").unwrap();
    let mut restored = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();
    restored.load_slot(save).unwrap();
    assert_eq!(restored.state_hash(), expected_hash);
}

#[astra_headless_test::test]
fn authored_system_page_return_continues_to_the_next_story_wait() {
    const BOOT: &str = r#"
story boot #@id story.boot

state initial #@id state.initial
  scene initial #@id scene.initial
    system_page kind:title policy:astra.policy.standard #@id page.boot.title
    text key:boot.first #@id line.boot.first
"#;
    const BOOT_SYSTEM: &str = r#"
story system #@id story.system

state title #@id state.system.title
  scene title #@id scene.system.title
    system_page kind:title policy:astra.policy.standard #@id page.title
"#;
    let compiled = compile_astra_project(
        [
            AstraSource::story("boot.astra", BOOT),
            AstraSource::story("system.astra", BOOT_SYSTEM),
        ],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();

    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.boot".to_string(),
            state_id: "state.initial".to_string(),
        })
        .unwrap();
    assert_eq!(runtime.state().system_stack.len(), 1);
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::SystemPage)
    );

    let returned = runtime.apply(VnPlayerCommand::ReturnSystem).unwrap();

    assert_eq!(runtime.state().system_stack.len(), 0);
    assert_eq!(
        runtime.state().pending_wait.as_ref().map(|wait| wait.kind),
        Some(VnWaitKind::Dialogue)
    );
    assert!(matches!(
        returned.presentation.last(),
        Some(PresentationCommand::Dialogue { key, .. }) if key == "boot.first"
    ));
}
