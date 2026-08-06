//! Stable, renderer-neutral contract between AstraEMU and legacy family providers.

mod ffi;
mod ffi_wire;
mod input_key;
mod provider;
mod scheduler;

pub use ffi::*;
pub use ffi_wire::*;
pub use input_key::*;
pub use provider::*;
pub use scheduler::*;

pub const LEGACY_FAMILY_API_SCHEMA: &str = "astra.emu.family_api.v2";
pub const LEGACY_EFFECT_SCHEMA: &str = "astra.emu.legacy_effect.v2";
pub const LEGACY_SNAPSHOT_SCHEMA: &str = "astra.emu.legacy_snapshot.v2";
