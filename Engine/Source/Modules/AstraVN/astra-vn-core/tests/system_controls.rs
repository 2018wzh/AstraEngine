use astra_vn_core::{
    compile_astra_project, AstraSource, PresentationCommand, SkipMode, SystemUnlockKind,
    VnPlayerCommand, VnRunConfig, VnRuntime,
};

const STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:line.a speaker:narrator voice:voice.narrator.a window:main #@id line.a
    text key:line.b speaker:narrator voice:voice.narrator.b window:main #@id line.b
    choice key:choice.next #@id choice.next
      option key:choice.end -> ending.good #@id choice.end
"#;

const ROUTE_STORY: &str = r#"
story main #@id story.main
state route.one #@id state.route.one
  scene first #@id scene.first
    text key:line.route.one speaker:narrator window:main #@id line.route.one
    jump state.route.two #@id jump.route.two
state route.two #@id state.route.two
  scene second #@id scene.second
    text key:line.route.two speaker:narrator window:main #@id line.route.two
"#;

#[astra_headless_test::test]
fn skip_read_advances_past_read_dialogue_but_stops_at_unread_dialogue() {
    let compiled = compile_astra_project(
        [AstraSource::story("skip.astra", STORY)],
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
    runtime
        .apply(VnPlayerCommand::SetSkip {
            mode: SkipMode::Read,
        })
        .unwrap();
    let line_b = runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();

    assert!(matches!(
        line_b.presentation.first(),
        Some(PresentationCommand::Dialogue { key, .. }) if key == "line.b"
    ));
    assert_eq!(runtime.state().cursor.as_ref().unwrap().ordinal, 2);
}

#[astra_headless_test::test]
fn skip_read_reaches_choice_when_all_prior_dialogue_is_read() {
    let compiled = compile_astra_project(
        [AstraSource::story("skip.astra", STORY)],
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
    runtime
        .apply(VnPlayerCommand::SetSkip {
            mode: SkipMode::Read,
        })
        .unwrap();
    let choice = runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();

    assert!(matches!(
        choice.presentation.first(),
        Some(PresentationCommand::Choice { key, options }) if key == "choice.next" && options.len() == 1
    ));
    assert_eq!(
        runtime.state().pending_choice.as_ref().unwrap().key,
        "choice.next"
    );
}

#[astra_headless_test::test]
fn replay_ui_snapshot_exposes_backlog_read_state_and_voice_entries() {
    let compiled = compile_astra_project(
        [AstraSource::story("replay.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled.clone(), VnRunConfig::classic("zh-Hans")).unwrap();

    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();
    runtime.apply(VnPlayerCommand::Advance).unwrap();

    let replay = runtime.replay_ui_state();
    assert_eq!(replay.schema, "astra.vn.replay_ui_state.v1");
    assert_eq!(replay.backlog.len(), 2);
    assert_eq!(replay.backlog[0].command_id, "line.a");
    assert_eq!(replay.backlog[0].story_id, "story.main");
    assert_eq!(replay.backlog[0].state_id, "state.prologue");
    assert_eq!(replay.backlog[0].route_position, 0);
    assert!(replay.backlog[0].read);
    assert_eq!(replay.backlog[0].layout.window.as_deref(), Some("main"));
    assert_eq!(replay.voice_replay.len(), 2);
    assert_eq!(replay.voice_replay[0].voice, "voice.narrator.a");
    assert_eq!(replay.read_count, 2);
    assert_eq!(replay.unread_count, 0);
    let hash = replay.state_hash();

    let save = runtime.save_slot("slot.replay").unwrap();
    let mut loaded = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();
    loaded.load_slot(save).unwrap();
    assert_eq!(loaded.replay_ui_state().state_hash(), hash);
}

#[astra_headless_test::test]
fn system_controls_persist_auto_skip_config_and_unlocks_through_save_load() {
    let compiled = compile_astra_project(
        [AstraSource::story("system.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled.clone(), VnRunConfig::classic("zh-Hans")).unwrap();

    runtime
        .apply(VnPlayerCommand::SetAuto { enabled: true })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::SetSkip {
            mode: SkipMode::Read,
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::SetConfig {
            key: "text_speed".to_string(),
            value: "instant".to_string(),
        })
        .unwrap();
    assert!(runtime
        .apply(VnPlayerCommand::SetConfig {
            key: "display.language".to_string(),
            value: "../en".to_string(),
        })
        .is_err());
    assert_eq!(runtime.state().locale, "zh-Hans");
    runtime
        .apply(VnPlayerCommand::SetConfig {
            key: "display.language".to_string(),
            value: "en".to_string(),
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::Unlock {
            kind: SystemUnlockKind::Gallery,
            id: "cg.opening".to_string(),
        })
        .unwrap();
    let saved_hash = runtime.state_hash();
    let save = runtime.save_slot("slot.system").unwrap();

    let mut loaded = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();
    loaded.load_slot(save).unwrap();

    assert_eq!(loaded.state_hash(), saved_hash);
    assert!(loaded.state().system.auto_enabled);
    assert_eq!(loaded.state().system.skip_mode, SkipMode::Read);
    assert_eq!(loaded.state().locale, "en");
    assert_eq!(
        loaded
            .state()
            .system
            .config
            .get("text_speed")
            .map(String::as_str),
        Some("instant")
    );
    assert!(loaded.state().system.gallery_unlocks.contains("cg.opening"));
}

#[astra_headless_test::test]
fn product_ui_requests_are_core_validated_and_use_stable_ids() {
    let compiled = compile_astra_project(
        [AstraSource::story("routes.astra", ROUTE_STORY)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("ja")).unwrap();

    runtime
        .apply(VnPlayerCommand::Unlock {
            kind: SystemUnlockKind::Gallery,
            id: "cg.opening".into(),
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::Unlock {
            kind: SystemUnlockKind::Replay,
            id: "state.route.two".into(),
        })
        .unwrap();
    let gallery = runtime
        .apply(VnPlayerCommand::PreviewGallery {
            item_id: "cg.opening".into(),
        })
        .unwrap();
    assert!(matches!(
        gallery.presentation.as_slice(),
        [PresentationCommand::Marker { id }] if id == "gallery.preview.cg.opening"
    ));
    assert!(runtime
        .apply(VnPlayerCommand::PreviewGallery {
            item_id: "cg.locked".into(),
        })
        .is_err());

    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.route.one".into(),
        })
        .unwrap();
    assert!(runtime
        .apply(VnPlayerCommand::JumpRoute {
            node_id: "state.route.two".into(),
        })
        .is_err());
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.route.two".into(),
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.route.one".into(),
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::JumpRoute {
            node_id: "state.route.two".into(),
        })
        .unwrap();
    assert_eq!(
        runtime.state().cursor.as_ref().unwrap().state_id,
        "state.route.two"
    );

    runtime
        .apply(VnPlayerCommand::StartReplay {
            replay_id: "state.route.two".into(),
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::SubmitText {
            input_id: "player_name".into(),
            value: "柘榴".into(),
        })
        .unwrap();
    assert_eq!(
        runtime
            .state()
            .system
            .config
            .get("text_input.player_name")
            .map(String::as_str),
        Some("柘榴")
    );
}

#[astra_headless_test::test]
fn backlog_jump_rejects_missing_entries_and_restores_compiled_command_location() {
    let compiled = compile_astra_project(
        [AstraSource::story("backlog.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.prologue".into(),
        })
        .unwrap();
    assert!(runtime
        .apply(VnPlayerCommand::JumpBacklog {
            command_id: "line.missing".into(),
        })
        .is_err());
    runtime
        .apply(VnPlayerCommand::JumpBacklog {
            command_id: "line.a".into(),
        })
        .unwrap();
    let cursor = runtime.state().cursor.as_ref().unwrap();
    assert_eq!(
        runtime.state().pending_wait.as_ref().unwrap().command_id,
        "line.a"
    );
    assert_eq!(cursor.ordinal, 1);
}
