//! Thin dylib boundary for RFVP's Astra Family ABI v9 provider.

#[cfg(feature = "dynamic-plugin-export")]
mod ffi;

pub use rfvp_astra_provider::{
    create_static_fvp_provider, release_opcode_ids, release_syscall_catalog_hash,
    release_syscall_ids,
};

pub const RFVP_REFERENCE_REVISION: &str = rfvp_astra_provider::RFVP_REFERENCE_REVISION;
pub const RFVP_HOSTED_FORK_REVISION: &str = "44be68c92a4b7c8c42837a32197ddeffc7160411";
