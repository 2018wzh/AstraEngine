use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::{mpsc, oneshot};

use astra_media_core::SceneCommand;

use crate::{
    AudioOutputHandle, DecodeSessionHandle, HeadlessHostProfile, HostLaunchProfile,
    MediaFrameHandle, PackageSourceHandle, PackageSourcePolicy, PlatformError, PlatformErrorCode,
    PlatformHostProfile, SaveTransactionHandle, SurfaceHandle, WindowHandle,
};

pub type HostStartFuture =
    Pin<Box<dyn Future<Output = Result<PlatformHostSession, PlatformError>> + 'static>>;

pub trait PlatformHostFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture;
}

#[derive(Debug, Clone, Copy)]
pub struct UnavailablePlatformFactory {
    platform: crate::PlatformId,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOutputRequest {
    pub sample_rate: u32,
    pub channels: u16,
    pub max_buffered_frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioDeviceFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

pub type AudioOutputFormat = AudioDeviceFormat;

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

#[derive(Debug, Clone, PartialEq)]
pub struct AudioPacket {
    pub sequence: u64,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl AudioPacket {
    pub fn frame_count(&self) -> usize {
        self.samples
            .len()
            .checked_div(usize::from(self.channels))
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioMeter {
    pub sample_count: u64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioOutputState {
    pub queued_frames: usize,
    pub callback_count: u64,
    pub submitted_samples: u64,
    pub consumed_samples: u64,
    pub underflow_count: u64,
    pub meter: AudioMeter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioOutputStatus {
    pub submitted_frames: u64,
    pub played_frames: u64,
    pub buffered_frames: u64,
    pub underflow_count: u64,
    pub meter: AudioMeter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeKind {
    Image,
    Audio,
    Video,
}

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
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeOutput {
    CpuBuffer {
        format: String,
        bytes: Vec<u8>,
        hash: String,
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

pub enum HostCommand {
    CreateWindow {
        request: WindowRequest,
        reply: oneshot::Sender<Result<WindowHandle, PlatformError>>,
    },
    CreateSurface {
        request: SurfaceRequest,
        reply: oneshot::Sender<Result<SurfaceHandle, PlatformError>>,
    },
    CaptureSurface {
        surface: SurfaceHandle,
        reply: oneshot::Sender<Result<CapturedFrame, PlatformError>>,
    },
    PresentRgba {
        surface: SurfaceHandle,
        frame: RgbaFrame,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    PresentScene {
        surface: SurfaceHandle,
        frame: SceneFrame,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    #[cfg(feature = "platform-test-driver")]
    InjectSurfaceDeviceLoss {
        surface: SurfaceHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    DestroySurface {
        surface: SurfaceHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    DestroyWindow {
        window: WindowHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    OpenAudioOutput {
        request: AudioOutputRequest,
        reply: oneshot::Sender<Result<AudioOutputHandle, PlatformError>>,
    },
    QueryAudioOutputFormat {
        reply: oneshot::Sender<Result<AudioOutputFormat, PlatformError>>,
    },
    QueryAudioDeviceFormat {
        reply: oneshot::Sender<Result<AudioDeviceFormat, PlatformError>>,
    },
    SubmitAudio {
        output: AudioOutputHandle,
        packet: AudioPacket,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    QueryAudio {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<AudioOutputState, PlatformError>>,
    },
    DrainAudio {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<AudioMeter, PlatformError>>,
    },
    QueryAudioOutput {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<AudioOutputStatus, PlatformError>>,
    },
    PauseAudio {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    ResumeAudio {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    AbortAudio {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    #[cfg(feature = "platform-test-driver")]
    InjectAudioDeviceLoss {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    CloseAudio {
        output: AudioOutputHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    OpenDecode {
        kind: DecodeKind,
        reply: oneshot::Sender<Result<DecodeSessionHandle, PlatformError>>,
    },
    Decode {
        session: DecodeSessionHandle,
        request: PlatformDecodeRequest,
        reply: oneshot::Sender<Result<DecodeOutput, PlatformError>>,
    },
    CloseDecode {
        session: DecodeSessionHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    BeginSave {
        slot: String,
        reply: oneshot::Sender<Result<SaveTransactionHandle, PlatformError>>,
    },
    WriteSave {
        transaction: SaveTransactionHandle,
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    CommitSave {
        transaction: SaveTransactionHandle,
        reply: oneshot::Sender<Result<String, PlatformError>>,
    },
    AbortSave {
        transaction: SaveTransactionHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    ReadSave {
        slot: String,
        reply: oneshot::Sender<Result<Vec<u8>, PlatformError>>,
    },
    ListSaves {
        reply: oneshot::Sender<Result<Vec<String>, PlatformError>>,
    },
    DeleteSave {
        slot: String,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    OpenPackage {
        source: PackageSourceRequest,
        reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    },
    ReadPackageRange {
        source: PackageSourceHandle,
        offset: u64,
        length: usize,
        reply: oneshot::Sender<Result<Vec<u8>, PlatformError>>,
    },
    ClosePackage {
        source: PackageSourceHandle,
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
    Shutdown {
        reply: oneshot::Sender<Result<(), PlatformError>>,
    },
}

impl HostCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::CreateWindow { .. } => "window.create",
            Self::CreateSurface { .. } => "surface.create",
            Self::CaptureSurface { .. } => "surface.capture",
            Self::PresentRgba { .. } => "surface.present_rgba",
            Self::PresentScene { .. } => "surface.present_scene",
            #[cfg(feature = "platform-test-driver")]
            Self::InjectSurfaceDeviceLoss { .. } => "surface.test.inject_device_loss",
            Self::DestroySurface { .. } => "surface.destroy",
            Self::DestroyWindow { .. } => "window.destroy",
            Self::OpenAudioOutput { .. } => "audio.open",
            Self::QueryAudioOutputFormat { .. } => "audio.format",
            Self::QueryAudioDeviceFormat { .. } => "audio.query_device_format",
            Self::SubmitAudio { .. } => "audio.submit",
            Self::QueryAudio { .. } => "audio.query",
            Self::DrainAudio { .. } => "audio.drain",
            Self::QueryAudioOutput { .. } => "audio.query",
            Self::PauseAudio { .. } => "audio.pause",
            Self::ResumeAudio { .. } => "audio.resume",
            Self::AbortAudio { .. } => "audio.abort",
            #[cfg(feature = "platform-test-driver")]
            Self::InjectAudioDeviceLoss { .. } => "audio.test.inject_device_loss",
            Self::CloseAudio { .. } => "audio.close",
            Self::OpenDecode { .. } => "decode.open",
            Self::Decode { .. } => "decode.submit",
            Self::CloseDecode { .. } => "decode.close",
            Self::BeginSave { .. } => "save.begin",
            Self::WriteSave { .. } => "save.write",
            Self::CommitSave { .. } => "save.commit",
            Self::AbortSave { .. } => "save.abort",
            Self::ReadSave { .. } => "save.read",
            Self::ListSaves { .. } => "save.list",
            Self::DeleteSave { .. } => "save.delete",
            Self::OpenPackage { .. } => "package.open",
            Self::ReadPackageRange { .. } => "package.read_range",
            Self::ClosePackage { .. } => "package.close",
            Self::Shutdown { .. } => "host.shutdown",
        }
    }

    pub fn reply_unit(self, result: Result<(), PlatformError>) -> Result<(), PlatformError> {
        let sent = match self {
            Self::PresentRgba { reply, .. }
            | Self::PresentScene { reply, .. }
            | Self::DestroySurface { reply, .. }
            | Self::DestroyWindow { reply, .. }
            | Self::SubmitAudio { reply, .. }
            | Self::PauseAudio { reply, .. }
            | Self::ResumeAudio { reply, .. }
            | Self::AbortAudio { reply, .. }
            | Self::CloseAudio { reply, .. }
            | Self::CloseDecode { reply, .. }
            | Self::WriteSave { reply, .. }
            | Self::DeleteSave { reply, .. }
            | Self::AbortSave { reply, .. }
            | Self::ClosePackage { reply, .. }
            | Self::Shutdown { reply } => reply.send(result),
            #[cfg(feature = "platform-test-driver")]
            Self::InjectAudioDeviceLoss { reply, .. }
            | Self::InjectSurfaceDeviceLoss { reply, .. } => reply.send(result),
            other => {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    other.operation(),
                    "host command does not have a unit reply",
                ));
            }
        };
        sent.map_err(|_| queue_closed("command.reply"))
    }

    pub fn reply_error(self, error: PlatformError) -> Result<(), PlatformError> {
        macro_rules! send_error {
            ($reply:expr) => {
                $reply
                    .send(Err(error))
                    .map_err(|_| queue_closed("command.reply"))
            };
        }
        match self {
            Self::CreateWindow { reply, .. } => send_error!(reply),
            Self::CreateSurface { reply, .. } => send_error!(reply),
            Self::CaptureSurface { reply, .. } => send_error!(reply),
            Self::PresentRgba { reply, .. } => send_error!(reply),
            Self::PresentScene { reply, .. } => send_error!(reply),
            #[cfg(feature = "platform-test-driver")]
            Self::InjectSurfaceDeviceLoss { reply, .. } => send_error!(reply),
            Self::DestroySurface { reply, .. } => send_error!(reply),
            Self::DestroyWindow { reply, .. } => send_error!(reply),
            Self::OpenAudioOutput { reply, .. } => send_error!(reply),
            Self::QueryAudioOutputFormat { reply } => send_error!(reply),
            Self::QueryAudioDeviceFormat { reply } => send_error!(reply),
            Self::SubmitAudio { reply, .. } => send_error!(reply),
            Self::QueryAudio { reply, .. } => send_error!(reply),
            Self::DrainAudio { reply, .. } => send_error!(reply),
            Self::QueryAudioOutput { reply, .. } => send_error!(reply),
            Self::PauseAudio { reply, .. } => send_error!(reply),
            Self::ResumeAudio { reply, .. } => send_error!(reply),
            Self::AbortAudio { reply, .. } => send_error!(reply),
            #[cfg(feature = "platform-test-driver")]
            Self::InjectAudioDeviceLoss { reply, .. } => send_error!(reply),
            Self::CloseAudio { reply, .. } => send_error!(reply),
            Self::OpenDecode { reply, .. } => send_error!(reply),
            Self::Decode { reply, .. } => send_error!(reply),
            Self::CloseDecode { reply, .. } => send_error!(reply),
            Self::BeginSave { reply, .. } => send_error!(reply),
            Self::WriteSave { reply, .. } => send_error!(reply),
            Self::CommitSave { reply, .. } => send_error!(reply),
            Self::AbortSave { reply, .. } => send_error!(reply),
            Self::ReadSave { reply, .. } => send_error!(reply),
            Self::ListSaves { reply } => send_error!(reply),
            Self::DeleteSave { reply, .. } => send_error!(reply),
            Self::OpenPackage { reply, .. } => send_error!(reply),
            Self::ReadPackageRange { reply, .. } => send_error!(reply),
            Self::ClosePackage { reply, .. } => send_error!(reply),
            Self::Shutdown { reply } => send_error!(reply),
        }
    }
}

impl std::fmt::Debug for HostCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostCommand")
            .field("operation", &self.operation())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlatformEvent {
    pub sequence: u64,
    pub kind: PlatformEventKind,
}

impl PlatformEvent {
    pub fn new(sequence: u64, kind: PlatformEventKind) -> Self {
        Self { sequence, kind }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
    Other(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowInsets {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFocusState {
    Gained,
    Lost,
    LostTransient,
    Duck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressureLevel {
    Moderate,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadControl {
    South,
    East,
    West,
    North,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftStickButton,
    RightStickButton,
    Start,
    Select,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformEventKind {
    Resumed,
    Suspended,
    WindowFocused {
        window: WindowHandle,
        focused: bool,
    },
    WindowClosed {
        window: WindowHandle,
    },
    WindowResized {
        window: WindowHandle,
        width: u32,
        height: u32,
        scale_factor: f64,
    },
    WindowInsetsChanged {
        window: WindowHandle,
        insets: WindowInsets,
    },
    AudioFocusChanged {
        state: AudioFocusState,
    },
    MemoryPressure {
        level: MemoryPressureLevel,
    },
    Keyboard {
        window: WindowHandle,
        physical_key: String,
        logical_key: Option<String>,
        state: InputState,
        repeat: bool,
    },
    ImePreedit {
        window: WindowHandle,
        text: String,
        cursor: Option<(usize, usize)>,
    },
    ImeCommit {
        window: WindowHandle,
        text: String,
    },
    PointerMoved {
        window: WindowHandle,
        x: f64,
        y: f64,
    },
    PointerButton {
        window: WindowHandle,
        button: PointerButton,
        state: InputState,
    },
    MouseWheel {
        window: WindowHandle,
        delta_x: f32,
        delta_y: f32,
    },
    Touch {
        window: WindowHandle,
        id: u64,
        x: f64,
        y: f64,
        phase: TouchPhase,
    },
    AccessibilityAction {
        window: WindowHandle,
        semantic_id: String,
        action: String,
        value: Option<String>,
    },
    GamepadConnected {
        device_id: u32,
    },
    GamepadDisconnected {
        device_id: u32,
    },
    GamepadInput {
        device_id: u32,
        control: GamepadControl,
        value: f32,
    },
    DeviceLost {
        provider: String,
    },
    DeviceRestored {
        provider: String,
    },
    ContextLost {
        provider: String,
    },
    ContextRestored {
        provider: String,
    },
}

#[derive(Clone)]
pub struct PlatformHostClient {
    command_tx: mpsc::Sender<HostCommand>,
    shutdown: Arc<AtomicBool>,
    profile: Arc<HostLaunchProfile>,
}

impl PlatformHostClient {
    pub async fn query_audio_device_format(&self) -> Result<AudioDeviceFormat, PlatformError> {
        self.ensure_running("audio.query_device_format")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::QueryAudioDeviceFormat { reply })?;
        let format = response
            .await
            .map_err(|_| queue_closed("audio.query_device_format"))??;
        if format.sample_rate == 0 || format.channels == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.query_device_format",
                "audio provider returned an invalid device format",
            ));
        }
        Ok(format)
    }

    pub async fn create_window(
        &self,
        request: WindowRequest,
    ) -> Result<WindowHandle, PlatformError> {
        if request.width == 0 || request.height == 0 || request.title.trim().is_empty() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "window.create",
                "window title and dimensions must be valid",
            ));
        }
        self.ensure_running("window.create")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CreateWindow { request, reply })?;
        response.await.map_err(|_| queue_closed("window.create"))?
    }

    pub async fn create_surface(
        &self,
        request: SurfaceRequest,
    ) -> Result<SurfaceHandle, PlatformError> {
        if request.width == 0 || request.height == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "surface.create",
                "surface dimensions must be non-zero",
            ));
        }
        self.ensure_running("surface.create")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CreateSurface { request, reply })?;
        response.await.map_err(|_| queue_closed("surface.create"))?
    }

    pub async fn capture_surface(
        &self,
        surface: SurfaceHandle,
    ) -> Result<CapturedFrame, PlatformError> {
        self.ensure_running("surface.capture")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CaptureSurface { surface, reply })?;
        let frame = response
            .await
            .map_err(|_| queue_closed("surface.capture"))??;
        validate_rgba_frame(
            frame.width,
            frame.height,
            &frame.rgba8,
            self.profile.limits().max_frame_bytes,
        )?;
        Ok(frame)
    }

    pub async fn present_rgba(
        &self,
        surface: SurfaceHandle,
        frame: RgbaFrame,
    ) -> Result<(), PlatformError> {
        if frame.sequence == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "surface.present_rgba",
                "presented frame sequence must be non-zero",
            ));
        }
        validate_rgba_frame(
            frame.width,
            frame.height,
            &frame.rgba8,
            self.profile.limits().max_frame_bytes,
        )?;
        self.ensure_running("surface.present_rgba")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::PresentRgba {
            surface,
            frame,
            reply,
        })?;
        response
            .await
            .map_err(|_| queue_closed("surface.present_rgba"))?
    }

    pub async fn present_scene(
        &self,
        surface: SurfaceHandle,
        frame: SceneFrame,
    ) -> Result<(), PlatformError> {
        validate_scene_frame(&frame, self.profile.limits().max_frame_bytes)?;
        self.ensure_running("surface.present_scene")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::PresentScene {
            surface,
            frame,
            reply,
        })?;
        response
            .await
            .map_err(|_| queue_closed("surface.present_scene"))?
    }

    #[cfg(feature = "platform-test-driver")]
    pub async fn inject_surface_device_loss(
        &self,
        surface: SurfaceHandle,
    ) -> Result<(), PlatformError> {
        self.ensure_running("surface.test.inject_device_loss")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::InjectSurfaceDeviceLoss { surface, reply })?;
        response
            .await
            .map_err(|_| queue_closed("surface.test.inject_device_loss"))?
    }

    pub async fn destroy_surface(&self, surface: SurfaceHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::DestroySurface { surface, reply },
            response,
            "surface.destroy",
        )
        .await
    }

    pub async fn destroy_window(&self, window: WindowHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::DestroyWindow { window, reply },
            response,
            "window.destroy",
        )
        .await
    }

    pub async fn open_audio_output(
        &self,
        request: AudioOutputRequest,
    ) -> Result<AudioOutputHandle, PlatformError> {
        if request.sample_rate == 0
            || request.channels == 0
            || request.max_buffered_frames == 0
            || request.max_buffered_frames > self.profile.limits().max_audio_frames
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.open",
                "audio output descriptor is invalid or exceeds profile limits",
            ));
        }
        self.ensure_running("audio.open")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::OpenAudioOutput { request, reply })?;
        response.await.map_err(|_| queue_closed("audio.open"))?
    }

    pub async fn preferred_audio_output_format(&self) -> Result<AudioOutputFormat, PlatformError> {
        self.ensure_running("audio.format")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::QueryAudioOutputFormat { reply })?;
        let format = response.await.map_err(|_| queue_closed("audio.format"))??;
        if !(8_000..=384_000).contains(&format.sample_rate) || !(1..=8).contains(&format.channels) {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.format",
                "audio provider returned an invalid preferred output format",
            ));
        }
        Ok(format)
    }

    pub async fn submit_audio(
        &self,
        output: AudioOutputHandle,
        packet: AudioPacket,
    ) -> Result<(), PlatformError> {
        if packet.sequence == 0
            || packet.channels == 0
            || packet.samples.is_empty()
            || !packet
                .samples
                .len()
                .is_multiple_of(usize::from(packet.channels))
            || packet.frame_count() > self.profile.limits().max_audio_frames
            || packet.samples.iter().any(|sample| !sample.is_finite())
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "audio.submit",
                "audio packet is invalid or exceeds profile limits",
            ));
        }
        self.ensure_running("audio.submit")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::SubmitAudio {
            output,
            packet,
            reply,
        })?;
        response.await.map_err(|_| queue_closed("audio.submit"))?
    }

    pub async fn drain_audio(
        &self,
        output: AudioOutputHandle,
    ) -> Result<AudioMeter, PlatformError> {
        self.ensure_running("audio.drain")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::DrainAudio { output, reply })?;
        let meter = response.await.map_err(|_| queue_closed("audio.drain"))??;
        if !meter.peak_dbfs.is_finite() || !meter.rms_dbfs.is_finite() {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.drain",
                "audio meter contains non-finite values",
            ));
        }
        Ok(meter)
    }

    pub async fn query_audio(
        &self,
        output: AudioOutputHandle,
    ) -> Result<AudioOutputState, PlatformError> {
        self.ensure_running("audio.query")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::QueryAudio { output, reply })?;
        let state = response.await.map_err(|_| queue_closed("audio.query"))??;
        if state.consumed_samples > state.submitted_samples
            || !state.meter.peak_dbfs.is_finite()
            || !state.meter.rms_dbfs.is_finite()
        {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.query",
                "audio output state is internally inconsistent",
            ));
        }
        Ok(state)
    }

    pub async fn query_audio_output(
        &self,
        output: AudioOutputHandle,
    ) -> Result<AudioOutputStatus, PlatformError> {
        self.ensure_running("audio.query")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::QueryAudioOutput { output, reply })?;
        let status = response.await.map_err(|_| queue_closed("audio.query"))??;
        if status.played_frames > status.submitted_frames
            || status.buffered_frames != status.submitted_frames - status.played_frames
            || !status.meter.peak_dbfs.is_finite()
            || !status.meter.rms_dbfs.is_finite()
        {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "audio.query",
                "audio output status is internally inconsistent",
            ));
        }
        Ok(status)
    }

    pub async fn pause_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::PauseAudio { output, reply },
            response,
            "audio.pause",
        )
        .await
    }

    pub async fn resume_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::ResumeAudio { output, reply },
            response,
            "audio.resume",
        )
        .await
    }

    pub async fn abort_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::AbortAudio { output, reply },
            response,
            "audio.abort",
        )
        .await
    }

    #[cfg(feature = "platform-test-driver")]
    pub async fn inject_audio_device_loss(
        &self,
        output: AudioOutputHandle,
    ) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::InjectAudioDeviceLoss { output, reply },
            response,
            "audio.test.inject_device_loss",
        )
        .await
    }

    pub async fn close_audio(&self, output: AudioOutputHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::CloseAudio { output, reply },
            response,
            "audio.close",
        )
        .await
    }

    pub fn launch_profile(&self) -> &HostLaunchProfile {
        &self.profile
    }

    pub fn platform_profile(&self) -> Result<&PlatformHostProfile, PlatformError> {
        self.profile.require_platform()
    }

    pub fn headless_profile(&self) -> Result<&HeadlessHostProfile, PlatformError> {
        self.profile.require_headless()
    }

    pub async fn open_decode(
        &self,
        kind: DecodeKind,
    ) -> Result<DecodeSessionHandle, PlatformError> {
        self.ensure_running("decode.open")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::OpenDecode { kind, reply })?;
        response.await.map_err(|_| queue_closed("decode.open"))?
    }

    pub async fn decode(
        &self,
        session: DecodeSessionHandle,
        request: PlatformDecodeRequest,
    ) -> Result<DecodeOutput, PlatformError> {
        if request.sequence == 0
            || request.codec.is_empty()
            || request.bytes.is_empty()
            || request.bytes.len() > self.profile.limits().max_frame_bytes
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "decode.submit",
                "decode request is invalid or exceeds profile limits",
            ));
        }
        self.ensure_running("decode.submit")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::Decode {
            session,
            request,
            reply,
        })?;
        response.await.map_err(|_| queue_closed("decode.submit"))?
    }

    pub async fn close_decode(&self, session: DecodeSessionHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::CloseDecode { session, reply },
            response,
            "decode.close",
        )
        .await
    }

    pub async fn begin_save(
        &self,
        slot: impl Into<String>,
    ) -> Result<SaveTransactionHandle, PlatformError> {
        let slot = slot.into();
        if !is_safe_slot(&slot) {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "save.begin",
                "save slot must be a safe relative symbol",
            ));
        }
        self.ensure_running("save.begin")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::BeginSave { slot, reply })?;
        response.await.map_err(|_| queue_closed("save.begin"))?
    }

    pub async fn write_save(
        &self,
        transaction: SaveTransactionHandle,
        bytes: Vec<u8>,
    ) -> Result<(), PlatformError> {
        if bytes.is_empty() || bytes.len() > self.profile.limits().max_frame_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "save.write",
                "save payload is empty or exceeds profile limits",
            ));
        }
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::WriteSave {
                transaction,
                bytes,
                reply,
            },
            response,
            "save.write",
        )
        .await
    }

    pub async fn commit_save(
        &self,
        transaction: SaveTransactionHandle,
    ) -> Result<String, PlatformError> {
        self.ensure_running("save.commit")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::CommitSave { transaction, reply })?;
        let hash = response.await.map_err(|_| queue_closed("save.commit"))??;
        if !hash.starts_with("sha256:") {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.commit",
                "save commit did not return a sha256 identity",
            ));
        }
        Ok(hash)
    }

    pub async fn abort_save(
        &self,
        transaction: SaveTransactionHandle,
    ) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::AbortSave { transaction, reply },
            response,
            "save.abort",
        )
        .await
    }

    pub async fn read_save(&self, slot: impl Into<String>) -> Result<Vec<u8>, PlatformError> {
        let slot = slot.into();
        if !is_safe_slot(&slot) {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "save.read",
                "save slot must be a safe relative symbol",
            ));
        }
        self.ensure_running("save.read")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::ReadSave { slot, reply })?;
        let bytes = response.await.map_err(|_| queue_closed("save.read"))??;
        if bytes.is_empty() || bytes.len() > self.profile.limits().max_frame_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.read",
                "save backend returned invalid payload length",
            ));
        }
        Ok(bytes)
    }

    pub async fn delete_save(&self, slot: impl Into<String>) -> Result<(), PlatformError> {
        let slot = slot.into();
        if !is_safe_slot(&slot) {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "save.delete",
                "save slot must be a safe relative symbol",
            ));
        }
        self.ensure_running("save.delete")?;
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::DeleteSave { slot, reply },
            response,
            "save.delete",
        )
        .await
    }

    pub async fn list_saves(&self) -> Result<Vec<String>, PlatformError> {
        self.ensure_running("save.list")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::ListSaves { reply })?;
        let slots = response.await.map_err(|_| queue_closed("save.list"))??;
        if slots.len() > 256
            || slots.windows(2).any(|pair| pair[0] >= pair[1])
            || slots.iter().any(|slot| !is_safe_slot(slot))
        {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.list",
                "save backend returned an invalid, duplicate, unsorted, or oversized slot catalog",
            ));
        }
        Ok(slots)
    }

    pub async fn open_package(
        &self,
        source: PackageSourceRequest,
    ) -> Result<PackageSourceHandle, PlatformError> {
        validate_package_source(&self.profile, &source)?;
        self.ensure_running("package.open")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::OpenPackage { source, reply })?;
        response.await.map_err(|_| queue_closed("package.open"))?
    }

    pub async fn read_package_range(
        &self,
        source: PackageSourceHandle,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, PlatformError> {
        if length == 0 || length > self.profile.limits().max_package_read_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.read_range",
                "package range length is invalid or exceeds profile limits",
            ));
        }
        self.ensure_running("package.read_range")?;
        let (reply, response) = oneshot::channel();
        self.try_send(HostCommand::ReadPackageRange {
            source,
            offset,
            length,
            reply,
        })?;
        let bytes = response
            .await
            .map_err(|_| queue_closed("package.read_range"))??;
        if bytes.len() > length {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.read_range",
                "package backend returned more bytes than requested",
            ));
        }
        Ok(bytes)
    }

    pub async fn close_package(&self, source: PackageSourceHandle) -> Result<(), PlatformError> {
        let (reply, response) = oneshot::channel();
        self.send_unit(
            HostCommand::ClosePackage { source, reply },
            response,
            "package.close",
        )
        .await
    }

    pub async fn shutdown(&self) -> Result<(), PlatformError> {
        if self
            .shutdown
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.shutdown",
                "platform host has already been shut down",
            ));
        }
        let (reply, response) = oneshot::channel();
        if let Err(error) = self.try_send(HostCommand::Shutdown { reply }) {
            self.shutdown.store(false, Ordering::Release);
            return Err(error);
        }
        match response.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.shutdown.store(false, Ordering::Release);
                Err(error)
            }
            Err(_) => {
                self.shutdown.store(false, Ordering::Release);
                Err(queue_closed("host.shutdown"))
            }
        }
    }

    fn ensure_running(&self, operation: &'static str) -> Result<(), PlatformError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                operation,
                "platform host is shutting down",
            ));
        }
        Ok(())
    }

    fn try_send(&self, command: HostCommand) -> Result<(), PlatformError> {
        let operation = command.operation();
        self.command_tx
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    operation,
                    "platform command queue is full",
                ),
                mpsc::error::TrySendError::Closed(_) => queue_closed(operation),
            })
    }

    async fn send_unit(
        &self,
        command: HostCommand,
        response: oneshot::Receiver<Result<(), PlatformError>>,
        operation: &'static str,
    ) -> Result<(), PlatformError> {
        self.ensure_running(operation)?;
        self.try_send(command)?;
        response.await.map_err(|_| queue_closed(operation))?
    }
}

pub struct PlatformEventStream {
    event_rx: mpsc::Receiver<PlatformEvent>,
    last_sequence: u64,
}

impl PlatformEventStream {
    pub async fn recv(&mut self) -> Result<PlatformEvent, PlatformError> {
        let event = self
            .event_rx
            .recv()
            .await
            .ok_or_else(|| queue_closed("event.recv"))?;
        if event.sequence <= self.last_sequence {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "event.recv",
                "platform event sequence is not strictly increasing",
            ));
        }
        self.last_sequence = event.sequence;
        Ok(event)
    }
}

pub struct PlatformBackendChannels {
    command_rx: mpsc::Receiver<HostCommand>,
    event_tx: mpsc::Sender<PlatformEvent>,
    last_event_sequence: Arc<std::sync::atomic::AtomicU64>,
}

impl PlatformBackendChannels {
    pub async fn next_command(&mut self) -> Option<HostCommand> {
        self.command_rx.recv().await
    }

    pub fn try_next_command(&mut self) -> Result<Option<HostCommand>, PlatformError> {
        match self.command_rx.try_recv() {
            Ok(command) => Ok(Some(command)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(queue_closed("command.recv")),
        }
    }

    pub fn emit_event(&mut self, event: PlatformEvent) -> Result<(), PlatformError> {
        let mut previous = self.last_event_sequence.load(Ordering::Acquire);
        loop {
            if event.sequence <= previous {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "event.emit",
                    "platform event sequence is not strictly increasing",
                ));
            }
            match self.last_event_sequence.compare_exchange(
                previous,
                event.sequence,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => previous = actual,
            }
        }
        self.event_tx
            .try_send(event.clone())
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    "event.emit",
                    "platform event queue is full",
                ),
                mpsc::error::TrySendError::Closed(_) => queue_closed("event.emit"),
            })?;
        Ok(())
    }

    pub fn event_emitter(&self) -> PlatformEventEmitter {
        PlatformEventEmitter {
            event_tx: self.event_tx.clone(),
            last_event_sequence: Arc::clone(&self.last_event_sequence),
        }
    }
}

#[derive(Clone)]
pub struct PlatformEventEmitter {
    event_tx: mpsc::Sender<PlatformEvent>,
    last_event_sequence: Arc<std::sync::atomic::AtomicU64>,
}

impl PlatformEventEmitter {
    pub fn emit(&self, kind: PlatformEventKind) -> Result<u64, PlatformError> {
        let sequence = self.last_event_sequence.fetch_add(1, Ordering::AcqRel) + 1;
        self.event_tx
            .try_send(PlatformEvent::new(sequence, kind))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    "event.emit",
                    "platform event queue is full",
                ),
                mpsc::error::TrySendError::Closed(_) => queue_closed("event.emit"),
            })?;
        Ok(sequence)
    }
}

pub struct PlatformHostSession {
    pub client: PlatformHostClient,
    pub events: PlatformEventStream,
    pub profile: HostLaunchProfile,
}

pub fn host_channel(
    profile: impl Into<HostLaunchProfile>,
    command_capacity: usize,
    event_capacity: usize,
) -> Result<
    (
        PlatformHostClient,
        PlatformBackendChannels,
        PlatformEventStream,
    ),
    PlatformError,
> {
    let profile = profile.into();
    profile.validate()?;
    if command_capacity == 0 || event_capacity == 0 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "host.channel",
            "platform channel capacity must be non-zero",
        ));
    }
    let (command_tx, command_rx) = mpsc::channel(command_capacity);
    let (event_tx, event_rx) = mpsc::channel(event_capacity);
    Ok((
        PlatformHostClient {
            command_tx,
            shutdown: Arc::new(AtomicBool::new(false)),
            profile: Arc::new(profile),
        },
        PlatformBackendChannels {
            command_rx,
            event_tx,
            last_event_sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        },
        PlatformEventStream {
            event_rx,
            last_sequence: 0,
        },
    ))
}

fn queue_closed(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::QueueClosed,
        operation,
        "platform host queue is closed",
    )
}

fn validate_rgba_frame(
    width: u32,
    height: u32,
    rgba8: &[u8],
    max_bytes: usize,
) -> Result<(), PlatformError> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(rgba8.len()) || rgba8.len() > max_bytes {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.frame.validate",
            "RGBA frame dimensions and byte length do not match",
        ));
    }
    Ok(())
}

fn validate_scene_frame(frame: &SceneFrame, max_bytes: usize) -> Result<(), PlatformError> {
    if let Some(semantics) = &frame.semantics {
        astra_ui_core::ValidateUi::validate(semantics).map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "surface.present_scene",
                format!("accessibility semantic snapshot is invalid: {error}"),
            )
        })?;
    }
    let output_bytes = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if frame.sequence == 0
        || frame.width == 0
        || frame.height == 0
        || output_bytes.is_none_or(|bytes| bytes > max_bytes)
        || frame.commands.len() > max_bytes / 16
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidState,
            "surface.present_scene",
            "text scene sequence, dimensions, or command count exceeds profile limits",
        ));
    }
    let mut resource_bytes = 0usize;
    let mut clip_depth = 0usize;
    let mut transform_depth = 0usize;
    let mut opacity_depth = 0usize;
    for command in &frame.commands {
        match command {
            SceneCommand::UploadTexture { frame: texture, .. } => {
                let expected = usize::try_from(texture.width)
                    .ok()
                    .and_then(|width| {
                        usize::try_from(texture.height)
                            .ok()
                            .and_then(|height| width.checked_mul(height))
                    })
                    .and_then(|pixels| pixels.checked_mul(4));
                if texture.width == 0
                    || texture.height == 0
                    || expected != Some(texture.rgba8.len())
                    || astra_core::Hash256::from_sha256(&texture.rgba8) != texture.hash
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "texture dimensions or content hash are invalid",
                    ));
                }
                resource_bytes =
                    resource_bytes
                        .checked_add(texture.rgba8.len())
                        .ok_or_else(|| {
                            PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "surface.present_scene",
                                "scene resource byte count overflowed",
                            )
                        })?;
            }
            SceneCommand::UploadGlyph { glyph, .. } => {
                let channels = match glyph.format {
                    astra_media_core::GlyphBitmapFormat::Alpha8 => 1usize,
                    astra_media_core::GlyphBitmapFormat::Rgba8 => 4usize,
                };
                let expected = usize::try_from(glyph.width)
                    .ok()
                    .and_then(|width| {
                        usize::try_from(glyph.height)
                            .ok()
                            .and_then(|height| width.checked_mul(height))
                    })
                    .and_then(|pixels| pixels.checked_mul(channels));
                if glyph.width == 0
                    || glyph.height == 0
                    || expected != Some(glyph.pixels.len())
                    || astra_core::Hash256::from_sha256(&glyph.pixels) != glyph.hash
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "glyph bitmap dimensions or content hash are invalid",
                    ));
                }
                resource_bytes =
                    resource_bytes
                        .checked_add(glyph.pixels.len())
                        .ok_or_else(|| {
                            PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "surface.present_scene",
                                "glyph upload byte count overflowed",
                            )
                        })?;
            }
            SceneCommand::ReleaseResource { .. } | SceneCommand::Clear { .. } => {}
            SceneCommand::GlyphRun { opacity, .. } | SceneCommand::Glyph { opacity, .. }
                if opacity.is_finite() && (0.0..=1.0).contains(opacity) => {}
            SceneCommand::Mesh2D {
                id,
                vertices,
                indices,
                material,
                texture_id,
                opacity,
                ..
            } => {
                let material_binding_valid = matches!(
                    (material, texture_id),
                    (astra_media_core::MeshMaterial2D::Solid, None)
                        | (
                            astra_media_core::MeshMaterial2D::ColorTexture
                                | astra_media_core::MeshMaterial2D::GlyphMask,
                            Some(_)
                        )
                );
                let vertices_valid = !vertices.is_empty()
                    && vertices.len() <= 250_000
                    && vertices.iter().all(|vertex| {
                        vertex.position.iter().all(|value| value.is_finite())
                            && vertex.uv.iter().all(|value| value.is_finite())
                            && vertex.premultiplied_rgba[0] <= vertex.premultiplied_rgba[3]
                            && vertex.premultiplied_rgba[1] <= vertex.premultiplied_rgba[3]
                            && vertex.premultiplied_rgba[2] <= vertex.premultiplied_rgba[3]
                    });
                let indices_valid = !indices.is_empty()
                    && indices.len() <= 750_000
                    && indices.len().is_multiple_of(3)
                    && indices
                        .iter()
                        .all(|index| (*index as usize) < vertices.len());
                if id.is_empty()
                    || !opacity.is_finite()
                    || !(0.0..=1.0).contains(opacity)
                    || !material_binding_valid
                    || !vertices_valid
                    || !indices_valid
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "indexed mesh geometry, material, color, or opacity is invalid",
                    ));
                }
            }
            SceneCommand::Rect { width, height, .. } => {
                if *width == 0 || *height == 0 {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "rectangle dimensions must be non-zero",
                    ));
                }
            }
            SceneCommand::Sprite {
                source,
                destination,
                opacity,
                ..
            } => {
                if destination.width == 0
                    || destination.height == 0
                    || source.is_some_and(|source| {
                        source.x < 0 || source.y < 0 || source.width == 0 || source.height == 0
                    })
                    || !opacity.is_finite()
                    || !(0.0..=1.0).contains(opacity)
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "sprite geometry, opacity, or blend mode is invalid",
                    ));
                }
            }
            SceneCommand::PushClip { rect } => {
                if rect.width == 0 || rect.height == 0 {
                    return Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "text clip dimensions must be non-zero",
                    ));
                }
                clip_depth = clip_depth.checked_add(1).ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "text clip stack overflowed",
                    )
                })?;
            }
            SceneCommand::PopClip if clip_depth > 0 => clip_depth -= 1,
            SceneCommand::PopClip => {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "surface.present_scene",
                    "text clip stack underflowed",
                ));
            }
            SceneCommand::Texture {
                frame: texture,
                destination,
                opacity,
                ..
            }
            | SceneCommand::VideoFrame {
                frame: texture,
                destination,
                opacity,
                ..
            } => {
                let expected = usize::try_from(texture.width)
                    .ok()
                    .and_then(|width| {
                        usize::try_from(texture.height)
                            .ok()
                            .and_then(|height| width.checked_mul(height))
                    })
                    .and_then(|pixels| pixels.checked_mul(4));
                if texture.width == 0
                    || texture.height == 0
                    || expected != Some(texture.rgba8.len())
                    || astra_core::Hash256::from_sha256(&texture.rgba8) != texture.hash
                    || destination.width == 0
                    || destination.height == 0
                    || !opacity.is_finite()
                    || !(0.0..=1.0).contains(opacity)
                {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "surface.present_scene",
                        "inline media frame geometry, opacity, or content hash is invalid",
                    ));
                }
                resource_bytes =
                    resource_bytes
                        .checked_add(texture.rgba8.len())
                        .ok_or_else(|| {
                            PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "surface.present_scene",
                                "scene resource byte count overflowed",
                            )
                        })?;
            }
            SceneCommand::PushTransform { transform } | SceneCommand::SetCamera { transform }
                if transform.m11.is_finite()
                    && transform.m12.is_finite()
                    && transform.m21.is_finite()
                    && transform.m22.is_finite()
                    && transform.tx.is_finite()
                    && transform.ty.is_finite() =>
            {
                if matches!(command, SceneCommand::PushTransform { .. }) {
                    transform_depth = transform_depth.checked_add(1).ok_or_else(|| {
                        PlatformError::new(
                            PlatformErrorCode::InvalidState,
                            "surface.present_scene",
                            "transform stack overflowed",
                        )
                    })?;
                }
            }
            SceneCommand::PopTransform if transform_depth > 0 => transform_depth -= 1,
            SceneCommand::PushOpacity { opacity }
                if opacity.is_finite() && (0.0..=1.0).contains(opacity) =>
            {
                opacity_depth = opacity_depth.checked_add(1).ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "surface.present_scene",
                        "opacity stack overflowed",
                    )
                })?;
            }
            SceneCommand::PopOpacity if opacity_depth > 0 => opacity_depth -= 1,
            SceneCommand::FilterGraph { graph }
                if graph.schema == "astra.filter_graph.v1" && !graph.nodes.is_empty() => {}
            _ => {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "surface.present_scene",
                    "scene contains an invalid renderer command or unbalanced stack operation",
                ));
            }
        }
    }
    if resource_bytes > max_bytes || clip_depth != 0 || transform_depth != 0 || opacity_depth != 0 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidState,
            "surface.present_scene",
            "scene resource budget or command stack is invalid",
        ));
    }
    Ok(())
}

fn validate_package_source(
    profile: &HostLaunchProfile,
    source: &PackageSourceRequest,
) -> Result<(), PlatformError> {
    let (allowed, expected_hash) = match source {
        PackageSourceRequest::Bundled {
            relative_path,
            expected_hash,
        } => (
            profile
                .package_sources()
                .iter()
                .any(|policy| matches!(policy, PackageSourcePolicy::Bundled))
                && is_safe_relative_path(relative_path),
            expected_hash,
        ),
        PackageSourceRequest::UserAuthorized { expected_hash } => (
            profile
                .package_sources()
                .iter()
                .any(|policy| matches!(policy, PackageSourcePolicy::UserAuthorized)),
            expected_hash,
        ),
        PackageSourceRequest::HttpsRange { url, expected_hash } => {
            let origin = https_origin(url);
            let allowed = origin.as_ref().is_some_and(|origin| {
                profile.package_sources().iter().any(|policy| match policy {
                    PackageSourcePolicy::HttpsRange { allowed_origins } => {
                        allowed_origins.iter().any(|allowed| allowed == origin)
                    }
                    _ => false,
                })
            });
            (allowed, expected_hash)
        }
    };
    if !allowed {
        return Err(PlatformError::new(
            PlatformErrorCode::PermissionDenied,
            "package.open",
            "package source is not allowed by the platform profile",
        ));
    }
    if !expected_hash.starts_with("sha256:") || expected_hash.len() <= "sha256:".len() {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.open",
            "package source requires a sha256 identity",
        ));
    }
    Ok(())
}

fn is_safe_slot(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains('\\')
        && !value.contains("://")
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(':'))
}

fn https_origin(value: &str) -> Option<String> {
    let rest = value.strip_prefix("https://")?;
    let authority = rest.split('/').next()?;
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('\\')
        || authority.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(format!("https://{}", authority.to_ascii_lowercase()))
}
