use astra_plugin::{
    EngineModuleSlot, ExtensionConflict, LoadPhase, PluginDependency, PluginRegistrar,
    RegisteredProvider,
};

fn provider(id: &str) -> RegisteredProvider {
    RegisteredProvider {
        slot: EngineModuleSlot("presentation".to_string()),
        provider_id: id.to_string(),
        capability: "presentation.headless".to_string(),
        phase: LoadPhase::Runtime,
        packaged: true,
    }
}

#[test]
fn extension_registry_preserves_explicit_binding_and_reports_conflicts() {
    let mut registrar = PluginRegistrar::default();
    registrar.register_provider(provider("astra.provider.first"));
    registrar
        .bind_provider(
            &EngineModuleSlot("presentation".to_string()),
            "astra.provider.first",
        )
        .unwrap();

    registrar.register_provider(provider("astra.provider.second"));

    let selected = registrar
        .selected_provider(&EngineModuleSlot("presentation".to_string()))
        .unwrap();
    assert_eq!(selected.provider_id, "astra.provider.first");

    let snapshot = registrar.extension_registry_snapshot();
    assert_eq!(snapshot.providers.len(), 2);
    assert_eq!(
        snapshot.conflicts,
        vec![ExtensionConflict {
            slot: "presentation".to_string(),
            selected_provider: "astra.provider.first".to_string(),
            conflicting_provider: "astra.provider.second".to_string(),
            reason: "provider slot already has an explicit binding".to_string(),
        }]
    );

    registrar
        .bind_provider(
            &EngineModuleSlot("presentation".to_string()),
            "astra.provider.second",
        )
        .unwrap();
    assert_eq!(
        registrar
            .selected_provider(&EngineModuleSlot("presentation".to_string()))
            .unwrap()
            .provider_id,
        "astra.provider.second"
    );

    registrar.record_dependency(PluginDependency {
        plugin_id: "astra.provider.first".to_string(),
        version_req: ">=0.1.0".to_string(),
        required: true,
        reason: "renderer replacement".to_string(),
        resolved: true,
    });
    assert_eq!(registrar.dependency_graph().len(), 1);

    let selected_provider = registrar
        .selected_provider(&EngineModuleSlot("presentation".to_string()))
        .unwrap()
        .clone();
    registrar.unregister_provider(&selected_provider);
    assert!(registrar
        .selected_provider(&EngineModuleSlot("presentation".to_string()))
        .is_none());
}
