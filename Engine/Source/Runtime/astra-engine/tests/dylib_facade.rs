use astra_engine::{
    core::StableId,
    package::{PackageBuildRequest, PackageBuilder},
    plugin::{
        provider_binding_context_from_runtime_package, EngineModuleSlot, LoadPhase,
        PluginRegistrar, RegisteredProvider,
    },
    runtime::{PackageHandle, RuntimeConfig, RuntimeWorld},
};

#[astra_headless_test::test]
fn dylib_facade_reexports_enginecore_public_api() {
    let world = RuntimeWorld::create(RuntimeConfig::default(), PackageHandle::default()).unwrap();
    assert_eq!(world.snapshot().step, 0);

    let stable_id = StableId::nil();
    assert_eq!(
        stable_id.to_string(),
        "00000000-0000-0000-0000-000000000000"
    );

    let package = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.facade",
        "headless",
        vec![],
    ))
    .unwrap();
    assert!(!package.as_bytes().is_empty());

    let mut registrar = PluginRegistrar::default();
    registrar
        .register_provider(RegisteredProvider {
            slot: EngineModuleSlot("presentation".to_string()),
            provider_id: "astra.fixture.headless".to_string(),
            capability: "presentation.headless".to_string(),
            phase: LoadPhase::Runtime,
            packaged: true,
            engine_version: "0.1.0".to_string(),
            rustc_fingerprint: "rustc-stable".to_string(),
            feature_fingerprint: "runtime-envelope-v2".to_string(),
            abi_fingerprint: "astra-plugin-abi-v2".to_string(),
        })
        .unwrap();
    registrar
        .bind_provider(
            &EngineModuleSlot("presentation".to_string()),
            "astra.fixture.headless",
            provider_binding_context_from_runtime_package(
                &PackageHandle::default(),
                "presentation.headless",
            ),
        )
        .unwrap();

    assert_eq!(
        registrar
            .selected_provider(&EngineModuleSlot("presentation".to_string()))
            .unwrap()
            .provider_id,
        "astra.fixture.headless"
    );
}
