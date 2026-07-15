use astra_vn_package::{compile_astra_sources, AstraSource, VnAdvancedPresentationManifest};

const ADVANCED_STORY: &str = r#"
story main #@id story.main

state prologue #@id state.prologue
  scene opening #@id scene.opening
    stage viewport:1920x1080 safe_area:16:9 #@id stage.opening
    layer id:bg kind:background z:0 blend:normal #@id layer.bg
    layer id:characters kind:sprite z:100 blend:normal #@id layer.characters
    layer id:video_fx kind:video z:200 blend:screen #@id layer.video_fx
    layer id:ui kind:text z:900 blend:normal #@id layer.ui
    camera target:main zoom:1.05 #@id camera.push
    timeline id:tl.enter target:hero property:opacity keyframes:0=0,300=1 join:block fence:tl.enter.done fallback:flat budget_ms:2 #@id timeline.enter
    timeline id:tl.enter action:cancel reason:replace_target #@id timeline.cancel
    movie layer:video_fx asset:asset:/movie/light fallback:asset:/movie/light_fallback #@id movie.light
    voice asset:asset:/voice/hero sync:text #@id voice.hero
    effect text:line.hello filter:astra.filter.bloom fallback:filter_missing budget_ms:2 #@id effect.reveal
    text key:hello speaker:hero #@id line.hello
"#;

#[astra_headless_test::test]
fn advanced_presentation_manifest_requires_real_evidence() {
    let compiled =
        compile_astra_sources([AstraSource::new("advanced.astra", ADVANCED_STORY)]).unwrap();
    let manifest = VnAdvancedPresentationManifest::from_compiled(&compiled, "advanced-vn");
    let report = manifest.validate_required();

    assert!(report.passed);
    for evidence in [
        "stage.multi_layer",
        "camera.task",
        "video.layer",
        "timeline.join_cancel",
        "presentation.fallback",
        "voice.sync",
        "renderer.effect_budget",
    ] {
        assert!(manifest.has_evidence(evidence), "missing {evidence}");
    }
}

#[astra_headless_test::test]
fn advanced_presentation_manifest_blocks_thin_stage() {
    let compiled = compile_astra_sources([AstraSource::new(
        "thin.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene opening #@id scene.opening
    text key:hello #@id line.hello
"#,
    )])
    .unwrap();
    let manifest = VnAdvancedPresentationManifest::from_compiled(&compiled, "advanced-vn");
    let report = manifest.validate_required();

    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_ADVANCED_PRESENTATION_EVIDENCE"));
}
