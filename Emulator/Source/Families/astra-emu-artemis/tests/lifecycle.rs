use astra_emu_abi::*;
use astra_emu_artemis::artemis_query;
use std::ptr;
unsafe extern "C" fn emit(_: *mut std::ffi::c_void, record: *const LogRecord) {
    let record = unsafe { &*record };
    eprintln!(
        "{}",
        String::from_utf8_lossy(unsafe { record.message.as_slice(16384).unwrap() })
    );
}
unsafe extern "C" fn flush(_: *mut std::ffi::c_void) {}
unsafe extern "C" fn cancelled(_: *mut std::ffi::c_void, _: u64) -> u32 {
    0
}
unsafe extern "C" fn unavailable(
    _: *mut std::ffi::c_void,
    _: *const WindowRequest,
    _: *mut WindowResult,
    _: *mut Error,
) -> Status {
    UNSUPPORTED
}
unsafe extern "C" fn invalidate(_: *mut std::ffi::c_void, _: u64, _: u64) {}
fn api() -> &'static CoreApi {
    let mut raw = ptr::null();
    assert_eq!(unsafe { artemis_query(ABI_VERSION, &mut raw) }, OK);
    let api = unsafe { &*raw };
    api.validate().unwrap();
    let diagnostics = Diagnostics {
        header: Header::of::<Diagnostics>(),
        context: ptr::null_mut(),
        emit: Some(emit),
        flush: Some(flush),
    };
    assert_eq!(
        unsafe { api.initialize.unwrap()(&diagnostics, &mut Error::default()) },
        OK
    );
    api
}
fn open<'a>(
    api: &CoreApi,
    root: &std::path::Path,
    epoch: u64,
    error: &'a mut Error,
    window_host: WindowService,
    render_extent: Extent,
) -> Result<OpenResult, &'a Error> {
    let path = root.to_str().unwrap();
    let request = OpenRequest {
        header: Header::of::<OpenRequest>(),
        game_path: Bytes::new(path.as_bytes()),
        session_epoch: epoch,
        configuration: ptr::null(),
        configuration_count: 0,
        window: WindowState {
            drawable_width: 64,
            drawable_height: 48,
            scale_factor: 1.0,
            focused: 1,
            suspended: 0,
        },
        host: HostServices {
            header: Header::of::<HostServices>(),
            text: TextService {
                context: ptr::null_mut(),
                request: None,
                acknowledge: None,
                invalidate: None,
            },
            window: window_host,
            cancellation: CancellationService {
                context: ptr::null_mut(),
                is_cancelled: Some(cancelled),
            },
        },
        module_owner: ptr::null_mut(),
        render_extent,
    };
    let mut output = std::mem::MaybeUninit::<OpenResult>::uninit();
    let status = unsafe { api.open.unwrap()(&request, output.as_mut_ptr(), error) };
    let output = unsafe { output.assume_init() };
    if status == OK {
        Ok(output)
    } else {
        assert!(output.session.is_null());
        Err(error)
    }
}
#[test]
fn malformed_archive_releases_native_session_lease() {
    let api = api();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("data.pfs"), b"bad").unwrap();
    for epoch in 1..=2 {
        let mut error = Error::default();
        let failure = open(
            api,
            root.path(),
            epoch,
            &mut error,
            WindowService {
                context: ptr::null_mut(),
                request: Some(unavailable),
                invalidate: Some(invalidate),
            },
            Extent::default(),
        )
        .unwrap_err();
        assert_eq!(
            &failure.code[..failure.code_len as usize],
            b"ASTRA_EMU_ARTEMIS_PROJECT_INI"
        );
    }
}
#[test]
fn descriptor_declares_v3_names_and_file_fields() {
    let api = api();
    let mut descriptor = std::mem::MaybeUninit::<Descriptor>::uninit();
    assert_eq!(
        unsafe { api.describe.unwrap()(descriptor.as_mut_ptr(), &mut Error::default()) },
        OK
    );
    let descriptor = unsafe { descriptor.assume_init() };
    unsafe { descriptor.validate() }.unwrap();
    assert_eq!(
        unsafe { descriptor.display_name.text(64) }.unwrap(),
        "Art3m1s"
    );
    assert_eq!(
        unsafe { descriptor.family_name.text(64) }.unwrap(),
        "Artemis"
    );
    assert_eq!(descriptor.capabilities & CAP_RENDER_EXTENT, 0);
    let fields = unsafe {
        std::slice::from_raw_parts(descriptor.configuration, descriptor.configuration_count)
    };
    unsafe { validate_config(fields, &[]) }.unwrap();
    for field in fields {
        assert_eq!(
            field.flags,
            CONFIG_FLAG_FILE_PATH | CONFIG_FLAG_GAME_RELATIVE
        );
        assert_eq!(unsafe { field.group.text(64) }.unwrap(), "文件");
        assert!(!unsafe { field.description.text(512) }.unwrap().is_empty());
    }
}
#[test]
fn non_default_render_extent_is_rejected() {
    let api = api();
    let root = tempfile::tempdir().unwrap();
    let mut error = Error::default();
    let failure = open(
        api,
        root.path(),
        1,
        &mut error,
        WindowService {
            context: ptr::null_mut(),
            request: Some(unavailable),
            invalidate: Some(invalidate),
        },
        Extent {
            width: 1920,
            height: 1080,
        },
    )
    .unwrap_err();
    assert_eq!(
        &failure.code[..failure.code_len as usize],
        b"ASTRA_EMU_ARTEMIS_RENDER_EXTENT"
    );
}
