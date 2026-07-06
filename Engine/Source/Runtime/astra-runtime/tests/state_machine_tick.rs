use astra_core::StableId;
use astra_runtime::{
    ActionInvocation, BlackboardValue, EventPayload, EventSource, GuardExpr, PackageHandle,
    PresentationCommand, RuntimeConfig, RuntimeWorld, StateDefinition, StateMachineDefinition,
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
    world.add_state_machine(StateMachineDefinition {
        id: StableId::deterministic_v7(1, 3, 11),
        owner: actor,
        states: vec![
            StateDefinition {
                id: start,
                name: "start".to_string(),
            },
            StateDefinition {
                id: done,
                name: "done".to_string(),
            },
        ],
        transitions: vec![TransitionDefinition {
            from: start,
            to: done,
            guard: GuardExpr::EventIs {
                kind: "scenario.start".to_string(),
            },
            action: ActionInvocation {
                action_id: "astra.core.set_blackboard".to_string(),
                input,
            },
            source_ref: None,
        }],
        initial_state: start,
    });
    world.emit_event(EventSource::Scenario, EventPayload::new("scenario.start"));
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 11,
        })
        .unwrap()
}

#[test]
fn state_machine_presentation_action_supports_generic_commands() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let actor = world.create_actor("system", vec![]);
    let start = StableId::deterministic_v7(2, 1, 11);
    let done = StableId::deterministic_v7(2, 2, 11);
    let mut input = BTreeMap::new();
    input.insert("kind".to_string(), BlackboardValue::from("text_event"));
    input.insert("key".to_string(), BlackboardValue::from("line.shown"));
    world.add_state_machine(StateMachineDefinition {
        id: StableId::deterministic_v7(2, 3, 11),
        owner: actor,
        states: vec![
            StateDefinition {
                id: start,
                name: "start".to_string(),
            },
            StateDefinition {
                id: done,
                name: "done".to_string(),
            },
        ],
        transitions: vec![TransitionDefinition {
            from: start,
            to: done,
            guard: GuardExpr::Always,
            action: ActionInvocation {
                action_id: "astra.core.presentation".to_string(),
                input,
            },
            source_ref: None,
        }],
        initial_state: start,
    });
    world.emit_event(EventSource::Scenario, EventPayload::new("scenario.start"));
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 11,
        })
        .unwrap();
    let presentation = world.debug_session().presentation_trace();
    assert_eq!(
        presentation[0].command,
        PresentationCommand::TextEvent {
            key: "line.shown".to_string()
        }
    );
}
