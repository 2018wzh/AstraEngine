use crate::{Bytes, Error, Header, Status};
use std::ffi::c_void;

pub const LOG_ERROR: u32 = 1;
pub const LOG_WARN: u32 = 2;
pub const LOG_INFO: u32 = 3;
pub const LOG_DEBUG: u32 = 4;
pub const LOG_TRACE: u32 = 5;
pub const FIELD_TEXT: u32 = 1;
pub const FIELD_I64: u32 = 2;
pub const FIELD_U64: u32 = 3;
pub const FIELD_F64: u32 = 4;
pub const FIELD_BOOL: u32 = 5;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LogField {
    pub name: Bytes,
    pub kind: u32,
    pub reserved: u32,
    pub text: Bytes,
    pub signed: i64,
    pub unsigned: u64,
    pub number: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LogRecord {
    pub level: u32,
    pub dropped_fields: u32,
    /// Empty targets identify unstructured native logs; the sink names them.
    pub target: Bytes,
    /// May be empty for native logging systems without structured event names.
    pub event: Bytes,
    pub message: Bytes,
    pub fields: *const LogField,
    pub field_count: usize,
}
/// Installed before descriptor/probe/open; sink storage lives until process exit.
/// Callback is thread-safe; all spans are borrowed only for this invocation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Diagnostics {
    pub header: Header,
    pub context: *mut c_void,
    pub emit: Option<unsafe extern "C" fn(*mut c_void, *const LogRecord)>,
    pub flush: Option<unsafe extern "C" fn(*mut c_void)>,
}

pub const TEXT_DIALOGUE: u32 = 1;
pub const TEXT_SPEAKER: u32 = 2;
pub const TEXT_CHOICE: u32 = 3;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TextItem {
    pub id: u64,
    pub kind: u32,
    pub reserved: u32,
    pub text: Bytes,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TextRequest {
    pub session_epoch: u64,
    pub context_epoch: u64,
    pub request_id: u64,
    pub items: *const TextItem,
    pub item_count: usize,
}
/// Caller provides text and item storage. On CAPACITY, required capacities are
/// returned and the same request ID is retried without executing Luau/network
/// twice. Host retains exactly one result until acknowledge or cancellation.
#[repr(C)]
#[derive(Debug)]
pub struct TextResult {
    pub bytes: *mut u8,
    pub byte_capacity: usize,
    pub byte_count: usize,
    pub items: *mut TextRange,
    pub item_capacity: usize,
    pub item_count: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TextRange {
    pub id: u64,
    pub offset: usize,
    pub len: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TextService {
    pub context: *mut c_void,
    pub request: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const TextRequest,
            *mut TextResult,
            *mut Error,
        ) -> Status,
    >,
    pub acknowledge: Option<unsafe extern "C" fn(*mut c_void, u64, u64, u64) -> Status>,
    /// Independent native cancellation, never queued behind the Luau VM.
    pub invalidate: Option<unsafe extern "C" fn(*mut c_void, u64, u64)>,
}

pub const WINDOW_TITLE: u32 = 1;
pub const WINDOW_FULLSCREEN: u32 = 2;
pub const WINDOW_ICON: u32 = 3;
pub const WINDOW_POINTER: u32 = 4;
pub const WINDOW_MODAL: u32 = 5;
pub const WINDOW_SIZE: u32 = 6;
pub const WINDOW_POSITION: u32 = 7;
/// flags is exactly 0/1; Host reports actual state using matching result bit.
pub const WINDOW_VISIBILITY: u32 = 8;
pub const WINDOW_MINIMIZED: u32 = 9;
/// Read actual window state without mutation. flags must be zero; inspect result valid_fields.
pub const WINDOW_QUERY_STATE: u32 = 10;
pub const MODAL_TEXT_INPUT: u32 = 1;
pub const MAX_MODAL_TEXT: usize = 65536;
pub const MAX_MODAL_BUTTONS: usize = 16;
pub const MAX_MODAL_BUTTON_LABEL: usize = 256;
pub const WINDOW_RESULT_MODAL: u32 = 1;
pub const WINDOW_RESULT_SIZE: u32 = 2;
pub const WINDOW_RESULT_POSITION: u32 = 4;
pub const WINDOW_RESULT_MODE: u32 = 8;
pub const WINDOW_RESULT_VISIBILITY: u32 = 16;
pub const WINDOW_RESULT_MINIMIZED: u32 = 32;
pub const FULLSCREEN_WINDOWED: u32 = 0;
pub const FULLSCREEN_BORDERLESS: u32 = 1;
pub const FULLSCREEN_EXCLUSIVE: u32 = 2;
pub const POINTER_SET_POSITION: u32 = 1;
pub const POINTER_SET_VISIBILITY: u32 = 2;
pub const POINTER_VISIBLE: u32 = 4;
pub const POINTER_SET_AUTOHIDE: u32 = 8;
pub const POINTER_SET_X: u32 = 16;
pub const POINTER_SET_Y: u32 = 32;
/// Typed bounded platform request; interpretation is selected by `kind`.
/// Window titles <=4096 bytes; modal caption/message/input <=65536 bytes.
/// Icons are <=256x256 tightly packed RGBA8. Modal buttons are <=16 items
/// with <=256-byte labels. MODAL_TEXT_INPUT distinguishes empty input from
/// a message-only dialog. Cancellation returns CANCELLED, not a button answer.
/// Modal buttons are explicit IDs; Host never invents an answer headlessly.
/// QUERY_STATE returns only fields the native platform can actually observe.
/// Focus/suspension must never be substituted for fullscreen/minimized state.
/// FULLSCREEN flags select one FULLSCREEN_* mode; exclusive width/height
/// request an exact physical display mode. Unsupported modes fail explicitly.
/// POINTER flags select independent properties; x/y use the logical stage.
/// SET_POSITION supplies both coordinates; SET_X/SET_Y preserve the other
/// coordinate. Do not combine SET_POSITION with the individual coordinate bits.
/// SIZE uses physical client pixels; POSITION uses physical screen coordinates.
/// These requests alter Host output O, never the core's raster R.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WindowRequest {
    pub session_epoch: u64,
    pub context_epoch: u64,
    pub request_id: u64,
    pub kind: u32,
    pub flags: u32,
    pub text: Bytes,
    pub pixels: Bytes,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub autohide_ms: u64,
    pub buttons: *const WindowButton,
    pub button_count: usize,
    pub caption: Bytes,
    pub initial_text: Bytes,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WindowButton {
    pub id: u32,
    pub reserved: u32,
    pub label: Bytes,
}
/// Caller-owned output, initialized before request; service preserves data and
/// capacity and writes only initialized UTF-8 bytes and len. Input dialogs
/// require capacity >= MAX_MODAL_TEXT before displaying UI; insufficient
/// capacity returns CAPACITY without displaying or consuming a user answer.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TextBuffer {
    pub data: *mut u8,
    pub capacity: usize,
    pub len: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WindowResult {
    pub selected_button: u32,
    pub reserved: u32,
    pub valid_fields: u32,
    pub fullscreen: u32,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub input: TextBuffer,
    pub visible: u32,
    pub minimized: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WindowService {
    pub context: *mut c_void,
    pub request: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const WindowRequest,
            *mut WindowResult,
            *mut Error,
        ) -> Status,
    >,
    pub invalidate: Option<unsafe extern "C" fn(*mut c_void, u64, u64)>,
}
/// Host-owned cancellation exists before open returns a Session. It is required
/// even without text/window services; return 1 when this session epoch is
/// cancelled, 0 otherwise. The callback must be nonblocking and thread-safe.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CancellationService {
    pub context: *mut c_void,
    pub is_cancelled: Option<unsafe extern "C" fn(*mut c_void, u64) -> u32>,
}

/// Services copied during open. Cancellation is required; text/window are
/// optional. Context objects stay alive until failed open or close returns.
/// Host cancellation also wakes its blocked services; core checks cancellation
/// between initialization stages and native work, including before open returns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostServices {
    pub header: Header,
    pub text: TextService,
    pub window: WindowService,
    pub cancellation: CancellationService,
}
