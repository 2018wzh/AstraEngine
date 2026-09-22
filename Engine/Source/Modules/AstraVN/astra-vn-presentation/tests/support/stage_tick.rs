use super::*;

fn track(property: &str, end: i64) -> VnTimelineTrack {
    VnTimelineTrack {
        target: "hero".into(),
        property: property.into(),
        keyframes: vec![
            VnTimelineKeyframe {
                time_ms: 0,
                value: FixedScalar::ZERO,
            },
            VnTimelineKeyframe {
                time_ms: 1000,
                value: fixed(end),
            },
        ],
    }
}

fn timeline(id: &str, tracks: Vec<VnTimelineTrack>, join: VnTimelineJoinPolicy) -> StageCommand {
    StageCommand::Timeline(TimelineCommand::Start(TimelineSpec {
        id: id.into(),
        join,
        tracks,
        fence: Some(id.into()),
        fallback: None,
        budget_us: 1000,
    }))
}

#[test]
fn queued_move_survives_save_and_activates_once_after_show() {
    let mut live = director();
    configure(&mut live);
    show_hero(&mut live);
    live.apply(&StageCommand::Move {
        id: "hero".into(),
        x: fixed(10_000_000),
        y: fixed(20_000_000),
        duration_ms: 100,
        preset: None,
        interrupt: PresentationInterruptPolicy::Queue,
    })
    .unwrap();
    live.tick(100_000_000).unwrap();
    let bytes = live.snapshot().unwrap();
    let mut restored = ProductStageDirector::restore(
        VnPresentationProviderManifest::standard(),
        "advanced-vn",
        &bytes,
    )
    .unwrap();
    for delta in [200_000_000, 50_000_000, 50_000_000, 100_000_000] {
        assert_eq!(live.tick(delta).unwrap(), restored.tick(delta).unwrap());
        assert_eq!(live.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert_eq!(live.state().entities["hero"].x, fixed(10_000_000));
    assert_eq!(live.state().entities["hero"].y, fixed(20_000_000));
    assert!(!live.requires_frame_tick());
}

#[test]
fn malformed_queued_show_is_rejected_before_sequence_or_state_changes() {
    let mut live = director();
    configure(&mut live);
    show_hero(&mut live);
    let before = live.snapshot().unwrap();
    let error = live
        .apply(&StageCommand::Show {
            id: "hero".into(),
            asset: "asset:/hero".into(),
            pose: None,
            layer: "characters".into(),
            placement: StagePlacement::Center,
            fit: StageFitMode::ContainHeight,
            opacity: fixed(2_000_000),
            preset: Some("fade".into()),
            interrupt: PresentationInterruptPolicy::Queue,
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_VN_STAGE_ENTITY_STATE");
    assert_eq!(live.snapshot().unwrap(), before);
    assert!(!live.is_failed());
    live.tick(300_000_000).unwrap();
}

#[test]
fn hide_cancels_entity_tracks_without_a_false_completion_and_restores_cleanly() {
    for duration_ms in [0, 100] {
        let mut live = director();
        configure(&mut live);
        show_hero(&mut live);
        live.tick(300_000_000).unwrap();
        live.apply(&timeline(
            "moving",
            vec![track("x", 1_000_000)],
            VnTimelineJoinPolicy::Block,
        ))
        .unwrap();
        live.apply(&StageCommand::Hide {
            id: "hero".into(),
            duration_ms,
            preset: None,
            interrupt: PresentationInterruptPolicy::ReplaceFromCurrent,
        })
        .unwrap();
        let outputs = live.tick(100_000_000).unwrap();
        assert!(outputs.is_empty());
        assert!(!live.state().entities.contains_key("hero"));
        assert_eq!(live.active_timeline_count(), 0);
        let saved = live.snapshot().unwrap();
        let mut restored = ProductStageDirector::restore(
            VnPresentationProviderManifest::standard(),
            "advanced-vn",
            &saved,
        )
        .unwrap();
        assert_eq!(
            live.tick(100_000_000).unwrap(),
            restored.tick(100_000_000).unwrap()
        );
        assert_eq!(live.snapshot().unwrap(), restored.snapshot().unwrap());
        restored
            .apply(&StageCommand::Timeline(TimelineCommand::Cancel {
                id: "moving".into(),
                reason: "authored cleanup".into(),
            }))
            .unwrap();
    }
}

#[test]
fn clear_layer_cancels_only_removed_entity_tracks() {
    let mut live = director();
    configure(&mut live);
    show_hero(&mut live);
    live.tick(300_000_000).unwrap();
    let camera = VnTimelineTrack {
        target: "camera".into(),
        property: "x".into(),
        keyframes: track("x", 2_000_000).keyframes,
    };
    live.apply(&timeline(
        "mixed",
        vec![track("x", 1_000_000), camera],
        VnTimelineJoinPolicy::Block,
    ))
    .unwrap();
    live.apply(&StageCommand::ClearLayer {
        layer: "characters".into(),
        duration_ms: 0,
        interrupt: PresentationInterruptPolicy::ReplaceFromCurrent,
    })
    .unwrap();
    assert_eq!(live.active_timeline_count(), 1);
    let saved = live.snapshot().unwrap();
    let mut restored = ProductStageDirector::restore(
        VnPresentationProviderManifest::standard(),
        "advanced-vn",
        &saved,
    )
    .unwrap();
    assert_eq!(
        live.tick(1_000_000_000).unwrap(),
        restored.tick(1_000_000_000).unwrap()
    );
    assert_eq!(live.state().camera.x, fixed(2_000_000));
    assert_eq!(live.snapshot().unwrap(), restored.snapshot().unwrap());
}

#[test]
fn replacing_one_timeline_property_preserves_other_tracks_and_cancellation_is_local() {
    let mut live = director();
    configure(&mut live);
    show_hero(&mut live);
    live.tick(300_000_000).unwrap();
    live.apply(&timeline(
        "original",
        vec![track("x", 1_000_000), track("y", 2_000_000)],
        VnTimelineJoinPolicy::Block,
    ))
    .unwrap();
    live.tick(250_000_000).unwrap();
    live.apply(&timeline(
        "replacement",
        vec![track("x", 4_000_000)],
        VnTimelineJoinPolicy::ReplaceTarget,
    ))
    .unwrap();
    assert_eq!(live.active_timeline_count(), 2);
    live.tick(250_000_000).unwrap();
    assert_eq!(live.state().entities["hero"].x, fixed(1_000_000));
    assert_eq!(live.state().entities["hero"].y, fixed(1_000_000));
    live.apply(&StageCommand::Timeline(TimelineCommand::Cancel {
        id: "replacement".into(),
        reason: "test cancellation".into(),
    }))
    .unwrap();
    let outputs = live.tick(500_000_000).unwrap();
    assert_eq!(live.state().entities["hero"].x, fixed(1_000_000));
    assert_eq!(live.state().entities["hero"].y, fixed(2_000_000));
    assert_eq!(live.active_timeline_count(), 0);
    assert!(
        matches!(outputs.as_slice(), [astra_vn_presentation::StageDirectorOutput::FenceCompleted { id, .. }] if id == "original")
    );
}

#[test]
fn invalid_timeline_endpoints_are_rejected_at_ingress() {
    let mut live = director();
    configure(&mut live);
    show_hero(&mut live);
    let before = live.snapshot().unwrap();
    for invalid in [
        track("opacity", 2_000_000),
        VnTimelineTrack {
            target: "camera".into(),
            property: "zoom".into(),
            keyframes: track("x", -1).keyframes,
        },
    ] {
        assert!(live
            .apply(&timeline(
                "invalid",
                vec![invalid],
                VnTimelineJoinPolicy::Block
            ))
            .is_err());
        assert_eq!(live.snapshot().unwrap(), before);
        assert!(!live.is_failed());
    }
    live.tick(300_000_000).unwrap();
}
