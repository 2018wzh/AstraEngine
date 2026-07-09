use astra_vn_package::{compile_astra_sources, AstraSource, VnCommercialBaselineManifest};

#[test]
fn commercial_baseline_manifest_detects_required_vn_features() {
    let compiled =
        compile_astra_sources([AstraSource::new("baseline.astra", baseline_story())]).unwrap();

    let manifest = VnCommercialBaselineManifest::from_compiled(&compiled);
    let report = manifest.validate_required();

    assert!(report.passed, "{report:?}");
    assert!(manifest.features_present.contains("dialogue"));
    assert!(manifest.features_present.contains("movie_wait"));
}

#[test]
fn commercial_baseline_manifest_blocks_incomplete_fixture() {
    let compiled = compile_astra_sources([AstraSource::new(
        "baseline.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:hello speaker:narrator #@id line.hello
"#,
    )])
    .unwrap();

    let report = VnCommercialBaselineManifest::from_compiled(&compiled).validate_required();

    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_COMMERCIAL_BASELINE_FEATURE"));
}

fn baseline_story() -> &'static str {
    r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    movie layer:video.opening asset:native-assets/movie/op.webm end:wait fallback:native-assets/movie/op_fallback.png #@id movie.opening
    voice asset:native-assets/voice/hero0001.ogg sync:text #@id voice.opening
    text key:hello speaker:narrator voice:voice.hero.0001 #@id line.hello
    choice key:where #@id choice.where
      option key:choice.library -> library #@id choice.library
      option key:choice.rooftop -> rooftop #@id choice.rooftop
state library #@id state.library
  scene library #@id scene.library
    bgm asset:native-assets/bgm/library.ogg loop:true #@id bgm.library
    se asset:native-assets/se/page.ogg #@id se.page
    wait fence:voice.opening.end #@id wait.voice
    jump ending.good #@id jump.good
state rooftop #@id state.rooftop
  scene rooftop #@id scene.rooftop
    text key:rooftop speaker:narrator #@id line.rooftop
    jump ending.rooftop #@id jump.rooftop

story system #@id story.system
state title #@id state.system.title
  scene title #@id scene.system.title
    system_page kind:title policy:astra.policy.standard #@id page.title
"#
}
