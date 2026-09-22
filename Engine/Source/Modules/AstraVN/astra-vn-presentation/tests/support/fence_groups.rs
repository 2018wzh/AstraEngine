use super::*;

fn joined(mut command: PresentationCommandEnvelope) -> PresentationCommandEnvelope {
    command.fence = Some("all".into());
    command
}

#[test]
fn parallel_members_complete_once_after_every_region_and_restore() {
    let mut c = PresentationCoordinator::default();
    c.apply_batch(
        &[
            joined(character(1, "hero")),
            joined(background(2, "bg")),
            joined(video(3, "movie")),
        ],
        4,
    )
    .unwrap();
    assert!(c.tick(500_000_000).unwrap().is_empty());
    assert_eq!(c.state().fences["all"], FenceStatus::Pending);
    let mut c = PresentationCoordinator::restore(&c.snapshot().unwrap()).unwrap();
    assert!(c.complete_video("opening").unwrap().is_empty());
    assert_eq!(c.tick(500_000_000).unwrap(), vec!["all"]);
    assert!(c.tick(500_000_000).unwrap().is_empty());
    assert!(c.complete_video("opening").unwrap().is_empty());
}

#[test]
fn queued_member_keeps_group_pending_after_first_activation_finishes() {
    let mut c = PresentationCoordinator::default();
    let mut queued = joined(character(2, "hero"));
    queued.interrupt = PresentationInterruptPolicy::Queue;
    c.apply_batch(&[joined(character(1, "hero")), queued], 1)
        .unwrap();
    assert!(c.tick(500_000_000).unwrap().is_empty());
    assert_eq!(c.take_activated_commands(), vec!["character.2"]);
    let mut c = PresentationCoordinator::restore(&c.snapshot().unwrap()).unwrap();
    assert_eq!(c.tick(500_000_000).unwrap(), vec!["all"]);
}

#[test]
fn immediate_text_reveal_cannot_complete_background_member() {
    let mut c = PresentationCoordinator::default();
    c.apply_batch(&[joined(text(1)), joined(background(2, "bg"))], 2)
        .unwrap();
    assert_eq!(
        c.request_text_advance(),
        TextAdvanceDisposition::RevealCompleted
    );
    assert_eq!(c.state().fences["all"], FenceStatus::Pending);
    assert_eq!(c.tick(1_000_000_000).unwrap(), vec!["all"]);
}

#[test]
fn failure_and_replacement_are_terminal_for_group_without_stopping_other_regions() {
    let mut failed = PresentationCoordinator::default();
    failed
        .apply_batch(&[joined(video(1, "movie")), joined(background(2, "bg"))], 2)
        .unwrap();
    failed.fail_video("opening").unwrap();
    assert!(failed.tick(1_000_000_000).unwrap().is_empty());
    assert_eq!(failed.state().fences["all"], FenceStatus::Failed);
    assert_eq!(
        failed.state().background.layers["bg"].current.as_deref(),
        Some("asset:/room.png")
    );

    let mut replaced = PresentationCoordinator::default();
    replaced
        .apply_batch(
            &[joined(character(1, "hero")), joined(background(2, "bg"))],
            2,
        )
        .unwrap();
    replaced.apply_batch(&[character(3, "hero")], 1).unwrap();
    assert_eq!(replaced.state().fences["all"], FenceStatus::Failed);
    assert_eq!(
        replaced.tick(500_000_000).unwrap(),
        vec!["fence.character.3"]
    );
    let mut replaced = PresentationCoordinator::restore(&replaced.snapshot().unwrap()).unwrap();
    assert!(replaced.tick(500_000_000).unwrap().is_empty());
    assert_eq!(replaced.state().fences["all"], FenceStatus::Failed);
}

#[test]
fn finished_group_can_be_reused_without_inheriting_old_completion() {
    let mut c = PresentationCoordinator::default();
    c.apply_batch(&[joined(character(1, "hero"))], 1).unwrap();
    assert_eq!(c.tick(500_000_000).unwrap(), vec!["all"]);
    c.apply_batch(&[joined(character(2, "hero"))], 1).unwrap();
    assert_eq!(c.state().fences["all"], FenceStatus::Pending);
    assert!(c.tick(250_000_000).unwrap().is_empty());
    assert_eq!(c.tick(250_000_000).unwrap(), vec!["all"]);
}

#[test]
fn ambiguous_member_identity_is_rejected_before_batch_commit() {
    let mut c = PresentationCoordinator::default();
    let first = joined(character(1, "hero"));
    let mut duplicate = first.clone();
    duplicate.sequence = 2;
    let before = c.clone();
    assert!(c
        .apply_batch(&[first.clone(), duplicate.clone()], 1)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_VN_PRESENTATION_FENCE_MEMBER_CONFLICT"));
    assert_eq!(c, before);
    c.apply_batch(&[first], 1).unwrap();
    let before = c.clone();
    assert!(c.apply_batch(&[duplicate], 1).is_err());
    assert_eq!(c, before);
}

#[test]
fn appending_after_partial_completion_preserves_members_and_restore() {
    let mut c = PresentationCoordinator::default();
    c.apply_batch(
        &[joined(character(1, "hero")), joined(background(2, "bg"))],
        1,
    )
    .unwrap();
    c.tick(500_000_000).unwrap();
    c.apply_batch(&[joined(video(3, "movie"))], 1).unwrap();
    let mut c = PresentationCoordinator::restore(&c.snapshot().unwrap()).unwrap();
    assert!(c.tick(500_000_000).unwrap().is_empty());
    let mut duplicate = joined(character(4, "other"));
    duplicate.command_id = "character.1".into();
    assert!(c.apply_batch(&[duplicate], 1).is_err());
    assert_eq!(c.complete_video("opening").unwrap(), vec!["all"]);
    assert!(c.complete_video("opening").unwrap().is_empty());
    PresentationCoordinator::restore(&c.snapshot().unwrap()).unwrap();
}
