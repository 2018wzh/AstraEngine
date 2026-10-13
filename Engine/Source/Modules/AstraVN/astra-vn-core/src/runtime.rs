mod session_step;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::Hash128;

use crate::{
    resolve_target, AudioCue, BacklogEntry, BacklogLayoutMetadata, BranchOp, ChoiceOption,
    CompiledCommand, CompiledStory, MutationOp, PendingChoice, PresentationCommand, SkipMode,
    StageCommand, SystemUnlockKind, TimelineCommand, VnAudioBus, VnAudioSync, VnCallFrame,
    VnCommandCursor, VnCoverage, VnError, VnMovieEndBehavior, VnPlayerCommand, VnReplayUiState,
    VnRouteFlag, VnRouteFlagKind, VnRunConfig, VnRuntimeState, VnSaveBlob, VnStepOutput,
    VnSystemFrame, VnTimelineJoinPolicy, VnWaitKind, VnWaitState, VoiceReplayEntry,
    VN_RUNTIME_STATE_SCHEMA,
};

mod vn_runtime;
pub use vn_runtime::VnRuntime;
pub use vn_runtime::VnRuntimeIndex;
mod new;

mod load_slot;

mod cursor_for;
use cursor_for::compiled_command_id;
pub use cursor_for::reduce_vn_step;
pub use cursor_for::reduce_vn_step_indexed;
use cursor_for::validate_system_value;
use cursor_for::vn_audio_command;
use cursor_for::vn_timeline_task;
use cursor_for::wait_state_from_presentation;
pub use cursor_for::PendingVnStepOutput;
