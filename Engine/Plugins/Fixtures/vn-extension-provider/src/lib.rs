use abi_stable::{
    prefix_type::PrefixTypeTrait,
    std_types::{RString, RVec},
};
use astra_plugin_abi::{
    AstraPluginModule, AstraPluginModuleRef, FfiPluginRegistration, FfiPluginShutdown,
    FfiProviderRegistration,
};

extern "C" fn descriptor_yaml() -> RString {
    r#"
id: astra.fixture.vn_extension_provider
version: 0.1.0
engine_version: 0.1.0
rustc_fingerprint: rustc-stable
feature_fingerprint: stage3-vn
abi_style: abi_stable_rust
capabilities:
  - astra.vn.policy_bundle
  - astra.vn.command
  - astra.vn.presentation_command
  - astra.vn.editor_metadata
  - astra.vn.release_check
permissions:
  - runtime.vn
packaged: true
"#
    .into()
}

extern "C" fn register() -> FfiPluginRegistration {
    tracing::info!(
        event = "fixture.vn_extension.register",
        provider_count = 5,
        "VN extension fixture registered"
    );
    FfiPluginRegistration {
        providers: RVec::from(vec![
            provider(
                "astra.vn.policy_bundle_provider",
                "astra.fixture.vn.policy_bundle",
                "astra.vn.policy_bundle",
            ),
            provider(
                "astra.vn.command_provider",
                "astra.fixture.vn.command",
                "astra.vn.command",
            ),
            provider(
                "astra.vn.presentation_command_provider",
                "astra.fixture.vn.presentation_command",
                "astra.vn.presentation_command",
            ),
            provider(
                "astra.vn.editor_metadata_provider",
                "astra.fixture.vn.editor_metadata",
                "astra.vn.editor_metadata",
            ),
            provider(
                "astra.vn.release_check_provider",
                "astra.fixture.vn.release_check",
                "astra.vn.release_check",
            ),
        ]),
        runtime_providers: RVec::new(),
        actions: RVec::new(),
        callbacks: 0,
    }
}

extern "C" fn shutdown() -> FfiPluginShutdown {
    tracing::info!(
        event = "fixture.vn_extension.shutdown",
        "VN extension fixture shut down"
    );
    FfiPluginShutdown {
        callbacks_released: true,
    }
}

fn provider(slot: &str, provider_id: &str, capability: &str) -> FfiProviderRegistration {
    FfiProviderRegistration {
        slot: slot.into(),
        provider_id: provider_id.into(),
        capability: capability.into(),
        phase: "runtime".into(),
        packaged: true,
    }
}

#[abi_stable::export_root_module]
pub fn astra_plugin_root_module() -> AstraPluginModuleRef {
    AstraPluginModule {
        descriptor_yaml,
        register,
        shutdown,
    }
    .leak_into_prefix()
}
