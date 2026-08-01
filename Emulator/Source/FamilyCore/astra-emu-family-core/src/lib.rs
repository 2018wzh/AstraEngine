//! In-process contracts shared by AstraEMU family implementations and hosts.

mod decrypt;
mod error;
mod factory;
mod vfs;

pub use decrypt::*;
pub use error::*;
pub use factory::*;
pub use vfs::*;

pub const LEGACY_FAMILY_CORE_SCHEMA: &str = "astra.emu.family_core.v1";
