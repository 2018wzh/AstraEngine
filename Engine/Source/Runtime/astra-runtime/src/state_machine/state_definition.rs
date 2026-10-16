use super::*;

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineInstance {
    pub definition: Arc<StateMachineDefinition>,
    pub current_state: StableId,
    pub completed: bool,
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) compiled: Arc<OnceLock<CompiledMachineDefinition>>,
}

impl PartialEq for StateMachineInstance {
    fn eq(&self, other: &Self) -> bool {
        self.definition == other.definition
            && self.current_state == other.current_state
            && self.completed == other.completed
    }
}

impl StateMachineInstance {
    pub fn new(definition: StateMachineDefinition) -> Self {
        let compiled = CompiledMachineDefinition::compile(&definition);
        let completed = definition
            .states
            .iter()
            .any(|state| state.id == definition.initial_state && state.terminal);
        Self {
            current_state: definition.initial_state,
            definition: Arc::new(definition),
            completed,
            compiled: Arc::new(OnceLock::from(compiled)),
        }
    }

    pub(super) fn compiled_definition(&self) -> &CompiledMachineDefinition {
        self.compiled
            .get_or_init(|| CompiledMachineDefinition::compile(&self.definition))
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineStore {
    pub(super) machines: Vec<StateMachineInstance>,
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) trace: VecDeque<ActionTrace>,
    #[serde(skip)]
    #[schemars(skip)]
    worker_pool: Option<Arc<rayon::ThreadPool>>,
    #[serde(skip)]
    #[schemars(skip)]
    access_plans: Vec<MachineAccessPlan>,
    #[serde(skip)]
    #[schemars(skip)]
    action_epoch: Option<Arc<()>>,
}

impl PartialEq for StateMachineStore {
    fn eq(&self, other: &Self) -> bool {
        self.machines == other.machines && self.trace == other.trace
    }
}

impl StateMachineStore {
    fn ensure_worker_pool(&mut self, workers: usize) -> Result<(), Diagnostic> {
        if !(1..=8).contains(&workers) {
            return Err(Diagnostic::blocking(
                "ASTRA_RUNTIME_WORKER_COUNT",
                "machine worker count must be within 1..=8",
            ));
        }
        if self
            .worker_pool
            .as_ref()
            .is_some_and(|pool| pool.current_num_threads() == workers)
        {
            return Ok(());
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|index| format!("astra-fsm-{index}"))
            .build()
            .map_err(|error| {
                Diagnostic::blocking("ASTRA_RUNTIME_WORKER_POOL", error.to_string())
            })?;
        self.worker_pool = Some(Arc::new(pool));
        Ok(())
    }

    fn refresh_access_plans(&mut self, actions: &ActionRegistry) {
        if self.access_plans.len() == self.machines.len()
            && self
                .action_epoch
                .as_ref()
                .is_some_and(|epoch| Arc::ptr_eq(epoch, actions.epoch()))
        {
            return;
        }
        self.access_plans = self
            .machines
            .iter()
            .map(|machine| machine_access_plan(machine, actions))
            .collect();
        self.action_epoch = Some(Arc::clone(actions.epoch()));
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
        self.access_plans.clear();
        self.action_epoch = None;
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
        evidence_mode: bool,
    ) -> StateMachineTickOutput {
        let mut output = StateMachineTickOutput::default();
        self.refresh_access_plans(actions);
        let waves = build_conflict_waves(&self.machines, &self.access_plans);
        if worker_count > 1 && waves.iter().any(|wave| wave.len() > 1) {
            if let Err(diagnostic) = self.ensure_worker_pool(worker_count) {
                output.diagnostics.push(diagnostic);
                return output;
            }
        }
        for wave in waves {
            let base_id_source = id_source.clone();
            let candidates = if worker_count > 1 && wave.len() > 1 {
                execute_parallel_wave(
                    &wave,
                    self.worker_pool
                        .as_ref()
                        .expect("parallel pool was prepared"),
                    &self.machines,
                    step,
                    events,
                    actors,
                    blackboard,
                    actions,
                    &base_id_source,
                    evidence_mode,
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
                            evidence_mode,
                        )
                    })
                    .collect()
            };
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
                crate::diagnostic_history::extend(
                    &mut self.trace,
                    candidate.output.trace.iter().cloned(),
                );
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

    pub fn trace(&self) -> &VecDeque<ActionTrace> {
        &self.trace
    }

    pub(crate) fn snapshot(&self) -> Self {
        Self {
            machines: self.machines.clone(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct MachineAccessPlan {
    pub(super) serial: bool,
    pub(super) reads: BTreeSet<ActionResourceKey>,
    pub(super) writes: BTreeSet<ActionResourceKey>,
}

pub(super) struct MachineCandidate {
    pub(super) machine_index: usize,
    pub(super) machine: StateMachineInstance,
    pub(super) actor_delta: ActorStoreDelta,
    pub(super) blackboard_delta: BlackboardDelta,
    pub(super) id_source: StableIdGenerator,
    pub(super) output: StateMachineTickOutput,
    pub(super) microsteps: u32,
    pub(super) failed: Option<Diagnostic>,
}

pub(super) fn build_conflict_waves(
    machines: &[StateMachineInstance],
    plans: &[MachineAccessPlan],
) -> Vec<Vec<usize>> {
    let mut waves = Vec::new();
    let mut current = Vec::new();
    let mut current_plans: Vec<&MachineAccessPlan> = Vec::new();
    for (machine_index, machine) in machines.iter().enumerate() {
        if machine.completed {
            continue;
        }
        let plan = &plans[machine_index];
        let conflicts = plan.serial
            || current_plans
                .iter()
                .any(|current_plan| access_conflicts(plan, current_plan));
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

pub(super) fn machine_access_plan(
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
        plan.reads.extend(descriptor.access.reads.iter().cloned());
        plan.writes.extend(descriptor.access.writes.iter().cloned());
    }
    plan
}

pub(super) fn access_conflicts(left: &MachineAccessPlan, right: &MachineAccessPlan) -> bool {
    left.serial
        || right.serial
        || resource_sets_conflict(&left.writes, &right.writes)
        || resource_sets_conflict(&left.writes, &right.reads)
        || resource_sets_conflict(&left.reads, &right.writes)
}

pub(super) fn resource_sets_conflict(
    left: &BTreeSet<ActionResourceKey>,
    right: &BTreeSet<ActionResourceKey>,
) -> bool {
    left.iter()
        .any(|left| right.iter().any(|right| resource_keys_overlap(left, right)))
}

pub(super) fn resource_keys_overlap(left: &ActionResourceKey, right: &ActionResourceKey) -> bool {
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

pub(super) fn resource_is_declared(
    declared: &BTreeSet<ActionResourceKey>,
    observed: &ActionResourceKey,
) -> bool {
    declared
        .iter()
        .any(|resource| resource_keys_overlap(resource, observed))
}
