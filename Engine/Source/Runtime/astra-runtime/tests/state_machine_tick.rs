use astra_core::StableId;
use astra_runtime::{
    validate_state_machine, ActionInvocation, ActionRegistry, BlackboardValue, EventPayload,
    EventSource, GuardExpr, PackageHandle, PresentationCommand, RuntimeConfig, RuntimeWorld,
    SetBlackboardAction, StateDefinition, StateMachineDefinition, StateMachineValidationReport,
    TickInput, TransitionDefinition,
};
use std::collections::BTreeMap;

#[test]
fn state_machine_tick_repeats_hash_for_same_seed_and_input() {
    let left = run_once();
    let right = run_once();
    assert_eq!(left, right);
}

fn run_once() -> astra_runtime::TickReport {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 11,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    let actor = world.create_actor("system", vec!["runtime".to_string()]);
    let start = StableId::deterministic_v7(1, 1, 11);
    let done = StableId::deterministic_v7(1, 2, 11);
    let mut input = BTreeMap::new();
    input.insert("key".to_string(), BlackboardValue::from("route"));
    input.insert("value".to_string(), BlackboardValue::from("library"));
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(1, 3, 11),
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
                guard: GuardExpr::EventIs {
                    kind: "scenario.start".to_string(),
                },
                actions: vec![ActionInvocation {
                    action_id: "astra.core.set_blackboard".to_string(),
                    input,
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();
    world.emit_event(EventSource::Scenario, EventPayload::new("scenario.start"));
    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 11,
            },
            Vec::new(),
        ))
        .unwrap()
}

#[test]
fn state_machine_presentation_action_supports_generic_commands() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 11,
            ..RuntimeConfig::default()
        },
        PackageHandle::default(),
    )
    .unwrap();
    let actor = world.create_actor("system", vec![]);
    let start = StableId::deterministic_v7(2, 1, 11);
    let done = StableId::deterministic_v7(2, 2, 11);
    let mut input = BTreeMap::new();
    input.insert("kind".to_string(), BlackboardValue::from("text_event"));
    input.insert("key".to_string(), BlackboardValue::from("line.shown"));
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(2, 3, 11),
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
                    action_id: "astra.core.presentation".to_string(),
                    input,
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
    let presentation = world.debug_session().presentation_trace();
    assert_eq!(
        presentation[0].command,
        PresentationCommand::TextEvent {
            key: "line.shown".to_string()
        }
    );
}

#[test]
fn state_machine_runs_transition_actions_in_order() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 11,
            ..RuntimeConfig::default()
        },
        PackageHandle::default(),
    )
    .unwrap();
    let actor = world.create_actor("system", vec![]);
    let start = StableId::deterministic_v7(3, 1, 11);
    let done = StableId::deterministic_v7(3, 2, 11);
    let first = set_blackboard_input("route", "library");
    let second = set_blackboard_input("route", "rooftop");

    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(3, 3, 11),
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
                guard: GuardExpr::EventIs {
                    kind: "scenario.start".to_string(),
                },
                actions: vec![
                    ActionInvocation {
                        action_id: "astra.core.set_blackboard".to_string(),
                        input: first,
                    },
                    ActionInvocation {
                        action_id: "astra.core.set_blackboard".to_string(),
                        input: second,
                    },
                ],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();

    world.emit_event(EventSource::Scenario, EventPayload::new("scenario.start"));
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

    let snapshot = world.snapshot();
    assert_eq!(
        snapshot.blackboard.get("route"),
        Some(&BlackboardValue::from("rooftop"))
    );
    let trace: Vec<_> = snapshot
        .machines
        .trace()
        .iter()
        .map(|trace| trace.action_id.as_str())
        .collect();
    assert_eq!(
        trace,
        vec!["astra.core.set_blackboard", "astra.core.set_blackboard"]
    );
}

#[test]
fn action_failure_keeps_machine_state_and_allows_other_machines() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 11,
            ..RuntimeConfig::default()
        },
        PackageHandle::default(),
    )
    .unwrap();
    let actor = world.create_actor("system", vec![]);
    let failed_start = StableId::deterministic_v7(4, 1, 11);
    let failed_done = StableId::deterministic_v7(4, 2, 11);
    let other_start = StableId::deterministic_v7(4, 3, 11);
    let other_done = StableId::deterministic_v7(4, 4, 11);

    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(4, 5, 11),
            owner: actor,
            states: vec![
                StateDefinition {
                    id: failed_start,
                    name: "start".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: failed_done,
                    name: "done".to_string(),
                    terminal: true,
                },
            ],
            transitions: vec![TransitionDefinition {
                from: failed_start,
                to: failed_done,
                guard: GuardExpr::EventIs {
                    kind: "scenario.start".to_string(),
                },
                actions: vec![ActionInvocation {
                    action_id: "astra.missing.action".to_string(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: failed_start,
        })
        .unwrap();
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(4, 6, 11),
            owner: actor,
            states: vec![
                StateDefinition {
                    id: other_start,
                    name: "start".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: other_done,
                    name: "done".to_string(),
                    terminal: true,
                },
            ],
            transitions: vec![TransitionDefinition {
                from: other_start,
                to: other_done,
                guard: GuardExpr::EventIs {
                    kind: "scenario.start".to_string(),
                },
                actions: vec![ActionInvocation {
                    action_id: "astra.core.set_blackboard".to_string(),
                    input: set_blackboard_input("other_machine", "continued"),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: other_start,
        })
        .unwrap();

    world.emit_event(EventSource::Scenario, EventPayload::new("scenario.start"));
    let report = world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 11,
            },
            Vec::new(),
        ))
        .unwrap();

    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_ACTION_MISSING"));
    let debug = world.debug_session();
    let machines = debug.state_machines(actor);
    assert!(machines
        .iter()
        .any(|machine| machine.id == StableId::deterministic_v7(4, 5, 11)
            && machine.current_state == failed_start));
    assert!(machines
        .iter()
        .any(|machine| machine.id == StableId::deterministic_v7(4, 6, 11)
            && machine.current_state == other_done
            && machine.completed));
    assert_eq!(
        world.snapshot().blackboard.get("other_machine"),
        Some(&BlackboardValue::from("continued"))
    );
}

#[test]
fn validates_state_machine_shape_and_conflicts() {
    let actor = astra_runtime::ActorId(StableId::deterministic_v7(5, 1, 11));
    let start = StableId::deterministic_v7(5, 2, 11);
    let done = StableId::deterministic_v7(5, 3, 11);
    let missing = StableId::deterministic_v7(5, 4, 11);

    let report = validate_state_machine(&StateMachineDefinition {
        id: StableId::deterministic_v7(5, 5, 11),
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
        transitions: vec![
            TransitionDefinition {
                from: start,
                to: done,
                guard: GuardExpr::EventIs {
                    kind: "scenario.start".to_string(),
                },
                actions: Vec::new(),
                priority: 10,
                source_ref: None,
            },
            TransitionDefinition {
                from: start,
                to: missing,
                guard: GuardExpr::EventIs {
                    kind: "scenario.start".to_string(),
                },
                actions: Vec::new(),
                priority: 10,
                source_ref: None,
            },
        ],
        initial_state: start,
    });

    assert!(matches!(
        report,
        StateMachineValidationReport { valid: false, .. }
    ));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_STATE_UNKNOWN"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_TRANSITION_CONFLICT"));
}

#[test]
fn terminal_state_marks_machine_completed_and_blocks_future_ticks() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 11,
            ..RuntimeConfig::default()
        },
        PackageHandle::default(),
    )
    .unwrap();
    let actor = world.create_actor("system", vec![]);
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
    let snapshot = world.snapshot();
    let trace: Vec<_> = snapshot
        .machines
        .trace()
        .iter()
        .map(|trace| trace.action_id.as_str())
        .collect();
    assert_eq!(trace, vec!["astra.core.set_blackboard"]);
}

#[test]
fn state_machine_runs_transitions_until_it_reaches_a_stable_state() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let actor = world.create_actor("stable", vec![]);
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
fn state_machine_cycle_blocks_without_committing_partial_progress() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let actor = world.create_actor("cycle", vec![]);
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

    let report = world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            Vec::new(),
        ))
        .unwrap();

    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_STATE_MACHINE_CYCLE"));
    assert_eq!(
        world.debug_session().state_machines(actor)[0].current_state,
        left
    );
    assert_eq!(world.snapshot().blackboard.get("cycle"), None);
}

#[test]
fn action_registry_rejects_duplicate_action_ids() {
    let mut registry = ActionRegistry::default();
    registry.register(SetBlackboardAction).unwrap();

    let error = registry
        .register_with_provider("astra.other", SetBlackboardAction)
        .unwrap_err();

    assert!(error.to_string().contains("ASTRA_RUNTIME_ACTION_CONFLICT"));
}

fn set_blackboard_input(key: &str, value: &str) -> BTreeMap<String, BlackboardValue> {
    let mut input = BTreeMap::new();
    input.insert("key".to_string(), BlackboardValue::from(key));
    input.insert("value".to_string(), BlackboardValue::from(value));
    input
}
