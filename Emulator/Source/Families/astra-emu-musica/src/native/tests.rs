use super::*;
use std::sync::atomic::AtomicU64;

fn handle_fixture() -> *mut c_void {
    let info = abi::SurfaceInfo {
        kind: abi::SURFACE_CPU,
        pixel_format: abi::PIXEL_RGBA8,
        color: abi::COLOR_SRGB,
        alpha: abi::ALPHA_IGNORED,
        rows: abi::ROWS_TOP_DOWN,
        backend: 0,
        logical: abi::Extent {
            width: 2,
            height: 1,
        },
        raster: abi::Extent {
            width: 2,
            height: 1,
        },
        stride: 8,
    };
    let services = Arc::new(text::ServiceContext {
        session: 19,
        epoch: AtomicU64::new(1),
        cancelled: AtomicBool::new(false),
        waiting: AtomicBool::new(false),
        text: None,
        window: None,
        host_cancel: None,
    });
    let canvas = astra_emu_sdk::StageCanvas::new(
        astra_media_core::Extent2D {
            width: 2,
            height: 1,
        },
        astra_media_core::Extent2D {
            width: 2,
            height: 1,
        },
    )
    .unwrap();
    let shared = Arc::new(Shared {
        services,
        audio_stop: Arc::new(AtomicBool::new(false)),
        canvas,
        frame: Mutex::new(Published {
            info,
            pixels: Arc::from([1, 2, 3, 255, 4, 5, 6, 255]),
            generation: 1,
            timestamp_ns: 7,
        }),
        state: Mutex::new(abi::SessionState {
            state: abi::SESSION_RUNNING,
            flags: 0,
            context_epoch: 1,
            output_epoch: 1,
            error: abi::Error::default(),
        }),
        leases: Arc::new(AtomicUsize::new(0)),
    });
    let (commands, _) = sync_channel(1);
    Box::into_raw(Box::new(Handle {
        shared,
        commands,
        worker: None,
    }))
    .cast()
}
#[test]
fn query_has_one_native_table_and_rejects_other_versions() {
    let mut table = std::ptr::null();
    assert_eq!(
        unsafe { musica_query(abi::ABI_VERSION, &mut table) },
        abi::OK
    );
    unsafe { &*table }.validate().unwrap();
    assert_eq!(
        unsafe { musica_query(abi::ABI_VERSION + 1, &mut table) },
        abi::UNSUPPORTED
    );
    assert!(table.is_null());
}
#[test]
fn cpu_leases_are_bounded_and_outlive_the_session() {
    let session = handle_fixture();
    let mut leases = Vec::new();
    for _ in 0..3 {
        let mut raw = std::mem::MaybeUninit::uninit();
        assert_eq!(
            unsafe {
                frame(
                    session,
                    abi::FrameIdentity::default(),
                    raw.as_mut_ptr(),
                    std::ptr::null_mut(),
                )
            },
            abi::OK
        );
        leases.push(unsafe { abi::CpuFrame::adopt(raw.assume_init(), 19) }.unwrap());
    }
    let mut pending = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe {
            frame(
                session,
                abi::FrameIdentity::default(),
                pending.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        },
        abi::OK
    );
    let pending = unsafe { pending.assume_init() };
    assert_eq!(pending.state, abi::FRAME_PENDING);
    assert!(pending.cpu.owner.is_null());
    let identity = leases[0].identity();
    let mut unchanged = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe {
            frame(
                session,
                identity,
                unchanged.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        },
        abi::OK
    );
    assert_eq!(
        unsafe { unchanged.assume_init() }.state,
        abi::FRAME_UNCHANGED
    );
    assert_eq!(unsafe { close(session, std::ptr::null_mut()) }, abi::OK);
    for lease in &leases {
        assert_eq!(lease.pixels(), [1, 2, 3, 255, 4, 5, 6, 255]);
    }
    std::thread::spawn(move || drop(leases)).join().unwrap();
}

#[test]
fn cancellation_preserves_failed_state_and_disables_retry() {
    let session = handle_fixture();
    let handle = unsafe { handle(session) }.unwrap_or_else(|_| panic!("valid fixture handle"));
    {
        let mut state = handle
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.state = abi::SESSION_FAILED;
        state.flags = 0;
        state
            .error
            .set("ASTRA_EMU_MUSICA_WORKER_PANIC", "native scheduler panicked");
    }
    handle.shared.services.cancel();
    let mut snapshot = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe { state(session, snapshot.as_mut_ptr(), std::ptr::null_mut()) },
        abi::OK
    );
    let snapshot = unsafe { snapshot.assume_init() };
    assert_eq!(snapshot.state, abi::SESSION_FAILED);
    assert_eq!(snapshot.flags, 0);
    snapshot.validate().unwrap();
    assert_eq!(
        unsafe { control(session, abi::CONTROL_RETRY, std::ptr::null_mut()) },
        abi::CANCELLED
    );
    assert_eq!(unsafe { close(session, std::ptr::null_mut()) }, abi::OK);
}
