use super::*;

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
        reply: oneshot::Sender<Result<OpenedAudioOutput, PlatformError>>,
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
            Self::InjectSurfaceDeviceLoss { .. } => "surface.test.inject_device_loss",
            Self::DestroySurface { .. } => "surface.destroy",
            Self::DestroyWindow { .. } => "window.destroy",
            Self::OpenAudioOutput { .. } => "audio.open",
            Self::PauseAudio { .. } => "audio.pause",
            Self::ResumeAudio { .. } => "audio.resume",
            Self::AbortAudio { .. } => "audio.abort",
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
            Self::InjectSurfaceDeviceLoss { reply, .. } => send_error!(reply),
            Self::DestroySurface { reply, .. } => send_error!(reply),
            Self::DestroyWindow { reply, .. } => send_error!(reply),
            Self::OpenAudioOutput { reply, .. } => send_error!(reply),
            Self::PauseAudio { reply, .. } => send_error!(reply),
            Self::ResumeAudio { reply, .. } => send_error!(reply),
            Self::AbortAudio { reply, .. } => send_error!(reply),
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
    pub(super) command_tx: mpsc::Sender<HostCommand>,
    pub(super) command_wake: Option<PlatformCommandWakeRegistration>,
    pub(super) audio_wake: AudioWakeRegistration,
    pub(super) shutdown: Arc<AtomicBool>,
    pub(super) profile: Arc<HostLaunchProfile>,
}
