use std::{collections::BTreeSet, future::Future, pin::Pin};

use astra_core::{Diagnostic, Hash256};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

mod platform_sink;

pub use astra_media::{PlayerAudioContractError, PlayerDecodedAudio};

pub use platform_sink::*;

mod media_lifecycle;

pub use media_lifecycle::*;

mod timeline;

pub use timeline::*;

mod host_commands;
pub use host_commands::PlayerAction;
pub use host_commands::PlayerActionBinding;
pub use host_commands::PlayerActionMap;
pub use host_commands::PlayerDecodeKind;
pub use host_commands::PlayerDecodeStreamAction;
pub use host_commands::PlayerHostCommand;
pub use host_commands::PlayerHostCommandBatch;
pub use host_commands::PlayerHostCommandError;
pub use host_commands::PlayerHostCommandExecutionError;
pub use host_commands::PlayerHostCommandExecutor;
pub use host_commands::PlayerHostCommandResult;
pub use host_commands::PlayerHostCommandSink;
pub use host_commands::PlayerHostResourceId;
pub use host_commands::PlayerPackageSource;
pub use host_commands::PlayerSaveTransactionError;
pub use host_commands::PlayerSaveTransactionPlan;
pub use host_commands::WEB_PLAYER_LIVE_EVIDENCE_SCHEMA;
mod player_automation_script;
pub use player_automation_script::evidence_scene_command_hash;
pub use player_automation_script::PlayerAudioMeterEvidence;
pub use player_automation_script::PlayerAutomationCheck;
pub use player_automation_script::PlayerAutomationEvidence;
pub use player_automation_script::PlayerAutomationReport;
pub use player_automation_script::PlayerAutomationScript;
pub use player_automation_script::PlayerAutomationStatus;
pub use player_automation_script::PlayerAutomationStep;
pub use player_automation_script::PlayerAutomationValidator;
pub use player_automation_script::PlayerInputConsumptionEvidence;
pub use player_automation_script::PlayerInputEvent;
pub use player_automation_script::PlayerInputTranscript;
pub use player_automation_script::PlayerPlatform;
pub use player_automation_script::PlayerPlatformEvidenceIdentity;
pub use player_automation_script::PlayerPresentationError;
pub use player_automation_script::PlayerPresentationReport;
pub use player_automation_script::PlayerPresentationRunIdentity;
pub use player_automation_script::PlayerRuntimeRouteEvidence;
pub use player_automation_script::PlayerTranscriptHashError;
pub use player_automation_script::PlayerVisualComparisonEvidence;
pub use player_automation_script::PlayerVisualRegionEvidence;
pub use player_automation_script::PLAYER_PRESENTATION_REPORT_SCHEMA;
mod player_automation_validator;
mod visual_region_check;
use visual_region_check::audio_meter_check;
use visual_region_check::blocked_check;
use visual_region_check::blocked_check_with_evidence;
use visual_region_check::evidence;
use visual_region_check::is_consumption_trace_source;
use visual_region_check::is_forbidden_input_source;
use visual_region_check::is_live_input_source;
use visual_region_check::is_safe_relative_ref;
use visual_region_check::pass_check;
use visual_region_check::route_coverage_check;
use visual_region_check::visual_comparison_check;
use visual_region_check::visual_region_check;
