use std::collections::BTreeMap;

use astra_core::{
    Diagnostic, Hash128, SchemaId, SchemaMigrationRegistry, SchemaVersion, StableId,
    StableIdGenerator,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{
    ActionRegistry, ActorId, ActorRecord, ActorSnapshot, ActorStore, AwaitQueue, AwaitResult,
    AwaitToken, Blackboard, BlackboardValue, ComponentId, ComponentRecord, ComponentSnapshot,
    CreateAwaitAction, DelayedEventId, DelayedEventQueue, EmitEventAction, EventId, EventPayload,
    EventQueue, EventSource, PresentationAction, PresentationCommand, PresentationRecord,
    RuntimeAction, RuntimeEvent, SaveBlob, SaveRequest, ScheduledEvent, SetBlackboardAction,
    StateMachineDefinition, StateMachineSnapshot, StateMachineStore,
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
}

impl Default for PackageHandle {
    fn default() -> Self {
        Self {
            package_id: "stage1.headless".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TickInput {
    pub fixed_step: u64,
    pub delta_ns: u64,
    pub seed: u64,
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
    pub actors: ActorStore,
    pub blackboard: Blackboard,
    pub machines: StateMachineStore,
    pub awaits: AwaitQueue,
    pub delayed_events: DelayedEventQueue,
    pub events: Vec<RuntimeEvent>,
    pub presentation: Vec<PresentationRecord>,
    pub mounted_modules: BTreeMap<String, String>,
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
    diagnostics: Vec<Diagnostic>,
    mounted_modules: BTreeMap<String, String>,
    step: u64,
}

impl RuntimeWorld {
    pub fn create(config: RuntimeConfig, package: PackageHandle) -> Result<Self, RuntimeError> {
        let mut actions = ActionRegistry::default();
        actions.register(SetBlackboardAction);
        actions.register(EmitEventAction);
        actions.register(CreateAwaitAction);
        actions.register(PresentationAction);
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
            diagnostics: Vec::new(),
            mounted_modules: BTreeMap::new(),
            step: 0,
        })
    }

    pub fn mount_module(&mut self, slot: impl Into<String>, provider_id: impl Into<String>) {
        let slot = slot.into();
        let provider_id = provider_id.into();
        info!(slot = %slot, provider_id = %provider_id, "runtime.module.mount");
        self.mounted_modules.insert(slot, provider_id);
    }

    pub fn register_action<A: RuntimeAction + 'static>(
        &mut self,
        provider_id: impl Into<String>,
        action: A,
    ) {
        let provider_id = provider_id.into();
        let action_id = action.descriptor().id;
        info!(
            provider_id = %provider_id,
            action_id = %action_id,
            "runtime.action.register"
        );
        self.actions.register_with_provider(provider_id, action);
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

    pub fn attach_component(
        &mut self,
        actor_id: ActorId,
        schema: impl Into<SchemaId>,
        data: BlackboardValue,
    ) -> Result<ComponentId, RuntimeError> {
        let component_id = ComponentId(self.next_id());
        let schema = schema.into();
        let attached = self.actors.attach_component(ComponentRecord {
            component_id,
            actor_id,
            schema: schema.clone(),
            version: SchemaVersion::default(),
            data,
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

    pub fn submit_await_result(&mut self, result: AwaitResult) {
        debug!(
            token_id = ?result.token_id,
            sequence = result.sequence,
            completed_at_step = result.completed_at_step,
            kind = %result.payload.kind,
            "runtime.await.submit_result"
        );
        self.awaits.submit_result(result);
    }

    pub fn insert_await_token(&mut self, token: AwaitToken) {
        debug!(
            token_id = ?token.token_id,
            requested_at_step = token.requested_at_step,
            timeout_step = ?token.deterministic_timeout_step,
            "runtime.await.insert"
        );
        self.awaits.insert(token);
    }

    pub fn apply_input(&mut self, input: PlayerInput) -> Result<(), RuntimeError> {
        let mut payload = input.payload;
        if payload.kind.is_empty() {
            payload.kind = input.kind;
        }
        self.emit_event(EventSource::PlayerInput, payload);
        Ok(())
    }

    pub fn tick(&mut self, input: TickInput) -> Result<TickReport, RuntimeError> {
        debug!(
            step = input.fixed_step,
            delta_ns = input.delta_ns,
            required_slot_count = self.config.required_slots.len(),
            "runtime.tick.start"
        );
        self.step = input.fixed_step;
        self.id_source.set_step(input.fixed_step);
        self.diagnostics.clear();
        for slot in &self.config.required_slots {
            if !self.mounted_modules.contains_key(slot) {
                let diagnostic = Diagnostic::blocking(
                    "ASTRA_RUNTIME_MODULE_MISSING",
                    format!("missing required module slot {slot}"),
                );
                warn!(
                    slot,
                    diagnostic_code = %diagnostic.code,
                    "runtime.diagnostic"
                );
                self.diagnostics.push(diagnostic);
            }
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
        let output = self.machines.tick(
            input.fixed_step,
            &ready,
            &mut self.actors,
            &mut self.blackboard,
            &self.actions,
            &mut self.id_source,
        );
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
            self.awaits.insert(await_token);
        }
        for command in output.presentation {
            self.emit_presentation(command);
        }
        let report = TickReport {
            step: input.fixed_step,
            state_hash: self.state_hash(),
            event_hash: self.event_hash(),
            presentation_hash: self.presentation_hash(),
            diagnostics: self.diagnostics.clone(),
        };
        info!(
            step = report.step,
            state_hash = %report.state_hash,
            event_hash = %report.event_hash,
            presentation_hash = %report.presentation_hash,
            diagnostic_count = report.diagnostics.len(),
            output_diagnostic_count,
            "runtime.tick"
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
        self.config = snapshot.config;
        self.package = snapshot.package;
        self.actors = snapshot.actors;
        self.blackboard = snapshot.blackboard;
        self.machines = snapshot.machines;
        self.awaits = snapshot.awaits;
        self.delayed_events = snapshot.delayed_events;
        self.events = EventQueue::default();
        for event in snapshot.events {
            self.events.push(event);
        }
        self.presentation = snapshot.presentation;
        self.mounted_modules = snapshot.mounted_modules;
        self.step = snapshot.step;
        let report = LoadReport {
            state_hash: self.state_hash(),
        };
        info!(state_hash = %report.state_hash, "runtime.load");
        Ok(report)
    }

    pub fn replay(&mut self, replay: ReplayInput) -> Result<ReplayReport, RuntimeError> {
        info!(input_count = replay.inputs.len(), "runtime.replay.start");
        for input in replay.inputs {
            self.tick(input)?;
        }
        let report = ReplayReport {
            state_hash: self.state_hash(),
            event_hash: self.event_hash(),
            presentation_hash: self.presentation_hash(),
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
            actors: self.actors.clone(),
            blackboard: self.blackboard.clone(),
            machines: self.machines.clone(),
            awaits: self.awaits.clone(),
            delayed_events: self.delayed_events.clone(),
            events: self.events.trace().to_vec(),
            presentation: self.presentation.clone(),
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
            &postcard::to_allocvec(&self.presentation)
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
pub struct ReplayInput {
    pub inputs: Vec<TickInput>,
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
}
