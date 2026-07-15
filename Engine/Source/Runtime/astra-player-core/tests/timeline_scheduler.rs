use astra_player_core::{
    PlayerTimelineCompletionKind, PlayerTimelineScheduler, PlayerTimelineTask,
    PlayerTimelineTaskAction,
};

fn start(id: &str, target: &str, fence: &str, duration_ms: u64) -> PlayerTimelineTask {
    PlayerTimelineTask {
        schema: "astra.player_timeline_task.v1".to_string(),
        task_id: id.to_string(),
        target: Some(target.to_string()),
        action: PlayerTimelineTaskAction::Start,
        duration_ms: Some(duration_ms),
        fence: Some(fence.to_string()),
    }
}

#[astra_headless_test::test]
fn timeline_completes_only_after_monotonic_deadline() {
    let mut scheduler = PlayerTimelineScheduler::new(8);
    scheduler
        .schedule(start("intro", "hero", "intro.done", 120), 40)
        .unwrap();

    assert!(scheduler.poll(159).unwrap().is_empty());
    let completed = scheduler.poll(160).unwrap();

    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].task_id, "intro");
    assert_eq!(completed[0].fence.as_deref(), Some("intro.done"));
    assert_eq!(completed[0].kind, PlayerTimelineCompletionKind::Completed);
}

#[astra_headless_test::test]
fn timeline_cancel_returns_the_original_join_fence() {
    let mut scheduler = PlayerTimelineScheduler::new(8);
    scheduler
        .schedule(start("intro", "hero", "intro.done", 120), 0)
        .unwrap();

    let completed = scheduler
        .schedule(
            PlayerTimelineTask {
                schema: "astra.player_timeline_task.v1".to_string(),
                task_id: "intro".to_string(),
                target: None,
                action: PlayerTimelineTaskAction::Cancel,
                duration_ms: None,
                fence: None,
            },
            20,
        )
        .unwrap();

    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].fence.as_deref(), Some("intro.done"));
    assert_eq!(completed[0].kind, PlayerTimelineCompletionKind::Cancelled);
    assert_eq!(scheduler.active_count(), 0);
}

#[astra_headless_test::test]
fn timeline_blocks_clock_regression_without_mutating_tasks() {
    let mut scheduler = PlayerTimelineScheduler::new(8);
    scheduler
        .schedule(start("intro", "hero", "intro.done", 120), 40)
        .unwrap();

    let error = scheduler.poll(39).unwrap_err();

    assert!(error
        .to_string()
        .contains("ASTRA_PLAYER_TIMELINE_CLOCK_REGRESSION"));
    assert_eq!(scheduler.active_count(), 1);
}

#[astra_headless_test::test]
fn timeline_blocks_duplicate_id_and_capacity_overflow() {
    let mut scheduler = PlayerTimelineScheduler::new(1);
    scheduler
        .schedule(start("intro", "hero", "intro.done", 120), 0)
        .unwrap();

    assert!(scheduler
        .schedule(start("intro", "other", "other.done", 20), 1)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_TIMELINE_TASK_DUPLICATE"));
    assert!(scheduler
        .schedule(start("second", "other", "second.done", 20), 1)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_TIMELINE_CAPACITY"));
}

#[astra_headless_test::test]
fn timeline_snapshot_restores_deadline_and_completed_identity() {
    let mut scheduler = PlayerTimelineScheduler::new(8);
    scheduler
        .schedule(start("intro", "hero", "intro.done", 120), 40)
        .unwrap();
    let restored = PlayerTimelineScheduler::restore(scheduler.snapshot()).unwrap();
    let completed = restored.clone().poll(160).unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].task_id, "intro");

    let mut completed_scheduler = restored;
    completed_scheduler.poll(160).unwrap();
    let mut restored = PlayerTimelineScheduler::restore(completed_scheduler.snapshot()).unwrap();
    let cancelled = restored
        .schedule(
            PlayerTimelineTask {
                schema: "astra.player_timeline_task.v1".into(),
                task_id: "intro".into(),
                target: None,
                action: PlayerTimelineTaskAction::Cancel,
                duration_ms: None,
                fence: None,
            },
            161,
        )
        .unwrap();
    assert!(cancelled.is_empty());
}
