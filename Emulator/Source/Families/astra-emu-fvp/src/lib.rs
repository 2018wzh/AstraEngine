//! RFVP's independent AstraEMU family provider.

mod audio;
mod embedding;
mod error;
mod events;
mod filesystem;
mod font_bindings;
mod gpu;
mod provider;
mod renderer;

#[cfg(feature = "dynamic-plugin-export")]
mod ffi;

pub use provider::{create_fvp_provider, fvp_descriptor, FvpProvider};

#[cfg(feature = "dynamic-plugin-export")]
pub use ffi::astra_fvp_family_root_module;

/// Upstream RFVP revision used for behavioral reference.
pub const RFVP_REFERENCE_REVISION: &str = "af557db750c7833274f22a6d82b0dc33cdea2f85";

/// Revision of the vendored hosted RFVP fork.
pub const RFVP_HOSTED_FORK_REVISION: &str = env!("ASTRA_FVP_HOSTED_FORK_REVISION");
