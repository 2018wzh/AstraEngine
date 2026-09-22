//! Artemis native Vulkan Family adapter.
mod audio;
mod error;
mod events;
mod ffi;
mod media;
mod provider;
mod session;
pub use provider::{artemis_descriptor, ArtemisProvider};
