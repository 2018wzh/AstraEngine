mod tasks;

use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};

use astra_core::{
    Diagnostic, SchemaId, SchemaMigrationRegistry, SchemaVersion, StableId, StableIdGenerator,
};

use schemars::JsonSchema;

use serde::de::DeserializeOwned;

use serde::{Deserialize, Serialize};

use thiserror::Error;

use tracing::{debug, info, trace, warn};

use crate::{
    ActionRegistry, ActorId, ActorRecord, ActorSnapshot, ActorStore, AwaitCompletion,
    AwaitCompletionHandle, AwaitQueue, AwaitToken, Blackboard, BlackboardValue, ComponentId,
    ComponentRecord, ComponentSnapshot, CreateAwaitAction, DelayedEventId, DelayedEventQueue,
    EmitEventAction, EventId, EventPayload, EventQueue, EventSource, PresentationAction,
    PresentationCommand, PresentationRecord, RuntimeAction, RuntimeComponentPayload, RuntimeEvent,
    RuntimeMutationRecord, SaveBlob, SaveRequest, ScheduledEvent, SetBlackboardAction,
    StateMachineDefinition, StateMachineSnapshot, StateMachineStore, TaskScope,
};

mod runtime_error;
pub use runtime_error::EngineModuleSlot;
pub use runtime_error::ModuleBindingContext;
pub use runtime_error::ModuleBindingSnapshot;
pub use runtime_error::OrderedTickIngress;
pub use runtime_error::PackageHandle;
pub use runtime_error::PlayerInput;
pub use runtime_error::RuntimeConfig;
pub use runtime_error::RuntimeError;
pub use runtime_error::RuntimeSnapshot;
pub use runtime_error::RuntimeWorld;
pub use runtime_error::TickIngress;
pub use runtime_error::TickInput;
pub use runtime_error::TickIntegrityMode;
pub use runtime_error::TickMode;
pub use runtime_error::TickReport;
pub use runtime_error::TickRequest;
pub use runtime_error::ValidatedModuleBinding;
mod create;

mod validate_tick_request;
use validate_tick_request::presentation_kind;
pub use validate_tick_request::LoadReport;
pub use validate_tick_request::RuntimeDebugSession;
