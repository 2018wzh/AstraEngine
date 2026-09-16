//! Stable ABI for an AstraEMU family running its own complete game runtime.
//!
//! The API deliberately contains no VFS, package, RuntimeWorld, renderer
//! backend, device handle, or product save type. A family receives a game
//! directory, consumes ordered input/window events, owns execution and
//! native persistence, and exposes one borrowed CPU frame plus an optional
//! fixed-format audio sink.

mod diagnostics;
mod ffi;
pub use diagnostics::*;
#[cfg(feature = "diagnostic-bridge")]
pub mod diagnostic_bridge;
#[cfg(feature = "diagnostic-bridge")]
mod provider_module;
#[cfg(feature = "diagnostic-bridge")]
pub use provider_module::ProviderModule;

pub use ffi::*;

/// Machine-readable schema name for this independent host contract.
pub const FAMILY_API_SCHEMA: &str = "astra.emu.independent_family_api.v3";
/// ABI identity is intentionally distinct from every historical family ABI.
pub const FAMILY_ABI_FINGERPRINT: &str = "astra.emu.independent_family_abi.v3";
