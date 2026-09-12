use astra_core::StableId;
use astra_runtime::*;

fn token(n: u64) -> AwaitToken {
    AwaitToken {
        token_id: AwaitTokenId(StableId::deterministic_v7(1, n, 0)),
        kind: AwaitKind::Custom("worker".into()),
        requested_at_step: 0,
        timeout_step: None,
        completion_policy: AwaitCompletionPolicy::HostResult,
    }
}

fn world() -> RuntimeWorld {
    RuntimeWorld::create(RuntimeConfig::default()).unwrap()
}

fn request(step: u64, restored: bool, completions: Vec<AwaitCompletion>) -> TickRequest {
    let timing = TickInput {
        fixed_step: step,
        delta_ns: 16_666_667,
        seed: 0,
    };
    let ingress = completions
        .into_iter()
        .enumerate()
        .map(|(index, completion)| OrderedTickIngress {
            sequence: index as u64 + 1,
            payload: TickIngress::AwaitCompletion(completion),
        })
        .collect();
    if restored {
        TickRequest::restore_continuation(timing, ingress)
    } else {
        TickRequest::live(timing, ingress)
    }
}

fn result(handle: &AwaitCompletionHandle, step: u64) -> AwaitCompletion {
    handle.complete(1, step, EventPayload::new("worker.done"))
}

#[test]
fn worker_started_before_restore_cannot_complete_the_restored_token() {
    let mut world = world();
    let token = token(1);
    world.insert_await_token(token.clone()).unwrap();
    let handle = world
        .await_handle(token.token_id, &world.task_scope())
        .unwrap();
    let saved = world.save(SaveRequest::default()).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_handle = handle.clone();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        result(&worker_handle, 1)
    });
    started_rx.recv().unwrap();
    world.load(saved).unwrap();
    assert_eq!(handle.status(), TaskStatus::Cancelled);
    release_tx.send(()).unwrap();
    let late = worker.join().unwrap();
    let report = world.tick(request(1, true, vec![late])).unwrap();
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code == "ASTRA_AWAIT_RESULT_STALE"));
    assert_eq!(
        world.snapshot().unwrap().awaits.pending(),
        std::slice::from_ref(&token)
    );
    assert!(world.debug_session().event_trace().is_empty());
    let resumed = world
        .await_handle(token.token_id, &world.task_scope())
        .unwrap();
    world
        .tick(request(2, false, vec![result(&resumed, 2)]))
        .unwrap();
    assert_eq!(resumed.status(), TaskStatus::Completed);
    assert!(world.snapshot().unwrap().awaits.pending().is_empty());
}

#[test]
fn rejected_restore_preserves_running_handles() {
    let mut world = world();
    let token = token(1);
    world.insert_await_token(token.clone()).unwrap();
    let handle = world
        .await_handle(token.token_id, &world.task_scope())
        .unwrap();
    let save = world.save(SaveRequest::default()).unwrap();
    assert!(world
        .load_with_validation(
            save,
            &astra_core::SchemaMigrationRegistry::default(),
            |_| { Err::<(), _>(RuntimeError::message("host rejected typed state")) }
        )
        .is_err());
    assert_eq!(handle.status(), TaskStatus::Pending);
    world
        .tick(request(1, false, vec![result(&handle, 1)]))
        .unwrap();
    assert_eq!(handle.status(), TaskStatus::Completed);
}

#[test]
fn cancelling_parent_removes_queued_results_but_preserves_sibling_scope() {
    let mut world = world();
    let parent = world.task_scope().child();
    let child = parent.child();
    let sibling = world.task_scope().child();
    let a = token(1);
    let b = token(2);
    world.insert_await_token(a.clone()).unwrap();
    world.insert_await_token(b.clone()).unwrap();
    let ha = world.await_handle(a.token_id, &child).unwrap();
    let hb = world.await_handle(b.token_id, &sibling).unwrap();
    world.tick(request(1, false, vec![result(&ha, 3)])).unwrap();
    parent.cancel();
    assert_eq!(ha.status(), TaskStatus::Cancelled);
    assert_eq!(hb.status(), TaskStatus::Pending);
    world.tick(request(2, false, vec![])).unwrap();
    assert_eq!(world.snapshot().unwrap().awaits.pending(), &[b]);
    world
        .tick(request(3, false, vec![result(&ha, 3), result(&hb, 3)]))
        .unwrap();
    let events = world.debug_session().event_trace();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.payload.kind == "await.cancelled")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.payload.kind == "worker.done")
            .count(),
        1
    );
    assert_eq!(hb.status(), TaskStatus::Completed);
}

#[test]
fn foreign_world_and_cancelled_token_handles_cannot_be_rebound() {
    let mut first = world();
    let mut second = world();
    let token = token(1);
    first.insert_await_token(token.clone()).unwrap();
    second.insert_await_token(token.clone()).unwrap();
    let handle = first
        .await_handle(token.token_id, &first.task_scope())
        .unwrap();
    assert!(second
        .await_handle(token.token_id, &first.task_scope())
        .is_err());
    second
        .tick(request(1, false, vec![result(&handle, 1)]))
        .unwrap();
    assert_eq!(
        second.snapshot().unwrap().awaits.pending(),
        std::slice::from_ref(&token)
    );
    assert!(second.debug_session().event_trace().is_empty());
    assert!(first.cancel_await(token.token_id).unwrap());
    assert!(!first.cancel_await(token.token_id).unwrap());
    first.insert_await_token(token.clone()).unwrap();
    let replacement = first
        .await_handle(token.token_id, &first.task_scope())
        .unwrap();
    first
        .tick(request(1, false, vec![result(&handle, 1)]))
        .unwrap();
    assert_eq!(replacement.status(), TaskStatus::Pending);
    assert_eq!(first.snapshot().unwrap().awaits.pending(), &[token]);
}

#[test]
fn completed_result_is_terminal_even_when_later_sequence_arrives() {
    let mut world = world();
    let token = token(1);
    world.insert_await_token(token.clone()).unwrap();
    let handle = world
        .await_handle(token.token_id, &world.task_scope())
        .unwrap();
    world
        .tick(request(
            1,
            false,
            vec![
                result(&handle, 1),
                handle.complete(2, 2, EventPayload::new("worker.second")),
            ],
        ))
        .unwrap();
    assert_eq!(handle.status(), TaskStatus::Completed);
    world
        .tick(request(
            2,
            false,
            vec![handle.complete(3, 2, EventPayload::new("worker.third"))],
        ))
        .unwrap();
    assert_eq!(world.debug_session().event_trace().len(), 1);
}

#[test]
fn destroying_world_cancels_workers_and_scope_binding_is_unique() {
    let mut world = world();
    let token = token(1);
    world.insert_await_token(token.clone()).unwrap();
    let scope = world.task_scope().child();
    let handle = world.await_handle(token.token_id, &scope).unwrap();
    assert!(world
        .await_handle(token.token_id, &world.task_scope())
        .is_err());
    assert_eq!(world.await_handle(token.token_id, &scope).unwrap(), handle);
    drop(world);
    assert!(scope.is_cancelled());
    assert_eq!(handle.status(), TaskStatus::Cancelled);
}

#[test]
fn saved_committed_results_survive_restore_without_worker_handles() {
    let mut world = world();
    let token = token(1);
    world.insert_await_token(token.clone()).unwrap();
    let handle = world
        .await_handle(token.token_id, &world.task_scope())
        .unwrap();
    world
        .tick(request(1, false, vec![result(&handle, 2)]))
        .unwrap();
    let saved = world.save(SaveRequest::default()).unwrap();
    world.load(saved).unwrap();
    assert_eq!(handle.status(), TaskStatus::Cancelled);
    assert!(world
        .await_handle(token.token_id, &world.task_scope())
        .unwrap_err()
        .to_string()
        .contains("ASTRA_AWAIT_RESULT_PENDING"));
    world.tick(request(2, true, vec![])).unwrap();
    assert!(world.snapshot().unwrap().awaits.pending().is_empty());
    assert_eq!(
        world
            .debug_session()
            .event_trace()
            .iter()
            .filter(|e| e.payload.kind == "worker.done")
            .count(),
        1
    );
}

#[test]
fn duplicate_saved_terminal_results_are_rejected_without_cancelling_current_work() {
    let mut world = world();
    let token = token(1);
    world.insert_await_token(token.clone()).unwrap();
    let handle = world
        .await_handle(token.token_id, &world.task_scope())
        .unwrap();
    world
        .tick(request(1, false, vec![result(&handle, 3)]))
        .unwrap();
    let before = world.save(SaveRequest::default()).unwrap();
    let mut snapshot = world.snapshot().unwrap();
    let mut queue = serde_json::to_value(&snapshot.awaits).unwrap();
    let completions = queue["completed"].as_array_mut().unwrap();
    completions.push(completions[0].clone());
    snapshot.awaits = serde_json::from_value(queue).unwrap();
    let corrupt = write_runtime_save(snapshot, SaveRequest::default()).unwrap();
    assert!(world
        .load(corrupt)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_AWAIT_SAVE_RESULT_INVALID"));
    assert_eq!(handle.status(), TaskStatus::Pending);
    assert_eq!(world.save(SaveRequest::default()).unwrap().0, before.0);
    world.tick(request(2, false, vec![])).unwrap();
    world.tick(request(3, false, vec![])).unwrap();
    assert_eq!(handle.status(), TaskStatus::Completed);
    assert_eq!(world.debug_session().event_trace().len(), 1);
}
