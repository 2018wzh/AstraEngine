//! AstraEMU Kirikiri family plugin.
//!
//! Wraps the vendored Kirikiri core (Kirikiroid2 lineage) behind the
//! independent Family ABI: CPU RGBA frames pulled after each tick, mixed PCM
//! pushed from the engine audio thread, and Windows virtual-key input.
//!
//! The `engine` feature compiles the vendored core through CMake and links
//! it; without it the crate keeps the ABI surface but every session open
//! reports a diagnostic instead of calling the engine.

#[cfg(feature = "engine")]
pub mod audio;
pub mod ffi;
pub mod provider;

#[cfg(feature = "engine")]
pub mod engine_ffi;
#[cfg(feature = "engine")]
pub mod events;
#[cfg(feature = "engine")]
pub mod session;

pub use provider::{krkr_descriptor, KrkrProvider};
