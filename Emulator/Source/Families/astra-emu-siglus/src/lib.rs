//! Registration/build bridge to the pinned native Siglus embedded adapter.
#[path = "../../../../ThirdParty/siglus_rs/astra/embedded/mod.rs"]
mod embedded;
pub use embedded::query;

/// Dynamic and static registration return the same native C function table.
///
/// # Safety
/// A non-null `out` must point to writable storage for one core-table pointer.
#[cfg(feature = "dynamic")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn astra_emu_query(
    version: u32,
    out: *mut *const astra_emu_abi::CoreApi,
) -> astra_emu_abi::Status {
    unsafe { query(version, out) }
}

#[cfg(all(test, feature = "dynamic"))]
#[test]
fn dynamic_and_static_registration_share_the_native_table() {
    let mut dynamic = std::ptr::null();
    let mut registered = std::ptr::null();
    assert_eq!(
        unsafe { astra_emu_query(astra_emu_abi::ABI_VERSION, &mut dynamic) },
        astra_emu_abi::OK,
    );
    assert_eq!(
        unsafe { query(astra_emu_abi::ABI_VERSION, &mut registered) },
        astra_emu_abi::OK,
    );
    assert!(!dynamic.is_null());
    assert_eq!(dynamic, registered);
}
