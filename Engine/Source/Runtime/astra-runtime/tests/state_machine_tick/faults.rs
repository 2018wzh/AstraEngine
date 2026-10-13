use super::*;

#[test]
pub(super) fn terminal_state_marks_machine_completed_and_blocks_future_ticks() {
    let mut world = RuntimeWorld::create(RuntimeConfig {
        seed: 11,
        ..RuntimeConfig::default()
    })
    .unwrap();
    let actor = world.create_actor("system", vec![]).unwrap();
    let start = StableId::deterministic_v7(6, 1, 11);
    let done = StableId::deterministic_v7(6, 2, 11);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(6, 3, 11),
            owner: actor,
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
                    action_id: "astra.core.set_blackboard".to_string(),
                    input: set_blackboard_input("terminal", "hit"),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();

    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 11,
            },
            Vec::new(),
        ))
        .unwrap();
    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 11,
            },
            Vec::new(),
        ))
        .unwrap();

    let machines = world.debug_session().state_machines(actor);
    assert_eq!(machines[0].current_state, done);
    assert!(machines[0].completed);
    let snapshot = world.snapshot().unwrap();
    let trace: Vec<_> = snapshot
        .machines
        .trace()
        .iter()
        .map(|trace| trace.action_id.as_str())
        .collect();
    assert_eq!(trace, vec!["astra.core.set_blackboard"]);
}

#[test]
pub(super) fn state_machine_runs_transitions_until_it_reaches_a_stable_state() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let actor = world.create_actor("stable", vec![]).unwrap();
    let start = StableId::deterministic_v7(7, 1, 11);
    let middle = StableId::deterministic_v7(7, 2, 11);
    let done = StableId::deterministic_v7(7, 3, 11);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(7, 4, 11),
            owner: actor,
            states: vec![
                StateDefinition {
                    id: start,
                    name: "start".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: middle,
                    name: "middle".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: done,
                    name: "done".to_string(),
                    terminal: true,
                },
            ],
            transitions: vec![
                TransitionDefinition {
                    from: start,
                    to: middle,
                    guard: GuardExpr::Always,
                    actions: vec![],
                    priority: 0,
                    source_ref: None,
                },
                TransitionDefinition {
                    from: middle,
                    to: done,
                    guard: GuardExpr::Always,
                    actions: vec![],
                    priority: 0,
                    source_ref: None,
                },
            ],
            initial_state: start,
        })
        .unwrap();

    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            Vec::new(),
        ))
        .unwrap();

    let machine = world.debug_session().state_machines(actor).remove(0);
    assert_eq!(machine.current_state, done);
    assert!(machine.completed);
}

#[test]
pub(super) fn evidence_state_machine_cycle_uses_microstep_budget_without_live_hash_guard() {
    let (mut world, actor) = cycle_world(TickIntegrityMode::Evidence);
    let error = world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            Vec::new(),
        ))
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_STATE_MACHINE_BUDGET"));
    assert!(world.is_failed());
    assert_eq!(
        world.debug_session().state_machines(actor)[0].current_state,
        StableId::deterministic_v7(8, 1, 11)
    );
    assert_eq!(world.debug_session().blackboard().get("cycle"), None);
}

#[test]
pub(super) fn shipping_state_machine_cycle_uses_microstep_budget_without_hash_guard() {
    let (mut world, actor) = cycle_world(TickIntegrityMode::Shipping);
    let error = world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            Vec::new(),
        ))
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_STATE_MACHINE_BUDGET"));
    assert!(world.is_failed());
    assert_eq!(
        world.debug_session().state_machines(actor)[0].current_state,
        StableId::deterministic_v7(8, 1, 11)
    );
    assert_eq!(world.debug_session().blackboard().get("cycle"), None);
}

pub(super) fn cycle_world(
    integrity_mode: TickIntegrityMode,
) -> (RuntimeWorld, astra_runtime::ActorId) {
    let mut world =
        RuntimeWorld::create_with_integrity(RuntimeConfig::default(), integrity_mode).unwrap();
    let actor = world.create_actor("cycle", vec![]).unwrap();
    let left = StableId::deterministic_v7(8, 1, 11);
    let right = StableId::deterministic_v7(8, 2, 11);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(8, 3, 11),
            owner: actor,
            states: vec![
                StateDefinition {
                    id: left,
                    name: "left".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: right,
                    name: "right".to_string(),
                    terminal: false,
                },
            ],
            transitions: vec![
                TransitionDefinition {
                    from: left,
                    to: right,
                    guard: GuardExpr::Always,
                    actions: vec![ActionInvocation {
                        action_id: "astra.core.set_blackboard".to_string(),
                        input: set_blackboard_input("cycle", "partial"),
                    }],
                    priority: 0,
                    source_ref: None,
                },
                TransitionDefinition {
                    from: right,
                    to: left,
                    guard: GuardExpr::Always,
                    actions: vec![],
                    priority: 0,
                    source_ref: None,
                },
            ],
            initial_state: left,
        })
        .unwrap();
    (world, actor)
}

#[test]
pub(super) fn action_registry_rejects_duplicate_action_ids() {
    let mut registry = ActionRegistry::default();
    registry.register(SetBlackboardAction).unwrap();

    let error = registry
        .register_with_provider("astra.other", SetBlackboardAction)
        .unwrap_err();

    assert!(error.to_string().contains("ASTRA_RUNTIME_ACTION_CONFLICT"));
}

pub(super) fn set_blackboard_input(key: &str, value: &str) -> BTreeMap<String, BlackboardValue> {
    let mut input = BTreeMap::new();
    input.insert("key".to_string(), BlackboardValue::from(key));
    input.insert("value".to_string(), BlackboardValue::from(value));
    input
}
