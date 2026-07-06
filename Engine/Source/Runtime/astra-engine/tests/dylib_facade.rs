use astra_engine::{
    core::StableId,
    package::{PackageBuildRequest, PackageBuilder},
    plugin::{EngineModuleSlot, LoadPhase, PluginRegistrar, RegisteredProvider},
    runtime::{PackageHandle, RuntimeConfig, RuntimeWorld},
};

#[test]
fn dylib_facade_reexports_enginecore_public_api() {
    let world = RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    assert_eq!(world.snapshot().step, 0);

    let stable_id = StableId::nil();
    assert_eq!(
        stable_id.to_string(),
        "00000000-0000-0000-0000-000000000000"
    );

    let package = PackageBuilder::build(PackageBuildRequest::minimal(
        "com.example.facade",
        "headless",
        vec![],
    ))
    .unwrap();
    assert!(!package.as_bytes().is_empty());

    let mut registrar = PluginRegistrar::default();
    registrar.register_provider(RegisteredProvider {
        slot: EngineModuleSlot("presentation".to_string()),
        provider_id: "astra.fixture.headless".to_string(),
        capability: "presentation.headless".to_string(),
        phase: LoadPhase::Runtime,
        packaged: true,
    });

    assert_eq!(
        registrar
            .selected_provider(&EngineModuleSlot("presentation".to_string()))
            .unwrap()
            .provider_id,
        "astra.fixture.headless"
    );
}
