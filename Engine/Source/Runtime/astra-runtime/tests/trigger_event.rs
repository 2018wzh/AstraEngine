use std::collections::BTreeMap;

use astra_core::StableId;
use astra_runtime::{
    ActionDescriptor, ActionInvocation, ActionTrace, BlackboardValue, ComponentId,
    DeterministicActionContext, EventPayload, EventSource, GuardExpr, RuntimeAction, RuntimeConfig,
    RuntimeWorld, StateDefinition, StateMachineDefinition, TickInput, TransitionDefinition,
};
use serde::{Deserialize, Serialize};

#[test]
fn action_context_exposes_transition_trigger_event() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default(), Default::default()).unwrap();
    world
        .register_action("astra.test", CaptureTriggerAction)
        .unwrap();
    let owner = world.create_actor("vn.driver", vec![]);

    let start = StableId::deterministic_v7(1, 1, 1);
    let done = StableId::deterministic_v7(1, 2, 1);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(1, 3, 1),
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
                guard: GuardExpr::EventIs {
                    kind: "vn.advance".to_string(),
                },
                actions: vec![ActionInvocation {
                    action_id: "astra.test.capture_trigger".to_string(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();

    world.emit_event(EventSource::PlayerInput, EventPayload::new("vn.advance"));
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 0,
        })
        .unwrap();

    assert_eq!(
        world.snapshot().blackboard.get("trigger.kind"),
        Some(&BlackboardValue::String("vn.advance".to_string()))
    );
}

struct CaptureTriggerAction;

impl RuntimeAction for CaptureTriggerAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor {
            id: "astra.test.capture_trigger".to_string(),
            input_schema: "astra.test.capture_trigger.v1".to_string(),
            output_schema: "astra.action_trace.v1".to_string(),
        }
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, astra_runtime::RuntimeError> {
        let kind = ctx
            .trigger_event()
            .expect("trigger event is present for event-guarded transition")
            .payload
            .kind
            .clone();
        ctx.set_blackboard("trigger.kind", BlackboardValue::String(kind));
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: input.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CounterComponent {
    value: u32,
}

#[test]
fn action_context_commits_typed_component_mutation() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default(), Default::default()).unwrap();
    let owner = world.create_actor("runtime.owner", vec![]);
    let component = world
        .attach_component(owner, "astra.test.counter", &CounterComponent { value: 1 })
        .unwrap();
    world
        .register_action("astra.test", IncrementComponentAction { component })
        .unwrap();
    let start = StableId::deterministic_v7(2, 1, 1);
    let done = StableId::deterministic_v7(2, 2, 1);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(2, 3, 1),
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
                    action_id: "astra.test.increment_component".to_string(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();

    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 0,
        })
        .unwrap();

    assert_eq!(
        world.read_component::<CounterComponent>(component).unwrap(),
        CounterComponent { value: 2 }
    );
    let mutations = world.debug_session().mutation_trace();
    assert_eq!(mutations.len(), 1);
    assert_eq!(mutations[0].source, "astra.test.increment_component");
}

struct IncrementComponentAction {
    component: ComponentId,
}

impl RuntimeAction for IncrementComponentAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor {
            id: "astra.test.increment_component".to_string(),
            input_schema: "astra.test.increment_component.v1".to_string(),
            output_schema: "astra.action_trace.v1".to_string(),
        }
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, astra_runtime::RuntimeError> {
        let mut counter = ctx.read_component::<CounterComponent>(self.component)?;
        counter.value += 1;
        ctx.replace_component(self.component, &counter)?;
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: input.clone(),
        })
    }
}
