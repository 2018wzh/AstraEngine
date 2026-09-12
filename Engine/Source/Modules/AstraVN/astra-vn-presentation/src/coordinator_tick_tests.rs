use super::*;

#[test]
fn activated_queue_exhaustion_terminates_only_this_coordinator() {
    let mut coordinator = PresentationCoordinator::default();
    coordinator.state.activated_commands = (0..MAX_REGION_QUEUE)
        .map(|i| format!("prior.{i}"))
        .collect();
    coordinator
        .state
        .character
        .queued
        .push_back(PresentationCommandEnvelope {
            fixed_step: 1,
            sequence: 1,
            command_id: "next".into(),
            interrupt: PresentationInterruptPolicy::Queue,
            fence: None,
            payload: PresentationRegionCommand::Character(CharacterRegionCommand {
                character_id: "hero".into(),
                asset: "asset:/hero".into(),
                pose: None,
                layer: "characters".into(),
                visible: true,
                duration_ns: 100,
            }),
        });
    assert_eq!(
        coordinator.tick(1).unwrap_err().code(),
        "ASTRA_VN_PRESENTATION_ACTIVATED_LIMIT"
    );
    assert!(coordinator.is_failed());
    assert_eq!(
        coordinator.tick(1).unwrap_err().code(),
        "ASTRA_VN_PRESENTATION_SESSION_FAILED"
    );
    assert_eq!(
        coordinator.apply_batch(&[], 1).unwrap_err().code(),
        "ASTRA_VN_PRESENTATION_SESSION_FAILED"
    );
    assert!(coordinator.snapshot().is_err());
    assert!(coordinator.take_activated_commands().is_empty());
    PresentationCoordinator::default().tick(1).unwrap();
}

#[test]
fn restore_rejects_wrong_region_queue_without_panicking_during_tick() {
    let mut coordinator = PresentationCoordinator::default();
    coordinator
        .state
        .video
        .queued
        .push_back(PresentationCommandEnvelope {
            fixed_step: 1,
            sequence: 1,
            command_id: "invalid".into(),
            interrupt: PresentationInterruptPolicy::Queue,
            fence: None,
            payload: PresentationRegionCommand::Text(TextRegionCommand {
                text_key: "line".into(),
                speaker: None,
                window: None,
                grapheme_count: 1,
                graphemes_per_second: 1,
            }),
        });
    let bytes = postcard::to_allocvec(&coordinator).unwrap();
    assert_eq!(
        PresentationCoordinator::restore(&bytes).unwrap_err().code(),
        "ASTRA_VN_PRESENTATION_QUEUE_STATE"
    );
}
