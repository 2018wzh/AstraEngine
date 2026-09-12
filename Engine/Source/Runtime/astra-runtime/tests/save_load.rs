use astra_core::SchemaVersion;
use astra_package::{
    AstraContainerBuilder, ContainerKind, MigrationPolicy, SectionCodec, SectionPayload,
};
use astra_runtime::{
    MigrationManifest, MigrationManifestEntry, RuntimeConfig, RuntimeWorld, SaveBlob, SaveRequest,
    TickInput, TickRequest,
};

#[test]
fn save_load_rejects_previous_runtime_world_layout_without_compatibility() {
    let world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let version = SchemaVersion::new(1, 0, 0);
    let manifest = MigrationManifest {
        sections: vec![MigrationManifestEntry {
            schema: "runtime.world".to_string(),
            minimum_supported_version: version,
            current_version: version,
        }],
    };
    let blob = AstraContainerBuilder::new(ContainerKind::Save)
        .add_section(SectionPayload::new(
            "runtime.world",
            "runtime.world",
            version,
            SectionCodec::Postcard,
            postcard::to_allocvec(&world.snapshot().unwrap()).unwrap(),
            MigrationPolicy::current(),
        ))
        .add_section(SectionPayload::new(
            "migration.manifest",
            "migration.manifest",
            version,
            SectionCodec::Postcard,
            postcard::to_allocvec(&manifest).unwrap(),
            MigrationPolicy::current(),
        ))
        .write()
        .unwrap();
    let mut loaded = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let error = loaded.load(SaveBlob(blob.into_bytes())).unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_SAVE_WORLD_VERSION_UNSUPPORTED"));
}

#[test]
fn save_load_preserves_typed_world_and_stable_id_sequence() {
    let config = RuntimeConfig {
        seed: 23,
        required_slots: vec![],
    };
    let mut uninterrupted = RuntimeWorld::create(config.clone()).unwrap();
    uninterrupted.create_actor("before-save", vec![]).unwrap();
    uninterrupted
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: config.seed,
            },
            Vec::new(),
        ))
        .unwrap();
    let before = uninterrupted.snapshot().unwrap();
    let save = uninterrupted.save(SaveRequest::default()).unwrap();
    let expected = uninterrupted.create_actor("after-save", vec![]).unwrap();

    let mut restored = RuntimeWorld::create(config).unwrap();
    restored.load(save).unwrap();
    assert_eq!(restored.snapshot().unwrap(), before);
    assert_eq!(
        restored.create_actor("after-save", vec![]).unwrap(),
        expected
    );
}

#[test]
fn restored_world_requires_exactly_one_restore_continuation_tick() {
    let config = RuntimeConfig {
        seed: 17,
        required_slots: vec![],
    };
    let world = RuntimeWorld::create(config.clone()).unwrap();
    let save = world.save(SaveRequest::default()).unwrap();
    let mut restored = RuntimeWorld::create(config).unwrap();
    restored.load(save).unwrap();
    assert!(restored
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .unwrap_err()
        .to_string()
        .contains("ASTRA_RUNTIME_TICK_MODE_INVALID"));
    restored
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .unwrap();
    assert!(restored
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 17,
            },
            vec![],
        ))
        .is_err());
}

#[test]
fn save_load_rejects_footer_hash_mismatch() {
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world.create_actor("corrupt", vec![]).unwrap();
    let mut save = world.save(SaveRequest::default()).unwrap();
    let payload_byte = save.0.len() / 2;
    save.0[payload_byte] ^= 1;
    let mut loaded = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    assert!(loaded.load(save).is_err());
}
