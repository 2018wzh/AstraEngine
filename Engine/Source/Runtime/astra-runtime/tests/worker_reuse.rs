use astra_core::StableId;
use astra_runtime::{
    ActionAccess, ActionDescriptor, ActionExecutionClass, ActionInvocation, ActionResourceKey,
    ActionTrace, BlackboardValue, DeterministicActionContext, EventPayload, EventSource, GuardExpr,
    RuntimeAction, RuntimeConfig, RuntimeError, RuntimeWorld, StateDefinition,
    StateMachineDefinition, TickInput, TickRequest, TransitionDefinition,
};
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    thread::ThreadId,
};

struct RecordWorker {
    id: &'static str,
    workers: Arc<Mutex<(usize, HashSet<ThreadId>)>>,
}
impl RuntimeAction for RecordWorker {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            self.id,
            "test.worker.input",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new([], [ActionResourceKey::BlackboardKey(self.id.into())]),
            0,
        )
    }
    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        _: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let mut workers = self.workers.lock();
        workers.0 += 1;
        workers.1.insert(std::thread::current().id());
        ctx.set_blackboard(self.id, BlackboardValue::I64(1));
        Ok(ActionTrace {
            action_id: self.id.into(),
            payload: BTreeMap::new(),
        })
    }
}

#[test]
fn repeated_parallel_ticks_reuse_the_configured_workers() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world.set_machine_worker_count(2).unwrap();
    let workers = Arc::new(Mutex::new((0, HashSet::new())));
    let owner = world.create_actor("worker-owner", vec![]).unwrap();
    for (index, action_id) in ["test.worker.a", "test.worker.b"].into_iter().enumerate() {
        world
            .register_action(
                "test.workers",
                RecordWorker {
                    id: action_id,
                    workers: workers.clone(),
                },
            )
            .unwrap();
        let state = StableId::deterministic_v7(7, index as u64, 1);
        world
            .add_state_machine(StateMachineDefinition {
                id: StableId::deterministic_v7(7, index as u64, 2),
                owner,
                states: vec![StateDefinition {
                    id: state,
                    name: "active".into(),
                    terminal: false,
                }],
                transitions: vec![TransitionDefinition {
                    from: state,
                    to: state,
                    guard: GuardExpr::EventIs {
                        kind: "worker.tick".into(),
                    },
                    actions: vec![ActionInvocation {
                        action_id: action_id.into(),
                        input: BTreeMap::new(),
                    }],
                    priority: 0,
                    source_ref: None,
                }],
                initial_state: state,
            })
            .unwrap();
    }
    for fixed_step in 1..=32 {
        world
            .emit_event(EventSource::Scenario, EventPayload::new("worker.tick"))
            .unwrap();
        world
            .tick(TickRequest::live(
                TickInput {
                    fixed_step,
                    delta_ns: 16_666_667,
                    seed: 0,
                },
                vec![],
            ))
            .unwrap();
    }
    let workers = workers.lock();
    assert_eq!(workers.0, 64);
    assert!(
        workers.1.len() <= 2,
        "worker count grew across ticks: {}",
        workers.1.len()
    );
}
