use super::*;
#[path = "../../tests/support/native_package.rs"]
mod native_package;

const STORY: &str = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    stage viewport:320x180 safe_area:16:9 #@id stage.main
    layer id:bg kind:background z:0 blend:normal clip:stage #@id layer.bg
    background asset:asset:/background/apartment-night layer:bg duration:1000 interrupt:replace_from_current #@id background.main
    text key:line.one speaker:hero #@id line.one
"#;

fn source() -> NativeVnHostCommandSource {
    let bytes = native_package::product_package_with_request(STORY, |_| {});
    let package = astra_package::PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    source
}

#[test]
fn frame_failure_keeps_advanced_state_and_requires_successful_restore() {
    let mut source = source();
    source
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    source
        .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 3_723_000)
        .unwrap();
    let save = source.save("slot.01").unwrap();
    let frame = source.stage_director.state().frame_index;
    assert!(source
        .tick_presentation(0)
        .unwrap_err()
        .to_string()
        .contains("TICK_DELTA"));
    assert!(!source.presentation_failed);
    assert_eq!(source.stage_director.state().frame_index, frame);

    let budget = source.texture_cpu_budget_bytes;
    source.textures.clear();
    source.live_texture_ids.clear();
    source.texture_cpu_bytes = 0;
    source.texture_cpu_budget_bytes = 1;
    assert!(source
        .tick_presentation(16_666_667)
        .unwrap_err()
        .to_string()
        .contains("CPU_TEXTURE_ENTRY_BUDGET"));
    assert!(source.stage_director.state().frame_index > frame);
    assert!(source.presentation_failed);
    assert!(source
        .tick_presentation(16_666_667)
        .unwrap_err()
        .to_string()
        .contains("SESSION_FAILED"));
    assert!(source
        .save("slot.01")
        .unwrap_err()
        .to_string()
        .contains("SESSION_FAILED"));
    assert!(source
        .command(VnPlayerCommand::Advance)
        .unwrap_err()
        .to_string()
        .contains("SESSION_FAILED"));
    assert!(source.restore(b"invalid").is_err());
    assert!(source.presentation_failed);
    // A validated save still fails if rebuilding its presentation cannot fit.
    assert!(source.restore(&save).is_err());
    assert!(source.presentation_failed);
    source.texture_cpu_budget_bytes = budget;
    source.restore(&save).unwrap();
    assert!(!source.presentation_failed);
    source.tick_presentation(16_666_667).unwrap();
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
fn only_failure_of_the_awaited_fence_ends_the_player_session() {
    use astra_vn_core::{
        FixedScalar, PresentationInterruptPolicy, StageBlendMode, StageClipPolicy, StageLayerKind,
        VnMovieEndBehavior, VnWaitState,
    };
    let mut source = source();
    source
        .stage_director
        .apply(&StageCommand::DeclareLayer {
            id: "movie".into(),
            kind: StageLayerKind::Video,
            z: 200,
            blend: StageBlendMode::Normal,
            clip: Some(StageClipPolicy::Stage),
            input: None,
        })
        .unwrap();
    source
        .stage_director
        .apply(&StageCommand::Movie {
            layer: "movie".into(),
            asset: "asset:/test.webm".into(),
            alpha: FixedScalar {
                millionths: 1_000_000,
            },
            loop_mode: MovieLoopMode::Once,
            end: VnMovieEndBehavior::Wait,
            fence: Some("movie.done".into()),
            fallback: None,
            interrupt: PresentationInterruptPolicy::Reject,
        })
        .unwrap();
    source.stage_director.fail_video("movie").unwrap();
    source.ensure_presentation_active().unwrap();
    source.runtime_state.as_mut().unwrap().pending_wait = Some(VnWaitState::new(
        VnWaitKind::MovieEnd,
        "movie.done",
        "movie.command",
    ));
    assert!(source
        .tick_presentation(16_666_667)
        .unwrap_err()
        .to_string()
        .contains("FENCE_FAILED"));
    assert!(source.presentation_failed);
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}
