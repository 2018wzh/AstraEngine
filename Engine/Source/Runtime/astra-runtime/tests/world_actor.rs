use astra_core::StableId;
use astra_runtime::{
    ActorId, BlackboardValue, PackageHandle, RuntimeConfig, RuntimeWorld, SaveRequest, TickInput,
};

#[test]
fn world_actor_creates_component_and_stable_snapshot_hash() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 7,
            required_slots: vec!["presentation".to_string()],
        },
        PackageHandle::default(),
    )
    .unwrap();
    world.mount_module("presentation", "astra.fixture.headless_presentation");
    let actor = world.create_actor("hero", vec!["player".to_string()]);
    world
        .attach_component(
            actor,
            "astra.test.component",
            BlackboardValue::from("ready"),
        )
        .unwrap();
    let report = world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 7,
        })
        .unwrap();

    let debug = world.debug_session();
    assert_eq!(debug.actors().len(), 1);
    assert_eq!(debug.components(actor).len(), 1);
    assert_eq!(report.state_hash, world.state_hash());

    let component = debug.components(actor)[0].component_id;
    assert!(world.detach_component(component));
    assert_eq!(world.debug_session().components(actor).len(), 0);
    assert!(world.remove_actor(actor));
    assert_eq!(world.debug_session().actors().len(), 0);

    let save = world.save(SaveRequest::default()).unwrap();
    assert!(!save.0.is_empty());
}

#[test]
fn world_actor_rejects_component_for_missing_actor() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let missing = ActorId(StableId::deterministic_v7(1, 2, 3));
    let err = world
        .attach_component(
            missing,
            "astra.test.component",
            BlackboardValue::from("orphan"),
        )
        .unwrap_err();
    assert!(err.to_string().contains("ASTRA_RUNTIME_ACTOR_MISSING"));
}
