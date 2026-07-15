#[cfg(feature = "ffi")]
use abi_stable::{
    library::RootModule,
    sabi_types::VersionStrings,
    std_types::{RString, RVec},
    StableAbi,
};
use astra_core::{Hash256, SchemaVersion};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
pub struct RuntimeOpenRequest {
    pub target_id: String,
    pub profile: String,
    pub locale: String,
    pub seed: u64,
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
pub struct RuntimeStepInput {
    pub session_id: GameRuntimeSessionId,
    pub fixed_step: u64,
    pub delta_ns: u64,
    pub session_seed: u64,
    pub mode: RuntimeStepMode,
    pub action: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStepMode {
    Live,
    RestoreContinuation,
    Replay,
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
pub enum RuntimeOutputCodec {
    Postcard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeOutputSchemaDescriptor {
    pub domain: RuntimeOutputDomain,
    pub schema: String,
    pub version: SchemaVersion,
    pub codec: RuntimeOutputCodec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeOutputEnvelope {
    pub domain: RuntimeOutputDomain,
    pub schema: String,
    pub version: SchemaVersion,
    pub codec: RuntimeOutputCodec,
    pub hash: Hash256,
    pub bytes: Vec<u8>,
}

impl RuntimeOutputEnvelope {
    pub fn postcard<T: Serialize>(
        domain: RuntimeOutputDomain,
        schema: impl Into<String>,
        version: SchemaVersion,
        value: &T,
    ) -> Result<Self, RuntimeEnvelopeError> {
        let payload = postcard::to_allocvec(value).map_err(|err| {
            RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_ENCODE",
                format!("encode runtime output envelope: {err}"),
            )
        })?;
        Ok(Self {
            domain,
            schema: schema.into(),
            version,
            codec: RuntimeOutputCodec::Postcard,
            hash: Hash256::from_sha256(&payload),
            bytes: payload,
        })
    }

    pub fn decode_postcard<T: for<'de> Deserialize<'de>>(
        &self,
        expected_domain: RuntimeOutputDomain,
        expected_schema: &str,
        expected_version: SchemaVersion,
    ) -> Result<T, RuntimeEnvelopeError> {
        if self.domain != expected_domain {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_DOMAIN",
                "runtime output envelope domain does not match consumer",
            ));
        }
        if self.schema != expected_schema {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_SCHEMA",
                "runtime output envelope schema is unknown to consumer",
            ));
        }
        if self.version != expected_version {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_VERSION",
                "runtime output envelope version does not match consumer",
            ));
        }
        if self.codec != RuntimeOutputCodec::Postcard {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_CODEC",
                "runtime output envelope codec does not match consumer",
            ));
        }
        if Hash256::from_sha256(&self.bytes) != self.hash {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_HASH",
                "runtime output envelope hash does not match payload",
            ));
        }
        postcard::from_bytes(&self.bytes).map_err(|err| {
            RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_DECODE",
                format!("decode runtime output envelope: {err}"),
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
                "ASTRA_RUNTIME_ENVELOPE_DOMAIN",
                "runtime output envelope domain does not match consumer",
            ));
        }
        if self.schema != expected_schema {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_SCHEMA",
                "runtime output envelope schema is unknown to consumer",
            ));
        }
        if self.version != expected_version {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_VERSION",
                "runtime output envelope version does not match consumer",
            ));
        }
        if self.codec != RuntimeOutputCodec::Postcard {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_CODEC",
                "runtime output envelope codec does not match consumer",
            ));
        }
        if Hash256::from_sha256(&self.bytes) != self.hash {
            return Err(RuntimeEnvelopeError::new(
                "ASTRA_RUNTIME_ENVELOPE_HASH",
                "runtime output envelope hash does not match payload",
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
    #[serde(default)]
    pub outputs: Vec<RuntimeOutputEnvelope>,
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
#[cfg(feature = "ffi")]
pub type FfiRuntimeProviderInvoke = extern "C" fn(RVec<u8>) -> FfiRuntimeProviderResult;

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeProviderResult {
    pub ok: bool,
    pub payload: RVec<u8>,
    pub diagnostics: RVec<RString>,
}

#[repr(C)]
#[cfg(feature = "ffi")]
#[derive(Debug, Clone, StableAbi)]
pub struct FfiRuntimeProviderRegistration {
    pub provider_id: RString,
    pub runtime_id: RString,
    pub capability: RString,
    pub phase: RString,
    pub packaged: bool,
    pub descriptor_schema: RString,
    pub descriptor_json: RVec<u8>,
    #[sabi(unsafe_opaque_field)]
    pub create_instance: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub destroy_instance: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub prepare: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub probe: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub open: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub step: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub save: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub restore: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub shutdown: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub package_sections: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub release_checks: FfiRuntimeProviderInvoke,
    #[sabi(unsafe_opaque_field)]
    pub editor_metadata: FfiRuntimeProviderInvoke,
}

#[repr(C)]
#[cfg(feature = "ffi")]
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

    extern "C" fn ok_runtime_provider_call(_payload: RVec<u8>) -> FfiRuntimeProviderResult {
        FfiRuntimeProviderResult {
            ok: true,
            payload: RVec::from(Vec::<u8>::new()),
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
            provider_id: RString::from(NATIVE_VN_PROVIDER_ID),
            runtime_id: RString::from(NATIVE_VN_RUNTIME_ID),
            capability: RString::from("runtime.native_vn"),
            phase: RString::from("runtime"),
            packaged: true,
            descriptor_schema: RString::from("astra.product_runtime_descriptor.v1"),
            descriptor_json: RVec::from(descriptor_json),
            create_instance: ok_runtime_provider_call,
            destroy_instance: ok_runtime_provider_call,
            prepare: ok_runtime_provider_call,
            probe: ok_runtime_provider_call,
            open: ok_runtime_provider_call,
            step: ok_runtime_provider_call,
            save: ok_runtime_provider_call,
            restore: ok_runtime_provider_call,
            shutdown: ok_runtime_provider_call,
            package_sections: ok_runtime_provider_call,
            release_checks: ok_runtime_provider_call,
            editor_metadata: ok_runtime_provider_call,
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
        assert!(registration.packaged);
        let roundtrip: ProductRuntimeDescriptor =
            serde_json::from_slice(registration.descriptor_json.as_slice()).unwrap();
        assert_eq!(roundtrip.runtime_id, NATIVE_VN_RUNTIME_ID);
    }
}
