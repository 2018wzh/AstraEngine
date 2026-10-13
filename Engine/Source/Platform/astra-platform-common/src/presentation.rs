use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use astra_platform::{
    CapturedFrame, PlatformError, PlatformErrorCode, PlatformEventKind, RgbaFrame, SceneFrame,
};

use crate::glyph_atlas::WgpuGlyphAtlasRenderer;

mod wgpu_presentation_core;
use wgpu_presentation_core::UploadFrame;
pub use wgpu_presentation_core::WgpuPresentationCore;
pub use wgpu_presentation_core::WgpuReadback;
mod new;

mod is_device_lost;
pub(crate) use is_device_lost::install_device_lost_callback;
use is_device_lost::invalid;
use is_device_lost::pipeline;
use is_device_lost::unavailable;
use is_device_lost::upload_frame;
pub use is_device_lost::wgpu_device_recovery_events;
pub use is_device_lost::wgpu_recovery_events;
