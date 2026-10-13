use super::*;

pub struct NativeVnProductMediaHost {
    pub(super) audio: NativeVnProductAudioHost,
    pub(super) timeline: PlayerTimelineScheduler,
    pub(super) clock: clock::PlaybackClock,
    pub(super) completed_signals: BTreeSet<String>,
    pub(super) active_videos: Vec<ActiveVideoStream>,
    pub(super) pending_video_closes: Vec<astra_player_core::PlayerHostResourceId>,
    pub(super) restored_videos: Vec<NativeVnVideoStreamSnapshot>,
    pub(super) decoded_audio_cache: BTreeMap<AudioCacheKey, CachedDecodedAudio>,
    pub(super) decoded_audio_lru: VecDeque<(AudioCacheKey, u64)>,
    pub(super) decoded_audio_access_epoch: u64,
    pub(super) decoded_audio_cache_bytes: u64,
    pub(super) max_video_frames: u64,
    pub(super) max_decode_output_bytes: u64,
    pub(super) max_decoded_cache_bytes: u64,
    pub(super) performance: Option<NativeVnMediaPerformanceSample>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeVnMediaPerformanceSample {
    pub prewarm_ns: u64,
    pub provider_decode_ns: u64,
    pub parse_convert_ns: u64,
    pub mixer_ns: u64,
    pub audio_query_ns: u64,
    pub audio_render_ns: u64,
    pub audio_submit_ns: u64,
    pub audio_completion_ns: u64,
}

#[derive(Clone)]
pub(super) struct CachedDecodedAudio {
    pub(super) asset: PcmAsset,
    pub(super) byte_size: u64,
    pub(super) last_access_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct AudioCacheKey {
    pub(super) asset_id: String,
    pub(super) codec: String,
    pub(super) encoded_length: u64,
    pub(super) target_sample_rate: u32,
    pub(super) target_channels: u16,
}

impl AudioCacheKey {
    pub(super) fn new(asset_id: &str, codec: &str, encoded_length: u64) -> Self {
        Self {
            asset_id: asset_id.to_string(),
            codec: codec.to_string(),
            encoded_length,
            target_sample_rate: CANONICAL_SAMPLE_RATE,
            target_channels: CANONICAL_CHANNELS,
        }
    }
}

pub(super) struct ActiveVideoStream {
    pub(super) request: NativeVnVideoRequest,
    pub(super) session: astra_player_core::PlayerHostResourceId,
    pub(super) decoded_end_us: u64,
    pub(super) presented_end_us: u64,
    pub(super) loop_duration_us: Option<u64>,
    pub(super) expected_frame_count: Option<u64>,
    pub(super) expected_decoded_byte_count: Option<u64>,
    pub(super) decoded_byte_count: u64,
    pub(super) pending_frame: Option<astra_media::DecodedVideoFrame>,
    pub(super) next_frame: u64,
    pub(super) next_request_sequence: u64,
    pub(super) reached_end: bool,
    pub(super) loop_index: u64,
    pub(super) started_at_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NativeVnProductMediaSnapshot {
    pub schema: String,
    pub playback_time_ms: u64,
    pub audio: crate::NativeVnProductAudioSnapshot,
    pub timeline: PlayerTimelineSchedulerSnapshot,
    pub completed_signals: Vec<String>,
    #[serde(default)]
    pub active_videos: Vec<NativeVnVideoStreamSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NativeVnVideoStreamSnapshot {
    pub layer: String,
    pub asset_id: String,
    pub encoded_length: u64,
    pub alpha_millionths: i64,
    #[serde(default)]
    pub looping: bool,
    pub fence: Option<String>,
    pub frame_end_us: u64,
    pub loop_duration_us: Option<u64>,
    pub decoded_byte_count: u64,
    pub next_frame: u64,
    #[serde(default)]
    pub loop_index: u64,
    pub started_at_ms: u64,
}

impl Default for NativeVnProductMediaHost {
    fn default() -> Self {
        Self::new(256)
    }
}
