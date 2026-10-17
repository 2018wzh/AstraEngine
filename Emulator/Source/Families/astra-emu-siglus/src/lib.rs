//! Registration/build bridge to the pinned native Siglus embedded adapter.
#[path = "../../../../ThirdParty/siglus_rs/astra/embedded/mod.rs"]
mod embedded;
pub use embedded::query;

/// Dynamic and static registration return the same native C function table.
#[cfg(feature = "dynamic")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn astra_emu_query(
    version: u32,
    out: *mut *const astra_emu_abi::CoreApi,
) -> astra_emu_abi::Status {
    unsafe { query(version, out) }
}
