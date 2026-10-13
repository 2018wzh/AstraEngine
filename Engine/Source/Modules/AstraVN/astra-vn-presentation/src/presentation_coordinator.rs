mod fences;

pub use fences::FenceTaskGroup;

mod text_tasks;

pub use text_tasks::TextTaskProgress;

use std::collections::{BTreeMap, VecDeque};

use astra_core::Diagnostic;

use astra_worker_budget::WorkerBudgetBroker;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use crate::{MovieLoopMode, PresentationInterruptPolicy, VnError, VnMovieEndBehavior};

#[cfg(test)]
#[path = "coordinator_tick_tests.rs"]
mod tick_tests;

mod coordinator_state;
pub use coordinator_state::BackgroundPresentationState;
pub use coordinator_state::BackgroundRegionCommand;
pub use coordinator_state::BackgroundRegionState;
pub use coordinator_state::CharacterPresentationState;
pub use coordinator_state::CharacterRegionCommand;
pub use coordinator_state::CharacterRegionState;
pub use coordinator_state::FenceStatus;
pub use coordinator_state::PresentationCommandEnvelope;
pub use coordinator_state::PresentationCoordinator;
pub use coordinator_state::PresentationCoordinatorState;
pub use coordinator_state::PresentationRegion;
pub use coordinator_state::PresentationRegionCommand;
pub use coordinator_state::PresentationRegionDelta;
pub use coordinator_state::TextAdvanceDisposition;
pub use coordinator_state::TextPresentationState;
pub use coordinator_state::TextRegionCommand;
pub use coordinator_state::TextRegionState;
pub use coordinator_state::VideoPhase;
pub use coordinator_state::VideoPresentationState;
pub use coordinator_state::VideoRegionCommand;
pub use coordinator_state::VideoRegionState;
use coordinator_state::MAX_FRAME_DELTA_NS;
use coordinator_state::MAX_REGION_QUEUE;
pub use coordinator_state::PRESENTATION_COORDINATOR_SCHEMA;
mod is_failed;

mod validate_restored_state;
use validate_restored_state::coordinator_error;
use validate_restored_state::coordinator_error_owned;
use validate_restored_state::drain_background_queue;
use validate_restored_state::drain_character_queue;
use validate_restored_state::drain_text_queue;
use validate_restored_state::drain_video_queue;
use validate_restored_state::prepare_background;
use validate_restored_state::prepare_character;
use validate_restored_state::prepare_text;
use validate_restored_state::prepare_video;
use validate_restored_state::push_activated;
use validate_restored_state::validate_batch;
use validate_restored_state::PreparedRegion;
