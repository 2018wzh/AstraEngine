//! Build and registration entry for RFVP's native embedded adapter.
//! Runtime, C table, CPU leases and lifecycle live in the pinned RFVP fork.

/// Register the same C protocol in static and dynamic builds.
///
/// # Safety
/// `output` must be writable storage for one API-table pointer.
#[cfg_attr(feature = "dynamic-plugin-export", no_mangle)]
pub unsafe extern "C" fn astra_emu_query(
    version: u32,
    output: *mut *const astra_emu_abi::CoreApi,
) -> astra_emu_abi::Status {
    unsafe { rfvp_astra_embedded::astra_emu_query(version, output) }
}
