use astra_vn_core::{
    compile_astra_sources, AstraSource, VnPlayerCommand, VnRunConfig, VnRuntime, VnWaitKind,
};

const MOVIE_WAIT_STORY: &str = r#"
story main #@id story.main

state prologue #@id state.prologue
  scene opening #@id scene.opening
    movie layer:video.opening asset:native-assets/movie/op.webm end:wait fallback:native-assets/movie/op_fallback.png #@id movie.opening
    text key:opening.after_movie speaker:narrator #@id line.after_movie
"#;

#[test]
fn movie_end_wait_blocks_cursor_and_resumes_from_serializable_fence() {
    let compiled =
        compile_astra_sources([AstraSource::new("movie_wait.astra", MOVIE_WAIT_STORY)]).unwrap();
    let mut runtime = VnRuntime::new(compiled, VnRunConfig::classic("zh-Hans")).unwrap();

    let output = runtime
        .apply(VnPlayerCommand::Launch {
            story_id: "story.main".to_string(),
            state_id: "state.prologue".to_string(),
        })
        .unwrap();

    assert_eq!(output.presentation.len(), 1);
    let wait = runtime.state().pending_wait.as_ref().unwrap();
    assert_eq!(wait.kind, VnWaitKind::MovieEnd);
    assert_eq!(wait.fence, "movie.opening.end");
    assert_eq!(runtime.state().command_cursor, 1);

    let still_blocked = runtime.apply(VnPlayerCommand::Advance).unwrap();
    assert!(still_blocked.presentation.is_empty());
    assert_eq!(runtime.state().command_cursor, 1);

    let save = runtime.save_slot("wait").unwrap();
    assert_eq!(
        save.state.pending_wait.as_ref().unwrap().fence,
        "movie.opening.end"
    );

    let output = runtime
        .apply(VnPlayerCommand::CompleteWait {
            fence: "movie.opening.end".to_string(),
        })
        .unwrap();

    assert!(runtime.state().pending_wait.is_none());
    assert_eq!(runtime.state().backlog[0].key, "opening.after_movie");
    assert_eq!(output.presentation.len(), 1);
}
