use std::collections::BTreeMap;

use astra_core::{
    Diagnostic, Hash128, SchemaId, SchemaMigrationRegistry, SchemaVersion, StableId,
    StableIdGenerator,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ActionRegistry, ActorId, ActorRecord, ActorSnapshot, ActorStore, AwaitQueue, AwaitResult,
    Blackboard, BlackboardValue, ComponentId, ComponentRecord, ComponentSnapshot,
    CreateAwaitAction, EmitEventAction, EventId, EventPayload, EventQueue, EventSource,
    PresentationAction, PresentationCommand, PresentationRecord, RuntimeEvent, SaveBlob,
    SaveRequest, SetBlackboardAction, StateMachineDefinition, StateMachineSnapshot,
    StateMachineStore,
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
        Ok(Self {
            id_source: StableIdGenerator::new(config.seed),
            config,
            package,
            actors: ActorStore::default(),
            blackboard: Blackboard::default(),
            events: EventQueue::default(),
            awaits: AwaitQueue::default(),
            machines: StateMachineStore::default(),
            actions,
            presentation: Vec::new(),
            diagnostics: Vec::new(),
            mounted_modules: BTreeMap::new(),
            step: 0,
        })
    }

    pub fn mount_module(&mut self, slot: impl Into<String>, provider_id: impl Into<String>) {
        self.mounted_modules.insert(slot.into(), provider_id.into());
    }

    pub fn create_actor(&mut self, name: impl Into<String>, tags: Vec<String>) -> ActorId {
        let actor_id = ActorId(self.next_id());
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
        let attached = self.actors.attach_component(ComponentRecord {
            component_id,
            actor_id,
            schema: schema.into(),
            version: SchemaVersion::default(),
            data,
        });
        if attached {
            Ok(component_id)
        } else {
            Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTOR_MISSING",
                format!("cannot attach component to missing actor {actor_id:?}"),
            )))
        }
    }

    pub fn remove_actor(&mut self, actor_id: ActorId) -> bool {
        self.actors.remove_actor(actor_id).is_some()
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> bool {
        self.actors.detach_component(component_id).is_some()
    }

    pub fn add_state_machine(&mut self, definition: StateMachineDefinition) {
        self.machines.add(definition);
    }

    pub fn emit_event(&mut self, source: EventSource, payload: EventPayload) {
        let event = RuntimeEvent {
            id: EventId(self.next_id()),
            source,
            step: self.step,
            sequence: 0,
            payload,
        };
        self.events.push(event);
    }

    pub fn emit_presentation(&mut self, command: PresentationCommand) {
        let sequence = self.presentation.len() as u64;
        self.presentation.push(PresentationRecord {
            step: self.step,
            sequence,
            command,
        });
    }

    pub fn submit_await_result(&mut self, result: AwaitResult) {
        self.awaits.submit_result(result);
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
        self.step = input.fixed_step;
        self.id_source.set_step(input.fixed_step);
        self.diagnostics.clear();
        for slot in &self.config.required_slots {
            if !self.mounted_modules.contains_key(slot) {
                self.diagnostics.push(Diagnostic::blocking(
                    "ASTRA_RUNTIME_MODULE_MISSING",
                    format!("missing required module slot {slot}"),
                ));
            }
        }
        for result in self.awaits.drain_ordered_results(input.fixed_step) {
            let id = EventId(self.next_id());
            self.events.push(RuntimeEvent {
                id,
                source: EventSource::AwaitResult,
                step: input.fixed_step,
                sequence: result.sequence,
                payload: result.payload,
            });
        }
        let ready = self.events.drain_ordered_for_step(input.fixed_step);
        let actor_snapshots = self.actors.actor_snapshots();
        let mut id_source = || self.id_source.next_id();
        let output = self.machines.tick(
            input.fixed_step,
            &ready,
            &actor_snapshots,
            &mut self.blackboard,
            &self.actions,
            &mut id_source,
        )?;
        for event in output.events {
            self.events.push(event);
        }
        for await_token in output.awaits {
            self.awaits.insert(await_token);
        }
        for command in output.presentation {
            self.emit_presentation(command);
        }
        Ok(TickReport {
            step: input.fixed_step,
            state_hash: self.state_hash(),
            event_hash: self.event_hash(),
            presentation_hash: self.presentation_hash(),
            diagnostics: self.diagnostics.clone(),
        })
    }

    pub fn save(&self, request: SaveRequest) -> Result<SaveBlob, RuntimeError> {
        crate::save::write_runtime_save(self.snapshot(), request)
    }

    pub fn load(&mut self, save: SaveBlob) -> Result<LoadReport, RuntimeError> {
        self.load_with_registry(save, &SchemaMigrationRegistry::default())
    }

    pub fn load_with_registry(
        &mut self,
        save: SaveBlob,
        registry: &SchemaMigrationRegistry,
    ) -> Result<LoadReport, RuntimeError> {
        let snapshot = crate::save::read_runtime_save(&save, registry)?;
        self.config = snapshot.config;
        self.package = snapshot.package;
        self.actors = snapshot.actors;
        self.blackboard = snapshot.blackboard;
        self.machines = snapshot.machines;
        self.awaits = snapshot.awaits;
        self.events = EventQueue::default();
        for event in snapshot.events {
            self.events.push(event);
        }
        self.presentation = snapshot.presentation;
        self.mounted_modules = snapshot.mounted_modules;
        self.step = snapshot.step;
        Ok(LoadReport {
            state_hash: self.state_hash(),
        })
    }

    pub fn replay(&mut self, replay: ReplayInput) -> Result<ReplayReport, RuntimeError> {
        for input in replay.inputs {
            self.tick(input)?;
        }
        Ok(ReplayReport {
            state_hash: self.state_hash(),
            event_hash: self.event_hash(),
            presentation_hash: self.presentation_hash(),
        })
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
