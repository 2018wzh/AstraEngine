use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioServiceConfig {
    pub max_voices: usize,
    pub max_buses: usize,
    pub max_events: usize,
    pub pcm_cache_bytes: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioServiceError {
    #[error("audio service configuration is invalid")]
    InvalidConfig,
    #[error("audio service command is invalid: {0}")]
    InvalidCommand(&'static str),
    #[error("audio PCM cache budget exceeded; required={required_bytes}, budget={budget_bytes}")]
    CacheBudget {
        required_bytes: usize,
        budget_bytes: usize,
    },
    #[error("audio service session is poisoned")]
    Poisoned,
    #[error("Kira audio manager failed: {0}")]
    Kira(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DevicePcmKey {
    pub(super) asset: AudioAssetRevision,
    pub(super) source_sample_rate: u32,
    pub(super) source_channels: u16,
    pub(super) target_sample_rate: u32,
    pub(super) target_channels: u16,
}

pub(super) struct CachedPcm {
    pub(super) samples: Arc<Vec<f32>>,
    pub(super) bytes: usize,
    pub(super) last_used: u64,
}

pub(super) struct ActiveVoice {
    pub(super) handle: AstraPcmSoundHandle,
    pub(super) command_sequence: u64,
    pub(super) completion_target_samples: Option<u64>,
}

pub(super) struct ActiveStream {
    pub(super) handle: AstraStreamSoundHandle,
    pub(super) bus: String,
}

pub(super) struct AudioBus {
    pub(super) track: TrackHandle,
    pub(super) panning: PanningControlHandle,
}

pub struct AudioServiceSession {
    pub(super) manager: AudioManager<AstraChunkBackend>,
    pub(super) timeline: AudioTimelineStateV1,
    pub(super) events: VecDeque<AudioServiceEvent>,
    pub(super) buses: BTreeMap<String, AudioBus>,
    pub(super) voices: BTreeMap<String, ActiveVoice>,
    pub(super) streams: BTreeMap<u64, ActiveStream>,
    pub(super) retiring_streams: Vec<AstraStreamSoundHandle>,
    pub(super) pcm_cache: BTreeMap<DevicePcmKey, CachedPcm>,
    pub(super) pcm_cache_bytes: usize,
    pub(super) lru_sequence: u64,
    pub(super) last_consumed_frames: u64,
    pub(super) config: AudioServiceConfig,
    pub(super) poisoned: bool,
}
