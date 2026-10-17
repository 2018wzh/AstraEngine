mod common;
use astra_emu_abi as abi;
use std::{mem::MaybeUninit, sync::atomic::AtomicBool};

#[test]
fn query_descriptor_config_and_cancelled_open() {
    let api = common::api();
    let mut pointer = api as *const _;
    assert_eq!(unsafe { astra_emu_siglus::query(abi::ABI_VERSION + 1, &mut pointer) }, abi::UNSUPPORTED);
    assert!(pointer.is_null());
    let mut descriptor = MaybeUninit::uninit(); let mut error = abi::Error::default();
    common::success(unsafe { api.describe.unwrap()(descriptor.as_mut_ptr(), &mut error) }, &error);
    let descriptor: abi::Descriptor = unsafe { descriptor.assume_init() };
    assert_eq!(unsafe { descriptor.family_id.text(128) }.unwrap(), "siglus");
    assert_eq!(descriptor.capabilities, abi::CAP_CPU_SURFACE);
    assert_eq!(descriptor.configuration_count, 0);
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().to_string_lossy().into_owned();
    let cancelled = AtomicBool::new(true);
    let mut request = abi::OpenRequest { header: abi::Header::of::<abi::OpenRequest>(), game_path: abi::Bytes::new(path.as_bytes()), session_epoch: 9, configuration: std::ptr::null(), configuration_count: 0, window: common::window(), host: common::services(&cancelled) };
    let mut opened: abi::OpenResult = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { api.open.unwrap()(&request, &mut opened, &mut error) }, abi::CANCELLED);
    assert!(opened.session.is_null());
    let config = abi::ConfigEntry { id: abi::Bytes::new(b"render_width"), value: abi::ConfigValue { kind: abi::VALUE_INTEGER, reserved: 0, integer: 1920, number: 0.0, text: abi::Bytes::EMPTY } };
    request.configuration = &config; request.configuration_count = 1;
    assert_eq!(unsafe { api.open.unwrap()(&request, &mut opened, &mut error) }, abi::INVALID_ARGUMENT);
    assert!(opened.session.is_null());
    assert_eq!(unsafe { api.close.unwrap()(std::ptr::null_mut(), &mut error) }, abi::INVALID_ARGUMENT);
    unsafe { api.cancel.unwrap()(std::ptr::null_mut()); }
    let mut extension = std::ptr::dangling::<std::ffi::c_void>();
    assert_eq!(unsafe { api.extension.unwrap()(abi::Bytes::new(b"gpu"), 1, &mut extension) }, abi::UNSUPPORTED);
    assert!(extension.is_null());
}
