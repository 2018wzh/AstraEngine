//! The original Kirikiroid2 VM, GL renderer and OpenAL scheduler live in the
//! linked native module. The Manager uses only the shared C query table.
use astra_emu_abi::{CoreApi, Query, Status};

unsafe extern "C" {
    #[link_name = "astra_emu_krkr_query"]
    pub fn query(version: u32, output: *mut *const CoreApi) -> Status;
}

pub const QUERY: Query = query;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linked_native_query_uses_current_c_abi() {
        let mut api = std::ptr::null();
        unsafe {
            assert_eq!(
                query(astra_emu_abi::ABI_VERSION, &mut api),
                astra_emu_abi::OK
            );
            assert!(!api.is_null());
            assert_eq!((*api).header.version, astra_emu_abi::ABI_VERSION);
            assert!((*api).validate().is_ok());
            let mut descriptor = std::mem::MaybeUninit::<astra_emu_abi::Descriptor>::zeroed();
            let mut error = astra_emu_abi::Error::default();
            assert_eq!(
                ((*api).describe.unwrap())(descriptor.as_mut_ptr(), &mut error),
                astra_emu_abi::OK
            );
            let descriptor = descriptor.assume_init();
            assert!(descriptor.validate().is_ok());
            assert_eq!(descriptor.display_name.as_slice(64), Ok(&b"KiriKiri2"[..]));
            assert_eq!(descriptor.family_name.as_slice(64), Ok(&b"KiriKiri"[..]));
            assert_eq!(descriptor.capabilities, astra_emu_abi::CAP_CPU_SURFACE);
            assert_eq!(
                descriptor.render,
                astra_emu_abi::RenderExtentSupport::default()
            );
            assert_eq!(descriptor.configuration_count, 0);
            assert_eq!(query(0, &mut api), astra_emu_abi::UNSUPPORTED);
            assert!(api.is_null());
        }
    }
}
