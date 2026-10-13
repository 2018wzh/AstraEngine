use super::*;

#[derive(Default)]
pub struct StateMachineTickOutput {
    pub events: Vec<RuntimeEvent>,
    pub presentation: Vec<PresentationCommand>,
    pub awaits: Vec<AwaitToken>,
    pub delayed_events: Vec<ScheduledEvent>,
    pub delayed_cancellations: Vec<DelayedEventId>,
    pub trace: Vec<ActionTrace>,
    pub mutations: Vec<crate::RuntimeMutationRecord>,
    pub diagnostics: Vec<Diagnostic>,
}

impl StateMachineTickOutput {
    pub(super) fn append(&mut self, mut other: Self) {
        self.events.append(&mut other.events);
        self.presentation.append(&mut other.presentation);
        self.awaits.append(&mut other.awaits);
        self.delayed_events.append(&mut other.delayed_events);
        self.delayed_cancellations
            .append(&mut other.delayed_cancellations);
        self.trace.append(&mut other.trace);
        self.mutations.append(&mut other.mutations);
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
    pub(super) fn evaluate(
        &self,
        event: Option<&RuntimeEvent>,
        actors: &dyn ActorStoreAccess,
        blackboard: &dyn BlackboardAccess,
    ) -> bool {
        match self {
            GuardExpr::Always => true,
            GuardExpr::EventIs { kind } => event.is_some_and(|event| event.payload.kind == *kind),
            GuardExpr::BlackboardEquals { key, value } => blackboard.get(key) == Some(value),
            GuardExpr::HasActorTag { actor, tag } => actors.actor_has_tag(*actor, tag),
            GuardExpr::And { terms } => terms
                .iter()
                .all(|term| term.evaluate(event, actors, blackboard)),
            GuardExpr::Or { terms } => terms
                .iter()
                .any(|term| term.evaluate(event, actors, blackboard)),
            GuardExpr::Not { term } => !term.evaluate(event, actors, blackboard),
        }
    }

    pub(super) fn depends_on_event(&self) -> bool {
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

    pub(super) fn positive_event_kinds(&self) -> Option<BTreeSet<String>> {
        match self {
            GuardExpr::EventIs { kind } => Some(BTreeSet::from([kind.clone()])),
            GuardExpr::And { terms } => {
                let mut event_kinds: Option<BTreeSet<String>> = None;
                for term in terms {
                    if !term.depends_on_event() {
                        continue;
                    }
                    let term_kinds = term.positive_event_kinds()?;
                    event_kinds = Some(match event_kinds {
                        Some(current) => current.intersection(&term_kinds).cloned().collect(),
                        None => term_kinds,
                    });
                }
                event_kinds
            }
            GuardExpr::Or { terms } => {
                let mut event_kinds = BTreeSet::new();
                for term in terms {
                    if !term.depends_on_event() {
                        return None;
                    }
                    event_kinds.extend(term.positive_event_kinds()?);
                }
                Some(event_kinds)
            }
            GuardExpr::Not { .. } => None,
            GuardExpr::Always
            | GuardExpr::BlackboardEquals { .. }
            | GuardExpr::HasActorTag { .. } => None,
        }
    }
}
