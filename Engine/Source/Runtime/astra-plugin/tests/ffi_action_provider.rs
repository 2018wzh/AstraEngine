use std::{collections::BTreeMap, path::Path, process::Command};

use astra_core::StableId;
use astra_plugin::{dylib_path, EngineModuleSlot, PluginGate, PluginLoader, PluginRegistrar};
use astra_runtime::{
    ActionInvocation, BlackboardValue, EventPayload, EventSource, GuardExpr, PackageHandle,
    PresentationCommand, RuntimeConfig, RuntimeWorld, StateDefinition, StateMachineDefinition,
    TickInput, TransitionDefinition,
};
use semver::Version;

#[test]
fn ffi_action_provider_registers_executes_and_unloads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let status = Command::new("cargo")
        .args(["build", "-p", "headless-presentation-provider"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());

    let mut registrar = PluginRegistrar::default();
    let loader = PluginLoader::new(PluginGate {
        engine_version: Version::parse("0.1.0").unwrap(),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "runtime-envelope-v2".to_string(),
        abi_fingerprint: "astra-plugin-abi-v2".to_string(),
        required_capabilities: vec![
            "presentation.headless".to_string(),
            "action.fixture".to_string(),
        ],
        required_permissions: vec![
            "runtime.presentation".to_string(),
            "runtime.action".to_string(),
        ],
    });
    let plugin = loader
        .load(
            dylib_path(root, "headless_presentation_provider"),
            &mut registrar,
        )
        .unwrap();

    let mut world =
        RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    plugin.install_runtime_actions(&mut world).unwrap();

    let actor = world.create_actor("system", vec![]);
    let start = StableId::deterministic_v7(1, 1, 77);
    let done = StableId::deterministic_v7(1, 2, 77);
    world
        .add_state_machine(StateMachineDefinition {
            id: StableId::deterministic_v7(1, 3, 77),
            owner: actor,
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
                    kind: "fixture.start".to_string(),
                },
                actions: vec![ActionInvocation {
                    action_id: "astra.fixture.action.set_flag".to_string(),
                    input: BTreeMap::new(),
                }],
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })
        .unwrap();

    world.emit_event(EventSource::Scenario, EventPayload::new("fixture.start"));
    world
        .tick(TickInput {
            fixed_step: 1,
            delta_ns: 16_666_667,
            seed: 0,
        })
        .unwrap();

    assert_eq!(
        world.snapshot().blackboard.get("fixture.action"),
        Some(&BlackboardValue::from("ran"))
    );
    assert_eq!(
        world.debug_session().presentation_trace()[0].command,
        PresentationCommand::Marker {
            name: "ffi_action".to_string()
        }
    );
    assert_eq!(
        registrar
            .selected_provider(&EngineModuleSlot("presentation".to_string()))
            .unwrap()
            .provider_id,
        "astra.fixture.headless_presentation"
    );

    let report = plugin
        .unload_from_runtime(&mut registrar, &mut world)
        .unwrap();
    assert_eq!(report.status, "unloaded");
    assert!(registrar.extensions.providers().is_empty());
}
