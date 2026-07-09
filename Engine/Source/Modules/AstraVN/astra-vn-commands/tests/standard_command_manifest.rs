use astra_vn_commands::{compile_astra_sources, AstraSource, VnStandardCommandManifest};

#[test]
fn standard_command_manifest_validates_compiled_presentation_usage() {
    let compiled = compile_astra_sources([AstraSource::new(
        "commands.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    movie layer:video.opening asset:native-assets/movie/op.webm end:wait fallback:native-assets/movie/op_fallback.png #@id movie.opening
    voice asset:native-assets/voice/hero0001.ogg sync:text #@id voice.hero.0001
    text key:opening.line speaker:hero #@id line.opening
"#,
    )])
    .unwrap();

    let report = VnStandardCommandManifest::standard().validate_usage(&compiled);

    assert!(report.passed, "{report:?}");
    assert_eq!(report.checked_usage_count, 2);
}

#[test]
fn standard_command_manifest_blocks_unknown_command_and_missing_movie_fallback() {
    let compiled = compile_astra_sources([AstraSource::new(
        "commands.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    movie layer:video.opening asset:native-assets/movie/op.webm end:wait #@id movie.opening
    warp asset:native-assets/effect/warp.json #@id command.warp
"#,
    )])
    .unwrap();

    let report = VnStandardCommandManifest::standard().validate_usage(&compiled);

    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_STANDARD_COMMAND_FALLBACK"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_STANDARD_COMMAND_UNKNOWN"));
}
