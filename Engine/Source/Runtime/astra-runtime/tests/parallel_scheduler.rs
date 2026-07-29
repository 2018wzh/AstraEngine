use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier},
};

use astra_core::StableId;
use astra_runtime::{
    ActionAccess, ActionDescriptor, ActionExecutionClass, ActionInvocation, ActionResourceKey,
    ActionTrace, BlackboardValue, DeterministicActionContext, GuardExpr, RuntimeAction,
    RuntimeConfig, RuntimeError, RuntimeWorld, StateDefinition, StateMachineDefinition, TickInput,
    TransitionDefinition,
};

struct SetKeyAction {
    id: &'static str,
    key: &'static str,
    barrier: Option<Arc<Barrier>>,
}

impl RuntimeAction for SetKeyAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            self.id,
            "astra.test.parallel.input.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new([], [ActionResourceKey::BlackboardKey(self.key.to_string())]),
            0,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        if let Some(barrier) = &self.barrier {
            barrier.wait();
        }
        ctx.set_blackboard(self.key, BlackboardValue::I64(1));
        Ok(ActionTrace {
            action_id: self.id.to_string(),
            payload: input.clone(),
        })
    }
}

fn install_machine(
    world: &mut RuntimeWorld,
    owner: astra_runtime::ActorId,
    machine_ordinal: u64,
    action_id: &str,
) {
    let start = StableId::deterministic_v7(17, machine_ordinal, 1);
    let done = StableId::deterministic_v7(17, machine_ordinal, 2);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(17, machine_ordinal, 3),
            owner,
            states: vec![
                StateDefinition {
                    id: start,
                    name: "start".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: done,
                    name: "done".to_string(),
                    terminal: true,
                },
            ],
            transitions: vec![TransitionDefinition {
                from: start,
                to: done,
                guard: GuardExpr::Always,
                actions: vec![ActionInvocation {
                    action_id: action_id.to_string(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();
}

fn run_world(worker_count: usize, barrier: Option<Arc<Barrier>>) -> astra_runtime::TickReport {
    let mut world = RuntimeWorld::create(RuntimeConfig::default(), Default::default()).unwrap();
    world.set_machine_worker_count(worker_count).unwrap();
    world
        .register_action(
            "astra.test",
            SetKeyAction {
                id: "astra.test.parallel.a",
                key: "parallel.a",
                barrier: barrier.clone(),
            },
        )
        .unwrap();
    world
        .register_action(
            "astra.test",
            SetKeyAction {
                id: "astra.test.parallel.b",
                key: "parallel.b",
                barrier,
            },
        )
        .unwrap();
    let owner = world.create_actor("owner", vec![]);
    install_machine(&mut world, owner, 1, "astra.test.parallel.a");
    install_machine(&mut world, owner, 2, "astra.test.parallel.b");
    let report = world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            vec![],
        ))
        .unwrap();
    assert_eq!(
        world.snapshot().blackboard.get("parallel.a"),
        Some(&BlackboardValue::I64(1))
    );
    assert_eq!(
        world.snapshot().blackboard.get("parallel.b"),
        Some(&BlackboardValue::I64(1))
    );
    report
}

#[astra_headless_test::test]
fn independent_machine_actions_execute_in_the_same_parallel_wave() {
    let report = run_world(2, Some(Arc::new(Barrier::new(2))));
    assert!(report.diagnostics.is_empty());
}

#[astra_headless_test::test]
fn worker_counts_preserve_authoritative_hashes() {
    let baseline = run_world(1, None);
    for worker_count in [2, 4, 8] {
        let report = run_world(worker_count, None);
        assert_eq!(report.state_hash, baseline.state_hash);
        assert_eq!(report.event_hash, baseline.event_hash);
        assert_eq!(report.presentation_hash, baseline.presentation_hash);
    }
}
