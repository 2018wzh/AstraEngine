use std::{collections::BTreeMap, sync::Arc};

use astra_core::{Diagnostic, SchemaVersion, StableId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ActorId, ActorRecord, ActorStore, AwaitKind, AwaitReplayPolicy, AwaitToken, AwaitTokenId,
    Blackboard, BlackboardValue, ComponentId, ComponentRecord, DelayedEventId, EventId,
    EventPayload, EventSource, PresentationCommand, RuntimeError, RuntimeEvent, ScheduledEvent,
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
        }
    }

    pub fn step(&self) -> u64 {
        self.step
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
            schema: schema.into(),
            version: SchemaVersion::default(),
            data,
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

    pub fn push_await(&mut self, token: AwaitToken) {
        self.awaits.push(token);
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
            ActionEffect::RemoveActor { actor_id } => {
                self.remove_actor(actor_id);
            }
            ActionEffect::DetachComponent { component_id } => {
                self.detach_component(component_id);
            }
            ActionEffect::EmitEvent { source, payload } => self.emit_event(source, payload),
            ActionEffect::Presentation { command } => self.emit_presentation(command),
            ActionEffect::Await { token } => self.push_await(token),
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
    pub fn register<A: RuntimeAction + 'static>(&mut self, action: A) {
        self.register_with_provider("astra.core", action);
    }

    pub fn register_with_provider<A: RuntimeAction + 'static>(
        &mut self,
        provider_id: impl Into<String>,
        action: A,
    ) {
        let provider_id = provider_id.into();
        self.actions.insert(
            action.descriptor().id.clone(),
            RegisteredAction {
                provider_id,
                action: Arc::new(action),
            },
        );
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
        ctx.push_await(token);
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
