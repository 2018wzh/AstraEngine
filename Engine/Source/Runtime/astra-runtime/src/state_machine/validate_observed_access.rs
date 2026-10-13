use super::*;

pub(super) fn validate_observed_access(
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
pub(super) fn execute_parallel_wave(
    wave: &[usize],
    worker_count: usize,
    machines: &[StateMachineInstance],
    step: u64,
    events: &[RuntimeEvent],
    actors: &ActorStore,
    blackboard: &Blackboard,
    actions: &ActionRegistry,
    id_source: &StableIdGenerator,
    evidence_mode: bool,
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
                                evidence_mode,
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
pub(super) fn execute_machine_caught(
    machine_index: usize,
    machine: &StateMachineInstance,
    step: u64,
    events: &[RuntimeEvent],
    actors: &ActorStore,
    blackboard: &Blackboard,
    actions: &ActionRegistry,
    id_source: &StableIdGenerator,
    evidence_mode: bool,
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
            evidence_mode,
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

pub(super) struct CandidateEvents<'a> {
    pub(super) events: &'a [RuntimeEvent],
    pub(super) consumed: Vec<bool>,
    pub(super) by_kind: BTreeMap<&'a str, Vec<usize>>,
}

impl<'a> CandidateEvents<'a> {
    pub(super) fn new(events: &'a [RuntimeEvent]) -> Self {
        let mut by_kind = BTreeMap::new();
        for (index, event) in events.iter().enumerate() {
            by_kind
                .entry(event.payload.kind.as_str())
                .or_insert_with(Vec::new)
                .push(index);
        }
        Self {
            events,
            consumed: vec![false; events.len()],
            by_kind,
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<&RuntimeEvent> {
        self.consumed
            .get(index)
            .is_some_and(|consumed| !consumed)
            .then(|| self.events.get(index))
            .flatten()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &RuntimeEvent)> {
        self.events
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.consumed[*index])
    }

    pub(super) fn iter_kinds<'b>(
        &'b self,
        kinds: &'b BTreeSet<String>,
    ) -> impl Iterator<Item = (usize, &'a RuntimeEvent)> + 'b {
        let mut indices = kinds
            .iter()
            .filter_map(|kind| self.by_kind.get(kind.as_str()))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        indices.sort_unstable();
        indices.dedup();
        indices
            .into_iter()
            .filter(|index| !self.consumed[*index])
            .map(|index| (index, &self.events[index]))
    }

    pub(super) fn consume(&mut self, index: usize) -> Result<(), Diagnostic> {
        let consumed = self.consumed.get_mut(index).ok_or_else(|| {
            Diagnostic::blocking(
                "ASTRA_RUNTIME_EVENT_CONSUME_INDEX",
                "state machine selected an event index outside the immutable tick snapshot",
            )
        })?;
        if *consumed {
            return Err(Diagnostic::blocking(
                "ASTRA_RUNTIME_EVENT_CONSUME_DUPLICATE",
                "state machine attempted to consume the same event twice",
            ));
        }
        *consumed = true;
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct CompiledMachineDefinition {
    pub(super) transitions_by_state: BTreeMap<StableId, Vec<usize>>,
    pub(super) terminal_states: BTreeSet<StableId>,
    pub(super) event_kinds_by_transition: BTreeMap<usize, BTreeSet<String>>,
}

impl CompiledMachineDefinition {
    pub(super) fn compile(definition: &StateMachineDefinition) -> Self {
        let mut transitions_by_state: BTreeMap<StableId, Vec<usize>> = BTreeMap::new();
        let mut event_kinds_by_transition = BTreeMap::new();
        for (index, transition) in definition.transitions.iter().enumerate() {
            transitions_by_state
                .entry(transition.from)
                .or_default()
                .push(index);
            if let Some(kinds) = transition.guard.positive_event_kinds() {
                event_kinds_by_transition.insert(index, kinds);
            }
        }
        for indices in transitions_by_state.values_mut() {
            indices.sort_by_key(|index| {
                (
                    std::cmp::Reverse(definition.transitions[*index].priority),
                    *index,
                )
            });
        }
        let terminal_states = definition
            .states
            .iter()
            .filter_map(|state| state.terminal.then_some(state.id))
            .collect();
        Self {
            transitions_by_state,
            terminal_states,
            event_kinds_by_transition,
        }
    }

    pub(super) fn transitions_from(&self, state: StableId) -> impl Iterator<Item = usize> + '_ {
        self.transitions_by_state
            .get(&state)
            .into_iter()
            .flatten()
            .copied()
    }

    pub(super) fn is_terminal(&self, state: StableId) -> bool {
        self.terminal_states.contains(&state)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_machine(
    machine_index: usize,
    machine: &StateMachineInstance,
    step: u64,
    events: &[RuntimeEvent],
    actors: &ActorStore,
    blackboard: &Blackboard,
    actions: &ActionRegistry,
    id_source: &StableIdGenerator,
    evidence_mode: bool,
) -> MachineCandidate {
    let compiled = machine.compiled_definition();
    let mut candidate_machine = machine.clone();
    let mut candidate_actors = ActorStoreOverlay::new(actors);
    let mut candidate_blackboard = BlackboardOverlay::new(blackboard);
    let mut candidate_id_source = id_source.clone();
    let mut candidate_output = StateMachineTickOutput::default();
    let mut available_events = CandidateEvents::new(events);
    let mut microsteps = 0_u32;
    let mut failed = None;
    loop {
        if candidate_machine.completed {
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
            compiled,
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
                invocation.action_id.clone(),
                trigger_event.clone(),
                evidence_mode,
            );
            let action_result = action.run(&mut ctx, &invocation.input);
            let observed_access = evidence_mode.then(|| ctx.observed_access());
            drop(ctx);
            if let Some(observed_access) = observed_access {
                if let Err(diagnostic) = validate_observed_access(
                    &invocation.action_id,
                    &descriptor.access,
                    &observed_access,
                ) {
                    transition_failed = Some(diagnostic);
                    break;
                }
            }
            if stable_ids_used > descriptor.stable_id_reservation && evidence_mode {
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
            if let Err(diagnostic) = available_events.consume(index) {
                failed = Some(diagnostic);
                break;
            }
        }
        candidate_machine.current_state = transition.to;
        if compiled.is_terminal(candidate_machine.current_state) {
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

pub(super) fn find_transition(
    machine: &StateMachineInstance,
    compiled: &CompiledMachineDefinition,
    events: &CandidateEvents<'_>,
    actors: &dyn ActorStoreAccess,
    blackboard: &dyn BlackboardAccess,
) -> Option<(TransitionDefinition, Option<SourceRef>, Option<usize>)> {
    for transition_index in compiled.transitions_from(machine.current_state) {
        let transition = &machine.definition.transitions[transition_index];
        let trigger_event_index = match transition.guard {
            GuardExpr::Always => Some(None),
            _ if !transition.guard.depends_on_event() => transition
                .guard
                .evaluate(None, actors, blackboard)
                .then_some(None),
            _ => {
                let matching = if let Some(kinds) =
                    compiled.event_kinds_by_transition.get(&transition_index)
                {
                    events.iter_kinds(kinds).find(|(_, event)| {
                        transition.guard.evaluate(Some(event), actors, blackboard)
                    })
                } else {
                    events.iter().find(|(_, event)| {
                        transition.guard.evaluate(Some(event), actors, blackboard)
                    })
                };
                matching.map(|(index, _)| Some(index))
            }
        };
        if let Some(trigger_event_index) = trigger_event_index {
            return Some((
                transition.clone(),
                transition.source_ref.clone(),
                trigger_event_index,
            ));
        }
    }
    None
}

pub(super) fn guard_conflict_key(guard: &GuardExpr) -> String {
    match guard {
        GuardExpr::Always => "always".to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| format!("{other:?}")),
    }
}
