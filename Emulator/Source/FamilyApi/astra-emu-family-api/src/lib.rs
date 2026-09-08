//! Stable ABI for an AstraEMU family running its own complete game runtime.
//!
//! The API deliberately contains no VFS, package, RuntimeWorld, renderer
//! backend, device handle, or product save type. A family receives a game
//! directory, consumes ordered input/window events, owns execution and
//! native persistence, and exposes one borrowed CPU frame plus an optional
//! fixed-format audio sink.

mod ffi;

pub use ffi::*;

/// Machine-readable schema name for this independent host contract.
pub const FAMILY_API_SCHEMA: &str = "astra.emu.independent_family_api.v1";
/// ABI identity is intentionally distinct from every historical family ABI.
pub const FAMILY_ABI_FINGERPRINT: &str = "astra.emu.independent_family_abi.v1";
