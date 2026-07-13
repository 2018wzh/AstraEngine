use std::collections::VecDeque;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MEDIA_PLAYBACK_SCHEMA: &str = "astra.media_playback.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LateVideoPolicy {
    Block,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MediaPlaybackConfig {
    pub has_audio: bool,
    pub has_video: bool,
    pub duration_us: u64,
    pub max_video_frames: usize,
    pub max_audio_packets: usize,
    pub max_tick_us: u64,
    pub max_audio_clock_jump_us: u64,
    pub max_video_lead_us: u64,
    pub max_video_lag_us: u64,
    pub late_video_policy: LateVideoPolicy,
}

impl Default for MediaPlaybackConfig {
    fn default() -> Self {
        Self {
            has_audio: true,
            has_video: true,
            duration_us: 60_000_000,
            max_video_frames: 12,
            max_audio_packets: 64,
            max_tick_us: 100_000,
            max_audio_clock_jump_us: 250_000,
            max_video_lead_us: 20_000,
            max_video_lag_us: 100_000,
            late_video_policy: LateVideoPolicy::Block,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaPlaybackState {
    Ready,
    Playing,
    Paused,
    Seeking,
    Ended,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoFramePacket {
    pub generation: u64,
    pub sequence: u64,
    pub resource_id: String,
    pub pts_us: u64,
    pub duration_us: u64,
    pub width: u32,
    pub height: u32,
    pub content_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioFramePacket {
    pub generation: u64,
    pub sequence: u64,
    pub resource_id: String,
    pub pts_us: u64,
    pub duration_us: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_count: u32,
    pub content_hash: Hash256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaTrackKind {
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlaybackTickRequest {
    pub sequence: u64,
    pub delta_us: u64,
    pub audio_playhead_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlaybackTickOutput {
    pub sequence: u64,
    pub position_us: u64,
    pub presented_video: Option<VideoFramePacket>,
    pub dropped_video: Vec<VideoFramePacket>,
    pub released_audio: Vec<AudioFramePacket>,
    pub av_drift_us: Option<i64>,
    pub ended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MediaPlaybackSession {
    pub schema: String,
    pub config: MediaPlaybackConfig,
    pub state: MediaPlaybackState,
    pub generation: u64,
    pub position_us: u64,
    pub last_tick_sequence: u64,
    pub last_audio_playhead_us: Option<u64>,
    pub video_eos_us: Option<u64>,
    pub audio_eos_us: Option<u64>,
    pub video_queue: VecDeque<VideoFramePacket>,
    pub audio_queue: VecDeque<AudioFramePacket>,
    pub(super) next_video_sequence: u64,
    pub(super) next_audio_sequence: u64,
    pub(super) resume_after_seek: bool,
}
