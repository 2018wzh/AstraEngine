//! Native embedded core protocol. Only C-layout values cross this boundary.
//!
//! Dynamic libraries export `astra_emu_query`; static cores register the same
//! `Query` function. Libraries and their descriptor storage remain resident.
//! Ordinary calls are serialized on the session owner thread. `cancel` alone
//! may run concurrently; it must wake blocked native services, not queue behind
//! them. No call unwinds across this ABI. Input and polling never advance a VM.
#![deny(unsafe_op_in_unsafe_fn)]

mod config;
mod core;
#[cfg(feature = "diagnostic-bridge")]
pub mod diagnostic_bridge;
mod lease;
mod module_owner;
mod services;
mod session_menu;
mod surface;
pub use config::*;
pub use core::*;
pub use lease::*;
pub use module_owner::*;
pub use services::*;
pub use session_menu::*;
pub use surface::*;

use std::ffi::c_void;

pub const ABI_VERSION: u32 = 3;
pub const QUERY_SYMBOL: &[u8] = b"astra_emu_query\0";
pub type Status = i32;
pub const OK: Status = 0;
pub const UNSUPPORTED: Status = 1;
pub const INVALID_ARGUMENT: Status = 2;
pub const INVALID_STATE: Status = 3;
pub const CAPACITY: Status = 4;
pub const CANCELLED: Status = 5;
pub const CORE_ERROR: Status = 6;
pub const PANICKED: Status = 7;
pub const NO_MATCH: Status = 8;
pub const STALE_CONTEXT: Status = 9;
pub type Session = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub size: u32,
    pub version: u32,
}
impl Header {
    pub const fn of<T>() -> Self {
        Self {
            size: std::mem::size_of::<T>() as u32,
            version: ABI_VERSION,
        }
    }
    pub fn accepts<T>(&self) -> bool {
        self.version == ABI_VERSION && self.size as usize >= std::mem::size_of::<T>()
    }
}

/// Borrowed bytes. Null is permitted only for an empty span. The owner keeps
/// storage immutable and alive for the whole call (descriptors: library life).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Bytes {
    pub data: *const u8,
    pub len: usize,
}
impl Bytes {
    pub const EMPTY: Self = Self {
        data: std::ptr::null(),
        len: 0,
    };
    pub fn new(bytes: &[u8]) -> Self {
        Self {
            data: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
    pub const fn from_static(bytes: &'static [u8]) -> Self {
        Self {
            data: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
    /// # Safety
    /// Nonempty storage must be readable for the returned lifetime. The caller
    /// must ensure no mutation or release occurs during that lifetime.
    pub unsafe fn as_slice<'a>(&self, limit: usize) -> Result<&'a [u8], Status> {
        if self.len > limit
            || self.len > isize::MAX as usize
            || (self.len != 0 && self.data.is_null())
        {
            return Err(INVALID_ARGUMENT);
        }
        if self.len == 0 {
            return Ok(&[]);
        }
        Ok(unsafe { std::slice::from_raw_parts(self.data, self.len) })
    }
    /// # Safety
    /// Same requirements as `as_slice`.
    pub unsafe fn text<'a>(&self, limit: usize) -> Result<&'a str, Status> {
        let bytes = unsafe { self.as_slice(limit)? };
        let text = std::str::from_utf8(bytes).map_err(|_| INVALID_ARGUMENT)?;
        if text.contains('\0') {
            return Err(INVALID_ARGUMENT);
        }
        Ok(text)
    }
}

/// Caller-owned error output. Every failure supplies a stable diagnostic code;
/// status remains machine-readable even when the optional error pointer is null.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Error {
    pub code_len: u32,
    pub message_len: u32,
    pub code: [u8; 128],
    pub message: [u8; 512],
}
impl Default for Error {
    fn default() -> Self {
        Self {
            code_len: 0,
            message_len: 0,
            code: [0; 128],
            message: [0; 512],
        }
    }
}
impl Error {
    pub fn set(&mut self, code: &str, message: &str) {
        fn copy(text: &str, output: &mut [u8]) -> u32 {
            let mut end = text.len().min(output.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            output[..end].copy_from_slice(&text.as_bytes()[..end]);
            end as u32
        }
        self.code_len = copy(code, &mut self.code);
        self.message_len = copy(message, &mut self.message);
    }
}

/// # Safety
/// `data` is aligned readable storage for `count` values, immutable for `'a`.
pub unsafe fn borrowed_array<'a, T>(
    data: *const T,
    count: usize,
    limit: usize,
) -> Result<&'a [T], Status> {
    if count > limit
        || count
            .checked_mul(std::mem::size_of::<T>())
            .is_none_or(|n| n > isize::MAX as usize)
        || (count != 0 && (data.is_null() || !data.is_aligned()))
    {
        return Err(INVALID_ARGUMENT);
    }
    if count == 0 {
        return Ok(&[]);
    }
    Ok(unsafe { std::slice::from_raw_parts(data, count) })
}
