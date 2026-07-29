use std::collections::BTreeMap;

use astra_core::StableId;
use astra_runtime::{
    ActionAccess, ActionDescriptor, ActionExecutionClass, ActionInvocation, ActionResourceKey,
    ActionTrace, BlackboardValue, DeterministicActionContext, GuardExpr, RuntimeAction,
    RuntimeConfig, RuntimeError, RuntimeWorld, StateDefinition, StateMachineDefinition, TickInput,
    TransitionDefinition,
};

struct InvalidPureAction;

impl RuntimeAction for InvalidPureAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.test.invalid_pure",
            "astra.test.input.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelPure,
            ActionAccess::new([], [ActionResourceKey::Blackboard]),
            0,
        )
    }

    fn run(
        &self,
        _ctx: &mut DeterministicActionContext<'_>,
        _input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        unreachable!("invalid descriptor must fail before installation")
    }
}

struct ExcessStableIdAction;

impl RuntimeAction for ExcessStableIdAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.test.excess_ids",
            "astra.test.input.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::Serial,
            ActionAccess::new([], [ActionResourceKey::StableIdSource]),
            1,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let _ = ctx.next_id();
        let _ = ctx.next_id();
        Ok(ActionTrace {
            action_id: "astra.test.excess_ids".to_string(),
            payload: input.clone(),
        })
    }
}

struct UndeclaredWriteAction;

impl RuntimeAction for UndeclaredWriteAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.test.undeclared_write",
            "astra.test.input.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new([], []),
            0,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        ctx.set_blackboard("undeclared", true.into());
        Ok(ActionTrace {
            action_id: "astra.test.undeclared_write".to_string(),
            payload: input.clone(),
        })
    }
}

#[astra_headless_test::test]
fn action_registration_rejects_invalid_parallel_pure_access() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default(), Default::default()).unwrap();
    let error = world
        .register_action("astra.test", InvalidPureAction)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_ACTION_ACCESS_INVALID"));
}

#[astra_headless_test::test]
fn action_execution_rolls_back_when_stable_id_reservation_is_exceeded() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default(), Default::default()).unwrap();
    world
        .register_action("astra.test", ExcessStableIdAction)
        .unwrap();
    let owner = world.create_actor("owner", vec![]);
    let start = StableId::deterministic_v7(9, 1, 1);
    let done = StableId::deterministic_v7(9, 1, 2);
    let machine_id = StableId::deterministic_v7(9, 1, 3);
    world
        .add_state_machine(StateMachineDefinition {
            id: machine_id,
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
                    action_id: "astra.test.excess_ids".to_string(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();

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

    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_ACTION_ID_RESERVATION_EXCEEDED"));
    let snapshot = world
        .debug_session()
        .state_machines(owner)
        .into_iter()
        .find(|machine| machine.id == machine_id)
        .unwrap();
    assert_eq!(snapshot.current_state, start);
    assert!(!snapshot.completed);
}

#[astra_headless_test::test]
fn action_execution_rolls_back_undeclared_access() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default(), Default::default()).unwrap();
    world
        .register_action("astra.test", UndeclaredWriteAction)
        .unwrap();
    let owner = world.create_actor("owner", vec![]);
    let start = StableId::deterministic_v7(9, 2, 1);
    let done = StableId::deterministic_v7(9, 2, 2);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(9, 2, 3),
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
                    action_id: "astra.test.undeclared_write".to_string(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();
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
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_ACTION_ACCESS_UNDECLARED"));
    assert!(!world
        .debug_session()
        .blackboard()
        .contains_key("undeclared"));
}
