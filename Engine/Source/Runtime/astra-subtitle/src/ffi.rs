//! Minimal ABI from libass 0.17.4 ass.h. Track fields are read by bridge.c
//! against the installed official header, not a replicated Rust track layout.
use std::ffi::{c_char, c_int};

pub const VERSION: c_int = 0x01704000;

#[repr(C)]
pub struct Library {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Renderer {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Track {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Image {
    pub w: c_int,
    pub h: c_int,
    pub stride: c_int,
    pub bitmap: *mut u8,
    pub color: u32,
    pub dst_x: c_int,
    pub dst_y: c_int,
    pub next: *mut Image,
    pub image_type: c_int,
}

unsafe extern "C" {
    pub fn ass_library_version() -> c_int;
    pub fn ass_library_init() -> *mut Library;
    pub fn ass_library_done(library: *mut Library);
    pub fn ass_renderer_init(library: *mut Library) -> *mut Renderer;
    pub fn ass_renderer_done(renderer: *mut Renderer);
    pub fn ass_free_track(track: *mut Track);
    pub fn ass_read_memory(
        library: *mut Library,
        data: *mut c_char,
        size: usize,
        codepage: *const c_char,
    ) -> *mut Track;
    pub fn ass_add_font(
        library: *mut Library,
        name: *const c_char,
        data: *const c_char,
        size: c_int,
    );
    pub fn ass_set_fonts(
        renderer: *mut Renderer,
        default_font: *const c_char,
        default_family: *const c_char,
        provider: c_int,
        config: *const c_char,
        update: c_int,
    );
    pub fn ass_set_storage_size(renderer: *mut Renderer, w: c_int, h: c_int);
    pub fn ass_set_frame_size(renderer: *mut Renderer, w: c_int, h: c_int);
    pub fn ass_render_frame(
        renderer: *mut Renderer,
        track: *mut Track,
        now: i64,
        change: *mut c_int,
    ) -> *mut Image;
    pub fn astra_ass_quiet(library: *mut Library, status: *mut c_int);
    pub fn astra_ass_matrix(track: *const Track) -> c_int;
    pub fn astra_ass_track_valid(track: *const Track) -> c_int;
}
