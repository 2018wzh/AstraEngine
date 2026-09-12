use super::*;

#[test]
fn queued_regions_restore_and_complete_once_while_other_regions_continue() {
    let mut live = PresentationCoordinator::default();
    live.apply_batch(
        &[
            character(1, "characters"),
            background(2, "background"),
            text(3),
        ],
        1,
    )
    .unwrap();
    let mut queued = character(4, "characters");
    queued.interrupt = PresentationInterruptPolicy::Queue;
    live.apply_batch(&[queued], 1).unwrap();
    let mut restored = PresentationCoordinator::restore(&live.snapshot().unwrap()).unwrap();
    for delta in [500_000_000, 500_000_000, 500_000_000] {
        assert_eq!(live.tick(delta).unwrap(), restored.tick(delta).unwrap());
        assert_eq!(
            live.take_activated_commands(),
            restored.take_activated_commands()
        );
        assert_eq!(live.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(live.state().character.queued.is_empty());
    assert_eq!(
        live.state().character.characters["hero"].command_id,
        "character.4"
    );
    for id in [
        "fence.character.1",
        "fence.character.4",
        "fence.background.2",
        "fence.text.3",
    ] {
        assert_eq!(live.state().fences[id], FenceStatus::Completed);
    }
    assert_eq!(
        live.state().background.layers["background"]
            .current
            .as_deref(),
        Some("asset:/room.png")
    );
    assert!(live.tick(500_000_000).unwrap().is_empty());
}

#[test]
fn revealed_text_stays_complete_and_background_can_explicitly_clear() {
    let mut live = PresentationCoordinator::default();
    live.apply_batch(&[background(1, "background"), text(2)], 1)
        .unwrap();
    assert_eq!(
        live.request_text_advance(),
        TextAdvanceDisposition::RevealCompleted
    );
    live.tick(1_000_000).unwrap();
    assert!(live.state().text.active.as_ref().unwrap().reveal_complete());
    live.tick(1_000_000_000).unwrap();
    let mut clear = background(3, "background");
    if let PresentationRegionCommand::Background(ref mut command) = clear.payload {
        command.asset = None;
        command.duration_ns = 0;
    }
    live.apply_batch(&[clear], 1).unwrap();
    live.tick(1).unwrap();
    assert_eq!(live.state().background.layers["background"].current, None);
    live.tick(1).unwrap();
    assert_eq!(live.state().background.layers["background"].current, None);
}

#[test]
fn invalid_delta_or_malformed_batch_leaves_coordinator_usable() {
    let mut live = PresentationCoordinator::default();
    live.apply_batch(&[character(1, "characters")], 1).unwrap();
    let before = live.snapshot().unwrap();
    for delta in [0, 1_000_000_001] {
        assert!(live.tick(delta).is_err());
    }
    let mut invalid = text(3);
    if let PresentationRegionCommand::Text(ref mut command) = invalid.payload {
        command.graphemes_per_second = 0;
    }
    assert_eq!(
        live.apply_batch(&[background(2, "background"), invalid], 1)
            .unwrap_err()
            .code(),
        "ASTRA_VN_TEXT_REVEAL_RATE"
    );
    assert_eq!(live.snapshot().unwrap(), before);
    assert!(!live.is_failed());
    live.tick(500_000_000).unwrap();
}
