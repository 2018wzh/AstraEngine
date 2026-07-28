use astra_vn_core::{compile_astra_project, AstraSource, VnPlayerCommand, VnRunConfig, VnRuntime};

const STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:line.a window:main #@id line.a
    text key:line.b window:main #@id line.b
"#;

fn launched_runtime() -> VnRuntime {
    let compiled = compile_astra_project(
        [AstraSource::story("reveal.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("ja-JP")).unwrap();
    runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".into(),
            state_id: "state.prologue".into(),
        })
        .unwrap();
    runtime
}

#[astra_headless_test::test]
fn reveal_uses_fixed_ticks_and_first_advance_only_completes_current_dialogue() {
    let mut runtime = launched_runtime();
    runtime
        .apply(VnPlayerCommand::ConfigureTextReveal {
            command_id: "line.a".into(),
            text_key: "line.a".into(),
            text_graphemes: 9,
            graphemes_per_second: 30,
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::TickTextReveal {
            delta_ns: 100_000_000,
        })
        .unwrap();
    assert_eq!(
        runtime
            .state()
            .text_reveal
            .as_ref()
            .unwrap()
            .visible_graphemes,
        3
    );

    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(
        runtime
            .state()
            .text_reveal
            .as_ref()
            .unwrap()
            .visible_graphemes,
        9
    );
    assert_eq!(
        runtime.state().pending_wait.as_ref().unwrap().command_id,
        "line.a"
    );

    runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert_eq!(
        runtime.state().pending_wait.as_ref().unwrap().command_id,
        "line.b"
    );
    assert!(runtime.state().text_reveal.is_none());
}

#[astra_headless_test::test]
fn reveal_save_load_resumes_the_exact_cursor_and_clock() {
    let mut runtime = launched_runtime();
    runtime
        .apply(VnPlayerCommand::ConfigureTextReveal {
            command_id: "line.a".into(),
            text_key: "line.a".into(),
            text_graphemes: 10,
            graphemes_per_second: 20,
        })
        .unwrap();
    runtime
        .apply(VnPlayerCommand::TickTextReveal {
            delta_ns: 350_000_000,
        })
        .unwrap();
    let saved = runtime.save_slot("slot.reveal").unwrap();
    let expected = runtime.state().text_reveal.clone();

    let mut restored = launched_runtime();
    restored.load_slot(saved).unwrap();
    assert_eq!(restored.state().text_reveal, expected);
    restored
        .apply(VnPlayerCommand::TickTextReveal {
            delta_ns: 150_000_000,
        })
        .unwrap();
    assert_eq!(
        restored
            .state()
            .text_reveal
            .as_ref()
            .unwrap()
            .visible_graphemes,
        10
    );
}
