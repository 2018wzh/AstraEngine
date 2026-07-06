use astra_core::{SchemaMigrationRegistry, SchemaVersion};
use astra_runtime::{PackageHandle, RuntimeConfig, RuntimeWorld, SaveRequest, TickInput};

#[test]
fn save_replay_loads_hash_and_blocks_missing_migrator() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 17,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    world.create_actor("save-test", vec![]);
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 17,
        })
        .unwrap();
    let before = world.state_hash();
    let save = world.save(SaveRequest::default()).unwrap();
    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    loaded.load(save).unwrap();
    assert_eq!(loaded.state_hash(), before);

    let save_needing_migration = world
        .save(SaveRequest {
            minimum_supported_version: SchemaVersion::new(0, 9, 0),
        })
        .unwrap();
    let mut blocked =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    assert!(blocked.load(save_needing_migration.clone()).is_err());

    let mut registry = SchemaMigrationRegistry::default();
    registry.register_identity(
        "runtime.world",
        SchemaVersion::new(0, 9, 0),
        SchemaVersion::new(1, 0, 0),
    );
    blocked
        .load_with_registry(save_needing_migration, &registry)
        .unwrap();
}

#[test]
fn save_load_rejects_footer_hash_mismatch() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 19,
            required_slots: vec![],
        },
        PackageHandle::default(),
    )
    .unwrap();
    world.create_actor("corrupt", vec![]);
    let mut save = world.save(SaveRequest::default()).unwrap();
    let payload_byte = save.0.len() / 2;
    save.0[payload_byte] ^= 0x01;

    let mut loaded =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    assert!(loaded.load(save).is_err());
}
