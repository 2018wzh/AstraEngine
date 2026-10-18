use super::{FamilyError, FamilyResult};
use astra_emu_abi as abi;
use std::sync::atomic::{AtomicBool, Ordering};

/// Owns one C lease, shared only inside Manager. Imports retain this owner until
/// the done queue signal completes, including after native session close.
pub struct D3D12FrameOwner {
    frame: abi::D3D12Frame,
    consumed: AtomicBool,
    claimed: AtomicBool,
}
// The backend contract makes handles immutable and release independent/thread-safe.
unsafe impl Send for D3D12FrameOwner {}
unsafe impl Sync for D3D12FrameOwner {}
impl D3D12FrameOwner {
    /// Caller transfers the one independently owned release obligation.
    pub(crate) unsafe fn adopt(frame: abi::D3D12Frame, epoch: u64) -> FamilyResult<Self> {
        let owner = Self {
            frame,
            consumed: AtomicBool::new(false),
            claimed: AtomicBool::new(false),
        };
        let frame = &owner.frame;
        if frame.state != abi::FRAME_READY
            || frame.identity.session_epoch != epoch
            || frame.identity.output_epoch == 0
            || frame.identity.generation == 0
            || frame.info.validate().is_err()
            || frame.info.kind != abi::SURFACE_GPU
            || frame.info.backend != abi::BACKEND_D3D12
            || frame.info.stride != 0
            || frame.resource_id == 0
            || frame.lease.owner.is_null()
            || frame.lease.release.is_none()
            || frame.lease.resource_handle == 0
            || frame.lease.ready_fence_handle == 0
            || frame.lease.done_fence_handle == 0
            || frame.lease.ready_value == 0
            || frame.lease.done_value == 0
            || !matches!(
                frame.dxgi_format,
                abi::D3D12_FORMAT_RGBA8_UNORM_SRGB | abi::D3D12_FORMAT_BGRA8_UNORM_SRGB
            )
        {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_D3D12_SURFACE",
                "invalid D3D12 lease",
            ));
        }
        Ok(owner)
    }
    pub fn descriptor(&self) -> &abi::D3D12Frame {
        &self.frame
    }
    /// Internal Arc clones share one consumption operation and one done value.
    /// A recreated graphics device must acquire a newer generation rather than
    /// submit the same lease on a second queue with an already-completed value.
    pub fn claim_consumption(&self) -> FamilyResult<()> {
        self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                FamilyError::invalid(
                    "ASTRA_EMU_D3D12_ALREADY_CONSUMED",
                    "GPU generation was already submitted",
                )
            })
    }
    /// # Safety
    /// Must follow successful queue submission and done Signal, not merely an
    /// encoder recording or CPU ownership handoff.
    pub unsafe fn mark_consumed(&self) {
        self.consumed.store(true, Ordering::Release);
    }
}
impl Drop for D3D12FrameOwner {
    fn drop(&mut self) {
        if let Some(release) = self.frame.lease.release {
            unsafe {
                release(
                    self.frame.lease.owner,
                    self.consumed.load(Ordering::Acquire) as u32,
                );
            }
        }
    }
}
