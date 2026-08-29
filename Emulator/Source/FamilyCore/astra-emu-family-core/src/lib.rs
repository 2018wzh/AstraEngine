//! In-process contracts shared by AstraEMU family implementations and hosts.

mod error;
mod factory;
mod vfs;

pub use error::*;
pub use factory::*;
pub use vfs::*;

pub const LEGACY_FAMILY_CORE_SCHEMA: &str = "astra.emu.family_core.v1";
