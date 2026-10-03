//! Fixed private NativeOwner.h ABI 1. All enums are explicit semantic maps.
use std::ffi::c_void;

#[cfg(windows)]
pub const VERSION: u32 = 1;
pub const OK: i32 = 0;
pub const WRITE_ACCEPTED: u32 = 0;
pub const WRITE_CANCELLED: u32 = 1;
pub const WRITE_CLOSED: u32 = 2;
pub const WRITE_FAILED: u32 = 3;

#[repr(C)]
pub struct Error {
    pub code: i32,
    pub message: [u8; 512],
}
impl Default for Error {
    fn default() -> Self {
        Self {
            code: 0,
            message: [0; 512],
        }
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub focused: u32,
    pub visible: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub stride: u32,
}

#[repr(C)]
pub struct Progress {
    pub accepted_source_frames: u64,
    pub played_source_frames: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Callbacks {
    pub context: *mut c_void,
    pub write_i16: unsafe extern "C" fn(*mut c_void, *const i16, u32) -> u32,
    pub progress: unsafe extern "C" fn(*mut c_void, *mut Progress) -> u32,
    pub cancel_audio: unsafe extern "C" fn(*mut c_void),
    pub is_cancelled: unsafe extern "C" fn(*mut c_void) -> u32,
}

#[repr(C)]
pub struct BootOptions {
    pub game_path_utf8: *const u8,
    pub game_path_bytes: u64,
    pub resource_root_utf8: *const u8,
    pub resource_root_bytes: u64,
    pub window: WindowState,
    pub callbacks: Callbacks,
}

#[repr(C)]
#[derive(Default)]
pub struct Event {
    pub kind: u32,
    pub key_code: u32,
    pub key_state: u32,
    pub modifiers: u32,
    pub pointer_button: u32,
    pub value: u32,
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
    pub text_utf8: *const u8,
    pub text_bytes: u64,
}

#[repr(C)]
#[derive(Default)]
pub struct WindowCommand {
    pub kind: u32,
    pub fullscreen: u32,
    pub icon_width: u32,
    pub icon_height: u32,
    pub data: *const u8,
    pub data_bytes: u64,
}

#[repr(C)]
#[derive(Default)]
pub struct AdvanceResult {
    pub state: u32,
    pub exit_code: i32,
    pub reset_clock: u32,
    pub window: WindowCommand,
}

pub type Boot =
    unsafe extern "C" fn(*const BootOptions, *mut *mut c_void, *mut FrameInfo, *mut Error) -> i32;
pub type Advance = unsafe extern "C" fn(
    *mut c_void,
    u64,
    *const Event,
    u32,
    *mut AdvanceResult,
    *mut Error,
) -> i32;
pub type GetFrame = unsafe extern "C" fn(*mut c_void, *mut FrameInfo, *mut Error) -> i32;
pub type CopyFrame =
    unsafe extern "C" fn(*mut c_void, *const FrameInfo, *mut u8, u64, *mut Error) -> i32;
pub type Shutdown = unsafe extern "C" fn(*mut c_void, *mut Error) -> i32;

#[derive(Clone, Copy)]
pub struct Api {
    pub boot: Boot,
    pub advance: Advance,
    pub frame_info: GetFrame,
    pub copy_frame: CopyFrame,
    pub shutdown: Shutdown,
}

// Same C17/C++17 x64 layout checked by the native owner. These types also
// compile on Linux for ownership/conversion checks without loading GL.
const _: () = {
    assert!(std::mem::size_of::<Error>() == 516);
    assert!(std::mem::size_of::<FrameInfo>() == 20);
    assert!(std::mem::size_of::<WindowState>() == 16);
};
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::size_of::<Callbacks>() == 40);
    assert!(std::mem::size_of::<BootOptions>() == 88);
    assert!(std::mem::size_of::<Event>() == 56);
    assert!(std::mem::size_of::<WindowCommand>() == 32);
    assert!(std::mem::size_of::<AdvanceResult>() == 48);
};
