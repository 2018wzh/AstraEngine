use astra_emu_abi::*;
use astra_emu_artemis::artemis_query;
use parking_lot::Mutex;
use std::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
static SERIAL: Mutex<()> = Mutex::new(());
static RESTORE_CLOSING: AtomicBool = AtomicBool::new(false);
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
unsafe extern "C" fn restore_invalidate(_: *mut std::ffi::c_void, _: u64, context: u64) {
    if context != 1 {
        RESTORE_CLOSING.store(true, Ordering::Release);
        return;
    }
    // Hold the restore callback until close cancels both native workers.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !RESTORE_CLOSING.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    // A slow Host invalidation must not turn normal cancellation into a queue error.
    std::thread::sleep(Duration::from_millis(50));
}
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
    invalidate: unsafe extern "C" fn(*mut std::ffi::c_void, u64, u64),
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
            window: WindowService {
                context: ptr::null_mut(),
                request: Some(unavailable),
                invalidate: Some(invalidate),
            },
            cancellation: CancellationService {
                context: ptr::null_mut(),
                is_cancelled: Some(cancelled),
            },
        },
        module_owner: ptr::null_mut(),
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
// Public-domain miniature archive generated from the documented PF6 layout.
fn archive(files: &[(&[u8], &[u8])]) -> Vec<u8> {
    let entries: usize = files.iter().map(|(n, _)| 16 + n.len()).sum();
    let index = 4 + entries + 4 + 8 * (files.len() + 1) + 4;
    let mut bytes = b"pf6".to_vec();
    bytes.extend((index as u32).to_le_bytes());
    bytes.extend((files.len() as u32).to_le_bytes());
    let mut offset = 7 + index;
    for (name, data) in files {
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(*name);
        bytes.extend([0; 4]);
        bytes.extend((offset as u32).to_le_bytes());
        bytes.extend((data.len() as u32).to_le_bytes());
        offset += data.len();
    }
    bytes.extend(((files.len() + 1) as u32).to_le_bytes());
    bytes.extend(vec![0; 8 * (files.len() + 1)]);
    bytes.extend(((11 + entries) as u32).to_le_bytes());
    for (_, data) in files {
        bytes.extend(*data);
    }
    bytes
}
fn frame(api: &CoreApi, session: Session, epoch: u64) -> CpuFrame {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut output = std::mem::MaybeUninit::<FrameSurface>::uninit();
        let mut error = Error::default();
        assert_eq!(
            unsafe {
                api.frame.unwrap()(
                    session,
                    FrameIdentity::default(),
                    output.as_mut_ptr(),
                    &mut error,
                )
            },
            OK,
            "{:?}",
            error
        );
        let output = unsafe { output.assume_init() };
        if output.state == FRAME_READY {
            return unsafe { CpuFrame::adopt(output, epoch) }.unwrap();
        }
        assert!(Instant::now() < deadline, "native frame timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn malformed_archive_releases_native_session_lease() {
    let _serial = SERIAL.lock();
    let api = api();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("data.pfs"), b"bad").unwrap();
    for epoch in 1..=2 {
        let mut error = Error::default();
        let failure = open(api, root.path(), epoch, &mut error, invalidate).unwrap_err();
        assert_eq!(
            &failure.code[..failure.code_len as usize],
            b"ASTRA_EMU_ARTEMIS_PROJECT_INI"
        );
    }
}
#[test]
#[ignore = "requires real hardware GPU and audio output device"]
fn native_gpu_device_pause_close_reopen_and_detached_lease() {
    let _serial = SERIAL.lock();
    let api = api();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("data.pfs"),
        archive(&[
            (
                b"system.ini",
                b"[WINDOWS]\nWIDTH=64\nHEIGHT=48\nBOOT=boot.iet\nCHARSET=UTF-8\n",
            ),
            (b"boot.iet", b"[stop]\n"),
        ]),
    )
    .unwrap();
    for epoch in 10..13 {
        let open = open(api, root.path(), epoch, &mut Error::default(), invalidate).unwrap();
        assert_eq!(
            open.surface.logical,
            Extent {
                width: 64,
                height: 48
            }
        );
        let baseline = frame(api, open.session, epoch);
        assert_eq!(
            unsafe { api.control.unwrap()(open.session, CONTROL_PAUSE, &mut Error::default()) },
            OK
        );
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            unsafe { api.control.unwrap()(open.session, CONTROL_RESUME, &mut Error::default()) },
            OK
        );
        unsafe { api.cancel.unwrap()(open.session) };
        let mut close_error = Error::default();
        assert_eq!(
            unsafe { api.close.unwrap()(open.session, &mut close_error) },
            OK,
            "{close_error:?}"
        );
        std::thread::spawn(move || {
            assert_eq!(baseline.pixels().len(), 64 * 48 * 4);
            drop(baseline);
        })
        .join()
        .unwrap();
    }
}
#[test]
#[ignore = "requires real hardware GPU and audio output device"]
fn native_save_restore_invalidates_output_context() {
    let _serial = SERIAL.lock();
    let api = api();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("data.pfs"),
        archive(&[
            (
                b"system.ini",
                b"[WINDOWS]\nWIDTH=64\nHEIGHT=48\nBOOT=boot.iet\nSAVEPATH=save\nCHARSET=UTF-8\n",
            ),
            (
                b"boot.iet",
                b"[save file=clock.dat]\n[wait input=1]\n[load file=clock.dat]\n[stop]\n",
            ),
        ]),
    )
    .unwrap();
    RESTORE_CLOSING.store(false, Ordering::Release);
    let open = open(
        api,
        root.path(),
        20,
        &mut Error::default(),
        restore_invalidate,
    )
    .unwrap();
    let before = frame(api, open.session, 20);
    let event = InputEvent {
        kind: INPUT_KEY,
        pressed: 1,
        repeat: 0,
        button: 0,
        physical_key: Bytes::new(b"Enter"),
        logical_key: Bytes::new(b"Enter"),
        x: 0.0,
        y: 0.0,
        displayed: before.identity(),
        window: WindowState {
            drawable_width: 64,
            drawable_height: 48,
            scale_factor: 1.0,
            focused: 1,
            suspended: 0,
        },
    };
    assert_eq!(
        unsafe { api.input.unwrap()(open.session, &event, 1, &mut Error::default()) },
        OK
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut state = std::mem::MaybeUninit::<SessionState>::uninit();
        assert_eq!(
            unsafe { api.state.unwrap()(open.session, state.as_mut_ptr(), &mut Error::default()) },
            OK
        );
        let state = unsafe { state.assume_init() };
        assert_ne!(state.state, SESSION_FAILED, "{:?}", state.error);
        if state.context_epoch > open.context_epoch {
            assert!(state.output_epoch > open.output_epoch);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native restore never invalidated context"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut close_error = Error::default();
    assert_eq!(
        unsafe { api.close.unwrap()(open.session, &mut close_error) },
        OK,
        "{close_error:?}"
    );
    assert_eq!(before.pixels().len(), 64 * 48 * 4);
}
#[test]
#[ignore = "requires a hardware native GPU adapter"]
fn undersized_native_capture_preserves_completed_target() {
    use art3m1s_core::{backend::BackendSelection, runtime::CoreRuntime};
    let _serial = SERIAL.lock();
    #[cfg(target_os = "macos")]
    let backend = BackendSelection::NativeMetal;
    #[cfg(not(target_os = "macos"))]
    let backend = BackendSelection::NativeVulkanHardware;
    let mut runtime = CoreRuntime::create(64, 48, backend).unwrap();
    assert!(runtime.try_advance_frame(16).unwrap());
    let size = runtime.pixel_buffer_size();
    let mut baseline = vec![0; size];
    assert_eq!(runtime.try_capture_frame_into(&mut baseline).unwrap(), size);
    let mut small = vec![0x7b; size - 1];
    assert!(runtime.try_capture_frame_into(&mut small).is_err());
    assert!(small.iter().all(|b| *b == 0x7b));
    let mut recovered = vec![0; size];
    runtime.try_capture_frame_into(&mut recovered).unwrap();
    assert_eq!(recovered, baseline);
}
