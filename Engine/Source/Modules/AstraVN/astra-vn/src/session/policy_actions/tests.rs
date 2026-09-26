use super::*;
use astra_vn_policy::{PolicyIoCapabilities, PolicyIoFailure, PolicyIoHost, PolicyReadText};
use std::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
const POLICY: &str = r#"
astra.command.register("note", {schema="demo.note.v1",fields={}}, function(args)
    astra.mutate.set_var("project", "count", (astra.var.get("project","count") or 0) + 1)
    local text,status = astra.host.read_text("note", "asset:/text/note")
    if status == "pending" then return astra.action.await("note") end
    if status == "ready" then assert(text == "hello") else assert(status == "interrupted") end
    return astra.action.complete()
end)
"#;
struct Host {
    blocked: bool,
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
struct Guard(Arc<AtomicUsize>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl PolicyIoHost for Host {
    fn read_text(
        &self,
        _: PolicyReadText,
    ) -> Pin<Box<dyn Future<Output = Result<String, PolicyIoFailure>> + Send>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let blocked = self.blocked;
        let guard = Guard(self.drops.clone());
        Box::pin(async move {
            let _guard = guard;
            if blocked {
                std::future::pending::<()>().await;
            }
            Ok("hello".into())
        })
    }
}
fn setup(blocked: bool) -> (VnSession, Arc<Host>) {
    let descriptor = LuauPolicy::action_descriptors(POLICY, Default::default())
        .unwrap()
        .remove(0);
    let binding = PolicyActionBinding {
        descriptor: descriptor.clone(),
        entry: "Policies/note.luau".into(),
        source: POLICY.into(),
        source_hash: astra_core::Hash256::from_sha256(POLICY.as_bytes()).to_string(),
        read_text: vec!["asset:/text/note".parse().unwrap()],
    };
    let compiled = compile_astra_project([AstraSource::story("actions.astra", "story main #@id main\nstate start #@id start\n  scene room #@id room\n    mutate project.count = 0 #@id init\n    jump loop #@id next\nstate loop #@id loop\n  scene again #@id again\n    note #@id note.load\n    jump loop #@id repeat\n"), AstraSource::story("system.astra", "story system.config #@id system.config\n  scene config #@id config\n    system_page kind:config policy:astra.policy.standard #@id page.config\n")], CompileAstraProjectOptions::default().bind_extension(descriptor)).unwrap();
    let mut session = VnSession::new(
        Arc::new(compiled.into()),
        VnRunConfig::classic("en"),
        VnSessionConfig {
            target_id: "policy".into(),
            seed: 41,
            package: None,
            integrity_mode: TickIntegrityMode::Shipping,
            worker_count: 1,
        },
    )
    .unwrap();
    let host = Arc::new(Host {
        blocked,
        calls: Arc::new(AtomicUsize::new(0)),
        drops: Arc::new(AtomicUsize::new(0)),
    });
    session.bind_policy_actions(vec![binding]).unwrap();
    session
        .bind_policy_io(
            host.clone(),
            PolicyIoCapabilities::read_text(["asset:/text/note".parse().unwrap()], 64).unwrap(),
        )
        .unwrap();
    (session, host)
}
fn tick(session: &mut VnSession, step: u64, mode: astra_runtime::TickMode) {
    session
        .step(NativeVnStepInput {
            timing: TickInput {
                fixed_step: step,
                delta_ns: 16_666_667,
                seed: 41,
            },
            mode,
            command: if step == 1 {
                NativeVnStepCommand::LaunchDefault
            } else {
                NativeVnStepCommand::PollPolicy
            },
        })
        .unwrap();
}
fn wait_calls(host: &Host, calls: usize) {
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while host.calls.load(Ordering::SeqCst) < calls {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn automatic_action_ticks_complete_and_repeat_source_without_old_results() {
    let (mut session, host) = setup(false);
    tick(&mut session, 1, astra_runtime::TickMode::Live);
    for step in 2..100 {
        tick(&mut session, step, astra_runtime::TickMode::Live);
        if session.runtime.state().variables["project"]["count"] == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(session.runtime.state().variables["project"]["count"], 2);
    assert_eq!(host.calls.load(Ordering::SeqCst), 2);
    assert!(session.policy_io_snapshot().unwrap().requests.is_empty());
    let save = session.save().unwrap();
    session.restore(save).unwrap();
    session.save().unwrap();
}
#[test]
fn pending_restore_interrupts_and_rejects_corrupt_candidate_without_stopping_live_io() {
    let (mut session, host) = setup(true);
    tick(&mut session, 1, astra_runtime::TickMode::Live);
    tick(&mut session, 2, astra_runtime::TickMode::Live);
    wait_calls(&host, 1);
    assert_eq!(session.runtime.state().variables["project"]["count"], 0);
    let save = session.save().unwrap();
    let mut snapshot = astra_runtime::read_runtime_save(&save, &Default::default()).unwrap();
    let component = snapshot
        .actors
        .component_mut(session.policy_component)
        .unwrap();
    let mut policy: super::super::policy_io::SessionPolicyState =
        component.payload.decode().unwrap();
    policy.action.as_mut().unwrap().instance += 1;
    component.payload = component.payload.replaced(policy).unwrap();
    let corrupt = astra_runtime::write_runtime_save(snapshot, SaveRequest::default()).unwrap();
    assert!(session.restore(corrupt).is_err());
    assert_eq!(host.drops.load(Ordering::SeqCst), 0);
    let mut snapshot = astra_runtime::read_runtime_save(&save, &Default::default()).unwrap();
    let component = snapshot
        .actors
        .component_mut(session.policy_component)
        .unwrap();
    let mut policy: super::super::policy_io::SessionPolicyState =
        component.payload.decode().unwrap();
    policy
        .io
        .requests
        .values_mut()
        .next()
        .unwrap()
        .request
        .resource = "asset:/text/denied".parse().unwrap();
    component.payload = component.payload.replaced(policy).unwrap();
    let denied = astra_runtime::write_runtime_save(snapshot, SaveRequest::default()).unwrap();
    assert!(session.restore(denied).is_err());
    assert_eq!(host.drops.load(Ordering::SeqCst), 0);
    let mut snapshot = astra_runtime::read_runtime_save(&save, &Default::default()).unwrap();
    snapshot
        .actors
        .component_mut(session.policy_component)
        .unwrap()
        .payload = astra_runtime::RuntimeComponentPayload::typed(
        "astra.vn.session_policy.v1",
        Default::default(),
        (),
    );
    let old = astra_runtime::write_runtime_save(snapshot, SaveRequest::default()).unwrap();
    assert!(session
        .restore(old)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_POLICY_RESTORE_VERSION"));
    assert_eq!(host.drops.load(Ordering::SeqCst), 0);
    session.restore(save).unwrap();
    assert_eq!(host.drops.load(Ordering::SeqCst), 1);
    tick(
        &mut session,
        3,
        astra_runtime::TickMode::RestoreContinuation,
    );
    assert_eq!(session.runtime.state().variables["project"]["count"], 1);
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
    session.save().unwrap();
}
#[test]
fn closing_action_wait_joins_blocked_host() {
    let (mut session, host) = setup(true);
    tick(&mut session, 1, astra_runtime::TickMode::Live);
    tick(&mut session, 2, astra_runtime::TickMode::Live);
    wait_calls(&host, 1);
    session.close();
    assert_eq!(host.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn system_page_pauses_action_and_restored_return_does_not_dispatch_again() {
    let (mut session, host) = setup(true);
    tick(&mut session, 1, astra_runtime::TickMode::Live);
    tick(&mut session, 2, astra_runtime::TickMode::Live);
    wait_calls(&host, 1);
    session
        .step(NativeVnStepInput {
            timing: TickInput {
                fixed_step: 3,
                delta_ns: 16_666_667,
                seed: 41,
            },
            mode: astra_runtime::TickMode::Live,
            command: NativeVnStepCommand::Execute(CoreVnPlayerCommand::OpenSystem {
                page: astra_vn_core::SystemPageKind::Config,
            }),
        })
        .unwrap();
    assert!(!session.policy_action_pending().unwrap());
    assert_eq!(session.runtime.state().variables["project"]["count"], 0);
    let save = session.save().unwrap();
    session.restore(save).unwrap();
    assert!(!session.policy_action_pending().unwrap());
    session
        .step(NativeVnStepInput {
            timing: TickInput {
                fixed_step: 4,
                delta_ns: 16_666_667,
                seed: 41,
            },
            mode: astra_runtime::TickMode::RestoreContinuation,
            command: NativeVnStepCommand::Execute(CoreVnPlayerCommand::ReturnSystem),
        })
        .unwrap();
    assert!(session.policy_action_pending().unwrap());
    tick(&mut session, 5, astra_runtime::TickMode::Live);
    assert_eq!(session.runtime.state().variables["project"]["count"], 1);
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.drops.load(Ordering::SeqCst), 1);
    session.save().unwrap();
}
