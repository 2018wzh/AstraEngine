use astra_core::StableId;

use astra_runtime::{
    validate_state_machine, ActionInvocation, ActionRegistry, BlackboardValue, EventPayload,
    EventSource, GuardExpr, PresentationCommand, RuntimeConfig, RuntimeWorld, SetBlackboardAction,
    StateDefinition, StateMachineDefinition, StateMachineValidationReport, TickInput,
    TickIntegrityMode, TransitionDefinition,
};

use std::collections::BTreeMap;

#[path = "state_machine_tick/faults.rs"]
mod faults;
#[path = "state_machine_tick/transitions.rs"]
mod transitions;
use faults::set_blackboard_input;
