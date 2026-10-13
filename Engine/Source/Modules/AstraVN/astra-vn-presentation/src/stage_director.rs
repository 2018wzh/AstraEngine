use std::collections::{BTreeMap, BTreeSet, VecDeque};

use astra_core::Diagnostic;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use crate::{
    AspectRatio, AudioControl, AudioCue, BackgroundRegionCommand, CharacterRegionCommand,
    FixedScalar, MovieLoopMode, PresentationCommandEnvelope, PresentationCoordinator,
    PresentationInterruptPolicy, PresentationRegionCommand, StageBlendMode, StageClipPolicy,
    StageCommand, StageFitMode, StageLayerKind, StagePlacement, StageViewport,
    TextAdvanceDisposition, TextRegionCommand, TimelineCommand, TimelineSpec, VideoRegionCommand,
    VnAudioBus, VnError, VnMovieEndBehavior, VnPresentationEasing, VnPresentationProviderManifest,
    VnTextRevealState, VnTimelineJoinPolicy,
};

#[cfg(test)]
#[path = "stage_tick_tests.rs"]
mod tick_tests;

mod stage_state;
pub use stage_state::ProductStageCamera;
pub use stage_state::ProductStageDirector;
pub use stage_state::ProductStageEffect;
pub use stage_state::ProductStageEntity;
pub use stage_state::ProductStageLayer;
pub use stage_state::ProductStageMovie;
pub use stage_state::ProductStageState;
pub use stage_state::ProductStageTransition;
pub use stage_state::StageDirectorOutput;
use stage_state::MAX_FRAME_DELTA_NS;
pub use stage_state::PRODUCT_STAGE_STATE_SCHEMA;
mod new;

mod region_envelope;

mod apply_inner;

mod apply_timeline;

mod prepare_interrupt;
use prepare_interrupt::canonical_timeline_target;
use prepare_interrupt::duration_ns;
use prepare_interrupt::eased_progress;
use prepare_interrupt::fixed_mul;
use prepare_interrupt::placement_x;
use prepare_interrupt::sample_track;
use prepare_interrupt::timeline_duration_ms;
use prepare_interrupt::timeline_properties;
use prepare_interrupt::timeline_target_property;
use prepare_interrupt::validate_viewport;
use prepare_interrupt::ActiveShake;
use prepare_interrupt::ActiveTimeline;
use prepare_interrupt::StageTween;
use prepare_interrupt::TweenProperty;
use prepare_interrupt::TweenTarget;
mod fixed_lerp;
use fixed_lerp::fixed_lerp;
use fixed_lerp::stage_error;
