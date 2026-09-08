#![allow(non_local_definitions)]

use abi_stable::{sabi_trait, std_types::RSlice, RMut, StableAbi};

use super::{
    descriptor::{FamilyError, FamilyResult, FfiFamilyResult},
    validate_dimensions,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FrameAlpha {
    Opaque,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub enum FrameFormat {
    Rgba8Srgb { alpha: FrameAlpha },
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, StableAbi)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: FrameFormat,
}

impl FrameInfo {
    pub fn validate(&self) -> FamilyResult<()> {
        validate_dimensions(self.width, self.height)?;
        let row = self.width.checked_mul(4).ok_or_else(|| {
            FamilyError::invalid("ASTRA_EMU_FAMILY_FRAME_SIZE", "frame row size overflows")
        })?;
        if self.stride < row {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_STRIDE",
                "frame stride is smaller than the RGBA row",
            ));
        }
        self.required_bytes().map(|_| ()).ok_or_else(|| {
            FamilyError::invalid("ASTRA_EMU_FAMILY_FRAME_SIZE", "frame size overflows")
        })
    }

    pub fn required_bytes(&self) -> Option<usize> {
        u64::from(self.stride)
            .checked_mul(u64::from(self.height))
            .and_then(|bytes| usize::try_from(bytes).ok())
    }
}

/// A frame view is valid only for the synchronous `FrameConsumer::accept`
/// call. The lifetime is carried by the slice and is never manufactured as
/// `'static`; a host must copy pixels before returning from the callback.
#[repr(C)]
#[derive(Debug, Clone, Copy, StableAbi)]
pub struct FrameView<'a> {
    pub info: FrameInfo,
    pub pixels: RSlice<'a, u8>,
}

impl<'a> FrameView<'a> {
    pub fn from_slice(pixels: &'a [u8], info: FrameInfo) -> FamilyResult<Self> {
        info.validate()?;
        let required = info.required_bytes().ok_or_else(|| {
            FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_SIZE",
                "frame size does not fit host usize",
            )
        })?;
        if pixels.len() < required {
            return Err(FamilyError::invalid(
                "ASTRA_EMU_FAMILY_FRAME_BYTES",
                "frame pixels are shorter than stride times height",
            ));
        }
        Ok(Self {
            info,
            pixels: RSlice::from_slice(pixels),
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        self.pixels.as_slice()
    }
}

#[sabi_trait]
pub trait FrameConsumer {
    fn accept(&mut self, frame: FrameView<'_>) -> FfiFamilyResult<()>;
}

/// A frame consumer borrowed for one `FamilyModule::frame` call.
///
/// `RMut` keeps the visitor's borrow in the generated ABI object. The family
/// can invoke it synchronously, but cannot retain the callback after the
/// borrowed lifetime ends.
pub type FrameConsumerRef<'a> = FrameConsumer_TO<'a, RMut<'a, ()>>;

pub trait FrameVisitor {
    fn accept(&mut self, frame: FrameView<'_>) -> FamilyResult<()>;
}
