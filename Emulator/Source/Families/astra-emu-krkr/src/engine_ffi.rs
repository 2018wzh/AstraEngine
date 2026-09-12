//! Raw C ABI to the vendored Kirikiri core (`astra_krkr_host.h`).
//!
//! The engine is a process-global singleton; one session per process is the
//! enforced lifecycle. All non-audio entry points must be called from the
//! thread that called [`astra_krkr_boot`]; PCM arrives on the engine's own
//! audio thread through the boot callback.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_double, c_int};

pub const ASTRA_KRKR_OK: c_int = 0;
pub const ASTRA_KRKR_ERR_STATE: c_int = 1;
pub const ASTRA_KRKR_ERR_BOOT: c_int = 2;
pub const ASTRA_KRKR_ERR_ARG: c_int = 3;
pub const ASTRA_KRKR_ERR_TERMINATED: c_int = 4;

pub const ASTRA_KRKR_INPUT_KEY_DOWN: u8 = 1;
pub const ASTRA_KRKR_INPUT_KEY_UP: u8 = 2;
pub const ASTRA_KRKR_INPUT_TEXT: u8 = 3;
pub const ASTRA_KRKR_INPUT_MOUSE_MOVE: u8 = 4;
pub const ASTRA_KRKR_INPUT_MOUSE_DOWN: u8 = 5;
pub const ASTRA_KRKR_INPUT_MOUSE_UP: u8 = 6;
pub const ASTRA_KRKR_INPUT_WHEEL: u8 = 7;

#[repr(C)]
pub struct AstraKrkrHostCallbacks {
    pub user: *mut std::ffi::c_void,
    pub push_pcm: Option<
        unsafe extern "C" fn(user: *mut std::ffi::c_void, samples: *const i16, frame_count: u32),
    >,
    pub log: Option<
        unsafe extern "C" fn(user: *mut std::ffi::c_void, level: u8, message: *const c_char),
    >,
}

unsafe impl Send for AstraKrkrHostCallbacks {}
unsafe impl Sync for AstraKrkrHostCallbacks {}

#[repr(C)]
pub struct AstraKrkrBootConfig {
    pub abi: u32,
    pub game_dir: *const c_char,
    pub save_dir: *const c_char,
    pub locale: *const c_char,
    pub initial_width: u32,
    pub initial_height: u32,
    pub callbacks: AstraKrkrHostCallbacks,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstraKrkrInputEvent {
    pub kind: u8,
    pub button: u8,
    pub shift: u8,
    pub control: u8,
    pub key: u16,
    pub utf32: u32,
    pub x: i32,
    pub y: i32,
    pub wheel: i32,
}

pub const fn input_event_repr_assertions() {
    // The struct is exchanged with C compiled with default packing; the
    // field order keeps the natural alignment (u8 x4, u16, u32, i32 x3).
    let _static_assert_size = std::mem::size_of::<AstraKrkrInputEvent>();
}

unsafe extern "C" {
    pub fn astra_krkr_abi_version() -> u32;
    pub fn astra_krkr_boot(config: *const AstraKrkrBootConfig) -> c_int;
    pub fn astra_krkr_tick(
        elapsed_seconds: c_double,
        events: *const AstraKrkrInputEvent,
        event_count: u32,
    ) -> c_int;
    pub fn astra_krkr_copy_frame(
        dst: *mut u8,
        capacity: u32,
        width: *mut u32,
        height: *mut u32,
    ) -> c_int;
    pub fn astra_krkr_audio_format(sample_rate: *mut u32, channels: *mut u16) -> c_int;
    pub fn astra_krkr_terminated(terminated: *mut u8) -> c_int;
    pub fn astra_krkr_shutdown() -> c_int;
}
