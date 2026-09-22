mod tasks;

use std::{collections::BTreeMap, time::Instant};

use astra_core::{
    Diagnostic, SchemaId, SchemaMigrationRegistry, SchemaVersion, StableId, StableIdGenerator,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, trace, warn};

use crate::{
    ActionRegistry, ActorId, ActorRecord, ActorSnapshot, ActorStore, AwaitCompletion,
    AwaitCompletionHandle, AwaitQueue, AwaitToken, Blackboard, BlackboardValue, ComponentId,
    ComponentRecord, ComponentSnapshot, CreateAwaitAction, DelayedEventId, DelayedEventQueue,
    EmitEventAction, EventId, EventPayload, EventQueue, EventSource, PresentationAction,
    PresentationCommand, PresentationRecord, RuntimeAction, RuntimeComponentPayload, RuntimeEvent,
    RuntimeMutationRecord, SaveBlob, SaveRequest, ScheduledEvent, SetBlackboardAction,
    StateMachineDefinition, StateMachineSnapshot, StateMachineStore, TaskScope,
};

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
    slot: EngineModuleSlot,
    snapshot: ModuleBindingSnapshot,
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

fn is_safe_binding_symbol(value: &str) -> bool {
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
    pub presentation: Vec<PresentationRecord>,
    pub mutations: Vec<RuntimeMutationRecord>,
    pub mounted_modules: BTreeMap<String, ModuleBindingSnapshot>,
    pub integrity_mode: TickIntegrityMode,
    pub step: u64,
}

pub struct RuntimeWorld {
    tasks: crate::task_scope::TaskRuntime,
    failed: bool,
    config: RuntimeConfig,
    package: Option<PackageHandle>,
    id_source: StableIdGenerator,
    actors: ActorStore,
    blackboard: Blackboard,
    events: EventQueue,
    awaits: AwaitQueue,
    delayed_events: DelayedEventQueue,
    machines: StateMachineStore,
    actions: ActionRegistry,
    presentation: Vec<PresentationRecord>,
    mutations: Vec<RuntimeMutationRecord>,
    diagnostics: Vec<Diagnostic>,
    mounted_modules: BTreeMap<String, ModuleBindingSnapshot>,
    step: u64,
    required_tick_mode: TickMode,
    integrity_mode: TickIntegrityMode,
    machine_worker_count: usize,
}

impl RuntimeWorld {
    pub fn create(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let integrity_mode = if cfg!(debug_assertions) {
            TickIntegrityMode::Evidence
        } else {
            TickIntegrityMode::Shipping
        };
        Self::create_with_integrity(config, integrity_mode)
    }

    pub fn create_with_integrity(
        config: RuntimeConfig,
        integrity_mode: TickIntegrityMode,
    ) -> Result<Self, RuntimeError> {
        let mut actions = ActionRegistry::default();
        actions.register(SetBlackboardAction)?;
        actions.register(EmitEventAction)?;
        actions.register(CreateAwaitAction)?;
        actions.register(PresentationAction)?;
        info!(
            seed = config.seed,
            required_slot_count = config.required_slots.len(),
            integrity_mode = ?integrity_mode,
            default_action_count = 4,
            "runtime.create"
        );
        Ok(Self {
            tasks: crate::task_scope::TaskRuntime::default(),
            failed: false,
            id_source: StableIdGenerator::new(config.seed),
            config,
            package: None,
            actors: ActorStore::default(),
            blackboard: Blackboard::default(),
            events: EventQueue::default(),
            awaits: AwaitQueue::default(),
            delayed_events: DelayedEventQueue::default(),
            machines: StateMachineStore::default(),
            actions,
            presentation: Vec::new(),
            mutations: Vec::new(),
            diagnostics: Vec::new(),
            mounted_modules: BTreeMap::new(),
            step: 0,
            required_tick_mode: TickMode::Live,
            integrity_mode,
            machine_worker_count: 1,
        })
    }

    pub fn seed(&self) -> u64 {
        self.config.seed
    }

    pub fn tick_integrity_mode(&self) -> TickIntegrityMode {
        self.integrity_mode
    }

    pub fn set_machine_worker_count(&mut self, worker_count: usize) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        if !(1..=8).contains(&worker_count) {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_WORKER_COUNT",
                "machine worker count must be within 1..=8",
            )));
        }
        if self.step != 0 {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_WORKER_COUNT_LIFECYCLE",
                "machine worker count must be configured before the first fixed tick",
            )));
        }
        self.machine_worker_count = worker_count;
        Ok(())
    }

    /// Attach product identity only for hosts using packaged module bindings.
    /// Standalone worlds use the same state, tick and save implementation without it.
    pub fn with_package(mut self, package: PackageHandle) -> Result<Self, RuntimeError> {
        self.ensure_active()?;
        if self.step != 0 || self.package.is_some() || !self.mounted_modules.is_empty() {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_PACKAGE_LIFECYCLE",
                "package identity must be attached once before ticking or mounting modules",
            )));
        }
        if [
            &package.package_id,
            &package.target,
            &package.profile,
            &package.engine_version,
            &package.rustc_fingerprint,
            &package.feature_fingerprint,
            &package.abi_fingerprint,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256 || value.chars().any(char::is_control))
        {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_PACKAGE_IDENTITY",
                "package identity fields are invalid",
            )));
        }
        self.package = Some(package);
        Ok(self)
    }

    pub fn package_id(&self) -> Option<&str> {
        self.package
            .as_ref()
            .map(|package| package.package_id.as_str())
    }

    pub fn package_handle(&self) -> Option<&PackageHandle> {
        self.package.as_ref()
    }

    pub fn mount_module(
        &mut self,
        slot: EngineModuleSlot,
        binding: ValidatedModuleBinding,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        if binding.slot != slot {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_SLOT_MISMATCH",
                "module binding token does not match the requested slot",
            )));
        }
        let package = self.package.as_ref().ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_PACKAGE_REQUIRED",
                "packaged module bindings require a host-supplied package identity",
            ))
        })?;
        let expected_context = ModuleBindingContext {
            package_id: package.package_id.clone(),
            target: package.target.clone(),
            profile: package.profile.clone(),
            engine_version: package.engine_version.clone(),
            rustc_fingerprint: package.rustc_fingerprint.clone(),
            feature_fingerprint: package.feature_fingerprint.clone(),
            abi_fingerprint: package.abi_fingerprint.clone(),
        };
        let actual_context = ModuleBindingContext {
            package_id: binding.snapshot.package_id.clone(),
            target: binding.snapshot.target.clone(),
            profile: binding.snapshot.profile.clone(),
            engine_version: binding.snapshot.engine_version.clone(),
            rustc_fingerprint: binding.snapshot.rustc_fingerprint.clone(),
            feature_fingerprint: binding.snapshot.feature_fingerprint.clone(),
            abi_fingerprint: binding.snapshot.abi_fingerprint.clone(),
        };
        if actual_context != expected_context {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_CONTEXT_MISMATCH",
                "module binding token does not match package target/profile/fingerprint context",
            )));
        }
        if self.mounted_modules.contains_key(&slot.0) {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_MODULE_SLOT_OCCUPIED",
                    "runtime module slot is already mounted",
                )
                .with_field("slot", &slot.0),
            ));
        }
        info!(slot = %slot.0, provider_id = %binding.snapshot.provider_id, "runtime.module.mount");
        self.mounted_modules.insert(slot.0, binding.snapshot);
        Ok(())
    }

    pub fn register_action<A: RuntimeAction + 'static>(
        &mut self,
        provider_id: impl Into<String>,
        action: A,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        let provider_id = provider_id.into();
        let action_id = action.descriptor().id;
        info!(
            provider_id = %provider_id,
            action_id = %action_id,
            "runtime.action.register"
        );
        self.actions.register_with_provider(provider_id, action)
    }

    pub fn unregister_action_provider(&mut self, provider_id: &str) {
        info!(provider_id, "runtime.action.unregister_provider");
        self.actions.unregister_provider(provider_id);
    }

    pub fn create_actor(
        &mut self,
        name: impl Into<String>,
        tags: Vec<String>,
    ) -> Result<ActorId, RuntimeError> {
        self.ensure_active()?;
        let actor_id = ActorId(self.next_id());
        debug!(?actor_id, tag_count = tags.len(), "runtime.actor.create");
        self.actors.insert_actor(ActorRecord {
            actor_id,
            name: name.into(),
            tags,
            components: Vec::new(),
        });
        Ok(actor_id)
    }

    pub fn attach_component<T>(
        &mut self,
        actor_id: ActorId,
        schema: impl Into<SchemaId>,
        data: &T,
    ) -> Result<ComponentId, RuntimeError>
    where
        T: Serialize + Clone + std::fmt::Debug + Send + Sync + 'static,
    {
        self.ensure_active()?;
        let component_id = ComponentId(self.next_id());
        let schema = schema.into();
        let payload =
            RuntimeComponentPayload::typed(schema.clone(), SchemaVersion::default(), data.clone());
        let attached = self.actors.attach_component(ComponentRecord {
            component_id,
            actor_id,
            payload,
        });
        if attached {
            debug!(
                ?actor_id,
                ?component_id,
                schema = %schema,
                "runtime.component.attach"
            );
            Ok(component_id)
        } else {
            let diagnostic = Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTOR_MISSING",
                format!("cannot attach component to missing actor {actor_id:?}"),
            );
            warn!(
                ?actor_id,
                diagnostic_code = %diagnostic.code,
                "runtime.diagnostic"
            );
            Err(RuntimeError::diagnostic(diagnostic))
        }
    }

    pub fn read_component<T>(&self, component_id: ComponentId) -> Result<T, RuntimeError>
    where
        T: Serialize + DeserializeOwned + Clone + std::fmt::Debug + Send + Sync + 'static,
    {
        let component = self.actors.component(component_id).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_MISSING",
                "runtime component does not exist",
            ))
        })?;
        component.payload.decode()
    }

    pub fn replace_component<T>(
        &mut self,
        component_id: ComponentId,
        data: &T,
    ) -> Result<(), RuntimeError>
    where
        T: Serialize + Clone + std::fmt::Debug + Send + Sync + 'static,
    {
        self.ensure_active()?;
        let component = self.actors.component_mut(component_id).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_MISSING",
                "runtime component does not exist",
            ))
        })?;
        let before_revision = component.payload.revision;
        let schema = component.payload.schema.clone();
        let payload = RuntimeComponentPayload::typed(
            component.payload.schema.clone(),
            component.payload.version,
            data.clone(),
        );
        let after_revision = before_revision.checked_add(1).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_REVISION_OVERFLOW",
                "runtime component revision overflowed",
            ))
        })?;
        let mut payload = payload;
        payload.revision = after_revision;
        component.payload = payload;
        self.mutations.push(RuntimeMutationRecord {
            step: self.step,
            component_id,
            schema,
            before_revision,
            after_revision,
            source: "runtime.api".to_string(),
        });
        Ok(())
    }

    pub fn remove_actor(&mut self, actor_id: ActorId) -> Result<bool, RuntimeError> {
        self.ensure_active()?;
        let removed = self.actors.remove_actor(actor_id).is_some();
        debug!(?actor_id, removed, "runtime.actor.remove");
        Ok(removed)
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> Result<bool, RuntimeError> {
        self.ensure_active()?;
        let detached = self.actors.detach_component(component_id).is_some();
        debug!(?component_id, detached, "runtime.component.detach");
        Ok(detached)
    }

    pub fn add_state_machine(
        &mut self,
        definition: StateMachineDefinition,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        debug!(
            machine_id = ?definition.id,
            owner = ?definition.owner,
            state_count = definition.states.len(),
            transition_count = definition.transitions.len(),
            "runtime.state_machine.add"
        );
        self.machines.add(definition)?;
        Ok(())
    }

    pub fn emit_event(
        &mut self,
        source: EventSource,
        payload: EventPayload,
    ) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        let kind = payload.kind.clone();
        let event = RuntimeEvent {
            id: EventId(self.next_id()),
            source,
            step: self.step,
            sequence: 0,
            payload,
        };
        debug!(
            event_id = ?event.id,
            source = ?event.source,
            step = event.step,
            kind = %kind,
            "runtime.event.emit"
        );
        self.events.push(event);
        Ok(())
    }

    pub fn enqueue_event(&mut self, event: RuntimeEvent) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        debug!(
            event_id = ?event.id,
            source = ?event.source,
            step = event.step,
            kind = %event.payload.kind,
            "runtime.event.enqueue"
        );
        self.events.push(event);
        Ok(())
    }

    pub fn schedule_event(
        &mut self,
        due_tick: u64,
        source: EventSource,
        payload: EventPayload,
    ) -> Result<DelayedEventId, RuntimeError> {
        self.ensure_active()?;
        let kind = payload.kind.clone();
        let source_for_log = source.clone();
        let event = ScheduledEvent {
            id: DelayedEventId(self.next_id()),
            due_tick,
            sequence: 0,
            source,
            payload,
        };
        let id = self.delayed_events.schedule(event);
        debug!(
            ?id,
            due_tick,
            source = ?source_for_log,
            kind = %kind,
            "runtime.delayed_event.schedule"
        );
        Ok(id)
    }

    pub fn cancel_delayed_event(&mut self, id: DelayedEventId) -> Result<bool, RuntimeError> {
        self.ensure_active()?;
        let cancelled = self.delayed_events.cancel(id);
        debug!(?id, cancelled, "runtime.delayed_event.cancel");
        Ok(cancelled)
    }

    pub fn emit_presentation(&mut self, command: PresentationCommand) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        let sequence = self.presentation.len() as u64;
        debug!(
            step = self.step,
            sequence,
            command_kind = presentation_kind(&command),
            "runtime.presentation.emit"
        );
        self.presentation.push(PresentationRecord {
            step: self.step,
            sequence,
            command,
        });
        Ok(())
    }

    pub fn insert_await_token(&mut self, token: AwaitToken) -> Result<(), RuntimeError> {
        self.ensure_active()?;
        debug!(
            token_id = ?token.token_id,
            requested_at_step = token.requested_at_step,
            timeout_step = ?token.timeout_step,
            "runtime.await.insert"
        );
        self.awaits.insert(token).map_err(RuntimeError::diagnostic)
    }

    fn apply_input(&mut self, input: PlayerInput) -> Result<(), RuntimeError> {
        let mut payload = input.payload;
        if payload.kind.is_empty() {
            payload.kind = input.kind;
        }
        self.emit_event(EventSource::PlayerInput, payload)?;
        Ok(())
    }

    pub fn is_failed(&self) -> bool {
        self.failed
    }

    fn ensure_active(&self) -> Result<(), RuntimeError> {
        if self.failed {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_SESSION_FAILED",
                "runtime execution failed; restore a saved state or recreate the world",
            )));
        }
        Ok(())
    }

    pub fn tick(&mut self, request: TickRequest) -> Result<TickReport, RuntimeError> {
        self.ensure_active()?;
        self.validate_tick_request(&request)?;
        let started = Instant::now();
        let performance_step = request.timing.fixed_step;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for ingress in request.ingress {
                match ingress.payload {
                    TickIngress::PlayerInput(input) => self.apply_input(input)?,
                    TickIngress::AwaitCompletion(result) => self.submit_await_result(result),
                }
            }
            let report = self.tick_validated(request.timing)?;
            self.required_tick_mode = TickMode::Live;
            Ok(report)
        }))
        .unwrap_or_else(|_| {
            Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_EXECUTION_PANIC",
                "runtime action execution panicked",
            )))
        });
        self.failed = result.is_err();
        if self.failed {
            self.tasks.scope().cancel();
        }
        trace!(
            event = "runtime.tick.execution.performance",
            step = performance_step,
            execution_ns = started.elapsed().as_nanos() as u64,
            succeeded = result.is_ok(),
            "measured RuntimeWorld tick execution"
        );
        result
    }

    pub(crate) fn validate_tick_request(&self, request: &TickRequest) -> Result<(), RuntimeError> {
        let input = request.timing;
        if request.mode != self.required_tick_mode {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_MODE_INVALID",
                    "runtime tick mode does not match world lifecycle state",
                )
                .with_field("expected_mode", format!("{:?}", self.required_tick_mode))
                .with_field("actual_mode", format!("{:?}", request.mode)),
            ));
        }
        let mut previous_sequence = 0;
        for ingress in &request.ingress {
            if ingress.sequence == 0 || ingress.sequence <= previous_sequence {
                if self.integrity_mode == TickIntegrityMode::Evidence {
                    return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                        "ASTRA_RUNTIME_TICK_INGRESS_ORDER_INVALID",
                        "tick ingress sequence must be non-zero and strictly increasing",
                    )));
                } else {
                    warn!(
                        sequence = ingress.sequence,
                        previous = previous_sequence,
                        "runtime.tick.ingress_order_warn"
                    );
                }
            }
            previous_sequence = ingress.sequence;
        }
        let expected_step = self.step.checked_add(1).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_TICK_STEP_OVERFLOW",
                "runtime fixed step cannot advance beyond u64::MAX",
            ))
        })?;
        if input.fixed_step != expected_step {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_STEP_INVALID",
                    "runtime tick must advance by exactly one fixed step",
                )
                .with_field("current_step", self.step)
                .with_field("expected_step", expected_step)
                .with_field("actual_step", input.fixed_step),
            ));
        }
        if input.delta_ns == 0 || input.delta_ns > 1_000_000_000 {
            if self.integrity_mode == TickIntegrityMode::Evidence {
                return Err(RuntimeError::diagnostic(
                    Diagnostic::blocking(
                        "ASTRA_RUNTIME_TICK_DELTA_INVALID",
                        "runtime tick delta must be within the supported fixed-step range",
                    )
                    .with_field("delta_ns", input.delta_ns),
                ));
            } else {
                warn!(delta_ns = input.delta_ns, "runtime.tick.delta_warn");
            }
        }
        if input.seed != self.config.seed {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_SEED_MISMATCH",
                    "runtime tick seed does not match the session seed",
                )
                .with_field("expected_seed", self.config.seed)
                .with_field("actual_seed", input.seed),
            ));
        }
        if let Some(slot) = self
            .config
            .required_slots
            .iter()
            .find(|slot| !self.mounted_modules.contains_key(*slot))
        {
            if self.integrity_mode == TickIntegrityMode::Evidence {
                return Err(RuntimeError::diagnostic(
                    Diagnostic::blocking(
                        "ASTRA_RUNTIME_MODULE_MISSING",
                        "runtime required module slot is not mounted",
                    )
                    .with_field("slot", slot),
                ));
            } else {
                warn!(slot = %slot, "runtime.tick.module_missing_warn");
            }
        }
        Ok(())
    }

    fn tick_validated(&mut self, input: TickInput) -> Result<TickReport, RuntimeError> {
        debug!(
            step = input.fixed_step,
            delta_ns = input.delta_ns,
            required_slot_count = self.config.required_slots.len(),
            "runtime.tick.start"
        );
        self.step = input.fixed_step;
        self.id_source.set_step(input.fixed_step);
        self.diagnostics.clear();
        for token_id in self.tasks.cancelled() {
            self.cancel_await(token_id)?;
        }
        let await_drain = self.awaits.drain_ordered_results(input.fixed_step);
        for diagnostic in &await_drain.diagnostics {
            warn!(
                step = input.fixed_step,
                diagnostic_code = %diagnostic.code,
                "runtime.diagnostic"
            );
        }
        self.diagnostics.extend(await_drain.diagnostics);
        debug!(
            step = input.fixed_step,
            count = await_drain.results.len(),
            "runtime.await.drain"
        );
        for result in await_drain.results {
            self.tasks.finish(result.token_id, false);
            let id = EventId(self.next_id());
            self.events.push(RuntimeEvent {
                id,
                source: EventSource::AwaitResult,
                step: input.fixed_step,
                sequence: result.sequence,
                payload: result.payload,
            });
        }
        let delayed_events = self.delayed_events.drain_due(input.fixed_step);
        debug!(
            step = input.fixed_step,
            count = delayed_events.len(),
            "runtime.delayed_event.drain"
        );
        for event in delayed_events {
            self.events.push(event);
        }
        let ready = self.events.drain_ordered_for_step(input.fixed_step);
        debug!(
            step = input.fixed_step,
            count = ready.len(),
            "runtime.event.drain"
        );
        let machine_started = Instant::now();
        let output = self.machines.tick(
            input.fixed_step,
            &ready,
            &mut self.actors,
            &mut self.blackboard,
            &self.actions,
            &mut self.id_source,
            self.machine_worker_count,
            self.integrity_mode == TickIntegrityMode::Evidence,
        );
        let machine_ns = machine_started.elapsed().as_nanos() as u64;
        for diagnostic in &output.diagnostics {
            warn!(
                step = input.fixed_step,
                diagnostic_code = %diagnostic.code,
                "runtime.diagnostic"
            );
        }
        let output_diagnostic_count = output.diagnostics.len();
        self.diagnostics.extend(output.diagnostics);
        for id in output.delayed_cancellations {
            self.delayed_events.cancel(id);
        }
        for event in output.delayed_events {
            self.delayed_events.schedule(event);
        }
        for event in output.events {
            self.events.push(event);
        }
        for await_token in output.awaits {
            self.awaits
                .insert(await_token)
                .map_err(RuntimeError::diagnostic)?;
        }
        for command in output.presentation {
            self.emit_presentation(command)?;
        }
        self.mutations.extend(output.mutations);
        if let Some(diagnostic) = self.diagnostics.iter().find(|diagnostic| {
            matches!(
                diagnostic.severity,
                astra_core::DiagnosticSeverity::Blocking | astra_core::DiagnosticSeverity::Error
            )
        }) {
            return Err(RuntimeError::diagnostic(diagnostic.clone()));
        }
        let report = TickReport {
            step: input.fixed_step,
            integrity_mode: self.integrity_mode,
            diagnostics: self.diagnostics.clone(),
        };
        trace!(
            event = "runtime.tick.performance",
            step = report.step,
            diagnostic_count = report.diagnostics.len(),
            output_diagnostic_count,
            machine_ns,
            "measured RuntimeWorld tick phases"
        );
        Ok(report)
    }

    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError> {
        self.ensure_active()?;
        debug!(
            minimum_supported_version = ?request.minimum_supported_version,
            step = self.step,
            "runtime.save"
        );
        crate::save::write_runtime_save(self.snapshot()?, request)
    }

    pub fn load(&mut self, save: SaveBlob) -> Result<LoadReport, RuntimeError> {
        debug!("runtime.load");
        self.load_with_registry(save, &SchemaMigrationRegistry::default())
    }

    pub fn load_with_registry(
        &mut self,
        save: SaveBlob,
        registry: &SchemaMigrationRegistry,
    ) -> Result<LoadReport, RuntimeError> {
        self.load_with_validation(save, registry, |_| Ok(()))
            .map(|(report, ())| report)
    }

    /// Validate and prepare decoded state before replacing the live world.
    /// A rejected snapshot leaves the world, including its failure status, unchanged.
    pub fn load_with_validation<T>(
        &mut self,
        save: SaveBlob,
        registry: &SchemaMigrationRegistry,
        validate: impl FnOnce(&mut RuntimeSnapshot) -> Result<T, RuntimeError>,
    ) -> Result<(LoadReport, T), RuntimeError> {
        debug!("runtime.load.with_validation");
        let mut snapshot = crate::save::read_runtime_save(&save, registry)?;
        snapshot
            .awaits
            .validate()
            .map_err(RuntimeError::diagnostic)?;
        let validated = validate(&mut snapshot)?;
        snapshot
            .awaits
            .validate()
            .map_err(RuntimeError::diagnostic)?;
        self.restore_snapshot(snapshot);
        self.required_tick_mode = TickMode::RestoreContinuation;
        let report = LoadReport {
            step: self.step,
            seed: self.config.seed,
        };
        info!(
            event = "runtime.load",
            step = report.step,
            seed = report.seed,
            "restored runtime world"
        );
        Ok((report, validated))
    }

    pub fn restore_snapshot(&mut self, snapshot: RuntimeSnapshot) {
        self.tasks = crate::task_scope::TaskRuntime::default();
        self.failed = false;
        self.config = snapshot.config;
        self.package = snapshot.package;
        self.id_source = snapshot.id_source;
        self.actors = snapshot.actors;
        self.blackboard = snapshot.blackboard;
        self.machines = snapshot.machines;
        self.awaits = snapshot.awaits;
        self.delayed_events = snapshot.delayed_events;
        self.events = snapshot.events;
        self.presentation = snapshot.presentation;
        self.mutations = snapshot.mutations;
        self.mounted_modules = snapshot.mounted_modules;
        self.integrity_mode = snapshot.integrity_mode;
        self.step = snapshot.step;
    }

    pub fn debug_session(&self) -> RuntimeDebugSession<'_> {
        RuntimeDebugSession { world: self }
    }

    pub fn snapshot(&self) -> Result<RuntimeSnapshot, RuntimeError> {
        self.ensure_active()?;
        Ok(RuntimeSnapshot {
            config: self.config.clone(),
            package: self.package.clone(),
            id_source: self.id_source.clone(),
            actors: self.actors.clone(),
            blackboard: self.blackboard.clone(),
            machines: self.machines.clone(),
            awaits: self.awaits.clone(),
            delayed_events: self.delayed_events.clone(),
            events: self.events.clone(),
            presentation: self.presentation.clone(),
            mutations: self.mutations.clone(),
            mounted_modules: self.mounted_modules.clone(),
            integrity_mode: self.integrity_mode,
            step: self.step,
        })
    }

    fn next_id(&mut self) -> StableId {
        self.id_source.next_id()
    }
}

fn presentation_kind(command: &PresentationCommand) -> &str {
    match command {
        PresentationCommand::Dialogue { .. } => "dialogue",
        PresentationCommand::Choice { .. } => "choice",
        PresentationCommand::TextEvent { .. } => "text_event",
        PresentationCommand::Marker { .. } => "marker",
        PresentationCommand::Custom { .. } => "custom",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LoadReport {
    pub step: u64,
    pub seed: u64,
}

pub struct RuntimeDebugSession<'a> {
    world: &'a RuntimeWorld,
}

impl RuntimeDebugSession<'_> {
    pub fn blackboard(&self) -> BTreeMap<String, BlackboardValue> {
        self.world.blackboard.values().clone()
    }

    pub fn actors(&self) -> Vec<ActorSnapshot> {
        self.world.actors.actor_snapshots()
    }

    pub fn components(&self, actor: ActorId) -> Vec<ComponentSnapshot> {
        self.world.actors.component_snapshots(actor)
    }

    pub fn state_machines(&self, actor: ActorId) -> Vec<StateMachineSnapshot> {
        self.world.machines.snapshots(actor)
    }

    pub fn event_trace(&self) -> Vec<RuntimeEvent> {
        self.world.events.trace().to_vec()
    }

    pub fn presentation_trace(&self) -> Vec<PresentationRecord> {
        self.world.presentation.clone()
    }

    pub fn mutation_trace(&self) -> Vec<RuntimeMutationRecord> {
        self.world.mutations.clone()
    }
}
