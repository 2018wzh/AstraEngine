use astra_core::StableId;
use astra_runtime::{
    ActionAccess, ActionDescriptor, ActionExecutionClass, ActionInvocation, ActionResourceKey,
    ActionTrace, BlackboardValue, DeterministicActionContext, EventPayload, EventSource, GuardExpr,
    RuntimeAction, RuntimeConfig, RuntimeError, RuntimeWorld, StateDefinition,
    StateMachineDefinition, TickInput, TickRequest, TransitionDefinition,
};
use std::collections::BTreeMap;

struct WriteAction {
    id: &'static str,
    key: &'static str,
    increment: bool,
}
impl RuntimeAction for WriteAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            self.id,
            "test.epoch.input",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new(
                [if self.increment {
                    ActionResourceKey::Blackboard
                } else {
                    ActionResourceKey::EventQueue
                }],
                [ActionResourceKey::BlackboardKey(self.key.into())],
            ),
            0,
        )
    }
    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        _: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let value = if self.increment {
            match ctx.blackboard().get(self.key) {
                Some(BlackboardValue::I64(value)) => value + 1,
                _ => 1,
            }
        } else {
            match ctx.trigger_event().unwrap().payload.data.get("value") {
                Some(BlackboardValue::I64(value)) => *value,
                _ => panic!("declared event value missing"),
            }
        };
        ctx.set_blackboard(self.key, BlackboardValue::I64(value));
        Ok(ActionTrace {
            action_id: self.id.into(),
            payload: BTreeMap::new(),
        })
    }
}

#[test]
fn replacing_an_action_rebuilds_its_cached_conflict_plan() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world.set_machine_worker_count(2).unwrap();
    let owner = world.create_actor("epoch-owner", vec![]).unwrap();
    for (index, (id, key)) in [("test.epoch.a", "a"), ("test.epoch.b", "b")]
        .into_iter()
        .enumerate()
    {
        world
            .register_action(
                id,
                WriteAction {
                    id,
                    key,
                    increment: false,
                },
            )
            .unwrap();
        let state = StableId::deterministic_v7(1, index as u64, 1);
        world
            .add_state_machine(StateMachineDefinition {
                id: StableId::deterministic_v7(1, index as u64, 2),
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
                        kind: "epoch.tick".into(),
                    },
                    actions: vec![ActionInvocation {
                        action_id: id.into(),
                        input: BTreeMap::new(),
                    }],
                    priority: 0,
                    source_ref: None,
                }],
                initial_state: state,
            })
            .unwrap();
    }
    let tick = |world: &mut RuntimeWorld, step: u64| {
        let mut event = EventPayload::new("epoch.tick");
        event
            .data
            .insert("value".into(), BlackboardValue::I64(step as i64));
        world.emit_event(EventSource::Scenario, event).unwrap();
        world
            .tick(TickRequest::live(
                TickInput {
                    fixed_step: step,
                    delta_ns: 16_666_667,
                    seed: 0,
                },
                vec![],
            ))
            .unwrap();
    };
    tick(&mut world, 1);
    world.unregister_action_provider("test.epoch.b");
    world
        .register_action(
            "test.epoch.b",
            WriteAction {
                id: "test.epoch.b",
                key: "a",
                increment: true,
            },
        )
        .unwrap();
    tick(&mut world, 2);
    assert_eq!(
        world.debug_session().blackboard().get("a"),
        Some(&BlackboardValue::I64(3))
    );
    assert_eq!(
        world.debug_session().blackboard().get("b"),
        Some(&BlackboardValue::I64(1))
    );
}
