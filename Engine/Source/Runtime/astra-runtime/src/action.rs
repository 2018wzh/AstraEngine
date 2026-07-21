use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::{Diagnostic, Hash256, SchemaId, SchemaVersion, StableId};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{
    ActorId, ActorRecord, ActorStore, AwaitKind, AwaitReplayPolicy, AwaitToken, AwaitTokenId,
    Blackboard, BlackboardValue, ComponentId, ComponentRecord, DelayedEventId, EventId,
    EventPayload, EventSource, PresentationCommand, RuntimeComponentPayload, RuntimeError,
    RuntimeEvent, RuntimeMutationRecord, ScheduledEvent, SerializedEffectEnvelope,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActionTrace {
    pub action_id: String,
    #[serde(default)]
    pub payload: BTreeMap<String, BlackboardValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActionCallRequest {
    pub step: u64,
    pub action_id: String,
    #[serde(default)]
    pub input: BTreeMap<String, BlackboardValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_event: Option<RuntimeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ActionCallResult {
    Ok {
        trace: ActionTrace,
        #[serde(default)]
        effects: Vec<ActionEffect>,
    },
    Err {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComponentSelector {
    ComponentId { component_id: ComponentId },
    ActorSchema { actor_id: ActorId, schema: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ActionEffect {
    SetBlackboard {
        key: String,
        value: BlackboardValue,
    },
    CreateActor {
        name: String,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default)]
        store_actor_id_key: Option<String>,
    },
    AttachComponent {
        actor_id: ActorId,
        schema: String,
        data: BlackboardValue,
        #[serde(default)]
        store_component_id_key: Option<String>,
    },
    ReplaceComponent {
        selector: ComponentSelector,
        expected_schema: String,
        expected_hash: Hash256,
        data: BlackboardValue,
    },
    PatchComponentMap {
        selector: ComponentSelector,
        expected_schema: String,
        expected_hash: Hash256,
        #[serde(default)]
        set: BTreeMap<String, BlackboardValue>,
        #[serde(default)]
        remove: BTreeSet<String>,
    },
    RemoveActor {
        actor_id: ActorId,
    },
    DetachComponent {
        component_id: ComponentId,
    },
    EmitEvent {
        source: EventSource,
        payload: EventPayload,
    },
    Presentation {
        command: PresentationCommand,
    },
    Await {
        token: AwaitToken,
    },
    ScheduleDelayedEvent {
        due_tick: u64,
        source: EventSource,
        payload: EventPayload,
    },
    CancelDelayedEvent {
        id: DelayedEventId,
    },
}

pub struct DeterministicActionContext<'a> {
    step: u64,
    id_source: &'a mut dyn FnMut() -> StableId,
    actors: &'a mut ActorStore,
    blackboard: &'a mut Blackboard,
    emitted_events: &'a mut Vec<RuntimeEvent>,
    presentation: &'a mut Vec<PresentationCommand>,
    awaits: &'a mut Vec<AwaitToken>,
    delayed_events: &'a mut Vec<ScheduledEvent>,
    delayed_cancellations: &'a mut Vec<DelayedEventId>,
    mutations: &'a mut Vec<RuntimeMutationRecord>,
    effects: &'a mut Vec<SerializedEffectEnvelope>,
    source: String,
    trigger_event: Option<RuntimeEvent>,
}

impl<'a> DeterministicActionContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        step: u64,
        id_source: &'a mut dyn FnMut() -> StableId,
        actors: &'a mut ActorStore,
        blackboard: &'a mut Blackboard,
        emitted_events: &'a mut Vec<RuntimeEvent>,
        presentation: &'a mut Vec<PresentationCommand>,
        awaits: &'a mut Vec<AwaitToken>,
        delayed_events: &'a mut Vec<ScheduledEvent>,
        delayed_cancellations: &'a mut Vec<DelayedEventId>,
        mutations: &'a mut Vec<RuntimeMutationRecord>,
        effects: &'a mut Vec<SerializedEffectEnvelope>,
        source: String,
        trigger_event: Option<RuntimeEvent>,
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
            effects,
            source,
            trigger_event,
        }
    }

    pub fn step(&self) -> u64 {
        self.step
    }

    pub fn trigger_event(&self) -> Option<&RuntimeEvent> {
        self.trigger_event.as_ref()
    }

    pub fn next_id(&mut self) -> StableId {
        (self.id_source)()
    }

    pub fn set_blackboard(&mut self, key: impl Into<String>, value: BlackboardValue) {
        self.blackboard.set(key, value);
    }

    pub fn blackboard(&self) -> &Blackboard {
        self.blackboard
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
        schema: impl Into<String>,
        data: BlackboardValue,
    ) -> Result<ComponentId, RuntimeError> {
        let component_id = ComponentId(self.next_id());
        if self.actors.attach_component(ComponentRecord {
            component_id,
            actor_id,
            payload: RuntimeComponentPayload::postcard(
                schema.into(),
                SchemaVersion::default(),
                &data,
            )?,
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
        self.actors.remove_actor(actor_id).is_some()
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> bool {
        self.actors.detach_component(component_id).is_some()
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

    pub fn read_component_postcard_bytes(
        &self,
        component_id: ComponentId,
    ) -> Result<Arc<[u8]>, RuntimeError> {
        let component = self.actors.component(component_id).ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_MISSING",
                "runtime component does not exist",
            ))
        })?;
        component.payload.validated_postcard_bytes()
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
            source: self.source.clone(),
        });
        Ok(())
    }

    fn resolve_component(&self, selector: &ComponentSelector) -> Result<ComponentId, RuntimeError> {
        match selector {
            ComponentSelector::ComponentId { component_id } => self
                .actors
                .component(*component_id)
                .map(|_| *component_id)
                .ok_or_else(component_missing),
            ComponentSelector::ActorSchema { actor_id, schema } => {
                let schema_id = SchemaId::from(schema.clone());
                let matches = self
                    .actors
                    .component_ids_for_actor_schema(*actor_id, &schema_id);
                match matches.as_slice() {
                    [component_id] => Ok(*component_id),
                    [] => Err(component_missing()),
                    _ => Err(RuntimeError::diagnostic(Diagnostic::blocking(
                        "ASTRA_RUNTIME_COMPONENT_SELECTOR_AMBIGUOUS",
                        "actor and schema selector resolves to multiple components",
                    ))),
                }
            }
        }
    }

    fn validate_component_precondition(
        &self,
        component_id: ComponentId,
        expected_schema: &str,
        expected_hash: Hash256,
    ) -> Result<(), RuntimeError> {
        let component = self
            .actors
            .component(component_id)
            .ok_or_else(component_missing)?;
        if component.payload.schema.as_str() != expected_schema {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_SCHEMA_MISMATCH",
                "runtime component schema does not match effect precondition",
            )));
        }
        if component.payload.hash != expected_hash {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_HASH_MISMATCH",
                "runtime component hash does not match effect precondition",
            )));
        }
        Ok(())
    }

    fn replace_component_value(
        &mut self,
        selector: ComponentSelector,
        expected_schema: String,
        expected_hash: Hash256,
        data: BlackboardValue,
    ) -> Result<(), RuntimeError> {
        let component_id = self.resolve_component(&selector)?;
        self.validate_component_precondition(component_id, &expected_schema, expected_hash)?;
        self.replace_component(component_id, &data)
    }

    fn patch_component_map(
        &mut self,
        selector: ComponentSelector,
        expected_schema: String,
        expected_hash: Hash256,
        set: BTreeMap<String, BlackboardValue>,
        remove: BTreeSet<String>,
    ) -> Result<(), RuntimeError> {
        if set.keys().any(|key| remove.contains(key)) {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_PATCH_CONFLICT",
                "component map patch cannot set and remove the same key",
            )));
        }
        let component_id = self.resolve_component(&selector)?;
        self.validate_component_precondition(component_id, &expected_schema, expected_hash)?;
        let value = self.read_component::<BlackboardValue>(component_id)?;
        let BlackboardValue::Map(mut values) = value else {
            return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_RUNTIME_COMPONENT_NOT_MAP",
                "component map patch requires a BlackboardValue::Map payload",
            )));
        };
        for key in remove {
            values.remove(&key);
        }
        values.extend(set);
        self.replace_component(component_id, &BlackboardValue::Map(values))
    }

    pub fn emit_event(&mut self, source: EventSource, payload: EventPayload) {
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
        self.presentation.push(command);
    }

    pub fn emit_serialized_effect<T: Serialize>(
        &mut self,
        domain: impl Into<String>,
        schema: impl Into<String>,
        value: &T,
    ) -> Result<(), RuntimeError> {
        self.effects
            .push(SerializedEffectEnvelope::postcard(domain, schema, value)?);
        Ok(())
    }

    pub fn push_await(&mut self, token: AwaitToken) -> Result<(), RuntimeError> {
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
        self.delayed_cancellations.push(id);
    }

    pub fn apply_effect(&mut self, effect: ActionEffect) -> Result<(), RuntimeError> {
        match effect {
            ActionEffect::SetBlackboard { key, value } => {
                self.set_blackboard(key, value);
            }
            ActionEffect::CreateActor {
                name,
                tags,
                store_actor_id_key,
            } => {
                let actor_id = self.create_actor(name, tags);
                if let Some(key) = store_actor_id_key {
                    self.set_blackboard(key, BlackboardValue::StableId(actor_id.0));
                }
            }
            ActionEffect::AttachComponent {
                actor_id,
                schema,
                data,
                store_component_id_key,
            } => {
                let component_id = self.attach_component(actor_id, schema, data)?;
                if let Some(key) = store_component_id_key {
                    self.set_blackboard(key, BlackboardValue::StableId(component_id.0));
                }
            }
            ActionEffect::ReplaceComponent {
                selector,
                expected_schema,
                expected_hash,
                data,
            } => self.replace_component_value(selector, expected_schema, expected_hash, data)?,
            ActionEffect::PatchComponentMap {
                selector,
                expected_schema,
                expected_hash,
                set,
                remove,
            } => self.patch_component_map(selector, expected_schema, expected_hash, set, remove)?,
            ActionEffect::RemoveActor { actor_id } => {
                self.remove_actor(actor_id);
            }
            ActionEffect::DetachComponent { component_id } => {
                self.detach_component(component_id);
            }
            ActionEffect::EmitEvent { source, payload } => self.emit_event(source, payload),
            ActionEffect::Presentation { command } => self.emit_presentation(command),
            ActionEffect::Await { token } => self.push_await(token)?,
            ActionEffect::ScheduleDelayedEvent {
                due_tick,
                source,
                payload,
            } => {
                self.schedule_event(due_tick, source, payload);
            }
            ActionEffect::CancelDelayedEvent { id } => self.cancel_delayed_event(id),
        }
        Ok(())
    }
}

fn component_missing() -> RuntimeError {
    RuntimeError::diagnostic(Diagnostic::blocking(
        "ASTRA_RUNTIME_COMPONENT_MISSING",
        "runtime component does not exist",
    ))
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
}

pub struct SetBlackboardAction;

impl RuntimeAction for SetBlackboardAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor {
            id: "astra.core.set_blackboard".to_string(),
            input_schema: "astra.action.set_blackboard.v1".to_string(),
            output_schema: "astra.action_trace.v1".to_string(),
        }
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
        ActionDescriptor {
            id: "astra.core.emit_event".to_string(),
            input_schema: "astra.action.emit_event.v1".to_string(),
            output_schema: "astra.action_trace.v1".to_string(),
        }
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
        ActionDescriptor {
            id: "astra.core.create_await".to_string(),
            input_schema: "astra.action.create_await.v1".to_string(),
            output_schema: "astra.action_trace.v1".to_string(),
        }
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
        ActionDescriptor {
            id: "astra.core.presentation".to_string(),
            input_schema: "astra.action.presentation.v1".to_string(),
            output_schema: "astra.action_trace.v1".to_string(),
        }
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
