use astra_vn_presentation::{
    AspectRatio, FixedScalar, ProductStageDirector, StageBlendMode, StageClipPolicy, StageCommand,
    StageFitMode, StageLayerKind, StagePlacement, StageViewport, TimelineCommand, TimelineSpec,
    VnAudioBus, VnPresentationProviderManifest, VnTimelineJoinPolicy, VnTimelineKeyframe,
    VnTimelineTrack,
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

#[astra_headless_test::test]
fn stage_batch_prepares_one_atomic_next_state_without_mutating_source() {
    let director = director();
    let initial = director.state().stable_hash().unwrap();
    let commands = [
        StageCommand::Backdrop {
            color: [12, 24, 36, 255],
        },
        StageCommand::Shade {
            color: [0, 0, 0, 255],
            opacity: fixed(500_000),
        },
    ];

    let (next, outputs) = director.prepare_batch(commands.iter()).unwrap();

    assert_eq!(director.state().stable_hash().unwrap(), initial);
    assert_eq!(outputs.len(), commands.len());
    assert_eq!(next.state().backdrop_color, Some([12, 24, 36, 255]));
    assert_eq!(next.state().shade_opacity, fixed(500_000));
}

#[astra_headless_test::test]
fn stage_batch_failure_discards_every_preceding_mutation() {
    let director = director();
    let initial = director.state().stable_hash().unwrap();
    let commands = [
        StageCommand::Backdrop {
            color: [12, 24, 36, 255],
        },
        StageCommand::Shade {
            color: [0, 0, 0, 128],
            opacity: fixed(500_000),
        },
    ];

    let error = director.prepare_batch(commands.iter()).unwrap_err();

    assert_eq!(error.code(), "ASTRA_VN_STAGE_SHADE_COLOR_ALPHA");
    assert_eq!(director.state().stable_hash().unwrap(), initial);
}

#[astra_headless_test::test]
fn stage_backdrop_is_authoritative_serializable_and_explicitly_clearable() {
    let mut director = director();
    let black = [0, 0, 0, 255];
    director
        .apply(&StageCommand::Backdrop { color: black })
        .unwrap();
    assert_eq!(director.state().backdrop_color, Some(black));

    let snapshot = director.snapshot().unwrap();
    let restored = ProductStageDirector::restore(
        VnPresentationProviderManifest::standard(),
        "advanced-vn",
        &snapshot,
    )
    .unwrap();
    assert_eq!(restored.state().backdrop_color, Some(black));

    director
        .apply(&StageCommand::Backdrop {
            color: [0, 0, 0, 0],
        })
        .unwrap();
    assert_eq!(director.state().backdrop_color, None);
}

#[astra_headless_test::test]
fn stage_shade_color_and_coverage_are_authoritative_and_serializable() {
    let mut director = director();
    let color = [0x22, 0x24, 0x20, 0xff];
    director
        .apply(&StageCommand::Shade {
            color,
            opacity: fixed(920_000),
        })
        .unwrap();
    assert_eq!(director.state().shade_color, color);
    assert_eq!(director.state().shade_opacity, fixed(920_000));

    let snapshot = director.snapshot().unwrap();
    let restored = ProductStageDirector::restore(
        VnPresentationProviderManifest::standard(),
        "advanced-vn",
        &snapshot,
    )
    .unwrap();
    assert_eq!(restored.state().shade_color, color);
    assert_eq!(restored.state().shade_opacity, fixed(920_000));

    let error = director
        .apply(&StageCommand::Shade {
            color: [0x22, 0x24, 0x20, 0x80],
            opacity: fixed(920_000),
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_VN_STAGE_SHADE_COLOR_ALPHA");
}

fn show_hero(director: &mut ProductStageDirector) {
    director
        .apply(&StageCommand::Show {
            id: "hero".to_string(),
            asset: "asset:/character/hero".to_string(),
            pose: Some("normal".to_string()),
            layer: "characters".to_string(),
            placement: StagePlacement::Center,
            fit: StageFitMode::ContainHeight,
            opacity: FixedScalar::ONE,
            preset: Some("hero_enter".to_string()),
        })
        .unwrap();
}

#[astra_headless_test::test]
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
            fit: StageFitMode::ContainHeight,
            opacity: FixedScalar::ONE,
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

#[astra_headless_test::test]
fn stage_director_tracks_preload_and_layer_authority_in_snapshot_state() {
    let mut director = director();
    let output = director
        .apply(&StageCommand::Preload {
            asset: "asset:/character/hero".to_string(),
        })
        .unwrap();
    assert_eq!(
        output,
        vec![astra_vn_presentation::StageDirectorOutput::Preload {
            asset: "asset:/character/hero".to_string(),
        }]
    );
    assert!(director
        .state()
        .preloaded_assets
        .contains("asset:/character/hero"));
    assert!(director
        .apply(&StageCommand::Preload {
            asset: "asset:/character/hero".to_string(),
        })
        .unwrap()
        .is_empty());

    configure(&mut director);
    director
        .apply(&StageCommand::Show {
            id: "half".to_string(),
            asset: "asset:/character/hero".to_string(),
            pose: None,
            layer: "characters".to_string(),
            placement: StagePlacement::Center,
            fit: StageFitMode::ContainHeight,
            opacity: fixed(500_000),
            preset: None,
        })
        .unwrap();
    assert_eq!(director.state().entities["half"].opacity, fixed(500_000));
    director
        .apply(&StageCommand::SetLayerVisibility {
            layer: "characters".to_string(),
            visible: false,
        })
        .unwrap();
    assert!(!director.state().layers["characters"].visible);
    director
        .apply(&StageCommand::ClearLayer {
            layer: "characters".to_string(),
            duration_ms: 0,
        })
        .unwrap();
    assert!(director.state().entities.is_empty());
}

#[astra_headless_test::test]
fn stage_director_can_clear_an_empty_video_layer() {
    let mut director = director();
    configure(&mut director);
    director
        .apply(&StageCommand::DeclareLayer {
            id: "video".to_string(),
            kind: StageLayerKind::Video,
            z: 200,
            blend: StageBlendMode::Normal,
            clip: Some(StageClipPolicy::Stage),
            input: None,
        })
        .unwrap();

    director
        .apply(&StageCommand::ClearLayer {
            layer: "video".to_string(),
            duration_ms: 0,
        })
        .unwrap();
}

#[astra_headless_test::test]
fn stage_director_resizes_transactionally_without_losing_live_state() {
    let mut director = director();
    configure(&mut director);
    show_hero(&mut director);
    director.tick(300_000_000).unwrap();

    director
        .resize_viewport(StageViewport {
            width: 2560,
            height: 1440,
        })
        .unwrap();
    assert_eq!(director.state().viewport.width, 2560);
    assert_eq!(director.state().viewport.height, 1440);
    assert!(director.state().entities.contains_key("hero"));

    let stable = director.state().stable_hash().unwrap();
    let error = director
        .resize_viewport(StageViewport {
            width: 0,
            height: 1440,
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_VN_STAGE_VIEWPORT");
    assert_eq!(director.state().stable_hash().unwrap(), stable);
}

#[astra_headless_test::test]
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

#[astra_headless_test::test]
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

#[astra_headless_test::test]
fn audio_bus_enabled_state_is_typed_and_snapshot_stable() {
    let manifest = VnPresentationProviderManifest::standard();
    let mut director = ProductStageDirector::new(
        manifest.clone(),
        "advanced-vn",
        StageViewport {
            width: 1280,
            height: 720,
        },
    )
    .unwrap();
    let output = director
        .apply(&StageCommand::SetAudioBusEnabled {
            bus: VnAudioBus::Bgm,
            enabled: false,
        })
        .unwrap();
    assert!(matches!(
        output.as_slice(),
        [
            astra_vn_presentation::StageDirectorOutput::AudioBusEnabled {
                bus: VnAudioBus::Bgm,
                enabled: false
            }
        ]
    ));
    assert!(!director.state().audio_bus_enabled[&VnAudioBus::Bgm]);
    let snapshot = director.snapshot().unwrap();
    let restored = ProductStageDirector::restore(manifest, "advanced-vn", &snapshot).unwrap();
    assert_eq!(restored.state(), director.state());
}
