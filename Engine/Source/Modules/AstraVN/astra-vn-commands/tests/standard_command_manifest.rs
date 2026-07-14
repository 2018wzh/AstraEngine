use astra_vn_commands::{compile_astra_sources, AstraSource, VnStandardCommandManifest};

#[test]
fn standard_command_manifest_validates_compiled_presentation_usage() {
    let compiled = compile_astra_sources([AstraSource::new(
        "commands.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    movie layer:video.opening asset:asset:/movie/op end:wait fence:movie.opening.end fallback:asset:/movie/op_fallback #@id movie.opening
    voice asset:asset:/voice/hero0001 sync:text #@id voice.hero.0001
    text key:opening.line speaker:hero #@id line.opening
"#,
    )])
    .unwrap();

    let report = VnStandardCommandManifest::standard().validate_usage(&compiled);

    assert!(report.passed, "{report:?}");
    assert_eq!(report.checked_usage_count, 2);
}

#[test]
fn compiler_blocks_missing_movie_fallback_before_manifest_generation() {
    let error = compile_astra_sources([AstraSource::new(
        "commands.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    movie layer:video.opening asset:asset:/movie/op end:wait fence:movie.opening.end #@id movie.opening
"#,
    )])
    .unwrap_err();

    assert_eq!(error.code(), "ASTRA_VN_MOVIE_WAIT_CONTRACT");
}

#[test]
fn compiler_blocks_unknown_command_before_manifest_generation() {
    let error = compile_astra_sources([AstraSource::new(
        "commands.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    warp asset:native-assets/effect/warp.json #@id command.warp
"#,
    )])
    .unwrap_err();

    assert_eq!(error.code(), "ASTRA_VN_COMMAND_UNBOUND");
}

#[test]
fn standard_command_manifest_blocks_unknown_audio_control_action() {
    let error = compile_astra_sources([AstraSource::new(
        "commands.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    audio action:rewind target:bgm.main #@id audio.invalid
"#,
    )])
    .unwrap_err();

    assert_eq!(error.code(), "ASTRA_VN_AUDIO_CONTROL_ACTION");
}
