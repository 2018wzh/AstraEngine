use std::{collections::BTreeMap, sync::Arc};

use astra_core::{Diagnostic, SourceRef, StableId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ActorId, AwaitKind, AwaitReplayPolicy, AwaitToken, AwaitTokenId, Blackboard, BlackboardValue,
    EventPayload, EventSource, PresentationCommand, RuntimeError, RuntimeEvent,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateDefinition {
    pub id: StableId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineDefinition {
    pub id: StableId,
    pub owner: ActorId,
    pub states: Vec<StateDefinition>,
    pub transitions: Vec<TransitionDefinition>,
    pub initial_state: StableId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TransitionDefinition {
    pub from: StableId,
    pub to: StableId,
    pub guard: GuardExpr,
    pub action: ActionInvocation,
    pub source_ref: Option<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum GuardExpr {
    Always,
    EventIs { kind: String },
    BlackboardEquals { key: String, value: BlackboardValue },
    HasActorTag { actor: ActorId, tag: String },
    And { terms: Vec<GuardExpr> },
    Or { terms: Vec<GuardExpr> },
    Not { term: Box<GuardExpr> },
}

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

pub struct ActionContext<'a> {
    pub step: u64,
    pub id_source: &'a mut dyn FnMut() -> StableId,
    pub blackboard: &'a mut Blackboard,
    pub emitted_events: &'a mut Vec<RuntimeEvent>,
    pub presentation: &'a mut Vec<PresentationCommand>,
    pub awaits: &'a mut Vec<AwaitToken>,
}

pub trait RuntimeAction: Send + Sync {
    fn descriptor(&self) -> ActionDescriptor;
    fn run(
        &self,
        ctx: &mut ActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError>;
}

#[derive(Default, Clone)]
pub struct ActionRegistry {
    actions: BTreeMap<String, Arc<dyn RuntimeAction>>,
}

impl ActionRegistry {
    pub fn register<A: RuntimeAction + 'static>(&mut self, action: A) {
        self.actions
            .insert(action.descriptor().id.clone(), Arc::new(action));
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn RuntimeAction>> {
        self.actions.get(id).cloned()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineInstance {
    pub definition: StateMachineDefinition,
    pub current_state: StableId,
    pub completed: bool,
}

impl StateMachineInstance {
    pub fn new(definition: StateMachineDefinition) -> Self {
        Self {
            current_state: definition.initial_state,
            definition,
            completed: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineStore {
    machines: Vec<StateMachineInstance>,
    trace: Vec<ActionTrace>,
}

impl StateMachineStore {
    pub fn add(&mut self, definition: StateMachineDefinition) {
        self.machines.push(StateMachineInstance::new(definition));
        self.machines
            .sort_by_key(|machine| machine.definition.id.to_string());
    }

    pub fn tick(
        &mut self,
        step: u64,
        events: &[RuntimeEvent],
        actors: &[crate::ActorSnapshot],
        blackboard: &mut Blackboard,
        actions: &ActionRegistry,
        id_source: &mut dyn FnMut() -> StableId,
    ) -> Result<StateMachineTickOutput, RuntimeError> {
        let mut output = StateMachineTickOutput::default();
        for machine in &mut self.machines {
            if machine.completed {
                continue;
            }
            for event in events {
                let Some(transition) = machine
                    .definition
                    .transitions
                    .iter()
                    .find(|transition| {
                        transition.from == machine.current_state
                            && transition.guard.evaluate(event, actors, blackboard)
                    })
                    .cloned()
                else {
                    continue;
                };
                let action = actions.get(&transition.action.action_id).ok_or_else(|| {
                    RuntimeError::diagnostic(Diagnostic::blocking(
                        "ASTRA_RUNTIME_ACTION_MISSING",
                        format!("missing action {}", transition.action.action_id),
                    ))
                })?;
                let mut ctx = ActionContext {
                    step,
                    id_source,
                    blackboard,
                    emitted_events: &mut output.events,
                    presentation: &mut output.presentation,
                    awaits: &mut output.awaits,
                };
                let trace = action.run(&mut ctx, &transition.action.input)?;
                self.trace.push(trace.clone());
                output.trace.push(trace);
                machine.current_state = transition.to;
                break;
            }
        }
        Ok(output)
    }

    pub fn snapshots(&self, actor: ActorId) -> Vec<StateMachineSnapshot> {
        self.machines
            .iter()
            .filter(|machine| machine.definition.owner == actor)
            .map(|machine| StateMachineSnapshot {
                id: machine.definition.id,
                owner: machine.definition.owner,
                current_state: machine.current_state,
                completed: machine.completed,
            })
            .collect()
    }

    pub fn trace(&self) -> &[ActionTrace] {
        &self.trace
    }
}

#[derive(Default)]
pub struct StateMachineTickOutput {
    pub events: Vec<RuntimeEvent>,
    pub presentation: Vec<PresentationCommand>,
    pub awaits: Vec<AwaitToken>,
    pub trace: Vec<ActionTrace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineSnapshot {
    pub id: StableId,
    pub owner: ActorId,
    pub current_state: StableId,
    pub completed: bool,
}

impl GuardExpr {
    fn evaluate(
        &self,
        event: &RuntimeEvent,
        actors: &[crate::ActorSnapshot],
        blackboard: &Blackboard,
    ) -> bool {
        match self {
            GuardExpr::Always => true,
            GuardExpr::EventIs { kind } => event.payload.kind == *kind,
            GuardExpr::BlackboardEquals { key, value } => blackboard.get(key) == Some(value),
            GuardExpr::HasActorTag { actor, tag } => actors
                .iter()
                .any(|snapshot| snapshot.actor_id == *actor && snapshot.tags.contains(tag)),
            GuardExpr::And { terms } => terms
                .iter()
                .all(|term| term.evaluate(event, actors, blackboard)),
            GuardExpr::Or { terms } => terms
                .iter()
                .any(|term| term.evaluate(event, actors, blackboard)),
            GuardExpr::Not { term } => !term.evaluate(event, actors, blackboard),
        }
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
        ctx: &mut ActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let Some(BlackboardValue::String(key)) = input.get("key") else {
            return Err(RuntimeError::message("set_blackboard requires string key"));
        };
        let value = input.get("value").cloned().unwrap_or(BlackboardValue::Null);
        ctx.blackboard.set(key.clone(), value.clone());
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
        ctx: &mut ActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let Some(BlackboardValue::String(kind)) = input.get("kind") else {
            return Err(RuntimeError::message("emit_event requires string kind"));
        };
        let event = RuntimeEvent {
            id: crate::EventId((ctx.id_source)()),
            source: EventSource::StateMachine,
            step: ctx.step,
            sequence: 0,
            payload: EventPayload::new(kind.clone()),
        };
        ctx.emitted_events.push(event);
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
        ctx: &mut ActionContext<'_>,
        input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let token = AwaitToken {
            token_id: AwaitTokenId((ctx.id_source)()),
            kind: AwaitKind::Custom("scenario".to_string()),
            requested_at_step: ctx.step,
            deterministic_timeout_step: None,
            replay_policy: AwaitReplayPolicy::RecordedResult,
        };
        ctx.awaits.push(token);
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
        ctx: &mut ActionContext<'_>,
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
        ctx.presentation.push(command);
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
