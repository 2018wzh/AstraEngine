//! Host-side support services shared by AstraEMU family adapters.

mod audio_service;
mod extract;
#[cfg(target_os = "linux")]
mod fuse;
mod permissions;
mod private_writable;
mod profile;
mod registry;
mod runtime_vfs;
mod surface_host;
mod surface_pixels;
#[cfg(test)]
mod test_support;
mod verify;
mod viewer;

pub use audio_service::*;
pub use extract::*;
#[cfg(target_os = "linux")]
pub use fuse::*;
pub use permissions::*;
pub use private_writable::*;
pub use profile::*;
pub use registry::*;
pub use runtime_vfs::*;
pub use surface_host::*;
pub use surface_pixels::*;
pub use verify::*;
pub use viewer::*;

pub const LEGACY_FAMILY_SUPPORT_SCHEMA: &str = "astra.emu.family_support.v1";
