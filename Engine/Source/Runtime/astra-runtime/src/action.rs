use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::{Diagnostic, SchemaVersion, StableId};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{
    actor::ActorStoreAccess, blackboard::BlackboardAccess, ActorId, ActorRecord, AwaitKind,
    AwaitReplayPolicy, AwaitToken, AwaitTokenId, BlackboardValue, ComponentId, ComponentRecord,
    DelayedEventId, EventId, EventPayload, EventSource, PresentationCommand,
    RuntimeComponentPayload, RuntimeError, RuntimeEvent, RuntimeMutationRecord, ScheduledEvent,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActionInvocation {
    pub action_id: String,
    #[serde(default)]
    pub input: BTreeMap<String, BlackboardValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActionDescriptor {
    pub id: String,
    pub input_schema: String,
    pub output_schema: String,
    pub execution: ActionExecutionClass,
    pub access: ActionAccess,
    pub stable_id_reservation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionExecutionClass {
    Serial,
    ParallelPure,
    ParallelTransactional,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ActionResourceKey {
    ActorStore,
    Blackboard,
    EventQueue,
    AwaitQueue,
    DelayedEventQueue,
    Presentation,
    MutationLog,
    StableIdSource,
    ComponentSchema(String),
    BlackboardKey(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActionAccess {
    pub reads: BTreeSet<ActionResourceKey>,
    pub writes: BTreeSet<ActionResourceKey>,
}

impl ActionAccess {
    pub fn new(
        reads: impl IntoIterator<Item = ActionResourceKey>,
        writes: impl IntoIterator<Item = ActionResourceKey>,
    ) -> Self {
        Self {
            reads: reads.into_iter().collect(),
            writes: writes.into_iter().collect(),
        }
    }
}

impl ActionDescriptor {
    pub fn declared(
        id: impl Into<String>,
        input_schema: impl Into<String>,
        output_schema: impl Into<String>,
        execution: ActionExecutionClass,
        access: ActionAccess,
        stable_id_reservation: u32,
    ) -> Self {
        Self {
            id: id.into(),
            input_schema: input_schema.into(),
            output_schema: output_schema.into(),
            execution,
            access,
            stable_id_reservation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActionTrace {
    pub action_id: String,
    #[serde(default)]
    pub payload: BTreeMap<String, BlackboardValue>,
}

pub struct DeterministicActionContext<'a> {
    step: u64,
    id_source: &'a mut dyn FnMut() -> StableId,
    actors: &'a mut dyn ActorStoreAccess,
    blackboard: &'a mut dyn BlackboardAccess,
    emitted_events: &'a mut Vec<RuntimeEvent>,
    presentation: &'a mut Vec<PresentationCommand>,
    awaits: &'a mut Vec<AwaitToken>,
    delayed_events: &'a mut Vec<ScheduledEvent>,
    delayed_cancellations: &'a mut Vec<DelayedEventId>,
    mutations: &'a mut Vec<RuntimeMutationRecord>,
    source: String,
    trigger_event: Option<RuntimeEvent>,
    evidence_mode: bool,
    observed_reads: RefCell<BTreeSet<ActionResourceKey>>,
    observed_writes: RefCell<BTreeSet<ActionResourceKey>>,
}

impl<'a> DeterministicActionContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        step: u64,
        id_source: &'a mut dyn FnMut() -> StableId,
        actors: &'a mut dyn ActorStoreAccess,
        blackboard: &'a mut dyn BlackboardAccess,
        emitted_events: &'a mut Vec<RuntimeEvent>,
        presentation: &'a mut Vec<PresentationCommand>,
        awaits: &'a mut Vec<AwaitToken>,
        delayed_events: &'a mut Vec<ScheduledEvent>,
        delayed_cancellations: &'a mut Vec<DelayedEventId>,
        mutations: &'a mut Vec<RuntimeMutationRecord>,
        source: String,
        trigger_event: Option<RuntimeEvent>,
        evidence_mode: bool,
    ) -> Self {
        Self {
            step,
            id_source,
            actors,
            blackboard,
            emitted_events,
            presentation,
            awaits,
            delayed_events,
            delayed_cancellations,
            mutations,
            source,
            trigger_event,
            evidence_mode,
            observed_reads: RefCell::new(BTreeSet::new()),
            observed_writes: RefCell::new(BTreeSet::new()),
        }
    }

    fn observe_read(&self, resource: ActionResourceKey) {
        self.observed_reads.borrow_mut().insert(resource);
    }

    fn observe_write(&self, resource: ActionResourceKey) {
        self.observed_writes.borrow_mut().insert(resource);
    }

    pub(crate) fn observed_access(&self) -> ActionAccess {
        ActionAccess {
            reads: self.observed_reads.borrow().clone(),
            writes: self.observed_writes.borrow().clone(),
        }
    }

    pub fn step(&self) -> u64 {
        self.step
    }

    pub fn evidence_mode(&self) -> bool {
        self.evidence_mode
    }

    pub fn trigger_event(&self) -> Option<&RuntimeEvent> {
        self.observe_read(ActionResourceKey::EventQueue);
        self.trigger_event.as_ref()
    }

    pub fn next_id(&mut self) -> StableId {
        self.observe_write(ActionResourceKey::StableIdSource);
        (self.id_source)()
    }

    pub fn set_blackboard(&mut self, key: impl Into<String>, value: BlackboardValue) {
        let key = key.into();
        self.observe_write(ActionResourceKey::BlackboardKey(key.clone()));
        self.blackboard.set(key, value);
    }

    pub fn blackboard(&self) -> BTreeMap<String, BlackboardValue> {
        self.observe_read(ActionResourceKey::Blackboard);
        self.blackboard.values()
    }

    pub fn create_actor(&mut self, name: impl Into<String>, tags: Vec<String>) -> ActorId {
        self.observe_write(ActionResourceKey::ActorStore);
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
        schema: impl Into<String>,
        data: BlackboardValue,
    ) -> Result<ComponentId, RuntimeError> {
        let schema = schema.into();
        self.observe_write(ActionResourceKey::ActorStore);
        self.observe_write(ActionResourceKey::ComponentSchema(schema.clone()));
        let component_id = ComponentId(self.next_id());
        if self.actors.attach_component(ComponentRecord {
            component_id,
            actor_id,
            payload: RuntimeComponentPayload::typed(schema, SchemaVersion::default(), data),
        }) {
            Ok(component_id)
        } else {
            Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTOR_MISSING",
                format!("cannot attach component to missing actor {actor_id:?}"),
            )))
        }
    }

    pub fn remove_actor(&mut self, actor_id: ActorId) -> bool {
        self.observe_write(ActionResourceKey::ActorStore);
        self.actors.remove_actor(actor_id).is_some()
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> bool {
        self.observe_write(ActionResourceKey::ActorStore);
        self.actors.detach_component(component_id).is_some()
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
        self.observe_read(ActionResourceKey::ComponentSchema(
            component.payload.schema.as_str().to_string(),
        ));
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
        let schema = self
            .actors
            .component(component_id)
            .map(|component| component.payload.schema.as_str().to_string())
            .ok_or_else(|| {
                RuntimeError::diagnostic(Diagnostic::blocking(
                    "ASTRA_RUNTIME_COMPONENT_MISSING",
                    "runtime component does not exist",
                ))
            })?;
        self.observe_read(ActionResourceKey::ComponentSchema(schema.clone()));
        self.observe_write(ActionResourceKey::ComponentSchema(schema));
        self.observe_write(ActionResourceKey::MutationLog);
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
            source: self.source.clone(),
        });
        Ok(())
    }

    pub fn emit_event(&mut self, source: EventSource, payload: EventPayload) {
        self.observe_write(ActionResourceKey::EventQueue);
        let event = RuntimeEvent {
            id: EventId(self.next_id()),
            source,
            step: self.step,
            sequence: 0,
            payload,
        };
        self.emitted_events.push(event);
    }

    pub fn emit_presentation(&mut self, command: PresentationCommand) {
        self.observe_write(ActionResourceKey::Presentation);
        self.presentation.push(command);
    }

    pub fn push_await(&mut self, token: AwaitToken) -> Result<(), RuntimeError> {
        self.observe_write(ActionResourceKey::AwaitQueue);
        token.validate().map_err(RuntimeError::diagnostic)?;
        self.awaits.push(token);
        Ok(())
    }

    pub fn create_await(&mut self, kind: AwaitKind) -> AwaitToken {
        AwaitToken {
            token_id: AwaitTokenId(self.next_id()),
            kind,
            requested_at_step: self.step,
            deterministic_timeout_step: None,
            replay_policy: AwaitReplayPolicy::RecordedResult,
        }
    }

    pub fn schedule_event(
        &mut self,
        due_tick: u64,
        source: EventSource,
        payload: EventPayload,
    ) -> DelayedEventId {
        self.observe_write(ActionResourceKey::DelayedEventQueue);
        let id = DelayedEventId(self.next_id());
        self.delayed_events.push(ScheduledEvent {
            id,
            due_tick,
            sequence: 0,
            source,
            payload,
        });
        id
    }

    pub fn cancel_delayed_event(&mut self, id: DelayedEventId) {
        self.observe_write(ActionResourceKey::DelayedEventQueue);
        self.delayed_cancellations.push(id);
    }
}

pub trait RuntimeAction: Send + Sync {
    fn descriptor(&self) -> ActionDescriptor;
    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError>;
}

#[derive(Clone)]
struct RegisteredAction {
    provider_id: String,
    action: Arc<dyn RuntimeAction>,
}

#[derive(Default, Clone)]
pub struct ActionRegistry {
    actions: BTreeMap<String, RegisteredAction>,
}

impl ActionRegistry {
    pub fn register<A: RuntimeAction + 'static>(&mut self, action: A) -> Result<(), RuntimeError> {
        self.register_with_provider("astra.core", action)
    }

    pub fn register_with_provider<A: RuntimeAction + 'static>(
        &mut self,
        provider_id: impl Into<String>,
        action: A,
    ) -> Result<(), RuntimeError> {
        let provider_id = provider_id.into();
        let descriptor = action.descriptor();
        if descriptor.id.trim().is_empty()
            || descriptor.input_schema.trim().is_empty()
            || descriptor.output_schema.trim().is_empty()
        {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTION_DESCRIPTOR",
                "action descriptor requires non-empty id and schemas",
            )));
        }
        if descriptor.execution == ActionExecutionClass::ParallelPure
            && (!descriptor.access.writes.is_empty() || descriptor.stable_id_reservation != 0)
        {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTION_ACCESS_INVALID",
                "parallel_pure actions cannot declare writes or reserve StableIds",
            )));
        }
        let writes_stable_ids = descriptor
            .access
            .writes
            .contains(&ActionResourceKey::StableIdSource);
        if (descriptor.stable_id_reservation > 0) != writes_stable_ids {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_ACTION_ID_RESERVATION_INVALID",
                "StableIdSource write access and a non-zero StableId reservation must be declared together",
            )));
        }
        if let Some(existing) = self.actions.get(&descriptor.id) {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_ACTION_CONFLICT",
                    "action id is already registered",
                )
                .with_field("action_id", &descriptor.id)
                .with_field("selected_provider", &existing.provider_id)
                .with_field("conflicting_provider", &provider_id),
            ));
        }
        self.actions.insert(
            descriptor.id,
            RegisteredAction {
                provider_id,
                action: Arc::new(action),
            },
        );
        Ok(())
    }

    pub fn unregister_provider(&mut self, provider_id: &str) {
        self.actions
            .retain(|_, action| action.provider_id != provider_id);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn RuntimeAction>> {
        self.actions.get(id).map(|action| action.action.clone())
    }

    pub(crate) fn descriptor(&self, id: &str) -> Option<ActionDescriptor> {
        self.actions
            .get(id)
            .map(|action| action.action.descriptor())
    }
}

pub struct SetBlackboardAction;

impl RuntimeAction for SetBlackboardAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.core.set_blackboard",
            "astra.action.set_blackboard.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new(
                [ActionResourceKey::Blackboard],
                [ActionResourceKey::Blackboard],
            ),
            0,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let Some(BlackboardValue::String(key)) = input.get("key") else {
            return Err(RuntimeError::message("set_blackboard requires string key"));
        };
        let value = input.get("value").cloned().unwrap_or(BlackboardValue::Null);
        ctx.set_blackboard(key.clone(), value.clone());
        let mut payload = BTreeMap::new();
        payload.insert("key".to_string(), BlackboardValue::String(key.clone()));
        payload.insert("value".to_string(), value);
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload,
        })
    }
}

pub struct EmitEventAction;

impl RuntimeAction for EmitEventAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.core.emit_event",
            "astra.action.emit_event.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new(
                [],
                [
                    ActionResourceKey::EventQueue,
                    ActionResourceKey::StableIdSource,
                ],
            ),
            1,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let Some(BlackboardValue::String(kind)) = input.get("kind") else {
            return Err(RuntimeError::message("emit_event requires string kind"));
        };
        ctx.emit_event(EventSource::StateMachine, EventPayload::new(kind.clone()));
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: input.clone(),
        })
    }
}

pub struct CreateAwaitAction;

impl RuntimeAction for CreateAwaitAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.core.create_await",
            "astra.action.create_await.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new(
                [],
                [
                    ActionResourceKey::AwaitQueue,
                    ActionResourceKey::StableIdSource,
                ],
            ),
            1,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let token = ctx.create_await(AwaitKind::Custom("scenario".to_string()));
        ctx.push_await(token)?;
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: input.clone(),
        })
    }
}

pub struct PresentationAction;

impl RuntimeAction for PresentationAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.core.presentation",
            "astra.action.presentation.v1",
            "astra.action_trace.v1",
            ActionExecutionClass::ParallelTransactional,
            ActionAccess::new([], [ActionResourceKey::Presentation]),
            0,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let command = match input.get("kind") {
            Some(BlackboardValue::String(kind)) if kind == "dialogue" => {
                PresentationCommand::Dialogue {
                    speaker: get_string(input, "speaker")?,
                    text: get_string(input, "text")?,
                }
            }
            Some(BlackboardValue::String(kind)) if kind == "choice" => {
                PresentationCommand::Choice {
                    prompt: get_string(input, "prompt")?,
                    options: get_list_strings(input, "options")?,
                }
            }
            Some(BlackboardValue::String(kind)) if kind == "text_event" => {
                PresentationCommand::TextEvent {
                    key: get_string(input, "key")?,
                }
            }
            Some(BlackboardValue::String(kind)) if kind == "marker" => {
                PresentationCommand::Marker {
                    name: get_string(input, "name")?,
                }
            }
            Some(BlackboardValue::String(kind)) => {
                let data = input
                    .iter()
                    .filter(|(key, _)| key.as_str() != "kind")
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                PresentationCommand::Custom {
                    kind: kind.clone(),
                    data,
                }
            }
            _ => return Err(RuntimeError::message("presentation requires string kind")),
        };
        ctx.emit_presentation(command);
        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: input.clone(),
        })
    }
}

fn get_string(
    input: &BTreeMap<String, BlackboardValue>,
    key: &str,
) -> Result<String, RuntimeError> {
    match input.get(key) {
        Some(BlackboardValue::String(value)) => Ok(value.clone()),
        _ => Err(RuntimeError::message(format!("missing string {key}"))),
    }
}

fn get_list_strings(
    input: &BTreeMap<String, BlackboardValue>,
    key: &str,
) -> Result<Vec<String>, RuntimeError> {
    match input.get(key) {
        Some(BlackboardValue::List(values)) => values
            .iter()
            .map(|value| match value {
                BlackboardValue::String(value) => Ok(value.clone()),
                _ => Err(RuntimeError::message(format!("{key} must contain strings"))),
            })
            .collect(),
        _ => Err(RuntimeError::message(format!("missing string list {key}"))),
    }
}
