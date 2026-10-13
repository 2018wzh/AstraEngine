mod clock;

mod restore;

mod video_open;

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Instant,
};

use astra_media::{PcmAsset, CANONICAL_CHANNELS, CANONICAL_SAMPLE_RATE};

use astra_platform::{PlatformError, PlatformErrorCode};

use astra_player_core::{
    PlatformCommandSink, PlayerDecodedAudio, PlayerHostCommandExecutor, PlayerTimelineCompletion,
    PlayerTimelineScheduler, PlayerTimelineSchedulerSnapshot,
};

use crate::{
    NativeVnAudioOutput, NativeVnAudioPreloadRequest, NativeVnDecodedCacheBudget,
    NativeVnHostCommandSource, NativeVnProductAudioHost, NativeVnVideoRequest,
    DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES,
};

#[cfg(test)]
mod tests;

mod native_vn_product_media_host;
use native_vn_product_media_host::ActiveVideoStream;
use native_vn_product_media_host::AudioCacheKey;
use native_vn_product_media_host::CachedDecodedAudio;
pub use native_vn_product_media_host::NativeVnMediaPerformanceSample;
pub use native_vn_product_media_host::NativeVnProductMediaHost;
pub use native_vn_product_media_host::NativeVnProductMediaSnapshot;
pub use native_vn_product_media_host::NativeVnVideoStreamSnapshot;
mod media_state;

mod fetch_video_frame;

mod compact_audio_recency_if_needed;
use compact_audio_recency_if_needed::decoded_bgra_frame;
use compact_audio_recency_if_needed::media_error;
