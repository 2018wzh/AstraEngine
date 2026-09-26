use super::*;
use astra_vn_policy::{PolicyIoFailure, PolicyReadText, PolicySnapshotValue};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Mutex,
    },
    time::Duration,
};

struct TestHost {
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    started: mpsc::Sender<()>,
    result: Mutex<Option<tokio::sync::oneshot::Receiver<String>>>,
}
struct Guard(Arc<AtomicUsize>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl PolicyIoHost for TestHost {
    fn read_text(
        &self,
        request: PolicyReadText,
    ) -> Pin<Box<dyn Future<Output = Result<String, PolicyIoFailure>> + Send>> {
        assert_eq!(request.resource.as_str(), "package:/policy/info.txt");
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.result.lock().unwrap().take().unwrap();
        let guard = Guard(Arc::clone(&self.drops));
        let started = self.started.clone();
        Box::pin(async move {
            let _guard = guard;
            started.send(()).unwrap();
            result.await.map_err(|_| PolicyIoFailure::Host)
        })
    }
}
fn session() -> VnSession {
    let compiled = compile_astra_project([AstraSource::story("io.astra", "story main #@id main\nstate start #@id start\n  scene room #@id room\n    text key:hello speaker:narrator #@id hello\n")], Default::default()).unwrap();
    VnSession::new(
        Arc::new(compiled.into()),
        VnRunConfig::classic("en"),
        VnSessionConfig {
            target_id: "io".into(),
            seed: 41,
            package: None,
            integrity_mode: TickIntegrityMode::Shipping,
            worker_count: 1,
        },
    )
    .unwrap()
}
fn tick(session: &mut VnSession, fixed_step: u64, mode: astra_runtime::TickMode) {
    session
        .step(NativeVnStepInput {
            timing: TickInput {
                fixed_step,
                delta_ns: 16_666_667,
                seed: 41,
            },
            mode,
            command: if fixed_step == 1 {
                NativeVnStepCommand::LaunchDefault
            } else {
                NativeVnStepCommand::Execute(CoreVnPlayerCommand::SetAuto { enabled: false })
            },
        })
        .unwrap();
}
const READ: &str = "local text, status = astra.host.read_text('config', 'package:/policy/info.txt'); if status == 'ready' then astra.snapshot.set('text', text) end; return status == 'ready'";
fn bind(
    session: &mut VnSession,
) -> (
    Arc<TestHost>,
    mpsc::Receiver<()>,
    tokio::sync::oneshot::Sender<String>,
) {
    let (started, received) = mpsc::channel();
    let (send, result) = tokio::sync::oneshot::channel();
    let host = Arc::new(TestHost {
        calls: Arc::new(AtomicUsize::new(0)),
        drops: Arc::new(AtomicUsize::new(0)),
        started,
        result: Mutex::new(Some(result)),
    });
    session
        .bind_policy_io(
            host.clone(),
            PolicyIoCapabilities::read_text(["package:/policy/info.txt".parse().unwrap()], 64)
                .unwrap(),
        )
        .unwrap();
    (host, received, send)
}
fn evaluate(session: &mut VnSession, source: &str) -> bool {
    session
        .evaluate_policy(source, PolicyExecutionBudget::default())
        .unwrap()
}
fn wait_drop(host: &TestHost) {
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while host.drops.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    // The result may still be crossing the bounded channel; tick polling below handles this.
}

#[test]
fn completed_io_commits_at_tick_and_survives_cold_restore_without_reissuing() {
    let mut session = session();
    let (host, started, send) = bind(&mut session);
    assert!(!evaluate(&mut session, READ));
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        session
            .engine
            .world()
            .snapshot()
            .unwrap()
            .awaits
            .pending()
            .len(),
        1
    );
    send.send("hello from host".into()).unwrap();
    wait_drop(&host);
    assert!(!evaluate(&mut session, READ)); // worker never mutates authoritative state
    for step in 1..=100 {
        tick(&mut session, step, astra_runtime::TickMode::Live);
        if evaluate(&mut session, READ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        session.policy_snapshot().unwrap().snapshot("text"),
        Some(&PolicySnapshotValue::String("hello from host".into()))
    );
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
    let revision = session
        .engine
        .world()
        .snapshot()
        .unwrap()
        .actors
        .component(session.policy_component)
        .unwrap()
        .payload
        .revision();
    let saved = session.save().unwrap();
    let mut cold = self::session();
    cold.restore(saved).unwrap();
    assert_eq!(
        cold.engine
            .world()
            .snapshot()
            .unwrap()
            .actors
            .component(cold.policy_component)
            .unwrap()
            .payload
            .revision(),
        revision
    );
    assert!(evaluate(&mut cold, READ)); // saved result remains readable without a host grant
    let resaved = cold.save().unwrap();
    cold.restore(resaved).unwrap();
    assert!(evaluate(&mut cold, READ));
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn closing_blocked_io_cancels_and_joins_worker_before_returning() {
    let mut session = session();
    let (host, started, _send) = bind(&mut session);
    evaluate(&mut session, READ);
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    session.close();
    assert_eq!(host.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn restore_interrupts_pending_io_rejects_late_completion_and_preserves_failed_candidates() {
    let mut session = session();
    let (host, started, send) = bind(&mut session);
    evaluate(&mut session, READ);
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    let saved = session.save().unwrap();
    let old_scope = session.engine.world().task_scope();
    let mut invalid =
        astra_runtime::read_runtime_save(&saved, &astra_core::SchemaMigrationRegistry::default())
            .unwrap();
    let component = invalid
        .actors
        .component_mut(session.policy_component)
        .unwrap();
    let mut policy: SessionPolicyState = component.payload.decode().unwrap();
    policy.io.next_id = 0;
    component.payload =
        RuntimeComponentPayload::typed(POLICY_STATE_SCHEMA, SchemaVersion::default(), policy);
    let invalid = astra_runtime::write_runtime_save(invalid, SaveRequest::default()).unwrap();
    assert!(session.validate_save(&invalid).is_err());
    assert!(session.restore(invalid).is_err());
    assert!(!old_scope.is_cancelled());
    assert_eq!(host.drops.load(Ordering::SeqCst), 0);
    send.send("late result".into()).unwrap();
    wait_drop(&host);
    session.restore(saved).unwrap();
    assert!(old_scope.is_cancelled());
    assert_eq!(host.drops.load(Ordering::SeqCst), 1);
    assert!(evaluate(&mut session, "local text, status = astra.host.read_text('config', 'package:/policy/info.txt'); return text == nil and status == 'interrupted'"));
    assert!(session
        .engine
        .world()
        .snapshot()
        .unwrap()
        .awaits
        .pending()
        .is_empty());
    let saved_again = session.save().unwrap();
    session.restore(saved_again).unwrap();
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn capability_denial_and_failed_eval_do_not_dispatch_or_commit_requests() {
    let mut session = session();
    assert!(session
        .evaluate_policy(READ, PolicyExecutionBudget::default())
        .unwrap_err()
        .to_string()
        .contains("HOST_IO_DENIED"));
    let (host, _started, _send) = bind(&mut session);
    assert!(session
        .evaluate_policy(
            "astra.host.read_text('config', 'package:/policy/info.txt'); error('abort')",
            PolicyExecutionBudget::default()
        )
        .is_err());
    assert!(session.policy_io_snapshot().unwrap().requests.is_empty());
    assert_eq!(host.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn explicit_cancel_drops_work_and_release_does_not_reuse_request_identity() {
    let mut session = session();
    let (host, started, _send) = bind(&mut session);
    evaluate(&mut session, READ);
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(evaluate(
        &mut session,
        "astra.host.cancel('config'); astra.host.release('config'); return true"
    ));
    assert_eq!(host.drops.load(Ordering::SeqCst), 1);
    assert!(session.policy_io_snapshot().unwrap().requests.is_empty());
    assert_eq!(session.policy_io_snapshot().unwrap().next_id, 2);
    assert!(session
        .engine
        .world()
        .snapshot()
        .unwrap()
        .awaits
        .pending()
        .is_empty());
}

#[test]
fn oversized_host_result_is_a_typed_failure_and_is_not_persisted() {
    let mut session = session();
    let (host, started, send) = bind(&mut session);
    evaluate(&mut session, READ);
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    send.send("x".repeat(65)).unwrap();
    wait_drop(&host);
    for step in 1..=100 {
        tick(&mut session, step, astra_runtime::TickMode::Live);
        if session.policy_io_snapshot().unwrap().requests["config"].status
            != PolicyIoStatus::Pending
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        session.policy_io_snapshot().unwrap().requests["config"].status,
        PolicyIoStatus::Failed(PolicyIoFailure::Limit)
    );
    assert!(session
        .policy_snapshot()
        .unwrap()
        .snapshot("text")
        .is_none());
    let save = session.save().unwrap();
    session.restore(save).unwrap();
    assert!(evaluate(&mut session, "local text, status = astra.host.read_text('config', 'package:/policy/info.txt'); return text == nil and status == 'limit'"));
}

#[test]
fn forged_policy_await_binding_does_not_replace_or_cancel_the_live_session() {
    let mut session = session();
    let (host, started, _send) = bind(&mut session);
    evaluate(&mut session, READ);
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    let scope = session.engine.world().task_scope();
    let mut snapshot = materialized_save_snapshot(&session).unwrap();
    let component = snapshot
        .actors
        .component_mut(session.policy_component)
        .unwrap();
    let mut policy: SessionPolicyState = component.payload.decode().unwrap();
    policy.awaits.clear();
    component.payload =
        RuntimeComponentPayload::typed(POLICY_STATE_SCHEMA, SchemaVersion::default(), policy);
    let invalid = astra_runtime::write_runtime_save(snapshot, SaveRequest::default()).unwrap();
    assert!(session
        .restore(invalid)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_POLICY_RESTORE_AWAIT"));
    assert!(!scope.is_cancelled());
    assert_eq!(host.drops.load(Ordering::SeqCst), 0);
    session.close();
    assert_eq!(host.drops.load(Ordering::SeqCst), 1);
}
