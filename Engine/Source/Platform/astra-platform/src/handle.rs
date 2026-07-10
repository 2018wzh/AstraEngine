use std::{fmt, num::NonZeroU32};

use crate::{PlatformError, PlatformErrorCode};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceHandle {
    slot: NonZeroU32,
    generation: NonZeroU32,
}

pub trait TypedHandle: Copy {
    fn from_resource(resource: ResourceHandle) -> Self;
    fn resource(self) -> ResourceHandle;
}

impl ResourceHandle {
    pub fn from_parts(slot: u32, generation: u32) -> Result<Self, PlatformError> {
        let slot = NonZeroU32::new(slot).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidHandle,
                "handle.create",
                "resource handle slot must be non-zero",
            )
        })?;
        let generation = NonZeroU32::new(generation).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidHandle,
                "handle.create",
                "resource handle generation must be non-zero",
            )
        })?;
        Ok(Self { slot, generation })
    }

    pub fn parts(self) -> (u32, u32) {
        (self.slot.get(), self.generation.get())
    }
}

impl fmt::Debug for ResourceHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceHandle")
            .field("slot", &self.slot)
            .field("generation", &self.generation)
            .finish()
    }
}

macro_rules! typed_handle {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(ResourceHandle);

        impl $name {
            pub fn from_parts(slot: u32, generation: u32) -> Result<Self, PlatformError> {
                ResourceHandle::from_parts(slot, generation).map(Self)
            }

            pub fn parts(self) -> (u32, u32) {
                self.0.parts()
            }

            pub fn resource(self) -> ResourceHandle {
                self.0
            }
        }

        impl From<ResourceHandle> for $name {
            fn from(value: ResourceHandle) -> Self {
                Self(value)
            }
        }

        impl TypedHandle for $name {
            fn from_resource(resource: ResourceHandle) -> Self {
                Self(resource)
            }

            fn resource(self) -> ResourceHandle {
                self.0
            }
        }
    };
}

typed_handle!(WindowHandle);
typed_handle!(SurfaceHandle);
typed_handle!(AudioOutputHandle);
typed_handle!(DecodeSessionHandle);
typed_handle!(MediaFrameHandle);
typed_handle!(SaveTransactionHandle);
typed_handle!(PackageSourceHandle);
