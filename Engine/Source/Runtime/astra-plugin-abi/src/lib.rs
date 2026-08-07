#[cfg(feature = "ffi")]
use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{ROption, RString, RVec},
    StableAbi,
};
use astra_core::{Hash256, SchemaVersion};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const GAME_RUNTIME_PROVIDER_SLOT: &str = "game_runtime_provider";
pub const NATIVE_VN_RUNTIME_ID: &str = "native_vn";
pub const NATIVE_VN_PROVIDER_ID: &str = "astra.runtime.native_vn";
pub const PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA: &str = "astra.product_runtime_descriptor.v1";
pub const RUNTIME_PROVIDER_BINDING_SCHEMA: &str = "astra.runtime_provider_binding.v1";
pub const RUNTIME_EDITOR_METADATA_SCHEMA: &str = "astra.runtime_editor_metadata.v1";
pub const PLUGIN_EXTENSION_REGISTRY_SCHEMA: &str = "astra.plugin_extension_registry.v2";
pub const PROVIDER_POLICY_SCHEMA: &str = "astra.provider_policy.v2";

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
pub struct ProviderPolicy {
    pub schema: String,
    pub profile: String,
    pub renderer: String,
    pub decode_fallback: String,
    pub runtime_provider: ProductRuntimeDescriptor,
    pub bindings: Vec<ProviderBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRuntimeProviderSelection {
    binding: ProviderBinding,
    descriptor: ProductRuntimeDescriptor,
}

impl ValidatedRuntimeProviderSelection {
    pub fn provider_id(&self) -> &str {
        &self.binding.provider_id
    }

    pub fn target(&self) -> &str {
        &self.binding.context.target
    }

    pub fn profile(&self) -> &str {
        &self.binding.context.profile
    }

    pub fn package_id(&self) -> &str {
        &self.binding.context.package_id
    }

    pub fn binding_hash(&self) -> Hash256 {
        self.binding.binding_hash
    }

    pub fn descriptor(&self) -> &ProductRuntimeDescriptor {
        &self.descriptor
    }

    pub fn validate_linked_descriptor(
        &self,
        descriptor: &ProductRuntimeDescriptor,
    ) -> Result<(), ProviderRegistryDiagnostic> {
        if descriptor.provider_id != self.binding.provider_id {
            return Err(diagnostic(
                "ASTRA_RUNTIME_PROVIDER_LINKED_ID_MISMATCH",
                "linked runtime provider id does not match the package-selected binding",
            ));
        }
        if descriptor != &self.descriptor {
            return Err(diagnostic(
                "ASTRA_RUNTIME_PROVIDER_LINKED_DESCRIPTOR_MISMATCH",
                "linked runtime provider descriptor does not exactly match provider.policy",
            ));
        }
        Ok(())
    }
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
    pub fn resolve_embedded_runtime_provider(
        &self,
        policy: &ProviderPolicy,
        package_id: &str,
        profile: &str,
    ) -> Result<ValidatedRuntimeProviderSelection, ProviderRegistryDiagnostic> {
        self.validate_embedded_package(policy, package_id, profile)?;
        let binding = self
            .bindings
            .iter()
            .find(|binding| binding.slot == GAME_RUNTIME_PROVIDER_SLOT)
            .ok_or_else(|| {
                diagnostic(
                    "ASTRA_RUNTIME_PROVIDER_BINDING_MISSING",
                    "package has no explicit game runtime provider binding",
                )
            })?;
        Ok(ValidatedRuntimeProviderSelection {
            binding: binding.clone(),
            descriptor: policy.runtime_provider.clone(),
        })
    }

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
        if policy.profile != profile {
            return Err(diagnostic(
                "ASTRA_PROVIDER_POLICY_PROFILE_MISMATCH",
                "provider policy profile does not match package profile",
            ));
        }
        if !matches!(
            policy.decode_fallback.as_str(),
            "profile_bound" | "forbid" | "required"
        ) {
            return Err(diagnostic(
                "ASTRA_PROVIDER_POLICY_FALLBACK_INVALID",
                "decode fallback must be an explicit supported policy",
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
        let runtime = registry_bindings
            .get(GAME_RUNTIME_PROVIDER_SLOT)
            .ok_or_else(|| {
                diagnostic(
                    "ASTRA_RUNTIME_PROVIDER_BINDING_MISSING",
                    "provider policy must bind the game runtime provider slot",
                )
            })?;
        if runtime.provider_id != policy.runtime_provider.provider_id
            || !policy
                .runtime_provider
                .capabilities
                .contains(&runtime.context.required_capability)
        {
            return Err(diagnostic(
                "ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_MISMATCH",
                "runtime descriptor does not match the bound provider capability",
            ));
        }
        if policy.runtime_provider.output_schemas.is_empty() {
            return Err(diagnostic(
                "ASTRA_RUNTIME_PROVIDER_OUTPUT_SCHEMA_MISSING",
                "runtime provider descriptor must declare its serialized output schemas",
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProductRuntimeDescriptor {
    pub runtime_id: String,
    pub product_kind: String,
    pub provider_id: String,
    pub supported_targets: Vec<String>,
    pub capabilities: Vec<String>,
    pub package_sections: Vec<String>,
    pub release_checks: Vec<String>,
    pub output_schemas: Vec<RuntimeOutputSchemaDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderBinding {
    pub schema: String,
    pub target_id: String,
    pub runtime_id: String,
    pub provider_id: String,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimePrepareRequest {
    pub target_id: String,
    pub profile: String,
    pub package_hash: String,
    pub section_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimePrepareReport {
    pub runtime_id: String,
    pub provider_id: String,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProbeRequest {
    pub target_id: String,
    pub profile: String,
    pub platform: Option<String>,
    pub section_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProbeReport {
    pub runtime_id: String,
    pub provider_id: String,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTickIntegrityMode {
    Shipping,
    Evidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutorKind {
    Serial,
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeExecutorConfig {
    pub kind: RuntimeExecutorKind,
    pub worker_count: u8,
}

impl RuntimeExecutorConfig {
    pub const fn serial() -> Self {
        Self {
            kind: RuntimeExecutorKind::Serial,
            worker_count: 1,
        }
    }

    pub const fn parallel(worker_count: u8) -> Self {
        Self {
            kind: RuntimeExecutorKind::Parallel,
            worker_count,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        if !(1..=8).contains(&self.worker_count)
            || (self.kind == RuntimeExecutorKind::Serial && self.worker_count != 1)
        {
            return Err("ASTRA_RUNTIME_EXECUTOR_CONFIG: executor worker count or kind is invalid");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeOpenRequest {
    pub target_id: String,
    pub profile: String,
    pub locale: String,
    pub seed: u64,
    pub integrity_mode: RuntimeTickIntegrityMode,
    pub executor: RuntimeExecutorConfig,
    pub package_hash: String,
    pub sections: Vec<RuntimeSectionPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct ProviderInstanceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderCreateRequest {
    pub instance_id: ProviderInstanceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderDestroyRequest {
    pub instance_id: ProviderInstanceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderInstanceReport {
    pub instance_id: ProviderInstanceId,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderCall {
    pub instance_id: ProviderInstanceId,
    pub payload: Vec<u8>,
}

/// Opaque provider-owned handle for one opened gameplay runtime session.
///
/// The host must never derive meaning from this value or use it as a pointer.
/// Handles are scoped to one provider instance and become invalid immediately
/// after a successful shutdown.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct RuntimeProviderSessionHandle(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderSessionCall {
    pub instance_id: ProviderInstanceId,
    pub session_handle: RuntimeProviderSessionHandle,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderSessionOpenReport {
    pub session_handle: RuntimeProviderSessionHandle,
    pub report: RuntimeOpenReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GameRuntimeSessionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeOpenReport {
    pub session_id: GameRuntimeSessionId,
    pub runtime_id: String,
    pub provider_id: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeInputEdge {
    pub control: String,
    pub pressed: bool,
    pub value: f32,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeAwaitResult {
    pub token_id: String,
    pub status: String,
    pub payload_len: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProviderResult {
    pub request_id: String,
    pub provider_id: String,
    pub status: String,
    pub payload_len: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeStepBudget {
    pub max_instructions: u64,
    pub max_effects: u32,
    pub max_trace_entries: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeStepInput {
    pub session_id: GameRuntimeSessionId,
    pub fixed_step: u64,
    pub delta_ns: u64,
    pub session_seed: u64,
    pub mode: RuntimeStepMode,
    pub action: String,
    pub argument: Option<String>,
    pub auxiliary: Option<String>,
    pub flag: Option<bool>,
    pub input_edges: Vec<RuntimeInputEdge>,
    pub await_results: Vec<RuntimeAwaitResult>,
    pub provider_results: Vec<RuntimeProviderResult>,
    pub budget: RuntimeStepBudget,
}

impl Default for RuntimeStepInput {
    fn default() -> Self {
        Self {
            session_id: GameRuntimeSessionId(String::new()),
            fixed_step: 0,
            delta_ns: 0,
            session_seed: 0,
            mode: RuntimeStepMode::Live,
            action: String::new(),
            argument: None,
            auxiliary: None,
            flag: None,
            input_edges: Vec::new(),
            await_results: Vec::new(),
            provider_results: Vec::new(),
            budget: RuntimeStepBudget {
                max_instructions: 0,
                max_effects: 0,
                max_trace_entries: 0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStepMode {
    Live,
    RestoreContinuation,
}

/// Runtime-owned presentation data.  This is deliberately a Rust-only live
/// contract: it is not serializable, does not carry a content hash, and is
/// never reused as a package/save/replay envelope.  Providers move the
/// allocation they already own into these values and the host moves it again
/// into the selected renderer/audio queue.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RuntimeLiveOutput {
    pub scenes: Vec<RuntimeLiveSceneTransaction>,
    pub resource_scenes: Vec<RuntimeLiveResourceScene>,
    pub audio: Vec<RuntimeLiveAudioPacket>,
    pub audio_commands: Vec<RuntimeLiveAudioCommand>,
    /// Product audio cue.  A VN cue carries the command identity and sync
    /// contract that cannot be represented by the lower-level stream control
    /// commands.  It is still live-only and never encoded as an output
    /// envelope.
    pub audio_cues: Vec<RuntimeLiveAudioCue>,
    pub text: Vec<RuntimeLiveTextLease>,
    pub text_presentations: Vec<RuntimeLiveTextPresentation>,
    pub video: Vec<RuntimeLiveVideoCommand>,
    pub waits: Vec<RuntimeLiveWait>,
    pub events: Vec<RuntimeLiveEvent>,
    pub blackboard: Vec<RuntimeLiveBlackboardMutation>,
    pub dirty_sections: Vec<RuntimeLiveDirtySection>,
    pub state_revision: u64,
    pub coverage: RuntimeLiveCoverage,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeLiveCoverage {
    pub instructions: u64,
    pub syscalls: u64,
    pub presentation_commands: u64,
    pub audio_commands: u64,
    pub text_events: u64,
    pub capture_bytes: u64,
    pub operation_bytes: u64,
    pub pcm_moved_bytes: u64,
    pub pcm_copied_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveSceneTransaction {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub resources: Vec<RuntimeLiveSceneResourceOperation>,
    pub draws: Vec<RuntimeLiveDraw>,
    pub reset_resources: bool,
}

/// RFVP reserves the maximum texture handle for its immutable 1x1 white
/// texture. It remains a retained typed resource; the live contract accepts
/// it only with the exact built-in descriptor and pixels.
pub const RUNTIME_LIVE_BUILTIN_WHITE_TEXTURE_ID: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveResourceScene {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub textures: Vec<RuntimeLiveResourceTexture>,
    pub draws: Vec<RuntimeLiveDraw>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveResourceTexture {
    pub texture_id: u32,
    pub resource_uri: String,
    pub codec: String,
    pub revision: u64,
    pub decoded_width: u32,
    pub decoded_height: u32,
    pub decoded_format: RuntimeLiveTextureFormat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeLiveSceneResourceOperation {
    CreateTexture {
        texture_id: u32,
        generation: u64,
        width: u32,
        height: u32,
        format: RuntimeLiveTextureFormat,
        pixels: Vec<u8>,
    },
    UpdateTexture {
        texture_id: u32,
        generation: u64,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: RuntimeLiveTextureFormat,
        pixels: Vec<u8>,
    },
    DestroyTexture {
        texture_id: u32,
        generation: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveTextureFormat {
    Rgba8,
    LumaAlpha8,
}

impl RuntimeLiveTextureFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8 => 4,
            Self::LumaAlpha8 => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveDraw {
    pub texture_id: u32,
    pub vertices: [RuntimeLiveVertex; 4],
    pub blend: RuntimeLiveBlendMode,
    pub scissor: Option<RuntimeLiveScissor>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuntimeLiveVertex {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub color: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveBlendMode {
    Alpha,
    Additive,
    Opaque,
    Multiply,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLiveScissor {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveAudioPacket {
    pub sequence: u64,
    pub stream_id: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub pcm: RuntimeLivePcmBuffer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveAudioCue {
    pub sequence: u64,
    pub command_id: String,
    pub bus: RuntimeLiveAudioBus,
    pub asset: String,
    pub looped: bool,
    pub fade_ms: u32,
    pub sync: RuntimeLiveAudioSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveAudioBus {
    Voice,
    Bgm,
    Se,
    Movie,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveAudioSync {
    None,
    Text,
    Fence(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeLiveAudioCommand {
    LoadResource {
        sequence: u64,
        stream_id: u32,
        encoding: RuntimeLiveAudioEncoding,
        resource_uri: String,
    },
    CreateStream {
        sequence: u64,
        stream_id: u32,
        sample_rate: u32,
        channels: u16,
        sample_format: RuntimeLiveAudioSampleFormat,
    },
    SubmitI16 {
        sequence: u64,
        stream_id: u32,
        samples: Vec<i16>,
    },
    SubmitF32 {
        sequence: u64,
        stream_id: u32,
        samples: Vec<f32>,
    },
    Play {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    },
    Stop {
        sequence: u64,
        stream_id: u32,
        fade_ms: u32,
    },
    Pause {
        sequence: u64,
        stream_id: u32,
    },
    Resume {
        sequence: u64,
        stream_id: u32,
    },
    SetParams {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
    },
    DestroyStream {
        sequence: u64,
        stream_id: u32,
    },
    MasterVolume {
        sequence: u64,
        volume: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveAudioEncoding {
    Unknown,
    Wav,
    Ogg,
    Mp3,
    Flac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveAudioSampleFormat {
    I16,
    F32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeLivePcmBuffer {
    I16(Vec<i16>),
    F32(Vec<f32>),
}

impl RuntimeLivePcmBuffer {
    pub fn sample_count(&self) -> usize {
        match self {
            Self::I16(samples) => samples.len(),
            Self::F32(samples) => samples.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sample_count() == 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveTextLease {
    pub sequence: u64,
    pub lease_id: String,
    pub byte_len: u32,
    pub source_ref: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveTextPresentation {
    pub sequence: u64,
    pub lease_id: String,
    pub layout_id: String,
    pub language: String,
    pub font_families: Vec<String>,
    pub body: RuntimeLiveTextRegion,
    pub speaker: Option<RuntimeLiveTextRegion>,
    pub rgba: [u8; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveTextRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub line_height: f32,
    pub max_lines: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeLiveVideoCommand {
    pub sequence: u64,
    pub command: RuntimeLiveVideoCommandKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveVideoCommandKind {
    Play {
        playback_id: String,
        resource_uri: String,
        mode: RuntimeLiveVideoMode,
        stage_width: u32,
        stage_height: u32,
    },
    Stop {
        playback_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLiveVideoMode {
    ModalWithAudio,
    LayerNoAudio,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveWait {
    pub sequence: u64,
    pub token_id: String,
    pub kind: RuntimeLiveWaitKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLiveWaitKind {
    Frame { frames: u32 },
    Time { milliseconds: u32 },
    Input { keys: Vec<String> },
    MediaFence { media_id: String },
    PresentationFence { fence_id: String },
    ProviderCompletion { request_id: String },
    FamilyOpaque { wait_kind: String, payload_len: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveEvent {
    pub sequence: u64,
    pub event: String,
    pub payload: Vec<u8>,
    pub due_tick: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveBlackboardMutation {
    pub sequence: u64,
    pub key: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLiveDirtySection {
    pub sequence: u64,
    pub section_id: String,
}

impl RuntimeLiveSceneTransaction {
    pub fn validate(&self) -> Result<(), RuntimeEnvelopeError> {
        if self.width == 0 || self.height == 0 || self.width > 16_384 || self.height > 16_384 {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_LIVE_SCENE_DIMENSIONS",
                "live scene dimensions are outside the bounded range",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for operation in &self.resources {
            let (texture_id, generation, width, height, format, pixels) = match operation {
                RuntimeLiveSceneResourceOperation::CreateTexture {
                    texture_id,
                    generation,
                    width,
                    height,
                    format,
                    pixels,
                } => (*texture_id, *generation, *width, *height, *format, pixels),
                RuntimeLiveSceneResourceOperation::UpdateTexture {
                    texture_id,
                    generation,
                    x: _,
                    y: _,
                    width,
                    height,
                    format,
                    pixels,
                } => (*texture_id, *generation, *width, *height, *format, pixels),
                RuntimeLiveSceneResourceOperation::DestroyTexture {
                    texture_id,
                    generation,
                } => {
                    if *generation == 0 {
                        return Err(RuntimeEnvelopeError::new(
                            "ASTRA_RUNTIME_LIVE_SCENE_GENERATION",
                            "live scene resource generation must be non-zero",
                        ));
                    }
                    if !ids.insert((*texture_id, *generation)) {
                        return Err(RuntimeEnvelopeError::new(
                            "ASTRA_RUNTIME_LIVE_SCENE_DUPLICATE",
                            "live scene transaction repeats a resource generation",
                        ));
                    }
                    continue;
                }
            };
            let valid_builtin_white = texture_id == RUNTIME_LIVE_BUILTIN_WHITE_TEXTURE_ID
                && matches!(
                    operation,
                    RuntimeLiveSceneResourceOperation::CreateTexture { .. }
                )
                && width == 1
                && height == 1
                && format == RuntimeLiveTextureFormat::Rgba8
                && pixels.as_slice() == [255, 255, 255, 255];
            if (texture_id == RUNTIME_LIVE_BUILTIN_WHITE_TEXTURE_ID && !valid_builtin_white)
                || generation == 0
                || width == 0
                || height == 0
            {
                return Err(RuntimeEnvelopeError::new(
                    "ASTRA_RUNTIME_LIVE_SCENE_RESOURCE",
                    "live scene resource metadata is invalid",
                ));
            }
            let expected = usize::try_from(width).ok().and_then(|width| {
                usize::try_from(height).ok().and_then(|height| {
                    width
                        .checked_mul(height)
                        .and_then(|pixels| pixels.checked_mul(format.bytes_per_pixel()))
                })
            });
            if expected != Some(pixels.len()) || !ids.insert((texture_id, generation)) {
                return Err(RuntimeEnvelopeError::new(
                    "ASTRA_RUNTIME_LIVE_SCENE_PIXELS",
                    "live scene pixel length or resource generation is invalid",
                ));
            }
        }
        for draw in &self.draws {
            if draw.texture_id != u32::MAX && !ids.iter().any(|(id, _)| *id == draw.texture_id) {
                // A draw may refer to a retained texture that is not touched by
                // this transaction.  The host validates that retained state.
                continue;
            }
            if draw.vertices.iter().any(|vertex| {
                !vertex.x.is_finite()
                    || !vertex.y.is_finite()
                    || !vertex.u.is_finite()
                    || !vertex.v.is_finite()
            }) {
                return Err(RuntimeEnvelopeError::new(
                    "ASTRA_RUNTIME_LIVE_SCENE_VERTEX",
                    "live scene contains a non-finite vertex",
                ));
            }
        }
        Ok(())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOutputDomain {
    Effect,
    Presentation,
    Audio,
    Await,
    Observation,
    Trace,
    DirtySaveSection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePersistedCodec {
    Postcard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeOutputSchemaDescriptor {
    pub domain: RuntimeOutputDomain,
    pub schema: String,
    pub version: SchemaVersion,
    pub codec: RuntimePersistedCodec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimePersistedOutput {
    pub domain: RuntimeOutputDomain,
    pub schema: String,
    pub version: SchemaVersion,
    pub codec: RuntimePersistedCodec,
    bytes: Arc<[u8]>,
}

impl RuntimePersistedOutput {
    pub fn postcard<T: Serialize>(
        domain: RuntimeOutputDomain,
        schema: impl Into<String>,
        version: SchemaVersion,
        value: &T,
    ) -> Result<Self, RuntimeEnvelopeError> {
        let payload = postcard::to_allocvec(value).map_err(|err| {
            RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_ENCODE",
                format!("encode runtime persisted output: {err}"),
            )
        })?;
        Ok(Self {
            domain,
            schema: schema.into(),
            version,
            codec: RuntimePersistedCodec::Postcard,
            bytes: payload.into(),
        })
    }

    pub fn postcard_bytes(
        domain: RuntimeOutputDomain,
        schema: impl Into<String>,
        version: SchemaVersion,
        bytes: Arc<[u8]>,
    ) -> Self {
        Self {
            domain,
            schema: schema.into(),
            version,
            codec: RuntimePersistedCodec::Postcard,
            bytes,
        }
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }

    pub fn decode_postcard<T: for<'de> Deserialize<'de>>(
        &self,
        expected_domain: RuntimeOutputDomain,
        expected_schema: &str,
        expected_version: SchemaVersion,
    ) -> Result<T, RuntimeEnvelopeError> {
        if self.domain != expected_domain {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_DOMAIN",
                "runtime persisted output domain does not match consumer",
            ));
        }
        if self.schema != expected_schema {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_SCHEMA",
                "runtime persisted output schema is unknown to consumer",
            ));
        }
        if self.version != expected_version {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_VERSION",
                "runtime persisted output version does not match consumer",
            ));
        }
        if self.codec != RuntimePersistedCodec::Postcard {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_CODEC",
                "runtime persisted output codec does not match consumer",
            ));
        }
        postcard::from_bytes(&self.bytes).map_err(|err| {
            RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_DECODE",
                format!("decode runtime persisted output: {err}"),
            )
        })
    }

    pub fn validate_binding(
        &self,
        expected_domain: RuntimeOutputDomain,
        expected_schema: &str,
        expected_version: SchemaVersion,
    ) -> Result<(), RuntimeEnvelopeError> {
        if self.domain != expected_domain {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_DOMAIN",
                "runtime persisted output domain does not match consumer",
            ));
        }
        if self.schema != expected_schema {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_SCHEMA",
                "runtime persisted output schema is unknown to consumer",
            ));
        }
        if self.version != expected_version {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_VERSION",
                "runtime persisted output version does not match consumer",
            ));
        }
        if self.codec != RuntimePersistedCodec::Postcard {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_PERSISTED_CODEC",
                "runtime persisted output codec does not match consumer",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEnvelopeError {
    code: &'static str,
    message: String,
}

impl RuntimeEnvelopeError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Display for RuntimeEnvelopeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeEnvelopeError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeStepOutput {
    pub session_id: GameRuntimeSessionId,
    pub status: String,
    /// Non-serialized live output. Save/replay use independent section DTOs.
    #[serde(skip)]
    #[schemars(skip)]
    pub live: RuntimeLiveOutput,
    #[serde(default)]
    pub persisted: Vec<RuntimePersistedOutput>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeSaveRequest {
    pub session_id: GameRuntimeSessionId,
    pub slot: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeSaveSections {
    pub session_id: GameRuntimeSessionId,
    pub sections: Vec<RuntimeSectionPayload>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeRestoreRequest {
    pub session_id: GameRuntimeSessionId,
    pub sections: Vec<RuntimeSectionPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeRestoreReport {
    pub session_id: GameRuntimeSessionId,
    pub restored_fixed_step: u64,
    pub session_seed: u64,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeShutdownReport {
    pub session_id: GameRuntimeSessionId,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimePackageSectionPlan {
    pub runtime_id: String,
    pub provider_id: String,
    pub sections: Vec<RuntimeSectionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeSectionRef {
    pub section_id: String,
    pub schema: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSectionCodec {
    Postcard,
    Raw,
    Zstd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeSectionPayload {
    pub section_id: String,
    pub schema: String,
    pub version: SchemaVersion,
    pub codec: RuntimeSectionCodec,
    pub hash: Hash256,
    pub bytes: Vec<u8>,
}

impl RuntimeSectionPayload {
    pub fn validate_hash(&self) -> bool {
        Hash256::from_sha256(&self.bytes) == self.hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReleaseCheckDescriptor {
    pub id: String,
    pub domain: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeEditorMetadata {
    pub schema: String,
    pub runtime_id: String,
    pub product_kind: String,
    pub project_templates: Vec<String>,
    pub authoring_surfaces: Vec<String>,
    pub debug_views: Vec<String>,
    pub release_checks: Vec<String>,
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

#[cfg(feature = "ffi")]
pub type FfiActionInvoke = extern "C" fn(RVec<u8>) -> RVec<u8>;

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeInstanceRequest {
    pub instance_id: RString,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimePrepareRequest {
    pub target_id: RString,
    pub profile: RString,
    pub package_id: RString,
    pub section_ids: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeProbeRequest {
    pub target_id: RString,
    pub profile: RString,
    pub platform: ROption<RString>,
    pub section_ids: RVec<RString>,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeIntegrityMode {
    Shipping,
    Evidence,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeStepMode {
    Live,
    RestoreContinuation,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeSectionCodec {
    Postcard,
    Raw,
    Zstd,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeSection {
    pub section_id: RString,
    pub schema: RString,
    pub version_major: u16,
    pub version_minor: u16,
    pub version_patch: u16,
    pub codec: FfiRuntimeSectionCodec,
    pub bytes: RVec<u8>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeOpenRequest {
    pub instance_id: RString,
    pub target_id: RString,
    pub profile: RString,
    pub locale: RString,
    pub seed: u64,
    pub integrity_mode: FfiRuntimeIntegrityMode,
    pub worker_count: u8,
    pub package_id: RString,
    pub sections: RVec<FfiRuntimeSection>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeInputEdge {
    pub control: RString,
    pub pressed: bool,
    pub value: f32,
    pub sequence: u64,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeAwaitResult {
    pub token_id: RString,
    pub status: RString,
    pub payload_len: u64,
    pub sequence: u64,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeProviderResultItem {
    pub request_id: RString,
    pub provider_id: RString,
    pub status: RString,
    pub payload_len: u64,
    pub sequence: u64,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeStepRequest {
    pub instance_id: RString,
    pub session_handle: u64,
    pub session_id: RString,
    pub fixed_step: u64,
    pub delta_ns: u64,
    pub session_seed: u64,
    pub mode: FfiRuntimeStepMode,
    pub action: RString,
    pub argument: ROption<RString>,
    pub auxiliary: ROption<RString>,
    pub flag: ROption<bool>,
    pub input_edges: RVec<FfiRuntimeInputEdge>,
    pub await_results: RVec<FfiRuntimeAwaitResult>,
    pub provider_results: RVec<FfiRuntimeProviderResultItem>,
    pub max_instructions: u64,
    pub max_effects: u32,
    pub max_trace_entries: u32,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeSaveRequest {
    pub instance_id: RString,
    pub session_handle: u64,
    pub session_id: RString,
    pub slot: RString,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeRestoreRequest {
    pub instance_id: RString,
    pub session_handle: u64,
    pub session_id: RString,
    pub sections: RVec<FfiRuntimeSection>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeShutdownRequest {
    pub instance_id: RString,
    pub session_handle: u64,
    pub session_id: RString,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeTextureFormat {
    Rgba8,
    LumaAlpha8,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeBlendMode {
    Alpha,
    Additive,
    Opaque,
    Multiply,
    Screen,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub struct FfiRuntimeVertex {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub struct FfiRuntimeScissor {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeSceneTextureCreate {
    pub texture_id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub format: FfiRuntimeTextureFormat,
    pub pixels: RVec<u8>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeSceneTextureUpdate {
    pub texture_id: u32,
    pub generation: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub format: FfiRuntimeTextureFormat,
    pub pixels: RVec<u8>,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiRuntimeSceneResourceOperation {
    Create(FfiRuntimeSceneTextureCreate),
    Update(FfiRuntimeSceneTextureUpdate),
    Destroy { texture_id: u32, generation: u64 },
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeDraw {
    pub texture_id: u32,
    pub vertices: [FfiRuntimeVertex; 4],
    pub blend: FfiRuntimeBlendMode,
    pub scissor: ROption<FfiRuntimeScissor>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeSceneTransaction {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub resources: RVec<FfiRuntimeSceneResourceOperation>,
    pub draws: RVec<FfiRuntimeDraw>,
    pub reset_resources: bool,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub enum FfiRuntimePcmBuffer {
    I16(RVec<i16>),
    F32(RVec<f32>),
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeAudioPacket {
    pub sequence: u64,
    pub stream_id: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub pcm: FfiRuntimePcmBuffer,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeTextLease {
    pub sequence: u64,
    pub lease_id: RString,
    pub byte_len: u32,
    pub source_ref: RString,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeVideoCommandKind {
    Play,
    Stop,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeVideoMode {
    ModalWithAudio,
    LayerNoAudio,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeVideoCommand {
    pub sequence: u64,
    pub playback_id: RString,
    pub resource_uri: RString,
    pub mode: FfiRuntimeVideoMode,
    pub stage_width: u32,
    pub stage_height: u32,
    pub command: FfiRuntimeVideoCommandKind,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeWaitKind {
    Frame,
    Time,
    Input,
    MediaFence,
    PresentationFence,
    ProviderCompletion,
    FamilyOpaque,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeWait {
    pub sequence: u64,
    pub token_id: RString,
    pub kind: FfiRuntimeWaitKind,
    pub number: u32,
    pub name: RString,
    pub keys: RVec<RString>,
    pub payload_len: u64,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeEvent {
    pub sequence: u64,
    pub event: RString,
    pub payload: RVec<u8>,
    pub due_tick: ROption<u64>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeBlackboardMutation {
    pub sequence: u64,
    pub key: RString,
    pub value: RVec<u8>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeDirtySection {
    pub sequence: u64,
    pub section_id: RString,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeResourceTexture {
    pub texture_id: u32,
    pub resource_uri: RString,
    pub codec: RString,
    pub revision: u64,
    pub decoded_width: u32,
    pub decoded_height: u32,
    pub decoded_format: FfiRuntimeTextureFormat,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeResourceScene {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub textures: RVec<FfiRuntimeResourceTexture>,
    pub draws: RVec<FfiRuntimeDraw>,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeAudioCommandKind {
    LoadResource,
    CreateStream,
    SubmitI16,
    SubmitF32,
    Play,
    Stop,
    Pause,
    Resume,
    SetParams,
    DestroyStream,
    MasterVolume,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeAudioEncoding {
    Unknown,
    Wav,
    Ogg,
    Mp3,
    Flac,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeAudioSampleFormat {
    I16,
    F32,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeAudioCommand {
    pub sequence: u64,
    pub kind: FfiRuntimeAudioCommandKind,
    pub stream_id: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub encoding: FfiRuntimeAudioEncoding,
    pub sample_format: FfiRuntimeAudioSampleFormat,
    pub resource_uri: RString,
    pub samples: FfiRuntimePcmBuffer,
    pub volume: f32,
    pub pan: f32,
    pub repeat: bool,
    pub fade_ms: u32,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeAudioBus {
    Voice,
    Bgm,
    Se,
    Movie,
}

#[repr(u8)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub enum FfiRuntimeAudioSyncKind {
    None,
    Text,
    Fence,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeAudioCue {
    pub sequence: u64,
    pub command_id: RString,
    pub bus: FfiRuntimeAudioBus,
    pub asset: RString,
    pub looped: bool,
    pub fade_ms: u32,
    pub sync_kind: FfiRuntimeAudioSyncKind,
    pub sync_fence: RString,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, Copy, StableAbi)]
pub struct FfiRuntimeTextRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub line_height: f32,
    pub max_lines: u32,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeTextPresentation {
    pub sequence: u64,
    pub lease_id: RString,
    pub layout_id: RString,
    pub language: RString,
    pub font_families: RVec<RString>,
    pub body: FfiRuntimeTextRegion,
    pub speaker: ROption<FfiRuntimeTextRegion>,
    pub rgba: [u8; 4],
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeLiveOutput {
    pub scenes: RVec<FfiRuntimeSceneTransaction>,
    pub resource_scenes: RVec<FfiRuntimeResourceScene>,
    pub audio: RVec<FfiRuntimeAudioPacket>,
    pub audio_commands: RVec<FfiRuntimeAudioCommand>,
    pub audio_cues: RVec<FfiRuntimeAudioCue>,
    pub text: RVec<FfiRuntimeTextLease>,
    pub text_presentations: RVec<FfiRuntimeTextPresentation>,
    pub video: RVec<FfiRuntimeVideoCommand>,
    pub waits: RVec<FfiRuntimeWait>,
    pub events: RVec<FfiRuntimeEvent>,
    pub blackboard: RVec<FfiRuntimeBlackboardMutation>,
    pub dirty_sections: RVec<FfiRuntimeDirtySection>,
    pub state_revision: u64,
    pub instructions: u64,
    pub syscalls: u64,
    pub presentation_commands: u64,
    pub audio_command_count: u64,
    pub text_events: u64,
    pub capture_bytes: u64,
    pub operation_bytes: u64,
    pub pcm_moved_bytes: u64,
    pub pcm_copied_bytes: u64,
}

#[cfg(feature = "ffi")]
impl FfiRuntimeLiveOutput {
    pub fn empty() -> Self {
        Self {
            scenes: RVec::new(),
            resource_scenes: RVec::new(),
            audio: RVec::new(),
            audio_commands: RVec::new(),
            audio_cues: RVec::new(),
            text: RVec::new(),
            text_presentations: RVec::new(),
            video: RVec::new(),
            waits: RVec::new(),
            events: RVec::new(),
            blackboard: RVec::new(),
            dirty_sections: RVec::new(),
            state_revision: 0,
            instructions: 0,
            syscalls: 0,
            presentation_commands: 0,
            audio_command_count: 0,
            text_events: 0,
            capture_bytes: 0,
            operation_bytes: 0,
            pcm_moved_bytes: 0,
            pcm_copied_bytes: 0,
        }
    }
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimePersistedOutput {
    pub domain: u8,
    pub schema: RString,
    pub version_major: u16,
    pub version_minor: u16,
    pub version_patch: u16,
    pub codec: FfiRuntimeSectionCodec,
    pub bytes: RVec<u8>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeStepResult {
    pub ok: bool,
    pub session_id: RString,
    pub status: RString,
    pub live: FfiRuntimeLiveOutput,
    pub persisted: RVec<FfiRuntimePersistedOutput>,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeSectionResult {
    pub section_id: RString,
    pub schema: RString,
    pub version_major: u16,
    pub version_minor: u16,
    pub version_patch: u16,
    pub codec: FfiRuntimeSectionCodec,
    pub bytes: RVec<u8>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeSaveResult {
    pub ok: bool,
    pub session_id: RString,
    pub sections: RVec<FfiRuntimeSectionResult>,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeRestoreResult {
    pub ok: bool,
    pub session_id: RString,
    pub restored_fixed_step: u64,
    pub session_seed: u64,
    pub status: RString,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeReportResult {
    pub ok: bool,
    pub runtime_id: RString,
    pub provider_id: RString,
    pub status: RString,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeOpenResult {
    pub ok: bool,
    pub session_handle: u64,
    pub session_id: RString,
    pub runtime_id: RString,
    pub provider_id: RString,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeShutdownResult {
    pub ok: bool,
    pub session_id: RString,
    pub status: RString,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimePackageSectionsResult {
    pub ok: bool,
    pub sections: RVec<RString>,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeReleaseChecksResult {
    pub ok: bool,
    pub checks: RVec<RString>,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeEditorMetadataResult {
    pub ok: bool,
    pub schema: RString,
    pub runtime_id: RString,
    pub product_kind: RString,
    pub project_templates: RVec<RString>,
    pub authoring_surfaces: RVec<RString>,
    pub debug_views: RVec<RString>,
    pub release_checks: RVec<RString>,
    pub diagnostics: RVec<RString>,
}

#[cfg(feature = "ffi")]
pub type FfiRuntimeCreateInstance =
    extern "C" fn(FfiRuntimeInstanceRequest) -> FfiRuntimeReportResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeDestroyInstance =
    extern "C" fn(FfiRuntimeInstanceRequest) -> FfiRuntimeReportResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimePrepare = extern "C" fn(FfiRuntimePrepareRequest) -> FfiRuntimeReportResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeProbe = extern "C" fn(FfiRuntimeProbeRequest) -> FfiRuntimeReportResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeOpen = extern "C" fn(FfiRuntimeOpenRequest) -> FfiRuntimeOpenResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeStep = extern "C" fn(FfiRuntimeStepRequest) -> FfiRuntimeStepResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeSave = extern "C" fn(FfiRuntimeSaveRequest) -> FfiRuntimeSaveResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeRestore = extern "C" fn(FfiRuntimeRestoreRequest) -> FfiRuntimeRestoreResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeShutdown = extern "C" fn(FfiRuntimeShutdownRequest) -> FfiRuntimeShutdownResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimePackageSections = extern "C" fn() -> FfiRuntimePackageSectionsResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeReleaseChecks = extern "C" fn() -> FfiRuntimeReleaseChecksResult;
#[cfg(feature = "ffi")]
pub type FfiRuntimeEditorMetadata = extern "C" fn() -> FfiRuntimeEditorMetadataResult;

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeProviderRegistration {
    pub abi_version: u32,
    pub provider_id: RString,
    pub runtime_id: RString,
    pub capability: RString,
    pub phase: RString,
    pub packaged: bool,
    pub descriptor_schema: RString,
    pub descriptor_json: RVec<u8>,
    #[sabi(unsafe_opaque_field)]
    pub create_instance: FfiRuntimeCreateInstance,
    #[sabi(unsafe_opaque_field)]
    pub destroy_instance: FfiRuntimeDestroyInstance,
    #[sabi(unsafe_opaque_field)]
    pub prepare: FfiRuntimePrepare,
    #[sabi(unsafe_opaque_field)]
    pub probe: FfiRuntimeProbe,
    #[sabi(unsafe_opaque_field)]
    pub open_session: FfiRuntimeOpen,
    #[sabi(unsafe_opaque_field)]
    pub step: FfiRuntimeStep,
    #[sabi(unsafe_opaque_field)]
    pub save: FfiRuntimeSave,
    #[sabi(unsafe_opaque_field)]
    pub restore: FfiRuntimeRestore,
    #[sabi(unsafe_opaque_field)]
    pub shutdown: FfiRuntimeShutdown,
    #[sabi(unsafe_opaque_field)]
    pub package_sections: FfiRuntimePackageSections,
    #[sabi(unsafe_opaque_field)]
    pub release_checks: FfiRuntimeReleaseChecks,
    #[sabi(unsafe_opaque_field)]
    pub editor_metadata: FfiRuntimeEditorMetadata,
}

#[cfg(feature = "ffi")]
pub const PRODUCT_RUNTIME_PROVIDER_ABI_VERSION: u32 = 3;

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiActionRegistration {
    pub abi_version: u32,
    pub provider_id: RString,
    pub action_id: RString,
    pub input_schema: RString,
    pub output_schema: RString,
    /// JSON-encoded host `ActionDescriptor` including access declarations,
    /// execution class, and StableId reservation.
    pub descriptor_json: RVec<u8>,
    #[sabi(unsafe_opaque_field)]
    pub invoke: FfiActionInvoke,
}

#[cfg(feature = "ffi")]
pub const ACTION_PLUGIN_ABI_VERSION: u32 = 2;

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiPluginRegistration {
    pub providers: RVec<FfiProviderRegistration>,
    pub runtime_providers: RVec<FfiRuntimeProviderRegistration>,
    pub actions: RVec<FfiActionRegistration>,
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

#[cfg(all(test, feature = "ffi"))]
mod tests {
    use super::*;

    extern "C" fn ok_runtime_report(_request: FfiRuntimeInstanceRequest) -> FfiRuntimeReportResult {
        FfiRuntimeReportResult {
            ok: true,
            runtime_id: RString::new(),
            provider_id: RString::new(),
            status: RString::from("ok"),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_prepare(_request: FfiRuntimePrepareRequest) -> FfiRuntimeReportResult {
        ok_runtime_report(FfiRuntimeInstanceRequest {
            instance_id: RString::new(),
        })
    }

    extern "C" fn ok_probe(_request: FfiRuntimeProbeRequest) -> FfiRuntimeReportResult {
        ok_runtime_report(FfiRuntimeInstanceRequest {
            instance_id: RString::new(),
        })
    }

    extern "C" fn ok_open(_request: FfiRuntimeOpenRequest) -> FfiRuntimeOpenResult {
        FfiRuntimeOpenResult {
            ok: true,
            session_handle: 0,
            session_id: RString::new(),
            runtime_id: RString::new(),
            provider_id: RString::new(),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_step(_request: FfiRuntimeStepRequest) -> FfiRuntimeStepResult {
        FfiRuntimeStepResult {
            ok: true,
            session_id: RString::new(),
            status: RString::from("ok"),
            live: FfiRuntimeLiveOutput::empty(),
            persisted: RVec::new(),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_save(_request: FfiRuntimeSaveRequest) -> FfiRuntimeSaveResult {
        FfiRuntimeSaveResult {
            ok: true,
            session_id: RString::new(),
            sections: RVec::new(),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_restore(_request: FfiRuntimeRestoreRequest) -> FfiRuntimeRestoreResult {
        FfiRuntimeRestoreResult {
            ok: true,
            session_id: RString::new(),
            restored_fixed_step: 0,
            session_seed: 0,
            status: RString::from("ok"),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_shutdown(_request: FfiRuntimeShutdownRequest) -> FfiRuntimeShutdownResult {
        FfiRuntimeShutdownResult {
            ok: true,
            session_id: RString::new(),
            status: RString::from("ok"),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_package() -> FfiRuntimePackageSectionsResult {
        FfiRuntimePackageSectionsResult {
            ok: true,
            sections: RVec::new(),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_release() -> FfiRuntimeReleaseChecksResult {
        FfiRuntimeReleaseChecksResult {
            ok: true,
            checks: RVec::new(),
            diagnostics: RVec::new(),
        }
    }

    extern "C" fn ok_editor() -> FfiRuntimeEditorMetadataResult {
        FfiRuntimeEditorMetadataResult {
            ok: true,
            schema: RString::new(),
            runtime_id: RString::new(),
            product_kind: RString::new(),
            project_templates: RVec::new(),
            authoring_surfaces: RVec::new(),
            debug_views: RVec::new(),
            release_checks: RVec::new(),
            diagnostics: RVec::new(),
        }
    }

    #[astra_headless_test::test]
    fn runtime_provider_abi_registers_descriptor_and_entrypoints() {
        let descriptor = ProductRuntimeDescriptor {
            runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
            product_kind: "visual_novel".to_string(),
            provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
            supported_targets: vec!["game".to_string()],
            capabilities: vec!["runtime.native_vn".to_string()],
            package_sections: vec!["vn.story".to_string()],
            release_checks: vec!["runtime_provider.native_vn".to_string()],
            output_schemas: Vec::new(),
        };
        let descriptor_json = serde_json::to_vec(&descriptor).unwrap();
        let registration = FfiRuntimeProviderRegistration {
            abi_version: PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
            provider_id: RString::from(NATIVE_VN_PROVIDER_ID),
            runtime_id: RString::from(NATIVE_VN_RUNTIME_ID),
            capability: RString::from("runtime.native_vn"),
            phase: RString::from("runtime"),
            packaged: true,
            descriptor_schema: RString::from("astra.product_runtime_descriptor.v1"),
            descriptor_json: RVec::from(descriptor_json),
            create_instance: ok_runtime_report,
            destroy_instance: ok_runtime_report,
            prepare: ok_prepare,
            probe: ok_probe,
            open_session: ok_open,
            step: ok_step,
            save: ok_save,
            restore: ok_restore,
            shutdown: ok_shutdown,
            package_sections: ok_package,
            release_checks: ok_release,
            editor_metadata: ok_editor,
        };

        let plugin = FfiPluginRegistration {
            providers: RVec::new(),
            runtime_providers: RVec::from(vec![registration.clone()]),
            actions: RVec::new(),
            callbacks: 0,
        };

        assert_eq!(GAME_RUNTIME_PROVIDER_SLOT, "game_runtime_provider");
        assert_eq!(plugin.runtime_providers.len(), 1);
        assert_eq!(registration.provider_id.as_str(), NATIVE_VN_PROVIDER_ID);
        assert_eq!(
            registration.abi_version,
            PRODUCT_RUNTIME_PROVIDER_ABI_VERSION
        );
        assert!(registration.packaged);
        let roundtrip: ProductRuntimeDescriptor =
            serde_json::from_slice(registration.descriptor_json.as_slice()).unwrap();
        assert_eq!(roundtrip.runtime_id, NATIVE_VN_RUNTIME_ID);
    }
}
