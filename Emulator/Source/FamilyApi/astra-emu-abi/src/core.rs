use crate::{
    Bytes, ConfigEntry, ConfigField, Diagnostics, Error, FrameIdentity, FrameSurface, Header,
    HostServices, Session, Status, SurfaceInfo,
};
use std::ffi::c_void;

pub const CAP_CPU_SURFACE: u64 = 1;
pub const CAP_GPU_SURFACE: u64 = 2;
pub const CAP_TEXT_HOOK: u64 = 4;
pub const CAP_LIVE_CONFIGURATION: u64 = 8;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Descriptor {
    pub header: Header,
    pub family_id: Bytes,
    pub plugin_id: Bytes,
    pub version: Bytes,
    pub capabilities: u64,
    pub formats: *const Bytes,
    pub format_count: usize,
    pub configuration: *const ConfigField,
    pub configuration_count: usize,
}
/// Probe strings borrow module storage until the next ordinary module call.
/// Manager copies bounded metadata before making that call. NO_MATCH is normal.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProbeResult {
    pub game_id: Bytes,
    pub format: Bytes,
    pub confidence_permyriad: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WindowState {
    pub drawable_width: u32,
    pub drawable_height: u32,
    pub scale_factor: f64,
    pub focused: u32,
    pub suspended: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OpenRequest {
    pub header: Header,
    pub game_path: Bytes,
    pub session_epoch: u64,
    pub configuration: *const ConfigEntry,
    pub configuration_count: usize,
    pub window: WindowState,
    pub host: HostServices,
    /// Opaque owner from the negotiated module-owner extension, or null when
    /// the core has no cross-session owner extension.
    pub module_owner: crate::ModuleOwner,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OpenResult {
    pub session: Session,
    pub context_epoch: u64,
    pub output_epoch: u64,
    pub surface: SurfaceInfo,
}
pub const INPUT_KEY: u32 = 1;
pub const INPUT_POINTER_MOVE: u32 = 2;
pub const INPUT_POINTER_BUTTON: u32 = 3;
pub const INPUT_WHEEL: u32 = 4;
pub const INPUT_WINDOW: u32 = 5;
/// Pointer coordinates are pixels in `displayed` frame, not window or logical
/// stage coordinates. Core applies its own frame-to-game/UI mapping once.
/// Physical keys use winit code names (KeyA, Digit1, Equal, Backquote, Enter).
/// Logical keys contain actual UTF-8 characters or named keys (Enter, Escape),
/// never Debug wrappers/control characters. Wheel x/y are fractional line
/// detents, not pixels. pressed/repeat and window booleans are exactly 0 or 1.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InputEvent {
    pub kind: u32,
    pub pressed: u32,
    pub repeat: u32,
    pub button: u32,
    pub physical_key: Bytes,
    pub logical_key: Bytes,
    pub x: f64,
    pub y: f64,
    pub displayed: FrameIdentity,
    pub window: WindowState,
}
pub const BUTTON_PRIMARY: u32 = 1;
pub const BUTTON_SECONDARY: u32 = 2;
pub const BUTTON_MIDDLE: u32 = 3;
pub const CONTROL_PAUSE: u32 = 1;
pub const CONTROL_RESUME: u32 = 2;
pub const CONTROL_RETRY: u32 = 3;
pub const SESSION_RUNNING: u32 = 1;
pub const SESSION_PAUSED: u32 = 2;
pub const SESSION_WAITING: u32 = 3;
pub const SESSION_FAILED: u32 = 4;
pub const SESSION_FINISHED: u32 = 5;
/// Only a retained, retry-safe operation may set this flag in SESSION_FAILED.
/// Panic and fatal native failures leave it clear; Host offers End only.
pub const SESSION_CAN_RETRY: u32 = 1;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SessionState {
    pub state: u32,
    pub flags: u32,
    pub context_epoch: u64,
    pub output_epoch: u64,
    pub error: Error,
}
impl SessionState {
    pub fn validate(&self) -> Result<(), Status> {
        if !matches!(
            self.state,
            SESSION_RUNNING | SESSION_PAUSED | SESSION_WAITING | SESSION_FAILED | SESSION_FINISHED
        ) || self.flags & !SESSION_CAN_RETRY != 0
            || (self.flags != 0 && self.state != SESSION_FAILED)
            || self.context_epoch == 0
            || self.output_epoch == 0
            || self.error.code_len as usize > self.error.code.len()
            || self.error.message_len as usize > self.error.message.len()
            || (self.state == SESSION_FAILED && self.error.code_len == 0)
        {
            return Err(crate::INVALID_ARGUMENT);
        }
        std::str::from_utf8(&self.error.code[..self.error.code_len as usize])
            .map_err(|_| crate::INVALID_ARGUMENT)?;
        std::str::from_utf8(&self.error.message[..self.error.message_len as usize])
            .map_err(|_| crate::INVALID_ARGUMENT)?;
        Ok(())
    }
}

/// Ordinary calls are serialized by the Host. The core schedules itself; no
/// elapsed time or advance callback exists. Session panic poisons gameplay but
/// cancel/close must still perform all reachable cleanup. `close` consumes the
/// handle on every outcome and returns only after all workers and callbacks end.
/// A failed `open` leaves a null session and performs the same cleanup itself.
/// Before close, Host prevents new cancel calls and waits for in-flight cancel
/// calls to return. No call may use the consumed handle during or after close.
/// Host publishes a successful open handle under the same synchronization as
/// cancellation; if cancellation won, it cancels/closes instead of publishing.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CoreApi {
    pub header: Header,
    pub initialize: Option<unsafe extern "C" fn(*const Diagnostics, *mut Error) -> Status>,
    pub describe: Option<unsafe extern "C" fn(*mut Descriptor, *mut Error) -> Status>,
    pub probe: Option<unsafe extern "C" fn(Bytes, *mut ProbeResult, *mut Error) -> Status>,
    pub open:
        Option<unsafe extern "C" fn(*const OpenRequest, *mut OpenResult, *mut Error) -> Status>,
    pub input:
        Option<unsafe extern "C" fn(Session, *const InputEvent, usize, *mut Error) -> Status>,
    pub control: Option<unsafe extern "C" fn(Session, u32, *mut Error) -> Status>,
    pub state: Option<unsafe extern "C" fn(Session, *mut SessionState, *mut Error) -> Status>,
    pub frame: Option<
        unsafe extern "C" fn(Session, FrameIdentity, *mut FrameSurface, *mut Error) -> Status,
    >,
    pub cancel: Option<unsafe extern "C" fn(Session)>,
    pub close: Option<unsafe extern "C" fn(Session, *mut Error) -> Status>,
    /// Returns a library-lifetime versioned C table, or UNSUPPORTED with null
    /// output. Backend protocols are defined only with a real implementation.
    pub extension: Option<unsafe extern "C" fn(Bytes, u32, *mut *const c_void) -> Status>,
}
impl CoreApi {
    pub fn validate(&self) -> Result<(), Status> {
        if !self.header.accepts::<Self>()
            || self.initialize.is_none()
            || self.describe.is_none()
            || self.probe.is_none()
            || self.open.is_none()
            || self.input.is_none()
            || self.control.is_none()
            || self.state.is_none()
            || self.frame.is_none()
            || self.cancel.is_none()
            || self.close.is_none()
            || self.extension.is_none()
        {
            return Err(crate::UNSUPPORTED);
        }
        Ok(())
    }
}
/// Query writes null on unsupported version, otherwise returns one immutable
/// module table. It performs no IO, starts no workers and installs no logger.
pub type Query = unsafe extern "C" fn(u32, *mut *const CoreApi) -> Status;
