use astra_vn_presentation::{
    AspectRatio, FixedScalar, ProductStageDirector, StageBlendMode, StageClipPolicy, StageCommand,
    StageLayerKind, StagePlacement, StageViewport, TimelineCommand, TimelineSpec,
    VnPresentationProviderManifest, VnTimelineJoinPolicy, VnTimelineKeyframe, VnTimelineTrack,
};

fn fixed(value: i64) -> FixedScalar {
    FixedScalar { millionths: value }
}

fn director() -> ProductStageDirector {
    ProductStageDirector::new(
        VnPresentationProviderManifest::standard(),
        "advanced-vn",
        StageViewport {
            width: 1280,
            height: 720,
        },
    )
    .unwrap()
}

fn configure(director: &mut ProductStageDirector) {
    director
        .apply(&StageCommand::Configure {
            viewport: StageViewport {
                width: 1280,
                height: 720,
            },
            safe_area: AspectRatio {
                width: 16,
                height: 9,
            },
        })
        .unwrap();
    director
        .apply(&StageCommand::DeclareLayer {
            id: "characters".to_string(),
            kind: StageLayerKind::Sprite,
            z: 100,
            blend: StageBlendMode::Normal,
            clip: Some(StageClipPolicy::SafeArea),
            input: None,
        })
        .unwrap();
}

fn show_hero(director: &mut ProductStageDirector) {
    director
        .apply(&StageCommand::Show {
            id: "hero".to_string(),
            asset: "asset:/character/hero".to_string(),
            pose: Some("normal".to_string()),
            layer: "characters".to_string(),
            placement: StagePlacement::Center,
            preset: Some("hero_enter".to_string()),
        })
        .unwrap();
}

#[test]
fn stage_director_applies_profile_bound_tween_without_partial_failure() {
    let mut director = director();
    let initial = director.state().stable_hash().unwrap();

    let error = director
        .apply(&StageCommand::Show {
            id: "hero".to_string(),
            asset: "asset:/character/hero".to_string(),
            pose: None,
            layer: "missing".to_string(),
            placement: StagePlacement::Center,
            preset: Some("hero_enter".to_string()),
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_VN_STAGE_NOT_CONFIGURED");
    assert_eq!(director.state().stable_hash().unwrap(), initial);

    configure(&mut director);
    show_hero(&mut director);
    assert_eq!(director.state().entities["hero"].opacity, FixedScalar::ZERO);
    director.tick(150_000_000).unwrap();
    let midpoint = director.state().entities["hero"].opacity.millionths;
    assert!(midpoint > 0 && midpoint < 1_000_000);
    director.tick(150_000_000).unwrap();
    assert_eq!(director.state().entities["hero"].opacity, FixedScalar::ONE);
}

#[test]
fn stage_director_timeline_snapshot_restore_matches_uninterrupted_run() {
    let manifest = VnPresentationProviderManifest::standard();
    let mut uninterrupted = ProductStageDirector::new(
        manifest.clone(),
        "advanced-vn",
        StageViewport {
            width: 1280,
            height: 720,
        },
    )
    .unwrap();
    configure(&mut uninterrupted);
    show_hero(&mut uninterrupted);
    uninterrupted.tick(300_000_000).unwrap();
    uninterrupted
        .apply(&StageCommand::Timeline(TimelineCommand::Start(
            TimelineSpec {
                id: "hero.move".to_string(),
                join: VnTimelineJoinPolicy::Block,
                tracks: vec![VnTimelineTrack {
                    target: "hero".to_string(),
                    property: "x".to_string(),
                    keyframes: vec![
                        VnTimelineKeyframe {
                            time_ms: 0,
                            value: fixed(640_000_000),
                        },
                        VnTimelineKeyframe {
                            time_ms: 400,
                            value: fixed(960_000_000),
                        },
                    ],
                }],
                fence: Some("hero.move.done".to_string()),
                fallback: None,
                budget_us: 2_000,
            },
        )))
        .unwrap();

    uninterrupted.tick(160_000_000).unwrap();
    let snapshot = uninterrupted.snapshot().unwrap();
    let mut restored = ProductStageDirector::restore(manifest, "advanced-vn", &snapshot).unwrap();

    let direct_output = uninterrupted.tick(240_000_000).unwrap();
    let restored_output = restored.tick(240_000_000).unwrap();
    assert_eq!(direct_output, restored_output);
    assert_eq!(uninterrupted.state(), restored.state());
    assert_eq!(uninterrupted.active_timeline_count(), 0);
    assert!(direct_output.iter().any(|output| matches!(
        output,
        astra_vn_presentation::StageDirectorOutput::FenceCompleted { id, .. }
            if id == "hero.move.done"
    )));
}

#[test]
fn stage_director_rejects_invalid_tick_and_timeline_without_mutation() {
    let mut director = director();
    configure(&mut director);
    show_hero(&mut director);
    director.tick(300_000_000).unwrap();
    let initial = director.state().stable_hash().unwrap();

    assert_eq!(
        director.tick(0).unwrap_err().code(),
        "ASTRA_VN_STAGE_TICK_DELTA"
    );
    assert_eq!(director.state().stable_hash().unwrap(), initial);
    let error = director
        .apply(&StageCommand::Timeline(TimelineCommand::Start(
            TimelineSpec {
                id: "invalid".to_string(),
                join: VnTimelineJoinPolicy::Block,
                tracks: vec![VnTimelineTrack {
                    target: "hero".to_string(),
                    property: "opacity".to_string(),
                    keyframes: vec![
                        VnTimelineKeyframe {
                            time_ms: 100,
                            value: FixedScalar::ZERO,
                        },
                        VnTimelineKeyframe {
                            time_ms: 50,
                            value: FixedScalar::ONE,
                        },
                    ],
                }],
                fence: None,
                fallback: None,
                budget_us: 2_000,
            },
        )))
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_VN_STAGE_TIMELINE_ORDER");
    assert_eq!(director.state().stable_hash().unwrap(), initial);
}
