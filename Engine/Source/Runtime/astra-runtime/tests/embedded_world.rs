use astra_runtime::{
    PackageHandle, RuntimeConfig, RuntimeWorld, SaveRequest, TickInput, TickRequest,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Position {
    x: f32,
    y: f32,
}

#[test]
fn standalone_world_updates_typed_state_and_restores_without_package_or_fsm() {
    let config = RuntimeConfig {
        seed: 37,
        required_slots: vec![],
    };
    let mut world = RuntimeWorld::create(config.clone()).unwrap();
    assert!(world.package_id().is_none());
    assert!(world.package_handle().is_none());
    let actor = world.create_actor("sprite", vec![]).unwrap();
    let component = world
        .attach_component(actor, "test.position", &Position { x: 1.0, y: 2.0 })
        .unwrap();
    assert!(world.debug_session().state_machines(actor).is_empty());
    world
        .replace_component(component, &Position { x: 3.0, y: 4.0 })
        .unwrap();
    world
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 37,
            },
            vec![],
        ))
        .unwrap();
    let saved = world.save(SaveRequest::default()).unwrap();
    let mut restored = RuntimeWorld::create(config).unwrap();
    restored.load(saved).unwrap();
    assert!(restored.package_handle().is_none());
    assert_eq!(
        restored.read_component::<Position>(component).unwrap(),
        Position { x: 3.0, y: 4.0 }
    );
    restored
        .tick(TickRequest::restore_continuation(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 37,
            },
            vec![],
        ))
        .unwrap();
    world
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 2,
                delta_ns: 16_666_667,
                seed: 37,
            },
            vec![],
        ))
        .unwrap();
    // Stable IDs continue from the saved generator even without any product metadata.
    assert_eq!(
        world.create_actor("next", vec![]).unwrap(),
        restored.create_actor("next", vec![]).unwrap()
    );
}

#[test]
fn package_identity_is_explicit_once_and_before_first_tick() {
    let world = RuntimeWorld::create(RuntimeConfig::default())
        .unwrap()
        .with_package(PackageHandle::default())
        .unwrap();
    assert_eq!(world.package_id(), Some("stage1.headless"));
    assert!(world
        .with_package(PackageHandle::default())
        .err()
        .unwrap()
        .to_string()
        .contains("ASTRA_RUNTIME_PACKAGE_LIFECYCLE"));
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    world
        .tick(TickRequest::live(
            TickInput {
                fixed_step: 1,
                delta_ns: 16_666_667,
                seed: 0,
            },
            vec![],
        ))
        .unwrap();
    assert!(world
        .with_package(PackageHandle::default())
        .err()
        .unwrap()
        .to_string()
        .contains("ASTRA_RUNTIME_PACKAGE_LIFECYCLE"));
}

#[test]
fn standalone_world_rejects_packaged_module_mount_without_identity() {
    use astra_runtime::{EngineModuleSlot, ModuleBindingContext, ValidatedModuleBinding};
    let mut world = RuntimeWorld::create(RuntimeConfig::default()).unwrap();
    let package = PackageHandle::default();
    let slot = EngineModuleSlot("presentation".into());
    let binding = ValidatedModuleBinding::validate(
        slot.clone(),
        "test.provider",
        "presentation",
        ModuleBindingContext {
            package_id: package.package_id,
            target: package.target,
            profile: package.profile,
            engine_version: package.engine_version,
            rustc_fingerprint: package.rustc_fingerprint,
            feature_fingerprint: package.feature_fingerprint,
            abi_fingerprint: package.abi_fingerprint,
        },
        true,
        true,
    )
    .unwrap();
    assert!(world
        .mount_module(slot, binding)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_RUNTIME_MODULE_PACKAGE_REQUIRED"));
    assert!(world.package_handle().is_none());
}
