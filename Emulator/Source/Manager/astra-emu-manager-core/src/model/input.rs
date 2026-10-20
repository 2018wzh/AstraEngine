use super::{validate_dimensions, FamilyError, FamilyResult, MAX_INPUT_EVENTS, MAX_TEXT_BYTES};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub focused: bool,
    pub visible: bool,
    pub suspended: bool,
}

impl WindowState {
    pub fn validate(&self) -> FamilyResult<()> {
        if !self.scale_factor.is_finite() || self.scale_factor <= 0.0 {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_WINDOW_SCALE",
                "invalid window scale factor",
            ));
        }
        validate_dimensions(self.width, self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
    Repeated,
}

/// Platform-neutral controls needed by legacy games. Both control keys are
/// explicit so a host can reliably expose Ctrl fast-forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FamilyEvent {
    Control {
        command: u32,
    },
    Key {
        code: KeyCode,
        state: KeyState,
        modifiers: KeyModifiers,
        logical: Option<String>,
    },
    /// Game-frame pixel coordinates after the host's letterbox/scaling
    /// transform. If the host uses Anime4K or another output scale, it maps
    /// pointer coordinates back to the original game-frame size first.
    PointerMove {
        x: f32,
        y: f32,
        displayed: Option<super::FrameIdentity>,
    },
    PointerButton {
        button: PointerButton,
        state: KeyState,
        displayed: Option<super::FrameIdentity>,
    },
    /// Horizontal and vertical wheel units; one unit is one OS wheel detent,
    /// and high-resolution wheel input may use fractional values.
    Wheel {
        delta_x: f32,
        delta_y: f32,
    },
    TextInput {
        text: String,
    },
    WindowFocused {
        focused: bool,
    },
    /// Physical client-pixel dimensions of the host window.
    WindowResized {
        width: u32,
        height: u32,
    },
    WindowScale {
        factor: f64,
    },
    WindowVisibility {
        visible: bool,
    },
    WindowSuspended {
        suspended: bool,
    },
}

impl FamilyEvent {
    pub(crate) fn validate(&self) -> FamilyResult<()> {
        match self {
            Self::PointerMove { x, y, .. }
            | Self::Wheel {
                delta_x: x,
                delta_y: y,
            } if !x.is_finite() || !y.is_finite() => Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_INPUT_VALUE",
                "pointer and wheel values must be finite",
            )),
            Self::TextInput { text } if text.len() > MAX_TEXT_BYTES => Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_TEXT_BOUNDS",
                "text input exceeds the ABI bound",
            )),
            Self::WindowResized { width, height } => validate_dimensions(*width, *height),
            Self::WindowScale { factor } if !factor.is_finite() || *factor <= 0.0 => Err(
                FamilyError::invalid("ASTRA_EMU_WINDOW_SCALE", "invalid window scale factor"),
            ),
            _ => Ok(()),
        }
    }
}

pub(crate) fn validate_events(events: &[FamilyEvent]) -> FamilyResult<()> {
    if events.len() > MAX_INPUT_EVENTS {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FAMILY_EVENTS",
            "input count exceeds the host bound",
        ));
    }
    for event in events {
        event.validate()?;
    }
    Ok(())
}
