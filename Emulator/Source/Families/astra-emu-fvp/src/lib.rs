//! FVP legacy family provider. This crate is licensed under MPL-2.0.

mod archive;
mod ffi;
mod hcb;
mod media_decode;
mod provider;

pub use archive::*;
pub use hcb::*;
pub use media_decode::*;
pub use provider::*;

pub const RFVP_REFERENCE_REVISION: &str = "657747252eb0d2c5fb4a340695ce6906c2d45133";
pub const FVP_FAMILY_ID: &str = "fvp";
pub const FVP_PROVIDER_ID: &str = "astra.emu.family.fvp";

pub fn release_syscall_ids() -> Vec<String> {
    let mut ids = rfvp::subsystem::components::syscalls::generated::SYSCALL_SPECS
        .iter()
        .filter(|spec| spec.name != "BREAKPOINT")
        .map(|spec| spec.name.to_owned())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

pub fn release_syscall_catalog_hash() -> astra_core::Hash256 {
    let ids = release_syscall_ids();
    astra_core::Hash256::from_sha256(format!("{}\n", ids.join("\n")).as_bytes())
}

pub fn release_opcode_ids() -> Vec<String> {
    (0_u8..=0x27)
        .map(|opcode| format!("0x{opcode:02x}"))
        .collect()
}

#[cfg(test)]
mod catalog_tests {
    #[test]
    fn release_catalog_identity_is_fixed_to_the_reviewed_rfvp_surface() {
        assert_eq!(super::release_syscall_ids().len(), 148);
        assert_eq!(
            super::release_syscall_catalog_hash().to_string(),
            "sha256:c53cb15a5a1fe29d11c8cf8b0cf14a20c2dab7d85dace74f3b35345d5aa97d6a"
        );
        assert_eq!(super::release_opcode_ids().len(), 0x28);
    }
}
