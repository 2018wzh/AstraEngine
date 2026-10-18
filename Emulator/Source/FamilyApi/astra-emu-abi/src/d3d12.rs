//! Windows D3D12 cross-device delivery. This extension describes NT shared
//! resources/fences, not Rust graphics objects or a portable GPU handle format.
use crate::{Error, FrameIdentity, Header, OpenRequest, OpenResult, Session, Status, SurfaceInfo};
use std::ffi::c_void;

pub const D3D12_EXTENSION: &[u8] = b"astra.emu.surface.d3d12";
pub const D3D12_EXTENSION_VERSION: u32 = 1;
pub const BACKEND_D3D12: u32 = 1;
pub const D3D12_FORMAT_RGBA8_UNORM_SRGB: u32 = 29;
pub const D3D12_FORMAT_BGRA8_UNORM_SRGB: u32 = 91;
pub const D3D12_COPY_SOURCE: u32 = 1;
pub const D3D12_COPY_DESTINATION: u32 = 2;
pub const D3D12_NOT_SUBMITTED: u32 = 0;
pub const D3D12_CONSUMED: u32 = 1;

/// Actual Host adapter LUID and supported resource limits. The native core
/// creates its own device; LUID equality never means device identity equality.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct D3D12Host {
    pub header: Header,
    pub adapter_luid: u64,
    pub max_width: u32,
    pub max_height: u32,
    pub dxgi_format: u32,
    pub copy_usage: u32,
}
impl D3D12Host {
    pub const HEADER: Header = Header {
        size: std::mem::size_of::<Self>() as u32,
        version: D3D12_EXTENSION_VERSION,
    };
    pub fn validate(&self) -> Result<(), Status> {
        if self.header.version != D3D12_EXTENSION_VERSION
            || self.header.size < Self::HEADER.size
            || self.max_width == 0
            || self.max_height == 0
            || self.max_width > 16384
            || self.max_height > 16384
            || !matches!(
                self.dxgi_format,
                D3D12_FORMAT_RGBA8_UNORM_SRGB | D3D12_FORMAT_BGRA8_UNORM_SRGB
            )
            || self.copy_usage != D3D12_COPY_SOURCE | D3D12_COPY_DESTINATION
        {
            return Err(crate::UNSUPPORTED);
        }
        Ok(())
    }
}

/// Exactly one owned generation. Same-process NT handles are borrowed until
/// release; Host opens its own COM references. Ready means the native queue
/// Signal has been enqueued after returning the export resource to COMMON.
/// Host queue waits ready, copies in COPY_SOURCE, returns it to COMMON, then
/// enqueues done Signal. Host retains imports/fences through actual completion.
///
/// release(owner, NOT_SUBMITTED) means no GPU access was submitted. CONSUMED
/// promises the done Signal is already enqueued. Other dispositions are invalid
/// and must never cause producer reuse. Return exactly once, from any thread.
/// Retired leases stay valid after close and touch no VM or Host callback.
#[repr(C)]
#[derive(Debug)]
pub struct D3D12Lease {
    pub owner: *mut c_void,
    pub resource_handle: usize,
    pub ready_fence_handle: usize,
    pub done_fence_handle: usize,
    pub ready_value: u64,
    pub done_value: u64,
    pub release: Option<unsafe extern "C" fn(*mut c_void, u32)>,
}
#[repr(C)]
#[derive(Debug)]
pub struct D3D12Frame {
    pub state: u32,
    pub dxgi_format: u32,
    pub identity: FrameIdentity,
    pub timestamp_ns: u64,
    pub info: SurfaceInfo,
    pub adapter_luid: u64,
    /// Unique allocation identity within session/output epoch, not a frame ID.
    pub resource_id: u64,
    pub lease: D3D12Lease,
}
impl D3D12Frame {
    /// Checks metadata only, without dereferencing handles or invoking release.
    /// Host must still inspect the imported resource/fences against negotiation.
    pub fn validate(&self, host: &D3D12Host, session_epoch: u64) -> Result<(), Status> {
        host.validate()?;
        if session_epoch == 0 || self.identity.session_epoch != session_epoch {
            return Err(crate::STALE_CONTEXT);
        }
        if self.identity.output_epoch == 0 {
            return Err(crate::INVALID_ARGUMENT);
        }
        if self.state != crate::FRAME_READY {
            if !matches!(self.state, crate::FRAME_PENDING | crate::FRAME_UNCHANGED)
                || (self.state == crate::FRAME_UNCHANGED && self.identity.generation == 0)
                || !self.lease.owner.is_null()
                || self.lease.release.is_some()
                || self.lease.resource_handle != 0
                || self.lease.ready_fence_handle != 0
                || self.lease.done_fence_handle != 0
                || self.lease.ready_value != 0
                || self.lease.done_value != 0
            {
                return Err(crate::INVALID_ARGUMENT);
            }
            return Ok(());
        }
        self.info.validate()?;
        let format = match self.dxgi_format {
            D3D12_FORMAT_RGBA8_UNORM_SRGB => crate::PIXEL_RGBA8,
            D3D12_FORMAT_BGRA8_UNORM_SRGB => crate::PIXEL_BGRA8,
            _ => return Err(crate::UNSUPPORTED),
        };
        if self.info.kind != crate::SURFACE_GPU
            || self.info.backend != BACKEND_D3D12
            || self.info.color != crate::COLOR_SRGB
            || self.info.pixel_format != format
            || self.info.stride != 0
            || self.dxgi_format != host.dxgi_format
            || self.adapter_luid != host.adapter_luid
            || self.info.raster.width > host.max_width
            || self.info.raster.height > host.max_height
            || u64::from(self.info.raster.width) * u64::from(self.info.raster.height) * 4
                > crate::MAX_FRAME_BYTES as u64
            || self.identity.generation == 0
            || self.resource_id == 0
            || self.lease.owner.is_null()
            || self.lease.release.is_none()
            || [
                self.lease.resource_handle,
                self.lease.ready_fence_handle,
                self.lease.done_fence_handle,
            ]
            .iter()
            .any(|&value| value == 0 || value == usize::MAX)
            || self.lease.ready_value == 0
            || self.lease.done_value == 0
        {
            return Err(crate::INVALID_ARGUMENT);
        }
        Ok(())
    }
}

/// open shares the normal core's native session/worker and all CoreApi controls.
/// Host descriptor may be null: explicit CPU configuration uses normal CPU
/// startup; explicit GPU configuration must reject a missing/incompatible Host.
/// GPU startup never performs CPU capture as an implicit fallback.
///
/// Each generation is single-acquire. Repeating an old `after` after issuance
/// returns Pending until a new generation; matching `after` returns Unchanged.
/// Native reuse requires lease return AND relevant ready/done completion.
/// Close stops production, drains native writes with device-loss handling, and
/// retires exports independently; it does not wait for Host-held leases.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D12Api {
    pub header: Header,
    pub open: Option<
        unsafe extern "C" fn(
            *const OpenRequest,
            *const D3D12Host,
            *mut OpenResult,
            *mut Error,
        ) -> Status,
    >,
    pub acquire:
        Option<unsafe extern "C" fn(Session, FrameIdentity, *mut D3D12Frame, *mut Error) -> Status>,
}
impl D3D12Api {
    pub const HEADER: Header = Header {
        size: std::mem::size_of::<Self>() as u32,
        version: D3D12_EXTENSION_VERSION,
    };
    pub fn validate(&self) -> Result<(), Status> {
        if self.header.version != D3D12_EXTENSION_VERSION
            || self.header.size < Self::HEADER.size
            || self.open.is_none()
            || self.acquire.is_none()
        {
            return Err(crate::UNSUPPORTED);
        }
        Ok(())
    }
}
