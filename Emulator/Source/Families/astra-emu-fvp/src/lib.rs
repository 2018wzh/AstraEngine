//! RFVP's independent AstraEMU family provider.

mod audio;
mod error;
mod events;
mod filesystem;
mod font_bindings;
mod provider;
mod renderer;
mod video;

#[cfg(feature = "dynamic-plugin-export")]
mod ffi;

pub use provider::{create_fvp_provider, fvp_descriptor, FvpProvider};

#[cfg(feature = "dynamic-plugin-export")]
pub use ffi::astra_fvp_family_root_module;

/// Upstream RFVP revision used for behavioral reference.
pub const RFVP_REFERENCE_REVISION: &str = "3b5ea6c96a925c12f95aef8554905e8fecbc77c3";

/// Revision of the vendored hosted RFVP fork.
pub const RFVP_HOSTED_FORK_REVISION: &str = env!("ASTRA_FVP_HOSTED_FORK_REVISION");
