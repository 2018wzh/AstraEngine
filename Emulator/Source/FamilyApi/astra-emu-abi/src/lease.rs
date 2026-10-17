use crate::{FrameIdentity, FrameSurface, Status, SurfaceInfo, FRAME_READY, INVALID_ARGUMENT};

/// Validated immutable CPU storage. Moving this owner never copies pixels.
/// The module contract requires CPU release to be thread-safe and independent
/// of the session, graphics context, native thread and host callbacks.
#[derive(Debug)]
pub struct CpuFrame {
    raw: FrameSurface,
}
// SAFETY: adoption requires immutable, thread-safe storage and release. This
// wrapper accepts CPU leases only, never native GPU/session handles.
unsafe impl Send for CpuFrame {}
unsafe impl Sync for CpuFrame {}
impl CpuFrame {
    /// Adopts exactly one owned lease, including on validation failure.
    /// # Safety
    /// The producer must obey the CPU lease allocation/lifetime contract. In
    /// particular pixels are readable and immutable for their declared length,
    /// and release is valid exactly once on any thread (also after close).
    pub unsafe fn adopt(raw: FrameSurface, session_epoch: u64) -> Result<Self, Status> {
        let frame = Self { raw };
        frame.raw.validate_cpu(session_epoch)?;
        if frame.raw.state != FRAME_READY {
            return Err(INVALID_ARGUMENT);
        }
        Ok(frame)
    }
    pub fn info(&self) -> SurfaceInfo {
        self.raw.info
    }
    pub fn identity(&self) -> FrameIdentity {
        self.raw.identity
    }
    pub fn timestamp_ns(&self) -> u64 {
        self.raw.timestamp_ns
    }
    pub fn pixels(&self) -> &[u8] {
        // SAFETY: adoption validates bounds; the owner remains alive and immutable.
        unsafe { std::slice::from_raw_parts(self.raw.cpu.pixels.data, self.raw.cpu.pixels.len) }
    }
}
impl Drop for CpuFrame {
    fn drop(&mut self) {
        if let Some(release) = self.raw.cpu.release.take() {
            // SAFETY: adoption transfers the unique lease, including errors.
            unsafe { release(self.raw.cpu.owner) };
        }
    }
}
