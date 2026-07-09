use astra_vn_package::{compile_astra_sources, AstraSource, VnAdvancedPresentationManifest};

const ADVANCED_STORY: &str = r#"
story main #@id story.main

state prologue #@id state.prologue
  scene opening #@id scene.opening
    stage viewport:1920x1080 #@id stage.opening
    layer id:bg kind:background z:0 #@id layer.bg
    layer id:characters kind:sprite z:100 #@id layer.characters
    layer id:video_fx kind:video z:200 #@id layer.video_fx
    layer id:ui kind:text z:900 #@id layer.ui
    camera target:main zoom:1.05 #@id camera.push
    timeline id:tl.enter target:hero join:block fence:tl.enter.done fallback:flat budget_ms:2 #@id timeline.enter
    timeline id:tl.enter action:cancel reason:replace_target #@id timeline.cancel
    movie layer:video_fx asset:movie/light.webm fallback:movie/light.png #@id movie.light
    voice asset:voice/hero.ogg sync:text #@id voice.hero
    effect filter:soft_glow fallback:plain_reveal budget_ms:2 #@id effect.reveal
    text key:hello speaker:hero #@id line.hello
"#;

#[test]
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

#[test]
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
