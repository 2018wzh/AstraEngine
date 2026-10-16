#![allow(non_local_definitions)]

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

use abi_stable::{
    sabi_trait,
    std_types::{RArc, ROption, RSlice, RStr},
    type_level::downcasting::TD_Opaque,
    StableAbi,
};

use super::{
    descriptor::{FamilyError, FamilyResult},
    validate_dimensions,
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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FrameAlpha {
    Opaque,
    /// Native RGB is already composited; ignore render-target alpha bytes.
    Ignored,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FrameRowOrder {
    TopToBottom,
    BottomToTop,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FrameIdentity {
    pub session_epoch: u64,
    pub generation: u64,
}

impl FrameIdentity {
    pub fn validate(&self, completed: bool) -> FamilyResult<()> {
        if self.session_epoch == 0 || (completed && self.generation == 0) {
            return Err(invalid(
                "ASTRA_EMU_FAMILY_FRAME_GENERATION",
                "frame epoch or completed generation is zero",
            ));
        }
        Ok(())
    }
}

/// Immutable CPU storage only; no VM/GPU/window or thread-bound destructor.
#[sabi_trait]
pub trait FramePixels: Send + Sync {
    fn pixels(&self) -> RSlice<'_, u8>;
}

type SharedPixels = FramePixels_TO<'static, RArc<()>>;

struct PixelOwner<T> {
    value: T,
    bytes: fn(&T) -> &[u8],
}

impl<T: Send + Sync> FramePixels for PixelOwner<T> {
    fn pixels(&self) -> RSlice<'_, u8> {
        RSlice::from_slice((self.bytes)(&self.value))
    }
}

#[repr(C)]
#[derive(StableAbi)]
pub struct FrameLease {
    pub info: FrameInfo,
    pub identity: FrameIdentity,
    pixels: SharedPixels,
}

impl Clone for FrameLease {
    fn clone(&self) -> Self {
        Self {
            info: self.info,
            identity: self.identity,
            pixels: FramePixels_TO::from_sabi(self.pixels.obj.shallow_clone()),
        }
    }
}

impl fmt::Debug for FrameLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameLease")
            .field("info", &self.info)
            .field("identity", &self.identity)
            .field("pixel_bytes", &self.pixels().len())
            .finish()
    }
}

impl FrameLease {
    /// Moves the existing CPU owner without reallocating its pixel payload.
    pub fn from_owner<T: Send + Sync + 'static>(
        value: T,
        bytes: fn(&T) -> &[u8],
        info: FrameInfo,
        identity: FrameIdentity,
    ) -> FamilyResult<Self> {
        info.validate()?;
        identity.validate(true)?;
        validate_pixels(bytes(&value), info)?;
        Ok(Self {
            info,
            identity,
            pixels: FramePixels_TO::from_ptr(RArc::new(PixelOwner { value, bytes }), TD_Opaque),
        })
    }

    pub fn from_vec(
        value: Vec<u8>,
        info: FrameInfo,
        identity: FrameIdentity,
    ) -> FamilyResult<Self> {
        Self::from_owner(value, Vec::as_slice, info, identity)
    }

    pub fn pixels(&self) -> &[u8] {
        self.pixels.pixels().as_slice()
    }

    /// ABI validation does not re-scan certified immutable alpha on every pull.
    pub fn validate(&self) -> FamilyResult<()> {
        self.info.validate()?;
        self.identity.validate(true)?;
        validate_length(self.pixels(), self.info)
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
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

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct FrameRequest<'a> {
    pub session_id: RStr<'a>,
    pub after: ROption<FrameIdentity>,
}

impl FrameRequest<'_> {
    pub fn validate(&self) -> FamilyResult<()> {
        super::descriptor::validate_symbol("session_id", &self.session_id)?;
        if let ROption::RSome(identity) = self.after {
            identity.validate(true)?;
        }
        Ok(())
    }
}

/// Family-local publication state. Logical clocks never reset this generation.
#[derive(Debug)]
pub struct FramePublisher {
    identity: FrameIdentity,
    current: Option<FrameLease>,
}

impl FramePublisher {
    pub fn new(session_epoch: u64) -> FamilyResult<Self> {
        let identity = FrameIdentity {
            session_epoch,
            generation: 0,
        };
        identity.validate(false)?;
        Ok(Self {
            identity,
            current: None,
        })
    }

    pub fn publish<T: Send + Sync + 'static>(
        &mut self,
        value: T,
        bytes: fn(&T) -> &[u8],
        info: FrameInfo,
    ) -> FamilyResult<()> {
        let identity = FrameIdentity {
            session_epoch: self.identity.session_epoch,
            generation: self.identity.generation.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_FAMILY_FRAME_GENERATION",
                    "frame generation exhausted",
                )
            })?,
        };
        let frame = FrameLease::from_owner(value, bytes, info, identity)?;
        self.current = Some(frame);
        self.identity = identity;
        Ok(())
    }

    pub fn current(&self) -> Option<&FrameLease> {
        self.current.as_ref()
    }

    pub fn poll(&self, after: Option<FrameIdentity>) -> FramePoll {
        match &self.current {
            None => self.pending(),
            Some(_) if after == Some(self.identity) => FramePoll::Unchanged(self.identity),
            Some(frame) => FramePoll::Ready(frame.clone()),
        }
    }

    pub fn pending(&self) -> FramePoll {
        FramePoll::Pending(self.identity)
    }
}

fn validate_length(pixels: &[u8], info: FrameInfo) -> FamilyResult<()> {
    let required = info.required_bytes().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_FAMILY_FRAME_SIZE",
            "frame size does not fit host usize",
        )
    })?;
    if pixels.len() < required {
        return Err(invalid(
            "ASTRA_EMU_FAMILY_FRAME_BYTES",
            "frame pixels are shorter than stride times height",
        ));
    }
    Ok(())
}

fn validate_pixels(pixels: &[u8], info: FrameInfo) -> FamilyResult<()> {
    validate_length(pixels, info)?;
    if info.format.alpha() == FrameAlpha::Opaque {
        for row in
            pixels[..info.required_bytes().expect("validated size")].chunks(info.stride as usize)
        {
            if row[..info.width as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 255)
            {
                return Err(invalid(
                    "ASTRA_EMU_FAMILY_FRAME_ALPHA",
                    "opaque frame contains non-opaque pixels",
                ));
            }
        }
    }
    Ok(())
}

fn invalid(code: &'static str, message: &'static str) -> FamilyError {
    FamilyError::invalid(code, message)
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
