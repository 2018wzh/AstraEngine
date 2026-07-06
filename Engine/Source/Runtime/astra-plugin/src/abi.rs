use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RString, RVec},
    StableAbi,
};

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiProviderRegistration {
    pub slot: RString,
    pub provider_id: RString,
    pub capability: RString,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiPluginRegistration {
    pub providers: RVec<FfiProviderRegistration>,
    pub callbacks: u32,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiPluginShutdown {
    pub callbacks_released: bool,
}

#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(
    prefix_ref = AstraPluginModuleRef,
    prefix_fields = AstraPluginModulePrefix
)))]
#[sabi(missing_field(panic))]
pub struct AstraPluginModule {
    pub descriptor_yaml: extern "C" fn() -> RString,
    pub register: extern "C" fn() -> FfiPluginRegistration,
    #[sabi(last_prefix_field)]
    pub shutdown: extern "C" fn() -> FfiPluginShutdown,
}

impl RootModule for AstraPluginModuleRef {
    abi_stable::declare_root_module_statics! {AstraPluginModuleRef}

    const BASE_NAME: &'static str = "astra_plugin_module";
    const NAME: &'static str = "astra-plugin";
    const VERSION_STRINGS: VersionStrings = abi_stable::package_version_strings!();
}
