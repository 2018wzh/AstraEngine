use astra_runtime::*;

#[test]
fn sequence_starts_only_current_member_and_stops_on_failure() {
    let scope = TaskScope::new();
    let mut group = TaskGroup::new(&scope, TaskGroupMode::Sequence, 3).unwrap();
    assert!(group.handle(1).is_err());
    let first = group.handle(0).unwrap();
    group.complete(&first, TaskTerminal::Completed).unwrap();
    assert!(group.complete(&first, TaskTerminal::Completed).is_err());
    assert!(group.handle(2).is_err());
    let second = group.handle(1).unwrap();
    group.complete(&second, TaskTerminal::Failed).unwrap();
    assert_eq!(group.snapshot().terminal(), Some(TaskTerminal::Failed));
    assert!(group.handle(2).is_err());
    group.snapshot().validate().unwrap();
}

#[test]
fn all_waits_for_all_terminals_and_retains_failure() {
    let scope = TaskScope::new();
    let mut group = TaskGroup::new(&scope, TaskGroupMode::All, 3).unwrap();
    let handles: Vec<_> = (0..3).map(|i| group.handle(i).unwrap()).collect();
    group.complete(&handles[1], TaskTerminal::Failed).unwrap();
    group
        .complete(&handles[0], TaskTerminal::Cancelled)
        .unwrap();
    assert_eq!(group.snapshot().terminal(), None);
    group
        .complete(&handles[2], TaskTerminal::Completed)
        .unwrap();
    assert_eq!(group.snapshot().terminal(), Some(TaskTerminal::Failed));
    group.snapshot().validate().unwrap();
}

#[test]
fn race_first_terminal_wins_and_cancels_other_scopes() {
    for outcome in [
        TaskTerminal::Completed,
        TaskTerminal::Failed,
        TaskTerminal::Cancelled,
    ] {
        let scope = TaskScope::new();
        let mut group = TaskGroup::new(&scope, TaskGroupMode::Race, 2).unwrap();
        let first = group.handle(0).unwrap();
        let second = group.handle(1).unwrap();
        group.complete(&second, outcome).unwrap();
        assert!(first.scope().is_cancelled());
        assert!(group.complete(&first, TaskTerminal::Completed).is_err());
        assert_eq!(group.snapshot().winner(), Some(1));
        assert_eq!(group.snapshot().terminal(), Some(outcome));
        group.snapshot().validate().unwrap();
        assert!(
            !scope.is_cancelled(),
            "group cancellation must not cancel its successor"
        );
    }
}

#[test]
fn restored_sequence_preserves_progress_and_rejects_old_handles_without_restarting_work() {
    let scope = TaskScope::new();
    let mut group = TaskGroup::new(&scope, TaskGroupMode::Sequence, 3).unwrap();
    group
        .complete(&group.handle(0).unwrap(), TaskTerminal::Completed)
        .unwrap();
    let old = group.handle(1).unwrap();
    let bytes = postcard::to_allocvec(&group.snapshot()).unwrap();
    let restored = postcard::from_bytes(&bytes).unwrap();
    let mut replacement = TaskGroup::restore(&scope, restored).unwrap();
    drop(group);
    assert!(old.scope().is_cancelled());
    assert!(replacement.complete(&old, TaskTerminal::Completed).is_err());
    assert!(replacement.handle(0).is_err());
    assert!(replacement.handle(2).is_err());
    replacement
        .complete(&replacement.handle(1).unwrap(), TaskTerminal::Completed)
        .unwrap();
    assert!(replacement.handle(2).is_ok());
    replacement.snapshot().validate().unwrap();
}

#[test]
fn cancellation_and_drop_stop_workers_and_preserve_other_groups() {
    let parent = TaskScope::new();
    let mut group = TaskGroup::new(&parent, TaskGroupMode::All, 2).unwrap();
    let sibling = TaskGroup::new(&parent, TaskGroupMode::Sequence, 1).unwrap();
    let handle = group.handle(1).unwrap();
    let worker_scope = handle.scope().clone();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(worker_scope.cancelled());
        42
    });
    group
        .complete(&group.handle(0).unwrap(), TaskTerminal::Failed)
        .unwrap();
    group.cancel();
    assert_eq!(worker.join().unwrap(), 42);
    assert_eq!(group.snapshot().terminal(), Some(TaskTerminal::Cancelled));
    group.snapshot().validate().unwrap();
    assert!(group.complete(&handle, TaskTerminal::Completed).is_err());
    assert!(sibling.handle(0).is_ok());
    parent.cancel();
    assert_eq!(sibling.snapshot().terminal(), Some(TaskTerminal::Cancelled));
}

#[test]
fn corrupt_progress_is_rejected_without_cancelling_current_work() {
    let parent = TaskScope::new();
    let current = TaskGroup::new(&parent, TaskGroupMode::Sequence, 2).unwrap();
    let handle = current.handle(0).unwrap();
    let mut saved = serde_json::to_value(current.snapshot()).unwrap();
    saved["members"][1] = serde_json::json!("Running");
    let invalid: TaskGroupState = serde_json::from_value(saved).unwrap();
    assert!(TaskGroup::restore(&parent, invalid).is_err());
    assert!(!handle.scope().is_cancelled());
    assert!(TaskGroup::new(&parent, TaskGroupMode::All, 0).is_err());
}

#[test]
fn race_completion_keeps_parent_await_valid_and_restore_rejects_old_group() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let token = world
        .create_host_await(AwaitKind::Custom("group".into()))
        .unwrap();
    let scope = world.task_scope();
    let completion = world.await_handle(token, &scope).unwrap();
    let saved = world.save(SaveRequest::default()).unwrap();
    let mut group = TaskGroup::new(&scope, TaskGroupMode::Race, 2).unwrap();
    let late = group.handle(1).unwrap();
    group
        .complete(&group.handle(0).unwrap(), TaskTerminal::Completed)
        .unwrap();
    assert_eq!(completion.status(), TaskStatus::Pending);
    world
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            vec![OrderedTickIngress {
                sequence: 1,
                payload: TickIngress::AwaitCompletion(completion.complete(
                    1,
                    1,
                    EventPayload::new("group.done"),
                )),
            }],
        ))
        .unwrap();
    assert_eq!(completion.status(), TaskStatus::Completed);
    world.load(saved).unwrap();
    assert!(group.complete(&late, TaskTerminal::Completed).is_err());
    assert!(scope.is_cancelled());
    assert!(world.await_handle(token, &world.task_scope()).is_ok());
}
