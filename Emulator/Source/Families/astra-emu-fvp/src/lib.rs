//! Thin dylib boundary for RFVP's Astra Family ABI v9 provider.

#[cfg(feature = "dynamic-plugin-export")]
mod ffi;

pub use rfvp_astra_provider::{
    create_static_fvp_provider, release_opcode_ids, release_syscall_ids,
};

pub const RFVP_REFERENCE_REVISION: &str = rfvp_astra_provider::RFVP_REFERENCE_REVISION;
pub const RFVP_HOSTED_FORK_REVISION: &str = env!("ASTRA_FVP_HOSTED_FORK_REVISION");
