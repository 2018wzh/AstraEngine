//! Artemis native embedded C ABI. VM, GPU, media, save and scheduling stay native.
mod audio;
mod error;
mod events;
mod ffi;
mod media;
mod platform;
mod provider;
mod session;
pub use ffi::query as artemis_query;
