//! One child process per session with three dedicated loopback TCP lanes:
//! control/replies/observed progress, synchronous PCM requests/replies, and
//! binary final frames. Standard output and standard error remain diagnostic
//! streams, so native/dependency logging cannot corrupt transport payloads.
//!
//! Control cancellation is served independently of PCM writes and frame
//! copying. Audio is fixed I16/stereo/48 kHz, 480 source frames per request.
//! Progress is an actual v10 Host observation; neither endpoint extrapolates
//! it or substitutes accepted/written counts for played source frames.

mod bounded;
mod codec;
mod frame;
mod lane;

pub use bounded::{Items, Text};
pub use codec::{read_packet, write_packet, Packet, PacketKind, Sequence};
pub use frame::{read_frame, write_frame, FinalFrame, FrameInfo};
pub use lane::{read_handshake, write_handshake, Lane};
use serde::{Deserialize, Serialize};

pub const VERSION: u16 = 1;
pub const MAX_PACKET_BYTES: usize = 512 * 1024;
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub const PCM_FRAMES: usize = 480;
pub const PCM_SAMPLES: usize = PCM_FRAMES * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub session: u64,
    pub sequence: u64,
}

impl Identity {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.session == 0 || self.sequence == 0 {
            return Err("zero session or sequence");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub focused: bool,
    pub visible: bool,
}

impl WindowState {
    fn validate(self) -> Result<(), &'static str> {
        if self.width == 0 || self.height == 0 {
            return Err("zero window dimension");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyState {
    Pressed,
    Released,
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    // Stable private codes are declared below, never platform VK numbers.
    Key {
        code: KeyCode,
        state: KeyState,
        modifiers: Modifiers,
    },
    PointerMove {
        x: f32,
        y: f32,
    },
    PointerButton {
        button: PointerButton,
        state: KeyState,
    },
    Wheel {
        delta_x: f32,
        delta_y: f32,
    },
    Text(Text<32768>),
    Focused(bool),
    Resized {
        width: u32,
        height: u32,
    },
    Visible(bool),
    Suspended(bool),
    CloseRequested,
}

impl Event {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::PointerMove { x, y }
            | Self::Wheel {
                delta_x: x,
                delta_y: y,
            } if !x.is_finite() || !y.is_finite() => Err("nonfinite input"),
            Self::Resized { width, height } if *width == 0 || *height == 0 => {
                Err("zero window dimension")
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyCode {
    Unknown,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Escape,
    Enter,
    Tab,
    Backspace,
    Space,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    ControlLeft,
    ControlRight,
    ShiftLeft,
    ShiftRight,
    AltLeft,
    AltRight,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadEnter,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    Minus,
    Equals,
    Comma,
    Period,
    Slash,
    Semicolon,
    Apostrophe,
    BracketLeft,
    BracketRight,
    Backslash,
    Grave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackClock {
    DeviceTimestamp,
    TestConsumption,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub accepted_source_frames: u64,
    pub played_source_frames: u64,
    pub clock: PlaybackClock,
}

impl Progress {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.played_source_frames > self.accepted_source_frames {
            return Err("played exceeds accepted");
        }
        Ok(())
    }
    pub fn validate_after(self, previous: Self) -> Result<(), &'static str> {
        self.validate()?;
        if self.clock != previous.clock
            || self.accepted_source_frames < previous.accepted_source_frames
            || self.played_source_frames < previous.played_source_frames
        {
            return Err("progress regressed or changed clock");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    NativeUnavailable,
    Boot,
    Advance,
    Frame,
    Input,
    Audio,
    Protocol,
    Shutdown,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KrKr {self:?}")
    }
}
impl std::error::Error for Failure {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteStatus {
    Accepted,
    Cancelled,
    Closed,
    Failed(Failure),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Boot {
        game_path: Text<4096>,
        window: WindowState,
        progress: Progress,
    },
    Advance {
        elapsed_ns: u64,
        events: Items<Event, 4096>,
    },
    Capture,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParentPacket {
    Command(Command),
    PcmReply {
        status: WriteStatus,
        progress: Option<Progress>,
    },
    Progress(Progress),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Running,
    Waiting,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowCommand {
    SetFullscreen(bool),
    SetTitle(Text<4096>),
    SetIcon {
        width: u32,
        height: u32,
        rgba: Items<u8, 262144>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    // The test shell never emits NativeReady.
    NativeReady {
        frame: FrameInfo,
    },
    Advanced {
        status: Status,
        window: Option<WindowCommand>,
        reset_clock: bool,
    },
    FrameReady {
        info: FrameInfo,
    },
    Stopped,
    Failed(Failure),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChildPacket {
    Reply(Reply),
    PcmWrite(Items<i16, PCM_SAMPLES>),
}

impl ParentPacket {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Command(Command::Boot {
                game_path,
                window,
                progress,
            }) => {
                if game_path.as_str().is_empty() || game_path.as_str().contains('\0') {
                    return Err("invalid game path");
                }
                window.validate()?;
                progress.validate()?;
                if progress.accepted_source_frames != 0 || progress.played_source_frames != 0 {
                    return Err("boot needs a fresh configured sink");
                }
            }
            Self::Command(Command::Advance { events, .. }) => {
                for event in events.as_slice() {
                    event.validate()?;
                }
            }
            Self::PcmReply { status, progress } => {
                if matches!(status, WriteStatus::Accepted) && progress.is_none() {
                    return Err("accepted PCM requires a real progress observation");
                }
                if let Some(progress) = progress {
                    progress.validate()?;
                }
            }
            Self::Progress(progress) => progress.validate()?,
            _ => {}
        }
        Ok(())
    }
}

impl ChildPacket {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Reply(Reply::NativeReady { frame } | Reply::FrameReady { info: frame }) => {
                frame.required_bytes()?;
            }
            Self::Reply(Reply::Advanced {
                window: Some(command),
                ..
            }) => match command {
                WindowCommand::SetTitle(title) if title.as_str().chars().any(char::is_control) => {
                    return Err("invalid window title")
                }
                WindowCommand::SetIcon {
                    width,
                    height,
                    rgba,
                } if *width == 0
                    || *height == 0
                    || *width > 256
                    || *height > 256
                    || width
                        .checked_mul(*height)
                        .and_then(|v| v.checked_mul(4))
                        .map(|v| v as usize)
                        != Some(rgba.as_slice().len()) =>
                {
                    return Err("invalid window icon");
                }
                _ => {}
            },
            Self::PcmWrite(samples) if samples.as_slice().len() != PCM_SAMPLES => {
                return Err("PCM request must contain 480 stereo frames")
            }
            _ => {}
        }
        Ok(())
    }
}
