use std::collections::{BTreeMap, BTreeSet};

use astra_core::{Diagnostic, Hash128, Hash256, SourceRef};

use astra_ui_core::{UiBindingManifest, UiBlueprintBundle, UiThemeManifest};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use crate::CommandSourceMap;

use crate::VnError;

mod runtime_state;
pub use runtime_state::AstraSource;
pub use runtime_state::AstraSourceRole;
pub use runtime_state::BranchOp;
pub use runtime_state::ChoiceOption;
pub use runtime_state::CommandManifest;
pub use runtime_state::CommandManifestEntry;
pub use runtime_state::CompiledCommand;
pub use runtime_state::CompiledStory;
pub use runtime_state::CompiledVnProject;
pub use runtime_state::MutationOp;
pub use runtime_state::ReadingMode;
pub use runtime_state::SaveCompletionPolicy;
pub use runtime_state::Scene;
pub use runtime_state::State;
pub use runtime_state::Story;
pub use runtime_state::StoryManifest;
pub use runtime_state::StoryManifestEntry;
pub use runtime_state::SystemActionEffect;
pub use runtime_state::SystemActionProgram;
pub use runtime_state::SystemPageKind;
pub use runtime_state::SystemStoryEntry;
pub use runtime_state::SystemStoryManifest;
pub use runtime_state::SystemStoryValidationReport;
pub use runtime_state::SystemStoryValidationStatus;
pub use runtime_state::SystemUiProfilePolicy;
pub use runtime_state::VariableCondition;
pub use runtime_state::VariableManifest;
pub use runtime_state::VariableScopeManifest;
pub use runtime_state::VN_RUNTIME_STATE_SCHEMA;
pub use runtime_state::VN_RUNTIME_STATE_SCHEMA_MAJOR;
mod system_story_manifest;
pub use system_story_manifest::BacklogEntry;
pub use system_story_manifest::BacklogLayoutMetadata;
pub use system_story_manifest::PendingChoice;
pub use system_story_manifest::PresentationCommand;
pub use system_story_manifest::RouteEdge;
pub use system_story_manifest::RouteGraph;
pub use system_story_manifest::RouteNode;
pub use system_story_manifest::SkipMode;
pub use system_story_manifest::SystemUnlockKind;
pub use system_story_manifest::VnCallFrame;
pub use system_story_manifest::VnCommandCursor;
pub use system_story_manifest::VnCoverage;
pub use system_story_manifest::VnReplayUiState;
pub use system_story_manifest::VnRouteFlag;
pub use system_story_manifest::VnRouteFlagKind;
pub use system_story_manifest::VnRunConfig;
pub use system_story_manifest::VnRuntimeState;
pub use system_story_manifest::VnStepOutput;
pub use system_story_manifest::VnSystemFrame;
pub use system_story_manifest::VnSystemState;
pub use system_story_manifest::VnTextRevealState;
pub use system_story_manifest::VnWaitKind;
pub use system_story_manifest::VnWaitState;
pub use system_story_manifest::VoiceReplayEntry;
mod vn_event;
pub use vn_event::VnAudioCommand;
pub use vn_event::VnEvent;
pub use vn_event::VnMutationRecord;
pub use vn_event::VnPlayerCommand;
pub use vn_event::VnProfileManifest;
pub use vn_event::VnSaveBlob;
pub use vn_event::VnTimelineTask;
