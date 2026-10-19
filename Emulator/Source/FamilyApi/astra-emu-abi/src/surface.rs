use crate::{Bytes, Status, INVALID_ARGUMENT, STALE_CONTEXT};
use std::ffi::c_void;

pub const SURFACE_CPU: u32 = 1;
pub const PIXEL_RGBA8: u32 = 1;
pub const PIXEL_BGRA8: u32 = 2;
pub const COLOR_SRGB: u32 = 1;
pub const COLOR_LINEAR_SRGB: u32 = 2;
pub const ALPHA_OPAQUE: u32 = 1;
pub const ALPHA_IGNORED: u32 = 2;
pub const ALPHA_STRAIGHT: u32 = 3;
pub const ALPHA_PREMULTIPLIED: u32 = 4;
pub const ROWS_TOP_DOWN: u32 = 1;
pub const ROWS_BOTTOM_UP: u32 = 2;
pub const FRAME_PENDING: u32 = 0;
pub const FRAME_READY: u32 = 1;
pub const FRAME_UNCHANGED: u32 = 2;
pub const MAX_FRAME_BYTES: usize = 512 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameIdentity {
    pub session_epoch: u64,
    pub output_epoch: u64,
    pub generation: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SurfaceInfo {
    pub kind: u32,
    pub pixel_format: u32,
    pub color: u32,
    pub alpha: u32,
    pub rows: u32,
    pub logical: Extent,
    pub raster: Extent,
    pub stride: u64,
}
impl SurfaceInfo {
    pub fn validate(&self) -> Result<(), Status> {
        if self.kind != SURFACE_CPU
            || !matches!(self.pixel_format, PIXEL_RGBA8 | PIXEL_BGRA8)
            || !matches!(self.color, COLOR_SRGB | COLOR_LINEAR_SRGB)
            || !matches!(
                self.alpha,
                ALPHA_OPAQUE | ALPHA_IGNORED | ALPHA_STRAIGHT | ALPHA_PREMULTIPLIED
            )
            || !matches!(self.rows, ROWS_TOP_DOWN | ROWS_BOTTOM_UP)
            || [
                self.logical.width,
                self.logical.height,
                self.raster.width,
                self.raster.height,
            ]
            .iter()
            .any(|&n| n == 0 || n > 16384)
        {
            return Err(INVALID_ARGUMENT);
        }
        self.cpu_bytes()?;
        Ok(())
    }
    pub fn cpu_bytes(&self) -> Result<usize, Status> {
        if self.kind != SURFACE_CPU || self.stride < u64::from(self.raster.width) * 4 {
            return Err(INVALID_ARGUMENT);
        }
        self.stride
            .checked_mul(u64::from(self.raster.height))
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n != 0 && n <= MAX_FRAME_BYTES)
            .ok_or(INVALID_ARGUMENT)
    }
}

/// One independently owned immutable lease. Return it exactly once, after the
/// actual upload API no longer reads the source. The CPU owner and release
/// callback remain valid after session close; release must never access a VM,
/// device, window, session or host callback. No pixel scans are required.
#[repr(C)]
#[derive(Debug)]
pub struct CpuLease {
    pub pixels: Bytes,
    pub owner: *mut c_void,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}
#[repr(C)]
#[derive(Debug)]
pub struct FrameSurface {
    pub state: u32,
    pub reserved: u32,
    pub identity: FrameIdentity,
    pub timestamp_ns: u64,
    pub info: SurfaceInfo,
    /// Populated only for a Ready frame.
    pub cpu: CpuLease,
}
impl FrameSurface {
    /// Validate metadata without dereferencing or scanning pixel data.
    pub fn validate_cpu(&self, session_epoch: u64) -> Result<(), Status> {
        if session_epoch == 0 || self.identity.session_epoch != session_epoch {
            return Err(STALE_CONTEXT);
        }
        if self.reserved != 0 || self.identity.output_epoch == 0 {
            return Err(INVALID_ARGUMENT);
        }
        match self.state {
            FRAME_READY => {
                self.info.validate()?;
                if self.identity.generation == 0
                    || self.cpu.pixels.len != self.info.cpu_bytes()?
                    || self.cpu.pixels.data.is_null()
                    || self.cpu.owner.is_null()
                    || self.cpu.release.is_none()
                {
                    return Err(INVALID_ARGUMENT);
                }
            }
            FRAME_PENDING | FRAME_UNCHANGED => {
                if (self.state == FRAME_UNCHANGED && self.identity.generation == 0)
                    || !self.cpu.pixels.data.is_null()
                    || self.cpu.pixels.len != 0
                    || !self.cpu.owner.is_null()
                    || self.cpu.release.is_some()
                {
                    return Err(INVALID_ARGUMENT);
                }
            }
            _ => return Err(INVALID_ARGUMENT),
        }
        Ok(())
    }
}
