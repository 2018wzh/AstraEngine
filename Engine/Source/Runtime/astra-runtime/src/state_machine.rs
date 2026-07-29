use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::{Diagnostic, DiagnosticSeverity, SourceRef, StableId, StableIdGenerator};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    actor::{ActorStoreAccess, ActorStoreDelta, ActorStoreOverlay},
    blackboard::{BlackboardAccess, BlackboardDelta, BlackboardOverlay},
    ActionExecutionClass, ActionInvocation, ActionRegistry, ActionResourceKey, ActionTrace,
    ActorId, ActorStore, AwaitToken, Blackboard, BlackboardValue, DelayedEventId,
    DeterministicActionContext, PresentationCommand, RuntimeError, RuntimeEvent, ScheduledEvent,
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
    pub definition: Arc<StateMachineDefinition>,
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
            definition: Arc::new(definition),
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

pub(crate) struct StateMachineTransactionCheckpoint {
    machine_states: Vec<(StableId, bool)>,
    trace_len: usize,
}

impl StateMachineStore {
    pub(crate) fn definition_fingerprint(&self) -> astra_core::Hash128 {
        astra_core::Hash128::from_blake3(
            &postcard::to_allocvec(
                &self
                    .machines
                    .iter()
                    .map(|machine| machine.definition.as_ref())
                    .collect::<Vec<_>>(),
            )
            .expect("state machine definitions must serialize for deterministic fingerprinting"),
        )
    }

    pub(crate) fn state_fingerprint(
        &self,
        definition_fingerprint: astra_core::Hash128,
    ) -> astra_core::Hash128 {
        astra_core::Hash128::from_blake3(
            &postcard::to_allocvec(&(
                definition_fingerprint,
                self.machines
                    .iter()
                    .map(|machine| (machine.current_state, machine.completed))
                    .collect::<Vec<_>>(),
            ))
            .expect("state machine state must serialize for deterministic fingerprinting"),
        )
    }

    pub(crate) fn transaction_checkpoint(&self) -> StateMachineTransactionCheckpoint {
        StateMachineTransactionCheckpoint {
            machine_states: self
                .machines
                .iter()
                .map(|machine| (machine.current_state, machine.completed))
                .collect(),
            trace_len: self.trace.len(),
        }
    }

    pub(crate) fn restore_transaction_checkpoint(
        &mut self,
        checkpoint: StateMachineTransactionCheckpoint,
    ) {
        assert_eq!(
            self.machines.len(),
            checkpoint.machine_states.len(),
            "state machine topology must not change during a tick transaction"
        );
        for (machine, (current_state, completed)) in
            self.machines.iter_mut().zip(checkpoint.machine_states)
        {
            machine.current_state = current_state;
            machine.completed = completed;
        }
        self.trace.truncate(checkpoint.trace_len);
    }

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

    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        step: u64,
        events: &[RuntimeEvent],
        actors: &mut ActorStore,
        blackboard: &mut Blackboard,
        actions: &ActionRegistry,
        id_source: &mut StableIdGenerator,
        worker_count: usize,
    ) -> StateMachineTickOutput {
        let mut output = StateMachineTickOutput::default();
        let waves = build_conflict_waves(&self.machines, actions);
        for wave in waves {
            let base_id_source = id_source.clone();
            let mut candidates = if worker_count > 1 && wave.len() > 1 {
                execute_parallel_wave(
                    &wave,
                    worker_count,
                    &self.machines,
                    step,
                    events,
                    actors,
                    blackboard,
                    actions,
                    &base_id_source,
                )
            } else {
                wave.iter()
                    .map(|machine_index| {
                        execute_machine_caught(
                            *machine_index,
                            &self.machines[*machine_index],
                            step,
                            events,
                            actors,
                            blackboard,
                            actions,
                            &base_id_source,
                        )
                    })
                    .collect()
            };
            candidates.sort_by_key(|candidate| candidate.machine_index);
            for candidate in candidates {
                if let Some(diagnostic) = candidate.failed {
                    debug!(
                        step,
                        machine_id = ?candidate.machine.definition.id,
                        current_state = ?candidate.machine.current_state,
                        diagnostic_code = %diagnostic.code,
                        "state_machine.transition.rollback"
                    );
                    output.diagnostics.push(diagnostic);
                    continue;
                }
                candidate.actor_delta.commit(actors);
                candidate.blackboard_delta.commit(blackboard);
                *id_source = candidate.id_source;
                self.trace.extend(candidate.output.trace.iter().cloned());
                output.append(candidate.output);
                self.machines[candidate.machine_index] = candidate.machine;
                debug!(
                    step,
                    machine_id = ?self.machines[candidate.machine_index].definition.id,
                    current_state = ?self.machines[candidate.machine_index].current_state,
                    microsteps = candidate.microsteps,
                    "state_machine.transition.commit"
                );
            }
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

#[derive(Clone)]
struct MachineAccessPlan {
    serial: bool,
    reads: BTreeSet<ActionResourceKey>,
    writes: BTreeSet<ActionResourceKey>,
}

struct MachineCandidate {
    machine_index: usize,
    machine: StateMachineInstance,
    actor_delta: ActorStoreDelta,
    blackboard_delta: BlackboardDelta,
    id_source: StableIdGenerator,
    output: StateMachineTickOutput,
    microsteps: u32,
    failed: Option<Diagnostic>,
}

fn build_conflict_waves(
    machines: &[StateMachineInstance],
    actions: &ActionRegistry,
) -> Vec<Vec<usize>> {
    let mut waves = Vec::new();
    let mut current = Vec::new();
    let mut current_plans = Vec::new();
    for (machine_index, machine) in machines.iter().enumerate() {
        if machine.completed {
            continue;
        }
        let plan = machine_access_plan(machine, actions);
        let conflicts = plan.serial
            || current_plans
                .iter()
                .any(|current_plan| access_conflicts(&plan, current_plan));
        if conflicts && !current.is_empty() {
            waves.push(std::mem::take(&mut current));
            current_plans.clear();
        }
        if plan.serial {
            waves.push(vec![machine_index]);
        } else {
            current.push(machine_index);
            current_plans.push(plan);
        }
    }
    if !current.is_empty() {
        waves.push(current);
    }
    waves
}

fn machine_access_plan(
    machine: &StateMachineInstance,
    actions: &ActionRegistry,
) -> MachineAccessPlan {
    let mut plan = MachineAccessPlan {
        serial: false,
        reads: BTreeSet::new(),
        writes: BTreeSet::new(),
    };
    for invocation in machine
        .definition
        .transitions
        .iter()
        .flat_map(|transition| &transition.actions)
    {
        let Some(descriptor) = actions.descriptor(&invocation.action_id) else {
            plan.serial = true;
            continue;
        };
        plan.serial |= descriptor.execution == ActionExecutionClass::Serial;
        plan.reads.extend(descriptor.access.reads);
        plan.writes.extend(descriptor.access.writes);
    }
    plan
}

fn access_conflicts(left: &MachineAccessPlan, right: &MachineAccessPlan) -> bool {
    left.serial
        || right.serial
        || resource_sets_conflict(&left.writes, &right.writes)
        || resource_sets_conflict(&left.writes, &right.reads)
        || resource_sets_conflict(&left.reads, &right.writes)
}

fn resource_sets_conflict(
    left: &BTreeSet<ActionResourceKey>,
    right: &BTreeSet<ActionResourceKey>,
) -> bool {
    left.iter()
        .any(|left| right.iter().any(|right| resource_keys_overlap(left, right)))
}

fn resource_keys_overlap(left: &ActionResourceKey, right: &ActionResourceKey) -> bool {
    left == right
        || matches!(
            (left, right),
            (
                ActionResourceKey::ActorStore,
                ActionResourceKey::ComponentSchema(_)
            ) | (
                ActionResourceKey::ComponentSchema(_),
                ActionResourceKey::ActorStore
            ) | (
                ActionResourceKey::Blackboard,
                ActionResourceKey::BlackboardKey(_)
            ) | (
                ActionResourceKey::BlackboardKey(_),
                ActionResourceKey::Blackboard
            )
        )
}

fn resource_is_declared(
    declared: &BTreeSet<ActionResourceKey>,
    observed: &ActionResourceKey,
) -> bool {
    declared
        .iter()
        .any(|resource| resource_keys_overlap(resource, observed))
}

fn validate_observed_access(
    action_id: &str,
    declared: &crate::ActionAccess,
    observed: &crate::ActionAccess,
) -> Result<(), Diagnostic> {
    if let Some(resource) = observed.reads.iter().find(|resource| {
        !resource_is_declared(&declared.reads, resource)
            && !resource_is_declared(&declared.writes, resource)
    }) {
        return Err(Diagnostic::blocking(
            "ASTRA_RUNTIME_ACTION_ACCESS_UNDECLARED",
            "action performed an undeclared deterministic read",
        )
        .with_field("action_id", action_id)
        .with_field("access_mode", "read")
        .with_field("resource", format!("{resource:?}")));
    }
    if let Some(resource) = observed
        .writes
        .iter()
        .find(|resource| !resource_is_declared(&declared.writes, resource))
    {
        return Err(Diagnostic::blocking(
            "ASTRA_RUNTIME_ACTION_ACCESS_UNDECLARED",
            "action performed an undeclared deterministic write",
        )
        .with_field("action_id", action_id)
        .with_field("access_mode", "write")
        .with_field("resource", format!("{resource:?}")));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_parallel_wave(
    wave: &[usize],
    worker_count: usize,
    machines: &[StateMachineInstance],
    step: u64,
    events: &[RuntimeEvent],
    actors: &ActorStore,
    blackboard: &Blackboard,
    actions: &ActionRegistry,
    id_source: &StableIdGenerator,
) -> Vec<MachineCandidate> {
    let workers = worker_count.max(1).min(wave.len());
    let chunk_size = wave.len().div_ceil(workers);
    std::thread::scope(|scope| {
        let handles = wave
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|machine_index| {
                            execute_machine_caught(
                                *machine_index,
                                &machines[*machine_index],
                                step,
                                events,
                                actors,
                                blackboard,
                                actions,
                                id_source,
                            )
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .expect("execute_machine_caught contains provider action panics")
            })
            .collect()
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_machine_caught(
    machine_index: usize,
    machine: &StateMachineInstance,
    step: u64,
    events: &[RuntimeEvent],
    actors: &ActorStore,
    blackboard: &Blackboard,
    actions: &ActionRegistry,
    id_source: &StableIdGenerator,
) -> MachineCandidate {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute_machine(
            machine_index,
            machine,
            step,
            events,
            actors,
            blackboard,
            actions,
            id_source,
        )
    }))
    .unwrap_or_else(|_| MachineCandidate {
        machine_index,
        machine: machine.clone(),
        actor_delta: ActorStoreOverlay::new(actors).into_delta(),
        blackboard_delta: BlackboardOverlay::new(blackboard).into_delta(),
        id_source: id_source.clone(),
        output: StateMachineTickOutput::default(),
        microsteps: 0,
        failed: Some(Diagnostic::blocking(
            "ASTRA_RUNTIME_ACTION_PANIC",
            "runtime action panicked while executing a deterministic candidate",
        )),
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_machine(
    machine_index: usize,
    machine: &StateMachineInstance,
    step: u64,
    events: &[RuntimeEvent],
    actors: &ActorStore,
    blackboard: &Blackboard,
    actions: &ActionRegistry,
    id_source: &StableIdGenerator,
) -> MachineCandidate {
    let mut candidate_machine = machine.clone();
    let mut candidate_actors = ActorStoreOverlay::new(actors);
    let mut candidate_blackboard = BlackboardOverlay::new(blackboard);
    let mut candidate_id_source = id_source.clone();
    let mut candidate_output = StateMachineTickOutput::default();
    let mut available_events = events.to_vec();
    let mut visited = BTreeSet::new();
    let mut microsteps = 0_u32;
    let mut failed = None;
    loop {
        if candidate_machine.completed {
            break;
        }
        let fingerprint = machine_fingerprint(
            &candidate_machine,
            &candidate_actors,
            &candidate_blackboard,
            &candidate_id_source,
            &available_events,
        );
        if !visited.insert(fingerprint) {
            failed = Some(Diagnostic::blocking(
                "ASTRA_RUNTIME_STATE_MACHINE_CYCLE",
                "state machine repeated the same deterministic microstep state",
            ));
            break;
        }
        if microsteps >= 1024 {
            failed = Some(
                Diagnostic::blocking(
                    "ASTRA_RUNTIME_STATE_MACHINE_BUDGET",
                    "state machine exceeded the microstep budget",
                )
                .with_field("max_microsteps", 1024_u32),
            );
            break;
        }
        let Some((transition, failure_source, trigger_event_index)) = find_transition(
            &candidate_machine,
            &available_events,
            &candidate_actors,
            &candidate_blackboard,
        ) else {
            break;
        };
        let trigger_event =
            trigger_event_index.and_then(|index| available_events.get(index).cloned());
        debug!(
            step,
            machine_id = ?candidate_machine.definition.id,
            from_state = ?transition.from,
            to_state = ?transition.to,
            microstep = microsteps,
            action_count = transition.actions.len(),
            "state_machine.transition.match"
        );
        let mut transition_failed = None;
        for invocation in &transition.actions {
            let Some(action) = actions.get(&invocation.action_id) else {
                transition_failed = Some(Diagnostic::blocking(
                    "ASTRA_RUNTIME_ACTION_MISSING",
                    format!("missing action {}", invocation.action_id),
                ));
                break;
            };
            let descriptor = action.descriptor();
            let mut stable_ids_used = 0_u32;
            let mut next_id = || {
                stable_ids_used = stable_ids_used.saturating_add(1);
                candidate_id_source.next_id()
            };
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
                &mut candidate_output.mutations,
                &mut candidate_output.effects,
                invocation.action_id.clone(),
                trigger_event.clone(),
            );
            let action_result = action.run(&mut ctx, &invocation.input);
            let observed_access = ctx.observed_access();
            drop(ctx);
            if let Err(diagnostic) = validate_observed_access(
                &invocation.action_id,
                &descriptor.access,
                &observed_access,
            ) {
                transition_failed = Some(diagnostic);
                break;
            }
            if stable_ids_used > descriptor.stable_id_reservation {
                transition_failed = Some(
                    Diagnostic::blocking(
                        "ASTRA_RUNTIME_ACTION_ID_RESERVATION_EXCEEDED",
                        "action consumed more StableIds than declared",
                    )
                    .with_field("action_id", &invocation.action_id)
                    .with_field(
                        "stable_id_reservation",
                        descriptor.stable_id_reservation.to_string(),
                    )
                    .with_field("stable_ids_used", stable_ids_used.to_string()),
                );
                break;
            }
            match action_result {
                Ok(trace) => candidate_output.trace.push(trace),
                Err(err) => {
                    transition_failed = Some(match err {
                        RuntimeError::Diagnostic(diagnostic) => diagnostic,
                        RuntimeError::Message(message) => Diagnostic::blocking(
                            "ASTRA_RUNTIME_ACTION_FAILED",
                            format!("{} failed: {message}", invocation.action_id),
                        ),
                    });
                    break;
                }
            }
        }
        if let Some(mut diagnostic) = transition_failed {
            if let Some(source) = failure_source {
                diagnostic.source = Some(source);
            }
            failed = Some(diagnostic);
            break;
        }
        if let Some(index) = trigger_event_index {
            available_events.remove(index);
        }
        candidate_machine.current_state = transition.to;
        if state_is_terminal(&candidate_machine, candidate_machine.current_state) {
            candidate_machine.completed = true;
        }
        microsteps += 1;
    }
    MachineCandidate {
        machine_index,
        machine: candidate_machine,
        actor_delta: candidate_actors.into_delta(),
        blackboard_delta: candidate_blackboard.into_delta(),
        id_source: candidate_id_source,
        output: candidate_output,
        microsteps,
        failed,
    }
}

fn find_transition(
    machine: &StateMachineInstance,
    events: &[RuntimeEvent],
    actors: &dyn ActorStoreAccess,
    blackboard: &dyn BlackboardAccess,
) -> Option<(TransitionDefinition, Option<SourceRef>, Option<usize>)> {
    let actor_snapshots = actors.actor_snapshots();
    let mut best: Option<(&TransitionDefinition, Option<usize>)> = None;
    for transition in machine
        .definition
        .transitions
        .iter()
        .filter(|transition| transition.from == machine.current_state)
    {
        let trigger_event_index = match transition.guard {
            GuardExpr::Always => Some(None),
            _ if !transition.guard.depends_on_event() => transition
                .guard
                .evaluate(None, &actor_snapshots, blackboard)
                .then_some(None),
            _ => events.iter().enumerate().find_map(|(index, event)| {
                transition
                    .guard
                    .evaluate(Some(event), &actor_snapshots, blackboard)
                    .then_some(Some(index))
            }),
        };
        if let Some(trigger_event_index) = trigger_event_index {
            match best {
                Some((current, _)) if transition.priority <= current.priority => {}
                _ => best = Some((transition, trigger_event_index)),
            }
        }
    }
    best.map(|(transition, trigger_event_index)| {
        (
            transition.clone(),
            transition.source_ref.clone(),
            trigger_event_index,
        )
    })
}

fn machine_fingerprint(
    machine: &StateMachineInstance,
    actors: &dyn ActorStoreAccess,
    blackboard: &dyn BlackboardAccess,
    id_source: &StableIdGenerator,
    events: &[RuntimeEvent],
) -> astra_core::Hash128 {
    let actor_fingerprint = actors.deterministic_fingerprint();
    let blackboard_fingerprint = blackboard.deterministic_fingerprint();
    astra_core::Hash128::from_blake3(
        &postcard::to_allocvec(&(
            machine.current_state,
            machine.completed,
            actor_fingerprint,
            blackboard_fingerprint,
            id_source,
            events,
        ))
        .expect("state machine candidate must serialize for cycle detection"),
    )
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
    pub mutations: Vec<crate::RuntimeMutationRecord>,
    pub effects: Vec<crate::SerializedEffectEnvelope>,
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
        self.mutations.append(&mut other.mutations);
        self.effects.append(&mut other.effects);
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
        event: Option<&RuntimeEvent>,
        actors: &[crate::ActorSnapshot],
        blackboard: &dyn BlackboardAccess,
    ) -> bool {
        match self {
            GuardExpr::Always => true,
            GuardExpr::EventIs { kind } => event.is_some_and(|event| event.payload.kind == *kind),
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

    fn depends_on_event(&self) -> bool {
        match self {
            GuardExpr::EventIs { .. } => true,
            GuardExpr::And { terms } | GuardExpr::Or { terms } => {
                terms.iter().any(GuardExpr::depends_on_event)
            }
            GuardExpr::Not { term } => term.depends_on_event(),
            GuardExpr::Always
            | GuardExpr::BlackboardEquals { .. }
            | GuardExpr::HasActorTag { .. } => false,
        }
    }
}
