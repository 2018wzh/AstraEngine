use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RString, RVec},
    StableAbi,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoadPhase {
    EngineBoot,
    ProjectLoad,
    Editor,
    Cook,
    #[default]
    Runtime,
    Package,
    Shutdown,
}

impl LoadPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EngineBoot => "engine_boot",
            Self::ProjectLoad => "project_load",
            Self::Editor => "editor",
            Self::Cook => "cook",
            Self::Runtime => "runtime",
            Self::Package => "package",
            Self::Shutdown => "shutdown",
        }
    }
}

impl std::fmt::Display for LoadPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for LoadPhase {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "engine_boot" => Ok(Self::EngineBoot),
            "project_load" => Ok(Self::ProjectLoad),
            "editor" => Ok(Self::Editor),
            "cook" => Ok(Self::Cook),
            "runtime" => Ok(Self::Runtime),
            "package" => Ok(Self::Package),
            "shutdown" => Ok(Self::Shutdown),
            other => Err(format!("unknown load phase {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderExtensionRecord {
    pub slot: String,
    pub provider_id: String,
    pub capability: String,
    pub phase: LoadPhase,
    pub packaged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderBinding {
    pub slot: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionConflict {
    pub slot: String,
    pub selected_provider: String,
    pub conflicting_provider: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginDependency {
    pub plugin_id: String,
    pub version_req: String,
    pub required: bool,
    pub reason: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionRegistrationReport {
    pub plugin_id: String,
    pub phase: LoadPhase,
    pub registered: Vec<String>,
    pub conflicts: Vec<ExtensionConflict>,
    pub dependency_graph: Vec<PluginDependency>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginExtensionRegistrySnapshot {
    pub schema: String,
    pub providers: Vec<ProviderExtensionRecord>,
    pub bindings: Vec<ProviderBinding>,
    pub conflicts: Vec<ExtensionConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginDependencyGraphSnapshot {
    pub schema: String,
    pub dependencies: Vec<PluginDependency>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiProviderRegistration {
    pub slot: RString,
    pub provider_id: RString,
    pub capability: RString,
    pub phase: RString,
    pub packaged: bool,
}

pub type FfiActionInvoke = extern "C" fn(RVec<u8>) -> RVec<u8>;

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiActionRegistration {
    pub provider_id: RString,
    pub action_id: RString,
    pub input_schema: RString,
    pub output_schema: RString,
    #[sabi(unsafe_opaque_field)]
    pub invoke: FfiActionInvoke,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiPluginRegistration {
    pub providers: RVec<FfiProviderRegistration>,
    pub actions: RVec<FfiActionRegistration>,
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
