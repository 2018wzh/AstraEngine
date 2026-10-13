use super::*;
use crate::{
    AwaitKind, BlackboardValue, EventPayload, EventSource, PresentationCommand, RuntimeError,
};
use astra_core::Diagnostic;
use std::{collections::BTreeMap, sync::Arc};

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
