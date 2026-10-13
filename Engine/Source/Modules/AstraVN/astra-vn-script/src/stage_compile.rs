mod timeline;

pub use timeline::compile_timeline_command;

use std::collections::{BTreeMap, BTreeSet};

use astra_core::{is_safe_path_symbol as safe_symbol, Diagnostic};

use crate::{
    lower::ParsedLine, AspectRatio, AudioControl, AudioCue, ExtensionCommandDescriptor,
    ExtensionFieldKind, ExtensionPresentationCommand, ExtensionValue, FixedScalar, MovieLoopMode,
    PresentationInterruptPolicy, StageBlendMode, StageClipPolicy, StageCommand, StageFitMode,
    StageLayerKind, StagePlacement, StageViewport, TimelineCommand, TimelineSpec, VnAudioBus,
    VnAudioControlAction, VnAudioSync, VnError, VnMovieEndBehavior, VnPresentationEasing,
    VnTimelineJoinPolicy, VnTimelineKeyframe, VnTimelineTrack,
};

mod compile_stage_command;
pub(crate) use compile_stage_command::compile_extension_command;
pub(crate) use compile_stage_command::compile_stage_command;
mod validate_attrs;
use validate_attrs::asset_uri;
use validate_attrs::fixed;
use validate_attrs::invalid_value;
use validate_attrs::milliseconds_to_microseconds;
use validate_attrs::missing_attr;
use validate_attrs::opacity;
use validate_attrs::optional_bool;
use validate_attrs::optional_fixed;
use validate_attrs::optional_symbol;
use validate_attrs::optional_u32;
use validate_attrs::parse_aspect;
use validate_attrs::parse_blend;
use validate_attrs::parse_bool;
use validate_attrs::parse_clip;
use validate_attrs::parse_fixed;
use validate_attrs::parse_i32;
use validate_attrs::parse_interrupt;
use validate_attrs::parse_layer_kind;
use validate_attrs::parse_rgba;
use validate_attrs::parse_u32;
use validate_attrs::parse_viewport;
use validate_attrs::required;
use validate_attrs::safe_schema;
use validate_attrs::symbol;
use validate_attrs::unknown_attr;
use validate_attrs::validate_asset_uri;
use validate_attrs::validate_attrs;
use validate_attrs::validate_symbol;
