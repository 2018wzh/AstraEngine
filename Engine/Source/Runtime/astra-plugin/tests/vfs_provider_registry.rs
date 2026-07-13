use astra_plugin::{EngineModuleSlot, LoadPhase, PluginRegistrar, RegisteredProvider};

fn vfs_provider(provider_id: &str, capability: &str) -> RegisteredProvider {
    RegisteredProvider {
        slot: EngineModuleSlot("vfs_provider".to_string()),
        provider_id: provider_id.to_string(),
        capability: capability.to_string(),
        phase: LoadPhase::Runtime,
        packaged: true,
    }
}

#[test]
fn vfs_provider_registry_allows_multiple_providers_on_single_slot() {
    let mut registrar = PluginRegistrar::default();
    registrar.register_provider(vfs_provider("astra.vfs.package", "vfs.backend.package"));
    registrar.register_provider(vfs_provider("astra.vfs.local", "vfs.backend.local"));
    registrar.register_provider(vfs_provider("astra.vfs.fvp", "vfs.backend.legacy_pack.fvp"));

    let snapshot = registrar.extension_registry_snapshot();
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
    registrar.register_provider(RegisteredProvider {
        slot: slot.clone(),
        provider_id: "astra.runtime.native_vn".to_string(),
        capability: "runtime.native_vn".to_string(),
        phase: LoadPhase::Runtime,
        packaged: true,
    });
    registrar
        .bind_provider(&slot, "astra.runtime.native_vn")
        .unwrap();
    registrar.register_provider(RegisteredProvider {
        slot: slot.clone(),
        provider_id: "astra.runtime.astra_emu".to_string(),
        capability: "runtime.astra_emu".to_string(),
        phase: LoadPhase::Runtime,
        packaged: true,
    });
    assert!(registrar
        .bind_provider(&slot, "astra.runtime.astra_emu")
        .unwrap_err()
        .contains("ASTRA_PLUGIN_BINDING_CONFLICT"));

    let snapshot = registrar.extension_registry_snapshot();
    assert_eq!(snapshot.conflicts.len(), 1);
    assert_eq!(snapshot.conflicts[0].slot, "game_runtime_provider");
}
