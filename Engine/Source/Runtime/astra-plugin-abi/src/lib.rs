#[cfg(feature = "ffi")]
use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RString, RVec},
    StableAbi,
};
use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
mod live_presentation;
pub use live_presentation::*;
mod live_vn_state;
pub use live_vn_state::*;

pub const PLUGIN_EXTENSION_REGISTRY_SCHEMA: &str = "astra.plugin_extension_registry.v2";
pub const PROVIDER_POLICY_SCHEMA: &str = "astra.provider_policy.v3";

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
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderBindingContext {
    pub package_id: String,
    pub target: String,
    pub profile: String,
    pub required_capability: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderBinding {
    pub slot: String,
    pub provider_id: String,
    pub context: ProviderBindingContext,
    pub binding_hash: Hash256,
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
#[serde(deny_unknown_fields)]
pub struct ProviderPolicy {
    pub schema: String,
    pub profile: String,
    pub renderer: String,
    pub bindings: Vec<ProviderBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistryDiagnostic {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for ProviderRegistryDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProviderRegistryDiagnostic {}

impl ProviderBinding {
    pub fn new(
        slot: impl Into<String>,
        provider_id: impl Into<String>,
        context: ProviderBindingContext,
    ) -> Result<Self, ProviderRegistryDiagnostic> {
        let mut binding = Self {
            slot: slot.into(),
            provider_id: provider_id.into(),
            context,
            binding_hash: Hash256::from_sha256(&[]),
        };
        binding.binding_hash = binding.compute_hash()?;
        binding.validate_identity()?;
        Ok(binding)
    }

    pub fn compute_hash(&self) -> Result<Hash256, ProviderRegistryDiagnostic> {
        #[derive(Serialize)]
        struct BindingIdentity<'a> {
            slot: &'a str,
            provider_id: &'a str,
            context: &'a ProviderBindingContext,
        }
        let bytes = serde_json::to_vec(&BindingIdentity {
            slot: &self.slot,
            provider_id: &self.provider_id,
            context: &self.context,
        })
        .map_err(|error| diagnostic("ASTRA_PLUGIN_BINDING_HASH", error.to_string()))?;
        Ok(Hash256::from_sha256(&bytes))
    }

    pub fn validate_identity(&self) -> Result<(), ProviderRegistryDiagnostic> {
        for (field, value) in [
            ("slot", self.slot.as_str()),
            ("provider_id", self.provider_id.as_str()),
            (
                "required_capability",
                self.context.required_capability.as_str(),
            ),
            ("package_id", self.context.package_id.as_str()),
            ("target", self.context.target.as_str()),
            ("profile", self.context.profile.as_str()),
        ] {
            if !is_safe_provider_symbol(value) {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_BINDING_IDENTITY_INVALID",
                    format!("binding {field} is not a safe non-empty symbol"),
                ));
            }
        }
        for (field, value) in [
            ("engine_version", self.context.engine_version.as_str()),
            ("rustc_fingerprint", self.context.rustc_fingerprint.as_str()),
            (
                "feature_fingerprint",
                self.context.feature_fingerprint.as_str(),
            ),
            ("abi_fingerprint", self.context.abi_fingerprint.as_str()),
        ] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_whitespace) {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_BINDING_FINGERPRINT_INVALID",
                    format!("binding {field} is empty or malformed"),
                ));
            }
        }
        if self.compute_hash()? != self.binding_hash {
            return Err(diagnostic(
                "ASTRA_PLUGIN_BINDING_HASH_MISMATCH",
                format!("binding hash does not match slot {}", self.slot),
            ));
        }
        Ok(())
    }
}

impl PluginExtensionRegistrySnapshot {
    pub fn validate_embedded_package(
        &self,
        policy: &ProviderPolicy,
        package_id: &str,
        profile: &str,
    ) -> Result<&str, ProviderRegistryDiagnostic> {
        let target = self
            .bindings
            .first()
            .ok_or_else(|| {
                diagnostic(
                    "ASTRA_PLUGIN_BINDING_MISSING",
                    "plugin registry contains no explicit provider bindings",
                )
            })?
            .context
            .target
            .as_str();
        self.validate_for_package(policy, package_id, target, profile)?;
        Ok(target)
    }

    pub fn validate_for_package(
        &self,
        policy: &ProviderPolicy,
        package_id: &str,
        target: &str,
        profile: &str,
    ) -> Result<(), ProviderRegistryDiagnostic> {
        if self.schema != PLUGIN_EXTENSION_REGISTRY_SCHEMA {
            return Err(diagnostic(
                "ASTRA_PLUGIN_EXTENSION_REGISTRY_VERSION_UNSUPPORTED",
                format!(
                    "expected {PLUGIN_EXTENSION_REGISTRY_SCHEMA}, got {}",
                    self.schema
                ),
            ));
        }
        if policy.schema != PROVIDER_POLICY_SCHEMA {
            return Err(diagnostic(
                "ASTRA_PROVIDER_POLICY_VERSION_UNSUPPORTED",
                format!("expected {PROVIDER_POLICY_SCHEMA}, got {}", policy.schema),
            ));
        }
        if self
            .providers
            .iter()
            .any(|provider| provider.slot == "game_runtime_provider")
            || self
                .bindings
                .iter()
                .chain(&policy.bindings)
                .any(|binding| binding.slot == "game_runtime_provider")
        {
            return Err(diagnostic(
                "ASTRA_PLUGIN_SLOT_REMOVED",
                "gameplay is statically linked; rebuild obsolete gameplay provider bindings",
            ));
        }
        if policy.profile != profile {
            return Err(diagnostic(
                "ASTRA_PROVIDER_POLICY_PROFILE_MISMATCH",
                "provider policy profile does not match package profile",
            ));
        }
        if !self.conflicts.is_empty() {
            return Err(diagnostic(
                "ASTRA_PLUGIN_EXTENSION_CONFLICT",
                "plugin extension registry contains unresolved conflicts",
            ));
        }

        let mut providers = std::collections::BTreeMap::new();
        for provider in &self.providers {
            for (field, value) in [
                ("slot", provider.slot.as_str()),
                ("provider_id", provider.provider_id.as_str()),
                ("capability", provider.capability.as_str()),
            ] {
                if !is_safe_provider_symbol(value) {
                    return Err(diagnostic(
                        "ASTRA_PLUGIN_PROVIDER_IDENTITY_INVALID",
                        format!("provider {field} is not a safe non-empty symbol"),
                    ));
                }
            }
            let key = (provider.slot.as_str(), provider.provider_id.as_str());
            if providers.insert(key, provider).is_some() {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_PROVIDER_DUPLICATE",
                    format!(
                        "provider {} is duplicated for slot {}",
                        provider.provider_id, provider.slot
                    ),
                ));
            }
        }
        if self.bindings.is_empty() {
            return Err(diagnostic(
                "ASTRA_PLUGIN_BINDING_MISSING",
                "plugin registry contains no explicit provider bindings",
            ));
        }

        let mut registry_bindings = std::collections::BTreeMap::new();
        for binding in &self.bindings {
            binding.validate_identity()?;
            if binding.context.package_id != package_id
                || binding.context.target != target
                || binding.context.profile != profile
            {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_BINDING_CONTEXT_MISMATCH",
                    format!(
                        "binding context does not match package for slot {}",
                        binding.slot
                    ),
                ));
            }
            if registry_bindings
                .insert(binding.slot.as_str(), binding)
                .is_some()
            {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_BINDING_CONFLICT",
                    format!("slot {} has multiple explicit bindings", binding.slot),
                ));
            }
            let provider = providers
                .get(&(binding.slot.as_str(), binding.provider_id.as_str()))
                .ok_or_else(|| {
                    diagnostic(
                        "ASTRA_PLUGIN_BINDING_PROVIDER_MISSING",
                        format!(
                            "bound provider {} is not registered for slot {}",
                            binding.provider_id, binding.slot
                        ),
                    )
                })?;
            if !provider.packaged {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_PACKAGED_INELIGIBLE",
                    format!(
                        "bound provider {} is not package eligible",
                        binding.provider_id
                    ),
                ));
            }
            if provider.capability != binding.context.required_capability {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_BINDING_CAPABILITY_MISMATCH",
                    format!(
                        "bound provider capability does not match slot {}",
                        binding.slot
                    ),
                ));
            }
            if provider.engine_version != binding.context.engine_version
                || provider.rustc_fingerprint != binding.context.rustc_fingerprint
                || provider.feature_fingerprint != binding.context.feature_fingerprint
                || provider.abi_fingerprint != binding.context.abi_fingerprint
            {
                return Err(diagnostic(
                    "ASTRA_PLUGIN_BINDING_FINGERPRINT_MISMATCH",
                    format!(
                        "bound provider fingerprint does not match slot {}",
                        binding.slot
                    ),
                ));
            }
        }

        let mut policy_bindings = std::collections::BTreeMap::new();
        for binding in &policy.bindings {
            binding.validate_identity()?;
            if policy_bindings
                .insert(binding.slot.as_str(), binding)
                .is_some()
            {
                return Err(diagnostic(
                    "ASTRA_PROVIDER_POLICY_BINDING_CONFLICT",
                    format!("provider policy repeats slot {}", binding.slot),
                ));
            }
        }
        if registry_bindings != policy_bindings {
            return Err(diagnostic(
                "ASTRA_PROVIDER_POLICY_BINDING_MISMATCH",
                "provider policy bindings do not exactly match registry bindings",
            ));
        }
        let presentation = registry_bindings.get("presentation").ok_or_else(|| {
            diagnostic(
                "ASTRA_PROVIDER_POLICY_PRESENTATION_BINDING_MISSING",
                "provider policy must bind the presentation slot",
            )
        })?;
        if presentation.provider_id != policy.renderer {
            return Err(diagnostic(
                "ASTRA_PROVIDER_POLICY_RENDERER_MISMATCH",
                "renderer policy does not match the presentation binding",
            ));
        }
        Ok(())
    }

    pub fn validate_for_context(
        &self,
        policy: &ProviderPolicy,
        context: &ProviderBindingContext,
    ) -> Result<(), ProviderRegistryDiagnostic> {
        self.validate_for_package(
            policy,
            &context.package_id,
            &context.target,
            &context.profile,
        )?;
        if self
            .bindings
            .iter()
            .any(|binding| binding.context != *context)
        {
            return Err(diagnostic(
                "ASTRA_PLUGIN_BINDING_CONTEXT_MISMATCH",
                "registry binding fingerprints do not match runtime binding context",
            ));
        }
        Ok(())
    }
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> ProviderRegistryDiagnostic {
    ProviderRegistryDiagnostic {
        code,
        message: message.into(),
    }
}

fn is_safe_provider_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginDependencyGraphSnapshot {
    pub schema: String,
    pub dependencies: Vec<PluginDependency>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiProviderRegistration {
    pub slot: RString,
    pub provider_id: RString,
    pub capability: RString,
    pub phase: RString,
    pub packaged: bool,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiPluginRegistration {
    pub providers: RVec<FfiProviderRegistration>,
    pub callbacks: u32,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiPluginShutdown {
    pub callbacks_released: bool,
}

#[repr(C)]
#[cfg(feature = "ffi")]
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

#[cfg(feature = "ffi")]
impl RootModule for AstraPluginModuleRef {
    abi_stable::declare_root_module_statics! {AstraPluginModuleRef}

    const BASE_NAME: &'static str = "astra_plugin_module";
    const NAME: &'static str = "astra-plugin";
    const VERSION_STRINGS: VersionStrings = abi_stable::package_version_strings!();
}
