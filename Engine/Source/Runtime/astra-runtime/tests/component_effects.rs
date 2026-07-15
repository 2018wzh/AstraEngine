use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Hash256, SchemaVersion, StableId};
use astra_runtime::{
    ActionDescriptor, ActionEffect, ActionInvocation, ActionTrace, BlackboardValue,
    ComponentSelector, DeterministicActionContext, EventPayload, EventSource, GuardExpr,
    PackageHandle, RuntimeAction, RuntimeComponentPayload, RuntimeConfig, RuntimeError,
    RuntimeWorld, StateDefinition, StateMachineDefinition, TickInput, TransitionDefinition,
};

struct ApplyEffectsAction {
    id: &'static str,
    effects: Vec<ActionEffect>,
}

impl RuntimeAction for ApplyEffectsAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor {
            id: self.id.to_string(),
            input_schema: "astra.test.component_effects.input".to_string(),
            output_schema: "astra.action_trace.v1".to_string(),
        }
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        for effect in self.effects.clone() {
            ctx.apply_effect(effect)?;
        }
        Ok(ActionTrace {
            action_id: self.id.to_string(),
            payload: input.clone(),
        })
    }
}

fn map(entries: &[(&str, BlackboardValue)]) -> BlackboardValue {
    BlackboardValue::Map(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect(),
    )
}

fn payload_hash(value: &BlackboardValue) -> Hash256 {
    RuntimeComponentPayload::postcard("astra.test.hash", SchemaVersion::default(), value)
        .unwrap()
        .hash
}

fn install_machine(
    world: &mut RuntimeWorld,
    owner: astra_runtime::ActorId,
    action_id: &str,
    index: u64,
) {
    let start = StableId::deterministic_v7(9, 10, index);
    let done = StableId::deterministic_v7(9, 11, index);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(9, 12, index),
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
                    kind: "test.apply".to_string(),
                },
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

fn tick(world: &mut RuntimeWorld) -> astra_runtime::TickReport {
    world.emit_event(EventSource::Scenario, EventPayload::new("test.apply"));
    world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            Vec::new(),
        ))
        .unwrap()
}

#[astra_headless_test::test]
fn replace_component_supports_exact_component_id_selection() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let owner = world.create_actor("owner", vec![]);
    let component = world
        .attach_component(owner, "astra.test.map", &map(&[("old", 1_i64.into())]))
        .unwrap();
    let expected_hash = payload_hash(&map(&[("old", 1_i64.into())]));
    world
        .register_action(
            "astra.test",
            ApplyEffectsAction {
                id: "astra.test.replace",
                effects: vec![ActionEffect::ReplaceComponent {
                    selector: ComponentSelector::ComponentId {
                        component_id: component,
                    },
                    expected_schema: "astra.test.map".into(),
                    expected_hash,
                    data: map(&[("new", 2_i64.into())]),
                }],
            },
        )
        .unwrap();
    install_machine(&mut world, owner, "astra.test.replace", 1);

    assert!(tick(&mut world).diagnostics.is_empty());
    assert_eq!(
        world.read_component::<BlackboardValue>(component).unwrap(),
        map(&[("new", 2_i64.into())])
    );
}

#[astra_headless_test::test]
fn patch_component_map_supports_actor_schema_selection() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let owner = world.create_actor("owner", vec![]);
    let component = world
        .attach_component(
            owner,
            "astra.test.map",
            &map(&[("keep", 1_i64.into()), ("remove", 2_i64.into())]),
        )
        .unwrap();
    let expected_hash = payload_hash(&map(&[("keep", 1_i64.into()), ("remove", 2_i64.into())]));
    world
        .register_action(
            "astra.test",
            ApplyEffectsAction {
                id: "astra.test.patch",
                effects: vec![ActionEffect::PatchComponentMap {
                    selector: ComponentSelector::ActorSchema {
                        actor_id: owner,
                        schema: "astra.test.map".into(),
                    },
                    expected_schema: "astra.test.map".into(),
                    expected_hash,
                    set: BTreeMap::from([("added".into(), 3_i64.into())]),
                    remove: BTreeSet::from(["remove".into()]),
                }],
            },
        )
        .unwrap();
    install_machine(&mut world, owner, "astra.test.patch", 1);

    assert!(tick(&mut world).diagnostics.is_empty());
    assert_eq!(
        world.read_component::<BlackboardValue>(component).unwrap(),
        map(&[("added", 3_i64.into()), ("keep", 1_i64.into())])
    );
}

#[astra_headless_test::test]
fn failed_patch_rolls_back_prior_effect_and_preserves_other_machine() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let owner = world.create_actor("owner", vec![]);
    let component = world
        .attach_component(owner, "astra.test.scalar", &BlackboardValue::I64(7))
        .unwrap();
    let expected_hash = payload_hash(&BlackboardValue::I64(7));
    world
        .register_action(
            "astra.test",
            ApplyEffectsAction {
                id: "astra.test.invalid_patch",
                effects: vec![
                    ActionEffect::SetBlackboard {
                        key: "must_rollback".into(),
                        value: true.into(),
                    },
                    ActionEffect::PatchComponentMap {
                        selector: ComponentSelector::ComponentId {
                            component_id: component,
                        },
                        expected_schema: "astra.test.scalar".into(),
                        expected_hash,
                        set: BTreeMap::new(),
                        remove: BTreeSet::new(),
                    },
                ],
            },
        )
        .unwrap();
    install_machine(&mut world, owner, "astra.test.invalid_patch", 1);

    let other_owner = world.create_actor("other", vec![]);
    world
        .register_action(
            "astra.test",
            ApplyEffectsAction {
                id: "astra.test.other_machine",
                effects: vec![ActionEffect::SetBlackboard {
                    key: "other_machine".into(),
                    value: "continued".into(),
                }],
            },
        )
        .unwrap();
    install_machine(&mut world, other_owner, "astra.test.other_machine", 2);
    let report = tick(&mut world);

    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_COMPONENT_NOT_MAP"));
    assert_eq!(world.snapshot().blackboard.get("must_rollback"), None);
    assert_eq!(
        world.read_component::<BlackboardValue>(component).unwrap(),
        BlackboardValue::I64(7)
    );
    assert!(world
        .debug_session()
        .state_machines(other_owner)
        .iter()
        .any(|machine| machine.completed));
}

#[astra_headless_test::test]
fn selector_rejects_duplicate_actor_schema_targets() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let owner = world.create_actor("owner", vec![]);
    world
        .attach_component(owner, "astra.test.map", &map(&[]))
        .unwrap();
    world
        .attach_component(owner, "astra.test.map", &map(&[]))
        .unwrap();
    world
        .register_action(
            "astra.test",
            ApplyEffectsAction {
                id: "astra.test.ambiguous",
                effects: vec![ActionEffect::PatchComponentMap {
                    selector: ComponentSelector::ActorSchema {
                        actor_id: owner,
                        schema: "astra.test.map".into(),
                    },
                    expected_schema: "astra.test.map".into(),
                    expected_hash: payload_hash(&map(&[])),
                    set: BTreeMap::new(),
                    remove: BTreeSet::new(),
                }],
            },
        )
        .unwrap();
    install_machine(&mut world, owner, "astra.test.ambiguous", 1);

    assert!(tick(&mut world)
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_RUNTIME_COMPONENT_SELECTOR_AMBIGUOUS"));
}
