use std::{collections::BTreeMap, time::Instant};

use astra_core::{
    Diagnostic, Hash128, Hash256, SchemaId, SchemaMigrationRegistry, SchemaVersion, StableId,
    StableIdGenerator,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, trace, warn};

use crate::{
    ActionRegistry, ActorId, ActorRecord, ActorSnapshot, ActorStore, AwaitQueue, AwaitResult,
    AwaitToken, Blackboard, ComponentId, ComponentRecord, ComponentSnapshot, CreateAwaitAction,
    DelayedEventId, DelayedEventQueue, EmitEventAction, EventId, EventPayload, EventQueue,
    EventSource, PresentationAction, PresentationCommand, PresentationRecord, ProviderReplayOutput,
    RuntimeAction, RuntimeComponentPayload, RuntimeEffectRecord, RuntimeEvent,
    RuntimeMutationRecord, RuntimeReplayTranscript, SaveBlob, SaveRequest, ScheduledEvent,
    SetBlackboardAction, StateMachineDefinition, StateMachineSnapshot, StateMachineStore,
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
            feature_fingerprint: "runtime-envelope-v2".to_string(),
            abi_fingerprint: "astra-plugin-abi-v2".to_string(),
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
    Replay,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrderedTickIngress {
    pub sequence: u64,
    pub payload: TickIngress,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TickIngress {
    PlayerInput(PlayerInput),
    AwaitCompletion(AwaitResult),
    LiveProviderOutput(ProviderReplayOutput),
    RecordedProviderOutput(ProviderReplayOutput),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TickRequest {
    pub timing: TickInput,
    pub mode: TickMode,
    #[serde(default)]
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

    pub fn replay(timing: TickInput, ingress: Vec<OrderedTickIngress>) -> Self {
        Self {
            timing,
            mode: TickMode::Replay,
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
    pub state_hash: Hash128,
    pub event_hash: Hash128,
    pub presentation_hash: Hash128,
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
    pub package: PackageHandle,
    pub id_source: StableIdGenerator,
    pub actors: ActorStore,
    pub blackboard: Blackboard,
    pub machines: StateMachineStore,
    pub awaits: AwaitQueue,
    pub delayed_events: DelayedEventQueue,
    pub events: EventQueue,
    pub presentation: Vec<PresentationRecord>,
    pub mutations: Vec<RuntimeMutationRecord>,
    pub effects: Vec<RuntimeEffectRecord>,
    pub mounted_modules: BTreeMap<String, ModuleBindingSnapshot>,
    pub step: u64,
}

pub struct RuntimeWorld {
    config: RuntimeConfig,
    package: PackageHandle,
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
    effects: Vec<RuntimeEffectRecord>,
    diagnostics: Vec<Diagnostic>,
    mounted_modules: BTreeMap<String, ModuleBindingSnapshot>,
    step: u64,
    required_tick_mode: TickMode,
}

impl RuntimeWorld {
    pub fn create(config: RuntimeConfig, package: PackageHandle) -> Result<Self, RuntimeError> {
        let mut actions = ActionRegistry::default();
        actions.register(SetBlackboardAction)?;
        actions.register(EmitEventAction)?;
        actions.register(CreateAwaitAction)?;
        actions.register(PresentationAction)?;
        info!(
            seed = config.seed,
            required_slot_count = config.required_slots.len(),
            package_id = %package.package_id,
            default_action_count = 4,
            "runtime.create"
        );
        Ok(Self {
            id_source: StableIdGenerator::new(config.seed),
            config,
            package,
            actors: ActorStore::default(),
            blackboard: Blackboard::default(),
            events: EventQueue::default(),
            awaits: AwaitQueue::default(),
            delayed_events: DelayedEventQueue::default(),
            machines: StateMachineStore::default(),
            actions,
            presentation: Vec::new(),
            mutations: Vec::new(),
            effects: Vec::new(),
            diagnostics: Vec::new(),
            mounted_modules: BTreeMap::new(),
            step: 0,
            required_tick_mode: TickMode::Live,
        })
    }

    pub fn package_id(&self) -> &str {
        &self.package.package_id
    }

    pub fn package_handle(&self) -> &PackageHandle {
        &self.package
    }

    pub fn mount_module(
        &mut self,
        slot: EngineModuleSlot,
        binding: ValidatedModuleBinding,
    ) -> Result<(), RuntimeError> {
        if binding.slot != slot {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_MODULE_SLOT_MISMATCH",
                "module binding token does not match the requested slot",
            )));
        }
        let expected_context = ModuleBindingContext {
            package_id: self.package.package_id.clone(),
            target: self.package.target.clone(),
            profile: self.package.profile.clone(),
            engine_version: self.package.engine_version.clone(),
            rustc_fingerprint: self.package.rustc_fingerprint.clone(),
            feature_fingerprint: self.package.feature_fingerprint.clone(),
            abi_fingerprint: self.package.abi_fingerprint.clone(),
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

    pub fn create_actor(&mut self, name: impl Into<String>, tags: Vec<String>) -> ActorId {
        let actor_id = ActorId(self.next_id());
        debug!(?actor_id, tag_count = tags.len(), "runtime.actor.create");
        self.actors.insert_actor(ActorRecord {
            actor_id,
            name: name.into(),
            tags,
            components: Vec::new(),
        });
        actor_id
    }

    pub fn attach_component<T: Serialize>(
        &mut self,
        actor_id: ActorId,
        schema: impl Into<SchemaId>,
        data: &T,
    ) -> Result<ComponentId, RuntimeError> {
        let component_id = ComponentId(self.next_id());
        let schema = schema.into();
        let payload =
            RuntimeComponentPayload::postcard(schema.clone(), SchemaVersion::default(), data)?;
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

    pub fn read_component<T: DeserializeOwned>(
        &self,
        component_id: ComponentId,
    ) -> Result<T, RuntimeError> {
        let component = self.actors.component(component_id).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_MISSING",
                "runtime component does not exist",
            ))
        })?;
        component.payload.decode()
    }

    pub fn replace_component<T: Serialize>(
        &mut self,
        component_id: ComponentId,
        data: &T,
    ) -> Result<(), RuntimeError> {
        let component = self.actors.component_mut(component_id).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_MISSING",
                "runtime component does not exist",
            ))
        })?;
        let before_hash = component.payload.hash;
        let schema = component.payload.schema.clone();
        let payload = RuntimeComponentPayload::postcard(
            component.payload.schema.clone(),
            component.payload.version,
            data,
        )?;
        let after_hash = payload.hash;
        component.payload = payload;
        self.mutations.push(RuntimeMutationRecord {
            step: self.step,
            component_id,
            schema,
            before_hash,
            after_hash,
            source: "runtime.api".to_string(),
        });
        Ok(())
    }

    pub fn remove_actor(&mut self, actor_id: ActorId) -> bool {
        let removed = self.actors.remove_actor(actor_id).is_some();
        debug!(?actor_id, removed, "runtime.actor.remove");
        removed
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> bool {
        let detached = self.actors.detach_component(component_id).is_some();
        debug!(?component_id, detached, "runtime.component.detach");
        detached
    }

    pub fn add_state_machine(
        &mut self,
        definition: StateMachineDefinition,
    ) -> Result<(), RuntimeError> {
        debug!(
            machine_id = ?definition.id,
            owner = ?definition.owner,
            state_count = definition.states.len(),
            transition_count = definition.transitions.len(),
            "runtime.state_machine.add"
        );
        self.machines.add(definition)
    }

    pub fn emit_event(&mut self, source: EventSource, payload: EventPayload) {
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
    }

    pub fn enqueue_event(&mut self, event: RuntimeEvent) {
        debug!(
            event_id = ?event.id,
            source = ?event.source,
            step = event.step,
            kind = %event.payload.kind,
            "runtime.event.enqueue"
        );
        self.events.push(event);
    }

    pub fn schedule_event(
        &mut self,
        due_tick: u64,
        source: EventSource,
        payload: EventPayload,
    ) -> DelayedEventId {
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
        id
    }

    pub fn cancel_delayed_event(&mut self, id: DelayedEventId) -> bool {
        let cancelled = self.delayed_events.cancel(id);
        debug!(?id, cancelled, "runtime.delayed_event.cancel");
        cancelled
    }

    pub fn emit_presentation(&mut self, command: PresentationCommand) {
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
    }

    fn submit_await_result(&mut self, result: AwaitResult) {
        debug!(
            token_id = ?result.token_id,
            sequence = result.sequence,
            completed_at_step = result.completed_at_step,
            kind = %result.payload.kind,
            "runtime.await.submit_result"
        );
        self.awaits.submit_result(result);
    }

    pub fn insert_await_token(&mut self, token: AwaitToken) -> Result<(), RuntimeError> {
        debug!(
            token_id = ?token.token_id,
            requested_at_step = token.requested_at_step,
            timeout_step = ?token.deterministic_timeout_step,
            "runtime.await.insert"
        );
        self.awaits.insert(token).map_err(RuntimeError::diagnostic)
    }

    fn apply_provider_output(
        &mut self,
        step: u64,
        output: ProviderReplayOutput,
    ) -> Result<(), RuntimeError> {
        if output.provider_id.trim().is_empty()
            || output.session_id.trim().is_empty()
            || output.schema.trim().is_empty()
        {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_PROVIDER_OUTPUT_DESCRIPTOR",
                "recorded provider output requires provider, session and schema ids",
            )));
        }
        if Hash256::from_sha256(&output.payload) != output.payload_hash {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_PROVIDER_OUTPUT_HASH",
                "recorded provider output payload hash does not match its bytes",
            )));
        }
        if let Some(event) = output.events.iter().find(|event| event.step != step) {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_PROVIDER_OUTPUT_EVENT_STEP",
                    "recorded provider output event must target the transcript tick",
                )
                .with_field("provider_id", &output.provider_id)
                .with_field("event_step", event.step)
                .with_field("transcript_step", step),
            ));
        }
        if let Some(effect) = output.effects.iter().find(|effect| {
            effect.domain.trim().is_empty()
                || effect.schema.trim().is_empty()
                || !effect.validate_hash()
        }) {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_PROVIDER_OUTPUT_EFFECT",
                    "recorded provider effect descriptor or payload hash is invalid",
                )
                .with_field("provider_id", &output.provider_id)
                .with_field("effect_schema", &effect.schema),
            ));
        }

        let mut awaits = self.awaits.clone();
        for token in output.awaits {
            awaits.insert(token).map_err(RuntimeError::diagnostic)?;
        }
        for event in output.events {
            self.enqueue_event(event);
        }
        for command in output.presentation {
            self.emit_presentation(command);
        }
        for envelope in output.effects {
            let sequence = self.effects.len() as u64;
            self.effects.push(RuntimeEffectRecord {
                step,
                sequence,
                envelope,
            });
        }
        self.awaits = awaits;
        Ok(())
    }

    fn apply_input(&mut self, input: PlayerInput) -> Result<(), RuntimeError> {
        let mut payload = input.payload;
        if payload.kind.is_empty() {
            payload.kind = input.kind;
        }
        self.emit_event(EventSource::PlayerInput, payload);
        Ok(())
    }

    pub fn tick(&mut self, request: TickRequest) -> Result<TickReport, RuntimeError> {
        self.validate_tick_request(&request)?;
        let checkpoint_started = Instant::now();
        let checkpoint = self.snapshot();
        let checkpoint_ns = checkpoint_started.elapsed().as_nanos() as u64;
        let diagnostics = self.diagnostics.clone();
        let performance_step = request.timing.fixed_step;
        let transaction_started = Instant::now();
        let result = (|| {
            for ingress in request.ingress {
                match ingress.payload {
                    TickIngress::PlayerInput(input) => self.apply_input(input)?,
                    TickIngress::AwaitCompletion(result) => self.submit_await_result(result),
                    TickIngress::LiveProviderOutput(output) => {
                        self.apply_provider_output(request.timing.fixed_step, output)?
                    }
                    TickIngress::RecordedProviderOutput(output) => {
                        self.apply_provider_output(request.timing.fixed_step, output)?
                    }
                }
            }
            let report = self.tick_validated(request.timing)?;
            self.required_tick_mode = match request.mode {
                TickMode::Replay => TickMode::Replay,
                TickMode::Live | TickMode::RestoreContinuation => TickMode::Live,
            };
            Ok(report)
        })();
        let transaction_ns = transaction_started.elapsed().as_nanos() as u64;
        trace!(
            event = "runtime.tick.transaction.performance",
            step = performance_step,
            checkpoint_ns,
            transaction_ns,
            succeeded = result.is_ok(),
            "measured RuntimeWorld tick transaction phases"
        );
        match result {
            Ok(report) => Ok(report),
            Err(error) => {
                self.restore_snapshot(checkpoint);
                self.diagnostics = diagnostics;
                Err(error)
            }
        }
    }

    fn validate_tick_request(&self, request: &TickRequest) -> Result<(), RuntimeError> {
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
                return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_INGRESS_ORDER_INVALID",
                    "tick ingress sequence must be non-zero and strictly increasing",
                )));
            }
            if matches!(ingress.payload, TickIngress::RecordedProviderOutput(_))
                && request.mode != TickMode::Replay
            {
                return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                    "ASTRA_RUNTIME_LIVE_RECORDED_OUTPUT_FORBIDDEN",
                    "live tick cannot consume recorded provider output",
                )));
            }
            if matches!(ingress.payload, TickIngress::LiveProviderOutput(_))
                && request.mode == TickMode::Replay
            {
                return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                    "ASTRA_RUNTIME_REPLAY_LIVE_OUTPUT_FORBIDDEN",
                    "replay tick cannot consume live provider output",
                )));
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
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_TICK_DELTA_INVALID",
                    "runtime tick delta must be within the supported fixed-step range",
                )
                .with_field("delta_ns", input.delta_ns),
            ));
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
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_MODULE_MISSING",
                    "runtime required module slot is not mounted",
                )
                .with_field("slot", slot),
            ));
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
            self.emit_presentation(command);
        }
        self.mutations.extend(output.mutations);
        for envelope in output.effects {
            let sequence = self.effects.len() as u64;
            self.effects.push(RuntimeEffectRecord {
                step: input.fixed_step,
                sequence,
                envelope,
            });
        }
        let state_hash_started = Instant::now();
        let state_hash = self.state_hash();
        let state_hash_ns = state_hash_started.elapsed().as_nanos() as u64;
        let event_hash_started = Instant::now();
        let event_hash = self.event_hash();
        let event_hash_ns = event_hash_started.elapsed().as_nanos() as u64;
        let presentation_hash_started = Instant::now();
        let presentation_hash = self.presentation_hash();
        let presentation_hash_ns = presentation_hash_started.elapsed().as_nanos() as u64;
        let report = TickReport {
            step: input.fixed_step,
            state_hash,
            event_hash,
            presentation_hash,
            diagnostics: self.diagnostics.clone(),
        };
        trace!(
            event = "runtime.tick.performance",
            step = report.step,
            state_hash = %report.state_hash,
            event_hash = %report.event_hash,
            presentation_hash = %report.presentation_hash,
            diagnostic_count = report.diagnostics.len(),
            output_diagnostic_count,
            machine_ns,
            state_hash_ns,
            event_hash_ns,
            presentation_hash_ns,
            "measured RuntimeWorld tick phases"
        );
        Ok(report)
    }

    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError> {
        debug!(
            minimum_supported_version = ?request.minimum_supported_version,
            step = self.step,
            "runtime.save"
        );
        crate::save::write_runtime_save(self.snapshot(), request)
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
        debug!("runtime.load.with_registry");
        let snapshot = crate::save::read_runtime_save(&save, registry)?;
        self.restore_snapshot(snapshot);
        self.required_tick_mode = TickMode::RestoreContinuation;
        let report = LoadReport {
            state_hash: self.state_hash(),
        };
        info!(state_hash = %report.state_hash, "runtime.load");
        Ok(report)
    }

    pub fn restore_snapshot(&mut self, snapshot: RuntimeSnapshot) {
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
        self.effects = snapshot.effects;
        self.mounted_modules = snapshot.mounted_modules;
        self.step = snapshot.step;
    }

    pub fn replay(
        &mut self,
        replay: RuntimeReplayTranscript,
    ) -> Result<ReplayReport, RuntimeError> {
        if replay.schema != "astra.runtime_replay_transcript.v2" {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_REPLAY_SCHEMA",
                "runtime replay transcript schema is invalid",
            )));
        }
        info!(input_count = replay.ticks.len(), "runtime.replay.start");
        let original = self.snapshot();
        let original_mode = self.required_tick_mode;
        let result = (|| {
            self.restore_snapshot(replay.checkpoint);
            self.required_tick_mode = TickMode::Replay;
            for entry in replay.ticks {
                let report = self.tick(entry.request)?;
                let actual = crate::ReplayHashCheckpoint::from(&report);
                if actual != entry.expected {
                    return Err(RuntimeError::diagnostic(
                        Diagnostic::blocking(
                            "ASTRA_RUNTIME_REPLAY_HASH_MISMATCH",
                            "runtime replay hash checkpoint does not match the transcript",
                        )
                        .with_field("step", report.step.to_string())
                        .with_field("expected_state_hash", entry.expected.state_hash.to_string())
                        .with_field("actual_state_hash", report.state_hash.to_string()),
                    ));
                }
            }
            Ok(ReplayReport {
                state_hash: self.state_hash(),
                event_hash: self.event_hash(),
                presentation_hash: self.presentation_hash(),
            })
        })();
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                self.restore_snapshot(original);
                self.required_tick_mode = original_mode;
                return Err(error);
            }
        };
        info!(
            state_hash = %report.state_hash,
            event_hash = %report.event_hash,
            presentation_hash = %report.presentation_hash,
            "runtime.replay"
        );
        Ok(report)
    }

    pub fn debug_session(&self) -> RuntimeDebugSession<'_> {
        RuntimeDebugSession { world: self }
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
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
            effects: self.effects.clone(),
            mounted_modules: self.mounted_modules.clone(),
            step: self.step,
        }
    }

    pub fn state_hash(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(&self.snapshot())
                .expect("runtime snapshot must serialize for state hash"),
        )
    }

    pub fn event_hash(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(self.events.trace())
                .expect("runtime event trace must serialize for event hash"),
        )
    }

    pub fn presentation_hash(&self) -> Hash128 {
        Hash128::from_blake3(
            &postcard::to_allocvec(&(&self.presentation, &self.effects))
                .expect("runtime presentation trace must serialize for presentation hash"),
        )
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
    pub state_hash: Hash128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayReport {
    pub state_hash: Hash128,
    pub event_hash: Hash128,
    pub presentation_hash: Hash128,
}

pub struct RuntimeDebugSession<'a> {
    world: &'a RuntimeWorld,
}

impl RuntimeDebugSession<'_> {
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

    pub fn effect_trace(&self) -> Vec<RuntimeEffectRecord> {
        self.world.effects.clone()
    }
}
