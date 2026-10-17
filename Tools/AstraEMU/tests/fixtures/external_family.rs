#![deny(unsafe_op_in_unsafe_fn)]
use astra_emu_abi::*;
use parking_lot::Mutex;
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
const fn bytes(value: &'static [u8]) -> Bytes {
    Bytes {
        data: value.as_ptr(),
        len: value.len(),
    }
}
const fn value(kind: u32, integer: i64) -> ConfigValue {
    ConfigValue {
        kind,
        integer,
        reserved: 0,
        number: 0.0,
        text: Bytes::EMPTY,
    }
}
const fn field(id: &'static [u8], kind: u32, integer: i64) -> ConfigField {
    ConfigField {
        id: bytes(id),
        label: bytes(id),
        group: Bytes::EMPTY,
        default_value: value(kind, integer),
        integer_min: 1,
        integer_max: 255,
        number_min: 0.0,
        number_max: 0.0,
        max_bytes: 0,
        reserved: 0,
        choices: std::ptr::null(),
        choice_count: 0,
    }
}
struct Schema([ConfigField; 3]);
// SAFETY: every nested pointer refers to immutable static literal storage.
unsafe impl Sync for Schema {}
static SCHEMA: Schema = Schema([
    field(b"intensity", VALUE_INTEGER, 32),
    field(b"request_title", VALUE_BOOL, 0),
    field(b"request_icon", VALUE_BOOL, 0),
]);
struct Formats([Bytes; 1]);
// SAFETY: points only to immutable static literal storage.
unsafe impl Sync for Formats {}
static FORMATS: Formats = Formats([bytes(b"public-fixture")]);
#[derive(Clone, Copy)]
struct Services(HostServices);
// SAFETY: the ABI requires Host callbacks to support the native worker; the
// context owner remains alive until failed open or joined close returns.
unsafe impl Send for Services {}
impl Services {
    fn cancelled(&self, epoch: u64) -> bool {
        unsafe {
            self.0.cancellation.is_cancelled.unwrap()(self.0.cancellation.context, epoch) != 0
        }
    }
    fn window(&self, epoch: u64, kind: u32) -> Result<(), Status> {
        let Some(request) = self.0.window.request else {
            return Err(UNSUPPORTED);
        };
        let pixels = [255u8; 4];
        let mut command: WindowRequest = unsafe { std::mem::zeroed() };
        command.session_epoch = epoch;
        command.context_epoch = 1;
        command.request_id = 1;
        command.kind = kind;
        command.text = bytes(b"Public fixture");
        if kind == WINDOW_ICON {
            command.width = 1;
            command.height = 1;
            command.pixels = Bytes::new(&pixels);
        }
        let mut output: WindowResult = unsafe { std::mem::zeroed() };
        let status = unsafe {
            request(
                self.0.window.context,
                &command,
                &mut output,
                std::ptr::null_mut(),
            )
        };
        if status == OK { Ok(()) } else { Err(status) }
    }
}
struct Image {
    pixels: Arc<[u8; 16]>,
    generation: u64,
}
struct Shared {
    image: Mutex<Image>,
    stopped: AtomicBool,
    ticks: AtomicUsize,
    pressed: AtomicBool,
    released: AtomicBool,
}
enum Command {
    Key(bool),
    Wake,
}
struct NativeSession {
    epoch: u64,
    shared: Arc<Shared>,
    commands: mpsc::SyncSender<Command>,
    worker: Option<JoinHandle<()>>,
}
fn surface_info() -> SurfaceInfo {
    SurfaceInfo {
        kind: SURFACE_CPU,
        pixel_format: PIXEL_RGBA8,
        color: COLOR_SRGB,
        alpha: ALPHA_OPAQUE,
        rows: ROWS_TOP_DOWN,
        backend: 0,
        logical: Extent {
            width: 2,
            height: 2,
        },
        raster: Extent {
            width: 2,
            height: 2,
        },
        stride: 8,
    }
}
fn boundary(error: *mut Error, operation: impl FnOnce() -> Result<(), Status>) -> Status {
    let status = match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => return OK,
        Ok(Err(status)) => status,
        Err(_) => PANICKED,
    };
    if !error.is_null() {
        let code = if status == UNSUPPORTED {
            "ASTRA_EMU_WINDOW_SERVICE_UNAVAILABLE"
        } else if status == INVALID_ARGUMENT {
            "EXTERNAL_CONFIG"
        } else {
            "EXTERNAL_INCOMPLETE_FLOW"
        };
        unsafe { (*error).set(code, "independent native core development regression") };
    }
    status
}
unsafe extern "C" fn initialize(sink: *const Diagnostics, error: *mut Error) -> Status {
    boundary(error, || {
        if sink.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe { diagnostic_bridge::install(*sink) }
    })
}
unsafe extern "C" fn describe(output: *mut Descriptor, error: *mut Error) -> Status {
    boundary(error, || {
        if output.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            output.write(Descriptor {
                header: Header::of::<Descriptor>(),
                family_id: bytes(b"external"),
                plugin_id: bytes(b"astra.emu.external.probe"),
                version: bytes(b"0.1.0"),
                capabilities: CAP_CPU_SURFACE,
                formats: FORMATS.0.as_ptr(),
                format_count: 1,
                configuration: SCHEMA.0.as_ptr(),
                configuration_count: SCHEMA.0.len(),
            });
        }
        Ok(())
    })
}
unsafe extern "C" fn probe(path: Bytes, output: *mut ProbeResult, error: *mut Error) -> Status {
    boundary(error, || {
        if output.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        let path = unsafe { path.text(4096)? };
        if !std::path::Path::new(path).join("public.fixture").is_file() {
            return Err(NO_MATCH);
        }
        unsafe {
            output.write(ProbeResult {
                game_id: bytes(b"public-fixture"),
                format: bytes(b"public-fixture"),
                confidence_permyriad: 10_000,
                reserved: 0,
            });
        }
        Ok(())
    })
}
fn run(
    shared: Arc<Shared>,
    rx: mpsc::Receiver<Command>,
    host: Services,
    epoch: u64,
    intensity: u8,
    initial: Arc<[u8; 16]>,
) {
    let mut pool = [initial, Arc::new([0; 16]), Arc::new([0; 16])];
    let mut dirty = false;
    let mut green = false;
    while !shared.stopped.load(Ordering::Acquire) && !host.cancelled(epoch) {
        match rx.recv_timeout(Duration::from_millis(2)) {
            Ok(Command::Key(pressed)) => {
                if pressed {
                    shared.pressed.store(true, Ordering::Release);
                    green = !green;
                    dirty = true;
                } else {
                    shared.released.store(true, Ordering::Release);
                }
            }
            Ok(Command::Wake) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        shared.ticks.fetch_add(1, Ordering::Release);
        if dirty && let Some(slot) = pool.iter_mut().find(|slot| Arc::strong_count(slot) == 1) {
            let pixels = Arc::get_mut(slot).expect("unleased slot is uniquely owned");
            let color = if green {
                [0, intensity, 0, 255]
            } else {
                [intensity, 0, 0, 255]
            };
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&color);
            }
            let mut image = shared.image.lock();
            image.pixels = slot.clone();
            image.generation += 1;
            dirty = false;
        }
    }
}
unsafe extern "C" fn open(
    request: *const OpenRequest,
    output: *mut OpenResult,
    error: *mut Error,
) -> Status {
    if !output.is_null() {
        unsafe { (*output).session = std::ptr::null_mut() };
    }
    boundary(error, || {
        if request.is_null() || output.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        let request = unsafe { &*request };
        if !request.header.accepts::<OpenRequest>()
            || !request.host.header.accepts::<HostServices>()
            || request.session_epoch == 0
            || request.host.cancellation.is_cancelled.is_none()
        {
            return Err(INVALID_ARGUMENT);
        }
        let entries = unsafe {
            borrowed_array(
                request.configuration,
                request.configuration_count,
                MAX_CONFIG_FIELDS,
            )?
        };
        unsafe { validate_config(&SCHEMA.0, entries) }.map_err(|_| INVALID_ARGUMENT)?;
        let mut settings = [32i64, 0, 0];
        for (index, field) in SCHEMA.0.iter().enumerate() {
            for entry in entries {
                if unsafe { entry.id.text(128)? == field.id.text(128)? } {
                    settings[index] = entry.value.integer;
                }
            }
        }
        let host = Services(request.host);
        if host.cancelled(request.session_epoch) {
            return Err(CANCELLED);
        }
        if settings[1] != 0 {
            host.window(request.session_epoch, WINDOW_TITLE)?;
        }
        if settings[2] != 0 {
            host.window(request.session_epoch, WINDOW_ICON)?;
        }
        if ACTIVE.swap(true, Ordering::AcqRel) {
            return Err(INVALID_STATE);
        }
        let intensity = settings[0] as u8;
        let mut pixels = [0u8; 16];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[intensity, 0, 0, 255]);
        }
        let pixels = Arc::new(pixels);
        let shared = Arc::new(Shared {
            image: Mutex::new(Image {
                pixels: pixels.clone(),
                generation: 1,
            }),
            stopped: AtomicBool::new(false),
            ticks: AtomicUsize::new(0),
            pressed: AtomicBool::new(false),
            released: AtomicBool::new(false),
        });
        let (commands, receiver) = mpsc::sync_channel(32);
        let epoch = request.session_epoch;
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("external-native".into())
            .spawn(move || run(worker_shared, receiver, host, epoch, intensity, pixels));
        let worker = match worker {
            Ok(worker) => worker,
            Err(_) => {
                ACTIVE.store(false, Ordering::Release);
                return Err(CORE_ERROR);
            }
        };
        let session = Box::new(NativeSession {
            epoch,
            shared,
            commands,
            worker: Some(worker),
        });
        unsafe {
            output.write(OpenResult {
                session: Box::into_raw(session).cast(),
                context_epoch: 1,
                output_epoch: 1,
                surface: surface_info(),
            });
        }
        Ok(())
    })
}
unsafe extern "C" fn input(
    handle: Session,
    events: *const InputEvent,
    count: usize,
    error: *mut Error,
) -> Status {
    boundary(error, || {
        if handle.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        let session = unsafe { &*handle.cast::<NativeSession>() };
        for event in unsafe { borrowed_array(events, count, 256)? } {
            if event.kind == INPUT_KEY && unsafe { event.physical_key.text(64)? } == "Enter" {
                if event.pressed > 1 {
                    return Err(INVALID_ARGUMENT);
                }
                session
                    .commands
                    .try_send(Command::Key(event.pressed != 0))
                    .map_err(|_| CAPACITY)?;
            }
        }
        Ok(())
    })
}
unsafe extern "C" fn control(_: Session, _: u32, error: *mut Error) -> Status {
    boundary(error, || Err(UNSUPPORTED))
}
unsafe extern "C" fn state(
    handle: Session,
    output: *mut SessionState,
    error: *mut Error,
) -> Status {
    boundary(error, || {
        if handle.is_null() || output.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            output.write(SessionState {
                state: SESSION_RUNNING,
                flags: 0,
                context_epoch: 1,
                output_epoch: 1,
                error: Error::default(),
            });
        }
        Ok(())
    })
}
unsafe extern "C" fn release(owner: *mut c_void) {
    drop(unsafe { Arc::from_raw(owner.cast::<[u8; 16]>()) });
}
unsafe extern "C" fn frame(
    handle: Session,
    previous: FrameIdentity,
    output: *mut FrameSurface,
    error: *mut Error,
) -> Status {
    boundary(error, || {
        if handle.is_null() || output.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        let session = unsafe { &*handle.cast::<NativeSession>() };
        let image = session.shared.image.lock();
        let identity = FrameIdentity {
            session_epoch: session.epoch,
            output_epoch: 1,
            generation: image.generation,
        };
        let unchanged = previous == identity;
        let cpu = if unchanged {
            CpuLease {
                pixels: Bytes::EMPTY,
                owner: std::ptr::null_mut(),
                release: None,
            }
        } else {
            let owner = Arc::into_raw(image.pixels.clone());
            CpuLease {
                pixels: Bytes {
                    data: owner.cast(),
                    len: 16,
                },
                owner: owner.cast_mut().cast(),
                release: Some(release),
            }
        };
        unsafe {
            output.write(FrameSurface {
                state: if unchanged {
                    FRAME_UNCHANGED
                } else {
                    FRAME_READY
                },
                reserved: 0,
                identity,
                timestamp_ns: session.shared.ticks.load(Ordering::Acquire) as u64 * 2_000_000,
                info: surface_info(),
                cpu,
            });
        }
        Ok(())
    })
}
unsafe extern "C" fn cancel(handle: Session) {
    if !handle.is_null() {
        let session = unsafe { &*handle.cast::<NativeSession>() };
        session.shared.stopped.store(true, Ordering::Release);
        let _ = session.commands.try_send(Command::Wake);
    }
}
unsafe extern "C" fn close(handle: Session, error: *mut Error) -> Status {
    boundary(error, || {
        if handle.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe { cancel(handle) };
        let mut session = unsafe { Box::from_raw(handle.cast::<NativeSession>()) };
        let joined = session.worker.take().unwrap().join();
        ACTIVE.store(false, Ordering::Release);
        joined.map_err(|_| PANICKED)?;
        if !session.shared.pressed.load(Ordering::Acquire)
            || !session.shared.released.load(Ordering::Acquire)
            || session.shared.ticks.load(Ordering::Acquire) == 0
        {
            return Err(INVALID_STATE);
        }
        tracing::info!(
            event = "external.native.closed",
            native_steps = session.shared.ticks.load(Ordering::Acquire),
            input_complete = true
        );
        Ok(())
    })
}
unsafe extern "C" fn extension(_: Bytes, _: u32, output: *mut *const c_void) -> Status {
    if output.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { output.write(std::ptr::null()) };
    UNSUPPORTED
}
static API: CoreApi = CoreApi {
    header: Header::of::<CoreApi>(),
    initialize: Some(initialize),
    describe: Some(describe),
    probe: Some(probe),
    open: Some(open),
    input: Some(input),
    control: Some(control),
    state: Some(state),
    frame: Some(frame),
    cancel: Some(cancel),
    close: Some(close),
    extension: Some(extension),
};
/// # Safety
/// The caller supplies valid output storage and observes the native ABI contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn astra_emu_query(version: u32, output: *mut *const CoreApi) -> Status {
    if output.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { output.write(std::ptr::null()) };
    if version != ABI_VERSION {
        return UNSUPPORTED;
    }
    unsafe { output.write(&API) };
    OK
}
