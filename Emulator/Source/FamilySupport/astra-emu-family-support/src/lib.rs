//! Host-side support services shared by AstraEMU family adapters.

mod cache;
mod decoder;
mod extract;
#[cfg(target_os = "linux")]
mod fuse;
mod private_profile;
mod profile;
mod registry;
#[cfg(test)]
mod test_support;
mod verify;
mod viewer;

pub use cache::*;
pub use decoder::*;
pub use extract::*;
#[cfg(target_os = "linux")]
pub use fuse::*;
pub use private_profile::*;
pub use profile::*;
pub use registry::*;
pub use verify::*;
pub use viewer::*;

pub const LEGACY_FAMILY_SUPPORT_SCHEMA: &str = "astra.emu.family_support.v1";
