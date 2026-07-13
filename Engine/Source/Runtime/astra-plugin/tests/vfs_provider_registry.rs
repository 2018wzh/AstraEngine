use astra_plugin::{
    provider_binding_context_from_runtime_package, EngineModuleSlot, LoadPhase, PluginRegistrar,
    ProviderBindingContext, RegisteredProvider,
};

fn vfs_provider(provider_id: &str, capability: &str) -> RegisteredProvider {
    RegisteredProvider {
        slot: EngineModuleSlot("vfs_provider".to_string()),
        provider_id: provider_id.to_string(),
        capability: capability.to_string(),
        phase: LoadPhase::Runtime,
        packaged: true,
        engine_version: "0.1.0".to_string(),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "runtime-envelope-v2".to_string(),
        abi_fingerprint: "astra-plugin-abi-v2".to_string(),
    }
}

fn context(capability: &str) -> ProviderBindingContext {
    provider_binding_context_from_runtime_package(
        &astra_runtime::PackageHandle::default(),
        capability,
    )
}

#[test]
fn vfs_provider_registry_allows_multiple_providers_on_single_slot() {
    let mut registrar = PluginRegistrar::default();
    registrar
        .register_provider(vfs_provider("astra.vfs.package", "vfs.backend.package"))
        .unwrap();
    registrar
        .register_provider(vfs_provider("astra.vfs.local", "vfs.backend.local"))
        .unwrap();
    registrar
        .register_provider(vfs_provider("astra.vfs.fvp", "vfs.backend.legacy_pack.fvp"))
        .unwrap();

    let snapshot = registrar.extension_registry_snapshot().unwrap();
    assert_eq!(snapshot.providers.len(), 3);
    assert!(snapshot
        .providers
        .iter()
        .all(|provider| provider.slot == "vfs_provider"));
    assert!(snapshot.bindings.is_empty());
    assert!(snapshot.conflicts.is_empty());
}

#[test]
fn runtime_provider_registry_keeps_explicit_single_binding_conflicts() {
    let mut registrar = PluginRegistrar::default();
    let slot = EngineModuleSlot("game_runtime_provider".to_string());
    registrar
        .register_provider(RegisteredProvider {
            slot: slot.clone(),
            provider_id: "astra.runtime.native_vn".to_string(),
            capability: "runtime.native_vn".to_string(),
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
            &slot,
            "astra.runtime.native_vn",
            context("runtime.native_vn"),
        )
        .unwrap();
    registrar
        .register_provider(RegisteredProvider {
            slot: slot.clone(),
            provider_id: "astra.runtime.astra_emu".to_string(),
            capability: "runtime.astra_emu".to_string(),
            phase: LoadPhase::Runtime,
            packaged: true,
            engine_version: "0.1.0".to_string(),
            rustc_fingerprint: "rustc-stable".to_string(),
            feature_fingerprint: "runtime-envelope-v2".to_string(),
            abi_fingerprint: "astra-plugin-abi-v2".to_string(),
        })
        .unwrap();
    assert!(registrar
        .bind_provider(
            &slot,
            "astra.runtime.astra_emu",
            context("runtime.astra_emu"),
        )
        .unwrap_err()
        .contains("ASTRA_PLUGIN_BINDING_CONFLICT"));

    let snapshot = registrar.extension_registry_snapshot().unwrap();
    assert_eq!(snapshot.conflicts.len(), 1);
    assert_eq!(snapshot.conflicts[0].slot, "game_runtime_provider");
}
