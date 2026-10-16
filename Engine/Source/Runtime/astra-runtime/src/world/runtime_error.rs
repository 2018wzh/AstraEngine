use super::*;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("{0}")]
    Message(String),
    #[error("runtime diagnostic: {0:?}")]
    Diagnostic(Diagnostic),
}

impl RuntimeError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn diagnostic(diagnostic: Diagnostic) -> Self {
        Self::Diagnostic(diagnostic)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeConfig {
    pub seed: u64,
    pub required_slots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PackageHandle {
    pub package_id: String,
    pub target: String,
    pub profile: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EngineModuleSlot(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModuleBindingSnapshot {
    pub provider_id: String,
    pub capability: String,
    pub package_id: String,
    pub target: String,
    pub profile: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModuleBindingContext {
    pub package_id: String,
    pub target: String,
    pub profile: String,
    pub engine_version: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub abi_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedModuleBinding {
    pub(super) slot: EngineModuleSlot,
    pub(super) snapshot: ModuleBindingSnapshot,
}

impl ValidatedModuleBinding {
    pub fn validate(
        slot: EngineModuleSlot,
        provider_id: impl Into<String>,
        capability: impl Into<String>,
        context: ModuleBindingContext,
        packaged: bool,
        explicitly_selected: bool,
    ) -> Result<Self, RuntimeError> {
        let provider_id = provider_id.into();
        let capability = capability.into();
        for (code, name, value) in [
            ("ASTRA_RUNTIME_MODULE_SLOT_INVALID", "slot", slot.0.as_str()),
            (
                "ASTRA_RUNTIME_MODULE_PROVIDER_INVALID",
                "provider_id",
                provider_id.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_CAPABILITY_INVALID",
                "capability",
                capability.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_PACKAGE_INVALID",
                "package_id",
                context.package_id.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_TARGET_INVALID",
                "target",
                context.target.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_PROFILE_INVALID",
                "profile",
                context.profile.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_ENGINE_INVALID",
                "engine_version",
                context.engine_version.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_RUSTC_INVALID",
                "rustc_fingerprint",
                context.rustc_fingerprint.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_FEATURE_INVALID",
                "feature_fingerprint",
                context.feature_fingerprint.as_str(),
            ),
            (
                "ASTRA_RUNTIME_MODULE_ABI_INVALID",
                "abi_fingerprint",
                context.abi_fingerprint.as_str(),
            ),
        ] {
            if !is_safe_binding_symbol(value) {
                return Err(RuntimeError::diagnostic(
                    Diagnostic::blocking(code, "module binding contains an invalid identifier")
                        .with_field("field", name),
                ));
            }
        }
        if !packaged {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_NOT_PACKAGED",
                "module provider is not eligible for packaged runtime use",
            )));
        }
        if !explicitly_selected {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_BINDING_MISSING",
                "module provider has no explicit registry binding",
            )));
        }
        Ok(Self {
            slot,
            snapshot: ModuleBindingSnapshot {
                provider_id,
                capability,
                package_id: context.package_id,
                target: context.target,
                profile: context.profile,
                engine_version: context.engine_version,
                rustc_fingerprint: context.rustc_fingerprint,
                feature_fingerprint: context.feature_fingerprint,
                abi_fingerprint: context.abi_fingerprint,
            },
        })
    }

    pub fn slot(&self) -> &EngineModuleSlot {
        &self.slot
    }

    pub fn provider_id(&self) -> &str {
        &self.snapshot.provider_id
    }
}

pub(super) fn is_safe_binding_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl Default for PackageHandle {
    fn default() -> Self {
        Self {
            package_id: "stage1.headless".to_string(),
            target: "headless".to_string(),
            profile: "test".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            rustc_fingerprint: "rustc-stable".to_string(),
            feature_fingerprint: "runtime-typed-v3".to_string(),
            abi_fingerprint: "astra-plugin-abi-v3".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TickInput {
    pub fixed_step: u64,
    pub delta_ns: u64,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TickMode {
    #[default]
    Live,
    RestoreContinuation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TickIntegrityMode {
    #[default]
    Shipping,
    Evidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderedTickIngress {
    pub sequence: u64,
    pub payload: TickIngress,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TickIngress {
    PlayerInput(PlayerInput),
    AwaitCompletion(AwaitCompletion),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TickRequest {
    pub timing: TickInput,
    pub mode: TickMode,
    pub ingress: Vec<OrderedTickIngress>,
}

impl TickRequest {
    pub fn live(timing: TickInput, ingress: Vec<OrderedTickIngress>) -> Self {
        Self {
            timing,
            mode: TickMode::Live,
            ingress,
        }
    }

    pub fn restore_continuation(timing: TickInput, ingress: Vec<OrderedTickIngress>) -> Self {
        Self {
            timing,
            mode: TickMode::RestoreContinuation,
            ingress,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TickReport {
    pub step: u64,
    pub integrity_mode: TickIntegrityMode,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInput {
    pub kind: String,
    #[serde(default)]
    pub payload: EventPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub config: RuntimeConfig,
    pub package: Option<PackageHandle>,
    pub id_source: StableIdGenerator,
    pub actors: ActorStore,
    pub blackboard: Blackboard,
    pub machines: StateMachineStore,
    pub awaits: AwaitQueue,
    pub delayed_events: DelayedEventQueue,
    pub events: EventQueue,
    pub presentation_sequence: u64,
    pub mounted_modules: BTreeMap<String, ModuleBindingSnapshot>,
    pub integrity_mode: TickIntegrityMode,
    pub step: u64,
}

pub struct RuntimeWorld {
    pub(super) tasks: crate::task_scope::TaskRuntime,
    pub(super) failed: bool,
    pub(super) config: RuntimeConfig,
    pub(super) package: Option<PackageHandle>,
    pub(super) id_source: StableIdGenerator,
    pub(super) actors: ActorStore,
    pub(super) blackboard: Blackboard,
    pub(super) events: EventQueue,
    pub(super) awaits: AwaitQueue,
    pub(super) delayed_events: DelayedEventQueue,
    pub(super) machines: StateMachineStore,
    pub(super) actions: ActionRegistry,
    pub(super) presentation: VecDeque<PresentationRecord>,
    pub(super) presentation_sequence: u64,
    pub(super) mutations: VecDeque<RuntimeMutationRecord>,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) mounted_modules: BTreeMap<String, ModuleBindingSnapshot>,
    pub(super) step: u64,
    pub(super) required_tick_mode: TickMode,
    pub(super) integrity_mode: TickIntegrityMode,
    pub(super) machine_worker_count: usize,
}
