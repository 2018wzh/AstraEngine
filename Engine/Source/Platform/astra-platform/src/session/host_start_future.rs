use super::*;

pub type HostStartFuture =
    Pin<Box<dyn Future<Output = Result<PlatformHostSession, PlatformError>> + 'static>>;

pub trait PlatformHostFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture;
}

#[derive(Debug, Clone, Copy)]
pub struct UnavailablePlatformFactory {
    pub(super) platform: crate::PlatformId,
}

impl UnavailablePlatformFactory {
    pub fn new(platform: crate::PlatformId) -> Self {
        Self { platform }
    }
}

impl PlatformHostFactory for UnavailablePlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        let platform = self.platform;
        Box::pin(async move {
            let profile = profile.require_platform()?;
            if profile.platform != platform {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidProfile,
                    "host.start",
                    "platform profile does not match the requested factory",
                )
                .with_field("platform", platform.as_str()));
            }
            Err(PlatformError::new(
                PlatformErrorCode::PlatformNotImplemented,
                "host.start",
                "platform host is not implemented in this release",
            )
            .with_field("platform", platform.as_str()))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRequest {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRequest {
    pub window: WindowHandle,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub clear_rgba: [u8; 4],
    pub commands: Vec<SceneCommand>,
    /// Backend-neutral accessibility tree synchronized with this visual frame.
    /// Platform mirrors must route actions back through `PlatformEventKind`.
    pub semantics: Option<astra_ui_core::UiSemanticSnapshot>,
}

/// One bounded scene submission. Native hosts may poll this receipt while
/// continuing simulation and audio scheduling; presentation failure remains
/// observable and is never discarded.
pub struct ScenePresentReceipt {
    pub(super) response: oneshot::Receiver<Result<(), PlatformError>>,
}

impl ScenePresentReceipt {
    pub fn try_complete(&mut self) -> Result<bool, PlatformError> {
        match self.response.try_recv() {
            Ok(result) => result.map(|()| true),
            Err(oneshot::error::TryRecvError::Empty) => Ok(false),
            Err(oneshot::error::TryRecvError::Closed) => Err(queue_closed("surface.present_scene")),
        }
    }

    pub async fn complete(self) -> Result<(), PlatformError> {
        self.response
            .await
            .map_err(|_| queue_closed("surface.present_scene"))?
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOutputRequest {
    pub sample_rate: u32,
    pub channels: u16,
    pub chunk_frames: usize,
    pub max_buffered_frames: usize,
    /// Starts the device paused until its bounded producer has been primed.
    pub start_paused: bool,
    /// Headless Evidence-only endpoint observation. Native outputs reject it.
    pub capture_samples: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioDeviceFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

pub type AudioOutputFormat = AudioDeviceFormat;

/// Process-local producer lane owned by the selected audio mixer. Implementations are bounded and
/// callback-driven; they must move whole chunks and never route refill through `HostCommand`.
pub trait AudioOutputLane: Send + 'static {
    fn wait_for_capacity(
        &mut self,
        requested_samples: usize,
        stop: &AtomicBool,
    ) -> Result<(), PlatformError>;

    /// Moves a filled chunk to the endpoint and returns one exhausted allocation for reuse.
    fn submit(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlatformError>;

    fn consumed_samples(&self) -> u64;

    fn underflow_count(&self) -> u64;
}

pub struct OpenedAudioOutput {
    pub handle: AudioOutputHandle,
    pub format: AudioDeviceFormat,
    pub lane: Box<dyn AudioOutputLane>,
    pub capture: Option<AudioCaptureReader>,
}

#[derive(Clone, Default)]
pub struct AudioCaptureReader {
    pub(super) samples: Arc<Mutex<Vec<f32>>>,
}

impl AudioCaptureReader {
    pub fn take_samples(&self) -> Result<Vec<f32>, PlatformError> {
        let mut samples = self.samples.lock().map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.capture",
                "audio endpoint capture lock is poisoned",
            )
        })?;
        Ok(std::mem::take(&mut *samples))
    }

    pub fn append(&self, samples: &[f32]) -> Result<(), PlatformError> {
        self.samples
            .lock()
            .map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "audio.capture",
                    "audio endpoint capture lock is poisoned",
                )
            })?
            .extend_from_slice(samples);
        Ok(())
    }
}

impl std::fmt::Debug for OpenedAudioOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenedAudioOutput")
            .field("handle", &self.handle)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl AudioOutputRequest {
    /// Returns a drain deadline that covers the submitted playback duration plus
    /// a fixed device/callback margin. Unlike a fixed timeout, this remains
    /// valid for long-form voice and music streams.
    pub fn drain_timeout(&self, submitted_samples: u64) -> Duration {
        const CALLBACK_MARGIN_MS: u128 = 2_000;
        let channels = u128::from(self.channels.max(1));
        let sample_rate = u128::from(self.sample_rate.max(1));
        let frames = u128::from(submitted_samples).div_ceil(channels);
        let playback_ms = frames.saturating_mul(1_000).div_ceil(sample_rate);
        let timeout_ms = playback_ms.saturating_add(CALLBACK_MARGIN_MS);
        Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(u64::MAX))
    }
}

#[derive(Clone, Default)]
pub struct AudioWakeRegistration {
    pub(super) state: Arc<(Mutex<u64>, Condvar)>,
}

impl AudioWakeRegistration {
    pub fn notify(&self) {
        let (sequence, wake) = &*self.state;
        if let Ok(mut sequence) = sequence.lock() {
            *sequence = sequence.saturating_add(1);
            wake.notify_one();
        }
    }

    pub fn wait(&self, observed: u64) -> u64 {
        let (sequence, wake) = &*self.state;
        let mut current = sequence.lock().expect("audio wake state is not poisoned");
        while *current <= observed {
            current = wake
                .wait(current)
                .expect("audio wake state is not poisoned");
        }
        *current
    }

    /// Waits for the next callback edge without introducing a periodic poll.
    /// `None` means the supplied deadline elapsed before a new edge arrived.
    pub fn wait_timeout(&self, observed: u64, timeout: std::time::Duration) -> Option<u64> {
        let (sequence, wake) = &*self.state;
        let mut current = sequence.lock().expect("audio wake state is not poisoned");
        let deadline = std::time::Instant::now() + timeout;
        while *current <= observed {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = wake
                .wait_timeout(current, remaining)
                .expect("audio wake state is not poisoned");
            current = next;
            if result.timed_out() && *current <= observed {
                return None;
            }
        }
        Some(*current)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioMeter {
    pub sample_count: u64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeStreamAction {
    OneShot,
    Start,
    Next,
}

/// Stable diagnostic attached to a stream `Next` error when the decoder has
/// reached end-of-stream.  It is intentionally separate from the human
/// message so workers do not have to pattern-match localized text.
pub const DECODE_STREAM_EOS_DIAGNOSTIC: &str = "ASTRA_MEDIA_STREAM_EOS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDecodeRequest {
    pub sequence: u64,
    pub kind: DecodeKind,
    pub codec: String,
    pub description: Vec<u8>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub coded_width: Option<u32>,
    pub coded_height: Option<u32>,
    pub keyframe: bool,
    pub stream_action: DecodeStreamAction,
    pub bytes: astra_byte_source::OwnedByteBuffer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodeOutput {
    CpuBuffer {
        format: String,
        bytes: astra_byte_source::OwnedByteBuffer,
    },
    AudioPcmI16 {
        sample_rate: u32,
        channels: u16,
        samples: Vec<i16>,
    },
    AudioPcmF32 {
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    },
    VideoStreamStart {
        /// Optional demux estimate, never a frame bound or completion clock.
        duration_estimate_us: Option<u64>,
        frame_count: Option<u64>,
        decoded_byte_count: Option<u64>,
    },
    VideoFrame {
        sequence: u64,
        pts_us: u64,
        duration_us: u64,
        width: u32,
        height: u32,
        bgra8: astra_byte_source::OwnedByteBuffer,
    },
    /// Successful EOS finalizes the end of the last delivered VideoFrame.
    VideoStreamEnd {
        frame_count: u64,
        decoded_byte_count: u64,
    },
    MediaFrame(MediaFrameHandle),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSourceRequest {
    Bundled {
        relative_path: String,
        expected_hash: String,
    },
    UserAuthorized {
        expected_hash: String,
    },
    HttpsRange {
        url: String,
        expected_hash: String,
    },
}
