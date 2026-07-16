//! Stable, renderer-neutral contract between AstraEMU and legacy family providers.

mod ffi;
mod provider;
mod scheduler;
mod vfs;

pub use ffi::*;
pub use provider::*;
pub use scheduler::*;
pub use vfs::*;

pub const LEGACY_FAMILY_API_SCHEMA: &str = "astra.emu.family_api.v1";
pub const LEGACY_EFFECT_SCHEMA: &str = "astra.emu.legacy_effect.v1";
pub const LEGACY_SNAPSHOT_SCHEMA: &str = "astra.emu.legacy_snapshot.v1";
