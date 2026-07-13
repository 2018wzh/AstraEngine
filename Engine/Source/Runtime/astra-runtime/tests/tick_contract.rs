use astra_runtime::{
    EngineModuleSlot, PackageHandle, RuntimeConfig, RuntimeWorld, TickInput, ValidatedModuleBinding,
};

fn input(step: u64, seed: u64) -> TickInput {
    TickInput {
        fixed_step: step,
        delta_ns: 16_666_667,
        seed,
    }
}

#[test]
fn tick_rejects_duplicate_gap_regression_delta_and_seed_without_mutation() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 41,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    world.create_actor("system", vec![]);
    world.tick(input(1, 41)).unwrap();
    let checkpoint = postcard::to_allocvec(&world.snapshot()).unwrap();

    let cases = [
        input(1, 41),
        input(0, 41),
        input(3, 41),
        TickInput {
            delta_ns: 0,
            ..input(2, 41)
        },
        TickInput {
            delta_ns: 1_000_000_001,
            ..input(2, 41)
        },
        input(2, 42),
    ];
    for invalid in cases {
        assert!(world.tick(invalid).is_err());
        assert_eq!(
            postcard::to_allocvec(&world.snapshot()).unwrap(),
            checkpoint
        );
    }

    assert_eq!(world.tick(input(2, 41)).unwrap().step, 2);
}

#[test]
fn missing_required_module_blocks_before_step_or_id_state_changes() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 7,
            required_slots: vec!["presentation".to_string()],
        },
        PackageHandle::default(),
    )
    .unwrap();
    let checkpoint = postcard::to_allocvec(&world.snapshot()).unwrap();
    let error = world.tick(input(1, 7)).unwrap_err();
    assert!(error.to_string().contains("ASTRA_RUNTIME_MODULE_MISSING"));
    assert_eq!(
        postcard::to_allocvec(&world.snapshot()).unwrap(),
        checkpoint
    );
}

#[test]
fn module_mount_requires_matching_explicit_packaged_binding_and_unique_slot() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let slot = EngineModuleSlot("presentation".to_string());
    assert!(ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        "stage1.headless",
        true,
        false,
    )
    .is_err());
    assert!(ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        "stage1.headless",
        false,
        true,
    )
    .is_err());
    let wrong_package = ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        "other.package",
        true,
        true,
    )
    .unwrap();
    assert!(world.mount_module(slot.clone(), wrong_package).is_err());

    let binding = ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.presentation",
        "presentation.headless",
        "stage1.headless",
        true,
        true,
    )
    .unwrap();
    world.mount_module(slot.clone(), binding).unwrap();
    let duplicate = ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.second",
        "presentation.headless",
        "stage1.headless",
        true,
        true,
    )
    .unwrap();
    assert!(world.mount_module(slot, duplicate).is_err());
}
