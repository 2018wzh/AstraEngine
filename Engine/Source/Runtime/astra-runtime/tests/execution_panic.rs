use astra_core::StableId;
use astra_runtime::*;
use std::collections::BTreeMap;

struct PanickingAction;
impl RuntimeAction for PanickingAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "test.panic",
            "test.input",
            "test.trace",
            ActionExecutionClass::Serial,
            ActionAccess::new([], []),
            0,
        )
    }
    fn run(
        &self,
        _: &mut DeterministicActionContext<'_>,
        _: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        panic!("public test fixture panic")
    }
}

#[test]
fn action_panic_poisoning_is_contained_until_explicit_restore() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world.register_action("test", PanickingAction).unwrap();
    let saved = world.save(SaveRequest::default()).unwrap();
    let owner = world.create_actor("actor", vec![]).unwrap();
    let start = StableId::deterministic_v7(1, 1, 0);
    let done = StableId::deterministic_v7(1, 2, 0);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(1, 3, 0),
            owner,
            states: vec![
                StateDefinition {
                    id: start,
                    name: "start".into(),
                    terminal: false,
                },
                StateDefinition {
                    id: done,
                    name: "done".into(),
                    terminal: true,
                },
            ],
            initial_state: start,
            transitions: vec![TransitionDefinition {
                from: start,
                to: done,
                guard: GuardExpr::Always,
                actions: vec![ActionInvocation {
                    action_id: "test.panic".into(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
        })
        .unwrap();
    let input = TickInput {
        fixed_step: 1,
        delta_ns: 16_666_667,
        seed: 0,
    };
    let error = world.tick(TickRequest::live(input, vec![])).unwrap_err();
    assert!(error.to_string().contains("ASTRA_RUNTIME_ACTION_PANIC"));
    assert!(world.is_failed());
    assert!(world.save(SaveRequest::default()).is_err());
    assert!(world.snapshot().is_err());
    assert!(world.remove_actor(owner).is_err());
    world.load(saved).unwrap();
    assert!(!world.is_failed());
    world
        .tick(TickRequest::restore_continuation(input, vec![]))
        .unwrap();
}
