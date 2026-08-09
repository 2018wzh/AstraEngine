//! Siglus hosted family integration. The current surface binds Family ABI v8
//! host services to the pinned `siglus_rs` hosted ports. Runtime provider
//! export remains disabled until save/restore and the complete typed delta are
//! available from the fork.

mod host_ports;

pub use host_ports::*;

pub const SIGLUS_UPSTREAM_REVISION: &str = "a8a3376049f47a141a673a49f15ab7de8746e1e1";
pub const SIGLUS_HOSTED_FORK_REVISION: &str = "0351787f0a2163bf32e0fcb8e18106dff7a4e3a1";
pub const SIGLUS_FAMILY_ID: &str = "siglus";
pub const SIGLUS_PROVIDER_ID: &str = "astra.emu.family.siglus";
