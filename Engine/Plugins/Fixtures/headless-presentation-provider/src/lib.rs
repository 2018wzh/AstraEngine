use abi_stable::{
    prefix_type::PrefixTypeTrait,
    std_types::{RString, RVec},
};
use astra_plugin::{
    AstraPluginModule, AstraPluginModuleRef, FfiPluginRegistration, FfiPluginShutdown,
    FfiProviderRegistration,
};

extern "C" fn descriptor_yaml() -> RString {
    r#"
id: astra.fixture.headless_presentation
version: 0.1.0
engine_version: 0.1.0
rustc_fingerprint: rustc-stable
feature_fingerprint: stage1-core
abi_style: abi_stable_rust
capabilities:
  - presentation.headless
permissions:
  - runtime.presentation
packaged: true
"#
    .into()
}

extern "C" fn register() -> FfiPluginRegistration {
    FfiPluginRegistration {
        providers: RVec::from(vec![FfiProviderRegistration {
            slot: "presentation".into(),
            provider_id: "astra.fixture.headless_presentation".into(),
            capability: "presentation.headless".into(),
        }]),
        callbacks: 0,
    }
}

extern "C" fn shutdown() -> FfiPluginShutdown {
    FfiPluginShutdown {
        callbacks_released: true,
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
