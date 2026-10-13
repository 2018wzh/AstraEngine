mod restore;

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use kira::{
    effect::panning_control::{PanningControlBuilder, PanningControlHandle},
    track::{MainTrackBuilder, TrackBuilder, TrackHandle},
    AudioManager, AudioManagerSettings, Capacities, Decibels, Panning, Tween,
};

use crate::{
    AstraChunkBackend, AstraChunkBackendSettings, AstraPcmSoundData, AstraPcmSoundHandle,
    AstraStreamSoundData, AstraStreamSoundHandle, AudioAssetRevision, AudioBusState,
    AudioServiceCommand, AudioServiceEvent, AudioTimelineStateV1, AudioVoiceState,
};

mod audio_service_config;
use audio_service_config::ActiveStream;
use audio_service_config::ActiveVoice;
use audio_service_config::AudioBus;
pub use audio_service_config::AudioServiceConfig;
pub use audio_service_config::AudioServiceError;
pub use audio_service_config::AudioServiceSession;
use audio_service_config::CachedPcm;
use audio_service_config::DevicePcmKey;
mod new;

mod stream_is_completed;
use stream_is_completed::frames_to_duration;
use stream_is_completed::interpolated_fade_gain;
use stream_is_completed::linear_gain_to_decibels;
use stream_is_completed::validate_command;
