use astra_vn_core::{
    compile_astra_sources, AstraSource, PresentationCommand, SkipMode, SystemUnlockKind,
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

#[astra_headless_test::test]
fn skip_read_advances_past_read_dialogue_but_stops_at_unread_dialogue() {
    let compiled = compile_astra_sources([AstraSource::new("skip.astra", STORY)]).unwrap();
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
    let compiled = compile_astra_sources([AstraSource::new("skip.astra", STORY)]).unwrap();
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
    let compiled = compile_astra_sources([AstraSource::new("replay.astra", STORY)]).unwrap();
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
    let compiled = compile_astra_sources([AstraSource::new("system.astra", STORY)]).unwrap();
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
