use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

use astra_platform::{
    host_channel, AudioDeviceFormat, AudioOutputHandle, AudioOutputLane, DecodeKind, DecodeOutput,
    DecodeSessionHandle, HostCommand, OpenedAudioOutput, PlatformError, PlatformHostProfile,
    SurfaceHandle,
};

use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerHostResourceId,
};

use astra_player_vn::{
    NativeVnAudioControlRequest, NativeVnAudioRequest, NativeVnHostCommandSource,
    NativeVnProductAudioHost, NativeVnProductMediaHost,
};

mod support;

#[path = "product_audio_host/fixtures.rs"]
mod fixtures;
use fixtures::assert_endpoint_consumed_is_monotonic;
use fixtures::source;
use fixtures::ProductAudioTestLane;
#[path = "product_audio_host/video_restore.rs"]
mod video_restore;
