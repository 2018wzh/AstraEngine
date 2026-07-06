use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Diagnostic, DiagnosticSeverity, SourceRef, StableId, StableIdGenerator};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::{
    ActionInvocation, ActionRegistry, ActionTrace, ActorId, ActorStore, AwaitToken, Blackboard,
    BlackboardValue, DelayedEventId, DeterministicActionContext, PresentationCommand, RuntimeError,
    RuntimeEvent, ScheduledEvent,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateDefinition {
    pub id: StableId,
    pub name: String,
    #[serde(default)]
    pub terminal: bool,
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
    #[serde(default)]
    pub actions: Vec<ActionInvocation>,
    #[serde(default)]
    pub priority: i32,
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
pub struct StateMachineInstance {
    pub definition: StateMachineDefinition,
    pub current_state: StableId,
    pub completed: bool,
}

impl StateMachineInstance {
    pub fn new(definition: StateMachineDefinition) -> Self {
        let completed = definition
            .states
            .iter()
            .any(|state| state.id == definition.initial_state && state.terminal);
        Self {
            current_state: definition.initial_state,
            definition,
            completed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineValidationReport {
    pub valid: bool,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn validate_state_machine(definition: &StateMachineDefinition) -> StateMachineValidationReport {
    let mut diagnostics = Vec::new();
    if definition.states.is_empty() {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_RUNTIME_STATE_MACHINE_EMPTY",
            "state machine must define at least one state",
        ));
    }

    let mut state_ids = BTreeSet::new();
    for state in &definition.states {
        if !state_ids.insert(state.id) {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_STATE_DUPLICATE",
                    "state machine contains duplicate state ids",
                )
                .with_field("state", state.id),
            );
        }
    }

    if !state_ids.contains(&definition.initial_state) {
        diagnostics.push(
            Diagnostic::blocking(
                "ASTRA_RUNTIME_INITIAL_STATE_UNKNOWN",
                "state machine initial state is not declared",
            )
            .with_field("state", definition.initial_state),
        );
    }

    for transition in &definition.transitions {
        if !state_ids.contains(&transition.from) {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_STATE_UNKNOWN",
                    "transition source state is not declared",
                )
                .with_field("state", transition.from),
            );
        }
        if !state_ids.contains(&transition.to) {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_STATE_UNKNOWN",
                    "transition target state is not declared",
                )
                .with_field("state", transition.to),
            );
        }
    }

    let mut transition_keys: BTreeMap<(StableId, i32, String), SourceRef> = BTreeMap::new();
    for transition in &definition.transitions {
        let guard_key = guard_conflict_key(&transition.guard);
        let key = (transition.from, transition.priority, guard_key);
        if let Some(first_source) = transition_keys.get(&key) {
            let mut diagnostic = Diagnostic::blocking(
                "ASTRA_RUNTIME_TRANSITION_CONFLICT",
                "transitions from the same state share the same guard and priority",
            )
            .with_field("state", transition.from)
            .with_field("priority", transition.priority);
            diagnostic.source = transition
                .source_ref
                .clone()
                .or_else(|| Some(first_source.clone()));
            diagnostics.push(diagnostic);
        } else if let Some(source) = &transition.source_ref {
            transition_keys.insert(key, source.clone());
        } else {
            transition_keys.insert(
                key,
                SourceRef {
                    source: "state_machine".to_string(),
                    line: 0,
                    column: 0,
                    length: 0,
                },
            );
        }
    }

    let valid = !diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            DiagnosticSeverity::Blocking | DiagnosticSeverity::Error
        )
    });
    StateMachineValidationReport { valid, diagnostics }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineStore {
    machines: Vec<StateMachineInstance>,
    trace: Vec<ActionTrace>,
}

impl StateMachineStore {
    pub fn add(&mut self, definition: StateMachineDefinition) -> Result<(), RuntimeError> {
        if self
            .machines
            .iter()
            .any(|machine| machine.definition.id == definition.id)
        {
            return Err(RuntimeError::diagnostic(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_STATE_MACHINE_DUPLICATE",
                    "state machine id is already registered",
                )
                .with_field("machine", definition.id),
            ));
        }
        let report = validate_state_machine(&definition);
        if !report.valid {
            let diagnostic = report.diagnostics.into_iter().next().unwrap_or_else(|| {
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_STATE_MACHINE_INVALID",
                    "state machine validation failed",
                )
            });
            return Err(RuntimeError::diagnostic(diagnostic));
        }
        self.machines.push(StateMachineInstance::new(definition));
        self.machines
            .sort_by_key(|machine| machine.definition.id.to_string());
        Ok(())
    }

    pub fn tick(
        &mut self,
        step: u64,
        events: &[RuntimeEvent],
        actors: &mut ActorStore,
        blackboard: &mut Blackboard,
        actions: &ActionRegistry,
        id_source: &mut StableIdGenerator,
    ) -> StateMachineTickOutput {
        let mut output = StateMachineTickOutput::default();
        for machine in &mut self.machines {
            if machine.completed {
                continue;
            }
            let Some((transition, failure_source)) =
                find_transition(machine, events, actors, blackboard)
            else {
                continue;
            };
            debug!(
                step,
                machine_id = ?machine.definition.id,
                from_state = ?transition.from,
                to_state = ?transition.to,
                action_count = transition.actions.len(),
                "state_machine.transition.match"
            );
            let mut candidate_actors = actors.clone();
            let mut candidate_blackboard = blackboard.clone();
            let mut candidate_id_source = id_source.clone();
            let mut candidate_output = StateMachineTickOutput::default();
            let mut failed = None;

            for invocation in &transition.actions {
                debug!(
                    step,
                    machine_id = ?machine.definition.id,
                    action_id = %invocation.action_id,
                    "state_machine.action.start"
                );
                let Some(action) = actions.get(&invocation.action_id) else {
                    failed = Some(Diagnostic::blocking(
                        "ASTRA_RUNTIME_ACTION_MISSING",
                        format!("missing action {}", invocation.action_id),
                    ));
                    warn!(
                        step,
                        machine_id = ?machine.definition.id,
                        action_id = %invocation.action_id,
                        diagnostic_code = "ASTRA_RUNTIME_ACTION_MISSING",
                        "state_machine.action.missing"
                    );
                    break;
                };
                let mut next_id = || candidate_id_source.next_id();
                let mut ctx = DeterministicActionContext::new(
                    step,
                    &mut next_id,
                    &mut candidate_actors,
                    &mut candidate_blackboard,
                    &mut candidate_output.events,
                    &mut candidate_output.presentation,
                    &mut candidate_output.awaits,
                    &mut candidate_output.delayed_events,
                    &mut candidate_output.delayed_cancellations,
                );
                match action.run(&mut ctx, &invocation.input) {
                    Ok(trace) => {
                        debug!(
                            step,
                            machine_id = ?machine.definition.id,
                            action_id = %trace.action_id,
                            "state_machine.action.end"
                        );
                        candidate_output.trace.push(trace);
                    }
                    Err(err) => {
                        failed = Some(Diagnostic::blocking(
                            "ASTRA_RUNTIME_ACTION_FAILED",
                            format!("{} failed: {err}", invocation.action_id),
                        ));
                        warn!(
                            step,
                            machine_id = ?machine.definition.id,
                            action_id = %invocation.action_id,
                            diagnostic_code = "ASTRA_RUNTIME_ACTION_FAILED",
                            "state_machine.action.failed"
                        );
                        break;
                    }
                }
            }

            if let Some(mut diagnostic) = failed {
                if let Some(source) = failure_source {
                    diagnostic.source = Some(source);
                }
                debug!(
                    step,
                    machine_id = ?machine.definition.id,
                    current_state = ?machine.current_state,
                    diagnostic_code = %diagnostic.code,
                    "state_machine.transition.rollback"
                );
                output.diagnostics.push(diagnostic);
                continue;
            }

            *actors = candidate_actors;
            *blackboard = candidate_blackboard;
            *id_source = candidate_id_source;
            self.trace.extend(candidate_output.trace.iter().cloned());
            output.append(candidate_output);
            machine.current_state = transition.to;
            if state_is_terminal(machine, machine.current_state) {
                machine.completed = true;
            }
            debug!(
                step,
                machine_id = ?machine.definition.id,
                current_state = ?machine.current_state,
                "state_machine.transition.commit"
            );
        }
        output
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

fn find_transition(
    machine: &StateMachineInstance,
    events: &[RuntimeEvent],
    actors: &ActorStore,
    blackboard: &Blackboard,
) -> Option<(TransitionDefinition, Option<SourceRef>)> {
    let actor_snapshots = actors.actor_snapshots();
    let mut best: Option<&TransitionDefinition> = None;
    for transition in machine
        .definition
        .transitions
        .iter()
        .filter(|transition| transition.from == machine.current_state)
    {
        let matched = match transition.guard {
            GuardExpr::Always => true,
            _ => events.iter().any(|event| {
                transition
                    .guard
                    .evaluate(event, &actor_snapshots, blackboard)
            }),
        };
        if matched {
            match best {
                Some(current) if transition.priority <= current.priority => {}
                _ => best = Some(transition),
            }
        }
    }
    best.map(|transition| (transition.clone(), transition.source_ref.clone()))
}

fn state_is_terminal(machine: &StateMachineInstance, state_id: StableId) -> bool {
    machine
        .definition
        .states
        .iter()
        .any(|state| state.id == state_id && state.terminal)
}

fn guard_conflict_key(guard: &GuardExpr) -> String {
    match guard {
        GuardExpr::Always => "always".to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| format!("{other:?}")),
    }
}

#[derive(Default)]
pub struct StateMachineTickOutput {
    pub events: Vec<RuntimeEvent>,
    pub presentation: Vec<PresentationCommand>,
    pub awaits: Vec<AwaitToken>,
    pub delayed_events: Vec<ScheduledEvent>,
    pub delayed_cancellations: Vec<DelayedEventId>,
    pub trace: Vec<ActionTrace>,
    pub diagnostics: Vec<Diagnostic>,
}

impl StateMachineTickOutput {
    fn append(&mut self, mut other: Self) {
        self.events.append(&mut other.events);
        self.presentation.append(&mut other.presentation);
        self.awaits.append(&mut other.awaits);
        self.delayed_events.append(&mut other.delayed_events);
        self.delayed_cancellations
            .append(&mut other.delayed_cancellations);
        self.trace.append(&mut other.trace);
        self.diagnostics.append(&mut other.diagnostics);
    }
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
