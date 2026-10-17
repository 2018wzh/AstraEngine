use super::{validate_dimensions, FamilyError, FamilyResult};
use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
static NEXT_SESSION_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Allocate in the host, then pass the value into the family open request.
pub fn allocate_session_epoch() -> FamilyResult<u64> {
    NEXT_SESSION_EPOCH
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| invalid("ASTRA_EMU_FAMILY_SESSION_EPOCH", "session epoch exhausted"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAlpha {
    Opaque,
    /// Native RGB is already composited; ignore render-target alpha bytes.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    Rgba8Srgb { alpha: FrameAlpha },
    Bgra8Srgb { alpha: FrameAlpha },
    Rgba8Linear { alpha: FrameAlpha },
    Bgra8Linear { alpha: FrameAlpha },
}

impl FrameFormat {
    pub fn alpha(self) -> FrameAlpha {
        match self {
            Self::Rgba8Srgb { alpha }
            | Self::Bgra8Srgb { alpha }
            | Self::Rgba8Linear { alpha }
            | Self::Bgra8Linear { alpha } => alpha,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRowOrder {
    TopToBottom,
    BottomToTop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub stride: u32,
    pub format: FrameFormat,
    pub rows: FrameRowOrder,
}

impl FrameInfo {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_dimensions(self.width, self.height)?;
        validate_dimensions(self.logical_width, self.logical_height)?;
        let row = self
            .width
            .checked_mul(4)
            .ok_or_else(|| invalid("ASTRA_EMU_FAMILY_FRAME_SIZE", "frame row size overflows"))?;
        if self.stride < row {
            return Err(invalid(
                "ASTRA_EMU_FAMILY_FRAME_STRIDE",
                "frame stride is smaller than the RGBA row",
            ));
        }
        self.required_bytes()
            .map(|_| ())
            .ok_or_else(|| invalid("ASTRA_EMU_FAMILY_FRAME_SIZE", "frame size overflows"))
    }

    pub fn required_bytes(&self) -> Option<usize> {
        u64::from(self.stride)
            .checked_mul(u64::from(self.height))
            .and_then(|bytes| usize::try_from(bytes).ok())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameIdentity {
    pub session_epoch: u64,
    pub output_epoch: u64,
    pub generation: u64,
}

impl FrameIdentity {
    pub fn validate(&self, completed: bool) -> FamilyResult<()> {
        if self.session_epoch == 0
            || (completed && (self.output_epoch == 0 || self.generation == 0))
        {
            return Err(invalid(
                "ASTRA_EMU_FAMILY_FRAME_GENERATION",
                "frame epoch or completed generation is zero",
            ));
        }
        Ok(())
    }
}
trait PixelBytes: Send + Sync {
    fn bytes(&self) -> &[u8];
}
struct PixelOwner<T> {
    value: T,
    bytes: fn(&T) -> &[u8],
}
impl<T: Send + Sync> PixelBytes for PixelOwner<T> {
    fn bytes(&self) -> &[u8] {
        (self.bytes)(&self.value)
    }
}
#[derive(Clone)]
enum FrameStorage {
    Cpu(Arc<dyn PixelBytes>),
    D3D12(Arc<super::D3D12FrameOwner>),
}
#[derive(Clone)]
pub struct FrameLease {
    pub info: FrameInfo,
    pub identity: FrameIdentity,
    pub timestamp_ns: u64,
    storage: FrameStorage,
}
impl fmt::Debug for FrameLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameLease")
            .field("info", &self.info)
            .field("identity", &self.identity)
            .finish()
    }
}
impl FrameLease {
    pub fn from_owner<T: Send + Sync + 'static>(
        value: T,
        bytes: fn(&T) -> &[u8],
        info: FrameInfo,
        identity: FrameIdentity,
    ) -> FamilyResult<Self> {
        info.validate()?;
        identity.validate(true)?;
        if info.required_bytes() != Some(bytes(&value).len()) {
            return Err(invalid("ASTRA_EMU_FRAME_BYTES", "invalid frame byte count"));
        }
        Ok(Self {
            info,
            identity,
            timestamp_ns: 0,
            storage: FrameStorage::Cpu(Arc::new(PixelOwner { value, bytes })),
        })
    }
    pub fn from_vec(
        value: Vec<u8>,
        info: FrameInfo,
        identity: FrameIdentity,
    ) -> FamilyResult<Self> {
        Self::from_owner(value, Vec::as_slice, info, identity)
    }
    pub fn pixels(&self) -> Option<&[u8]> {
        match &self.storage {
            FrameStorage::Cpu(pixels) => Some(pixels.bytes()),
            _ => None,
        }
    }
    pub fn d3d12(&self) -> Option<&Arc<super::D3D12FrameOwner>> {
        match &self.storage {
            FrameStorage::D3D12(owner) => Some(owner),
            _ => None,
        }
    }
    pub fn same_storage(&self, other: &Self) -> bool {
        match (&self.storage, &other.storage) {
            (FrameStorage::Cpu(a), FrameStorage::Cpu(b)) => {
                a.bytes().as_ptr() == b.bytes().as_ptr()
            }
            (FrameStorage::D3D12(a), FrameStorage::D3D12(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    pub(crate) fn from_d3d12(owner: super::D3D12FrameOwner, info: FrameInfo) -> FamilyResult<Self> {
        let frame = Self {
            info,
            identity: owner.descriptor().identity.into(),
            timestamp_ns: owner.descriptor().timestamp_ns,
            storage: FrameStorage::D3D12(Arc::new(owner)),
        };
        frame.validate()?;
        Ok(frame)
    }
    pub fn validate(&self) -> FamilyResult<()> {
        self.info.validate()?;
        self.identity.validate(true)?;
        if let Some(pixels) = self.pixels() {
            if self.info.required_bytes() != Some(pixels.len()) {
                return Err(invalid("ASTRA_EMU_FRAME_BYTES", "invalid frame byte count"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub enum FramePoll {
    Ready(FrameLease),
    Unchanged(FrameIdentity),
    Pending(FrameIdentity),
}

impl FramePoll {
    pub fn identity(&self) -> FrameIdentity {
        match self {
            Self::Ready(frame) => frame.identity,
            Self::Unchanged(identity) | Self::Pending(identity) => *identity,
        }
    }

    pub fn validate(&self, session_epoch: u64) -> FamilyResult<()> {
        if self.identity().session_epoch != session_epoch {
            return Err(invalid(
                "ASTRA_EMU_FAMILY_FRAME_EPOCH",
                "frame belongs to a different session",
            ));
        }
        match self {
            Self::Ready(frame) => frame.validate(),
            Self::Unchanged(identity) => identity.validate(true),
            Self::Pending(identity) => identity.validate(false),
        }
    }
}
fn invalid(code: &str, message: &str) -> FamilyError {
    FamilyError::invalid(code, message)
}
impl From<astra_emu_abi::FrameIdentity> for FrameIdentity {
    fn from(value: astra_emu_abi::FrameIdentity) -> Self {
        Self {
            session_epoch: value.session_epoch,
            output_epoch: value.output_epoch,
            generation: value.generation,
        }
    }
}
impl From<FrameIdentity> for astra_emu_abi::FrameIdentity {
    fn from(value: FrameIdentity) -> Self {
        Self {
            session_epoch: value.session_epoch,
            output_epoch: value.output_epoch,
            generation: value.generation,
        }
    }
}
