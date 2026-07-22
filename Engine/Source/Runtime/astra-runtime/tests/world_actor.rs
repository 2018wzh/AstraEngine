use std::sync::Arc;

use astra_core::{Hash128, Hash256, SchemaVersion, StableId};
use astra_runtime::{
    ActorId, EngineModuleSlot, ModuleBindingContext, PackageHandle, RuntimeComponentPayload,
    RuntimeConfig, RuntimeWorld, SaveRequest, TickInput, ValidatedModuleBinding,
    ValidatedRuntimeComponentEncoding,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestComponent {
    status: String,
    count: u32,
}

#[astra_headless_test::test]
fn world_actor_creates_component_and_stable_snapshot_hash() {
    let mut world = RuntimeWorld::create(
        RuntimeConfig {
            seed: 7,
            required_slots: vec!["presentation".to_string()],
        },
        PackageHandle::default(),
    )
    .unwrap();
    let slot = EngineModuleSlot("presentation".to_string());
    let binding = ValidatedModuleBinding::validate(
        slot.clone(),
        "astra.fixture.headless_presentation",
        "presentation.headless",
        ModuleBindingContext {
            package_id: "stage1.headless".to_string(),
            target: "headless".to_string(),
            profile: "test".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            rustc_fingerprint: "rustc-stable".to_string(),
            feature_fingerprint: "runtime-envelope-v2".to_string(),
            abi_fingerprint: "astra-plugin-abi-v2".to_string(),
        },
        true,
        true,
    )
    .unwrap();
    world.mount_module(slot, binding).unwrap();
    let actor = world.create_actor("hero", vec!["player".to_string()]);
    let component = world
        .attach_component(
            actor,
            "astra.test.component",
            &TestComponent {
                status: "ready".to_string(),
                count: 1,
            },
        )
        .unwrap();
    let report = world
        .tick(astra_runtime::TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 7,
            },
            Vec::new(),
        ))
        .unwrap();

    let debug = world.debug_session();
    assert_eq!(debug.actors().len(), 1);
    assert_eq!(debug.components(actor).len(), 1);
    assert_eq!(report.state_hash, world.state_hash());
    assert_eq!(
        world.read_component::<TestComponent>(component).unwrap(),
        TestComponent {
            status: "ready".to_string(),
            count: 1,
        }
    );
    world
        .replace_component(
            component,
            &TestComponent {
                status: "running".to_string(),
                count: 2,
            },
        )
        .unwrap();
    assert_eq!(
        world
            .read_component::<TestComponent>(component)
            .unwrap()
            .count,
        2
    );
    let mutations = world.debug_session().mutation_trace();
    assert_eq!(mutations.len(), 1);
    assert_eq!(mutations[0].component_id, component);
    assert_ne!(mutations[0].before_hash, mutations[0].after_hash);

    assert!(world.detach_component(component));
    assert_eq!(world.debug_session().components(actor).len(), 0);
    assert!(world.remove_actor(actor));
    assert_eq!(world.debug_session().actors().len(), 0);

    let save = world.save(SaveRequest::default()).unwrap();
    assert!(!save.0.is_empty());
}

#[astra_headless_test::test]
fn world_actor_rejects_component_for_missing_actor() {
    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    let missing = ActorId(StableId::deterministic_v7(1, 2, 3));
    let err = world
        .attach_component(missing, "astra.test.component", &"orphan")
        .unwrap_err();
    assert!(err.to_string().contains("ASTRA_RUNTIME_ACTOR_MISSING"));
}

#[astra_headless_test::test]
fn component_payload_rejects_hash_mismatch() {
    let payload = RuntimeComponentPayload::postcard(
        "astra.test.component",
        SchemaVersion::default(),
        &TestComponent {
            status: "ready".to_string(),
            count: 1,
        },
    )
    .unwrap();
    let mut wire = serde_json::to_value(payload).unwrap();
    let bytes = wire
        .get_mut("bytes")
        .and_then(serde_json::Value::as_array_mut)
        .unwrap();
    let value = bytes[0].as_u64().unwrap();
    bytes[0] = serde_json::Value::from(value ^ 1);
    let error = serde_json::from_value::<RuntimeComponentPayload>(wire).unwrap_err();
    assert!(error.to_string().contains("ASTRA_RUNTIME_COMPONENT_HASH"));
}

#[test]
fn component_payload_clone_shares_wire_compatible_immutable_bytes() {
    let payload = RuntimeComponentPayload::postcard(
        "astra.test.shared_payload",
        SchemaVersion::default(),
        &TestComponent {
            status: "shared".to_string(),
            count: 7,
        },
    )
    .unwrap();
    let cloned = payload.clone();

    assert!(Arc::ptr_eq(payload.bytes(), cloned.bytes()));
    assert_eq!(
        postcard::to_allocvec(payload.bytes()).unwrap(),
        postcard::to_allocvec(&payload.bytes().to_vec()).unwrap()
    );
    assert_eq!(
        cloned.decode::<TestComponent>().unwrap(),
        TestComponent {
            status: "shared".to_string(),
            count: 7,
        }
    );
}

#[test]
fn validated_component_encoding_binds_both_hashes_to_shared_bytes() {
    let bytes: Arc<[u8]> = postcard::to_allocvec(&TestComponent {
        status: "validated".to_string(),
        count: 11,
    })
    .unwrap()
    .into();
    let encoding = ValidatedRuntimeComponentEncoding::postcard(Arc::clone(&bytes));

    assert_eq!(encoding.storage_hash(), Hash256::from_sha256(&bytes));
    assert_eq!(encoding.state_hash(), Hash128::from_blake3(&bytes));
}
