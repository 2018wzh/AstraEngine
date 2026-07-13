use astra_plugin_abi::{
    LoadPhase, PluginExtensionRegistrySnapshot, ProductRuntimeDescriptor, ProviderBinding,
    ProviderBindingContext, ProviderExtensionRecord, ProviderPolicy, RuntimeOutputCodec,
    RuntimeOutputDomain, RuntimeOutputSchemaDescriptor, PLUGIN_EXTENSION_REGISTRY_SCHEMA,
    PROVIDER_POLICY_SCHEMA,
};

fn context(required_capability: &str) -> ProviderBindingContext {
    ProviderBindingContext {
        package_id: "game.package".into(),
        target: "game".into(),
        profile: "desktop-release".into(),
        required_capability: required_capability.into(),
        engine_version: "0.1.0".into(),
        rustc_fingerprint: "rustc-stable".into(),
        feature_fingerprint: "runtime-envelope-v2".into(),
        abi_fingerprint: "astra-plugin-abi-v2".into(),
    }
}

fn registry_and_policy() -> (PluginExtensionRegistrySnapshot, ProviderPolicy) {
    let binding = ProviderBinding::new(
        "presentation",
        "astra.renderer.wgpu",
        context("renderer2d.wgpu"),
    )
    .unwrap();
    let runtime_binding = ProviderBinding::new(
        "game_runtime_provider",
        "astra.runtime.native_vn",
        context("runtime.native_vn"),
    )
    .unwrap();
    let registry = PluginExtensionRegistrySnapshot {
        schema: PLUGIN_EXTENSION_REGISTRY_SCHEMA.into(),
        providers: vec![
            ProviderExtensionRecord {
                slot: "presentation".into(),
                provider_id: "astra.renderer.wgpu".into(),
                capability: "renderer2d.wgpu".into(),
                phase: LoadPhase::Runtime,
                packaged: true,
                engine_version: "0.1.0".into(),
                rustc_fingerprint: "rustc-stable".into(),
                feature_fingerprint: "runtime-envelope-v2".into(),
                abi_fingerprint: "astra-plugin-abi-v2".into(),
            },
            ProviderExtensionRecord {
                slot: "game_runtime_provider".into(),
                provider_id: "astra.runtime.native_vn".into(),
                capability: "runtime.native_vn".into(),
                phase: LoadPhase::Runtime,
                packaged: true,
                engine_version: "0.1.0".into(),
                rustc_fingerprint: "rustc-stable".into(),
                feature_fingerprint: "runtime-envelope-v2".into(),
                abi_fingerprint: "astra-plugin-abi-v2".into(),
            },
        ],
        bindings: vec![binding.clone(), runtime_binding.clone()],
        conflicts: vec![],
    };
    let policy = ProviderPolicy {
        schema: PROVIDER_POLICY_SCHEMA.into(),
        profile: "desktop-release".into(),
        renderer: "astra.renderer.wgpu".into(),
        decode_fallback: "profile_bound".into(),
        runtime_provider: ProductRuntimeDescriptor {
            runtime_id: "native_vn".into(),
            product_kind: "visual_novel".into(),
            provider_id: "astra.runtime.native_vn".into(),
            supported_targets: vec!["game".into()],
            capabilities: vec!["runtime.native_vn".into()],
            package_sections: vec![],
            release_checks: vec![],
            output_schemas: vec![RuntimeOutputSchemaDescriptor {
                domain: RuntimeOutputDomain::Effect,
                schema: "astra.vn.runtime_step_effect.v2".into(),
                version: astra_core::SchemaVersion::new(2, 0, 0),
                codec: RuntimeOutputCodec::Postcard,
            }],
        },
        bindings: vec![binding, runtime_binding],
    };
    (registry, policy)
}

#[test]
fn v2_registry_closes_policy_provider_and_package_identity() {
    let (registry, policy) = registry_and_policy();
    assert_eq!(
        registry
            .validate_embedded_package(&policy, "game.package", "desktop-release")
            .unwrap(),
        "game"
    );
    let selection = registry
        .resolve_embedded_runtime_provider(&policy, "game.package", "desktop-release")
        .unwrap();
    assert_eq!(selection.provider_id(), "astra.runtime.native_vn");
    assert_eq!(selection.target(), "game");
    assert_eq!(selection.profile(), "desktop-release");
    assert_eq!(selection.package_id(), "game.package");
    selection
        .validate_linked_descriptor(&policy.runtime_provider)
        .unwrap();

    let mut linked_drift = policy.runtime_provider.clone();
    linked_drift.output_schemas[0].schema = "astra.vn.runtime_step_effect.drift".into();
    assert_eq!(
        selection
            .validate_linked_descriptor(&linked_drift)
            .unwrap_err()
            .code,
        "ASTRA_RUNTIME_PROVIDER_LINKED_DESCRIPTOR_MISMATCH"
    );
}

#[test]
fn v2_registry_blocks_hash_capability_fingerprint_and_policy_drift() {
    let (registry, policy) = registry_and_policy();

    let mut tampered = registry.clone();
    tampered.bindings[0].provider_id = "astra.renderer.other".into();
    assert_eq!(
        tampered
            .validate_embedded_package(&policy, "game.package", "desktop-release")
            .unwrap_err()
            .code,
        "ASTRA_PLUGIN_BINDING_HASH_MISMATCH"
    );

    let mut capability = registry.clone();
    capability.providers[0].capability = "renderer2d.other".into();
    assert_eq!(
        capability
            .validate_embedded_package(&policy, "game.package", "desktop-release")
            .unwrap_err()
            .code,
        "ASTRA_PLUGIN_BINDING_CAPABILITY_MISMATCH"
    );

    let mut fingerprint = registry.clone();
    fingerprint.providers[0].abi_fingerprint = "astra-plugin-abi-drift".into();
    assert_eq!(
        fingerprint
            .validate_embedded_package(&policy, "game.package", "desktop-release")
            .unwrap_err()
            .code,
        "ASTRA_PLUGIN_BINDING_FINGERPRINT_MISMATCH"
    );

    let mut policy_drift = policy.clone();
    policy_drift.bindings[0] = ProviderBinding::new(
        "presentation",
        "astra.renderer.other",
        context("renderer2d.wgpu"),
    )
    .unwrap();
    assert_eq!(
        registry
            .validate_embedded_package(&policy_drift, "game.package", "desktop-release")
            .unwrap_err()
            .code,
        "ASTRA_PROVIDER_POLICY_BINDING_MISMATCH"
    );
}

#[test]
fn v2_registry_blocks_duplicate_slot_and_context_drift() {
    let (mut registry, mut policy) = registry_and_policy();
    registry.bindings.push(registry.bindings[0].clone());
    policy.bindings.push(policy.bindings[0].clone());
    assert_eq!(
        registry
            .validate_embedded_package(&policy, "game.package", "desktop-release")
            .unwrap_err()
            .code,
        "ASTRA_PLUGIN_BINDING_CONFLICT"
    );

    let (registry, policy) = registry_and_policy();
    assert_eq!(
        registry
            .validate_for_package(&policy, "game.package", "web", "desktop-release")
            .unwrap_err()
            .code,
        "ASTRA_PLUGIN_BINDING_CONTEXT_MISMATCH"
    );
}
