//! AstraEMU SiglusEngine family plugin.
//!
//! Wraps the vendored Siglus runtime (`siglus_scene_vm`, xmoezzz/siglus_rs
//! lineage) behind the independent Family ABI: CPU RGBA frames read back from
//! an offscreen wgpu target after each engine step, mixed PCM pushed from the
//! kira tap worker, and engine input injected from family events.
//!
//! The engine is in-process Rust, so unlike the Kirikiri family there is no
//! FFI boundary; the vendored `astra-hosted` feature supplies the offscreen
//! renderer and the PCM tap audio backend.

pub mod audio;
pub mod error;
pub mod events;
mod ffi;
pub mod provider;
pub mod session;

pub use provider::{siglus_descriptor, SiglusProvider};
