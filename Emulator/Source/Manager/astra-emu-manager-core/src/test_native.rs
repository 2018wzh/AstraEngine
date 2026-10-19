//! Native C-table fixture for host lifecycle and policy tests.
use astra_emu_abi as abi;
use astra_emu_manager_core::LoadedFamilyPlugin;
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{JoinHandle, ThreadId},
    time::Duration,
};
#[derive(Clone, Debug)]
pub struct Call {
    pub name: &'static str,
    pub thread: ThreadId,
    pub key: String,
}
pub struct Fixture {
    pub plugin: &'static str,
    pub family: &'static str,
    pub format: &'static str,
    pub confidence: u32,
    pub marker: Option<&'static str>,
    pub config: bool,
    pub calls: Arc<Mutex<Vec<Call>>>,
    pub entered: Option<std::sync::mpsc::Sender<()>>,
    pub gate: Option<std::sync::mpsc::Receiver<()>>,
    pub close_error: bool,
    pub init_error: bool,
    pub module_owner: bool,
    pub owner_create_error: bool,
    pub owner_destroy_error: bool,
    formats: Box<[abi::Bytes]>,
    fields: Box<[abi::ConfigField]>,
    choices: Box<[abi::Bytes]>,
    choice_labels: Box<[abi::Bytes]>,
}
impl Fixture {
    pub fn new(plugin: &'static str, family: &'static str, format: &'static str) -> Self {
        Self {
            plugin,
            family,
            format,
            confidence: 9000,
            marker: None,
            config: false,
            calls: Arc::new(Mutex::new(Vec::new())),
            entered: None,
            gate: None,
            close_error: false,
            init_error: false,
            module_owner: false,
            owner_create_error: false,
            owner_destroy_error: false,
            formats: Box::new([]),
            fields: Box::new([]),
            choices: Box::new([]),
            choice_labels: Box::new([]),
        }
    }
}
thread_local! { static FIXTURES: RefCell<[Option<Box<Fixture>>; 4]> = const { RefCell::new([None, None, None, None]) }; }
fn fixture<const N: usize, R>(f: impl FnOnce(&Fixture) -> R) -> R {
    FIXTURES.with(|slots| f(slots.borrow()[N].as_deref().unwrap()))
}
fn record(calls: &Mutex<Vec<Call>>, name: &'static str, key: String) {
    calls.lock().push(Call {
        name,
        thread: std::thread::current().id(),
        key,
    });
}
pub fn install(slot: usize, value: Fixture) -> LoadedFamilyPlugin {
    try_install(slot, value).unwrap()
}
pub fn try_install(
    slot: usize,
    mut value: Fixture,
) -> Result<LoadedFamilyPlugin, astra_emu_manager_core::FamilyLoadError> {
    value.formats = vec![abi::Bytes::new(value.format.as_bytes())].into_boxed_slice();
    if value.config {
        value.choices =
            vec![abi::Bytes::new(b"direct"), abi::Bytes::new(b"title")].into_boxed_slice();
        value.choice_labels = vec![
            abi::Bytes::new("直接进入脚本".as_bytes()),
            abi::Bytes::new("标题菜单启动".as_bytes()),
        ]
        .into_boxed_slice();
        value.fields = vec![abi::ConfigField {
            id: abi::Bytes::new(b"launch_mode"),
            label: abi::Bytes::new(b"Launch mode"),
            group: abi::Bytes::new(b"Startup"),
            default_value: abi::ConfigValue {
                kind: abi::VALUE_ENUM,
                reserved: 0,
                integer: 0,
                number: 0.0,
                text: abi::Bytes::new(b"direct"),
            },
            integer_min: 0,
            integer_max: 0,
            number_min: 0.0,
            number_max: 0.0,
            max_bytes: 0,
            flags: 0,
            choices: value.choices.as_ptr(),
            choice_count: value.choices.len(),
            description: abi::Bytes::EMPTY,
            choice_labels: value.choice_labels.as_ptr(),
            step: 0.0,
            unit: abi::Bytes::EMPTY,
        }]
        .into_boxed_slice();
    }
    FIXTURES.with(|slots| slots.borrow_mut()[slot] = Some(Box::new(value)));
    unsafe {
        LoadedFamilyPlugin::from_query(match slot {
            0 => query::<0>,
            1 => query::<1>,
            2 => query::<2>,
            3 => query::<3>,
            _ => panic!("fixture slot"),
        })
    }
}
unsafe extern "C" fn initialize<const N: usize>(
    diagnostics: *const abi::Diagnostics,
    error: *mut abi::Error,
) -> abi::Status {
    if diagnostics.is_null() {
        return abi::INVALID_ARGUMENT;
    }
    fixture::<N, _>(|v| {
        record(&v.calls, "initialize", String::new());
        if v.init_error {
            unsafe {
                (*error).set(
                    "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT",
                    "diagnostic bridge conflict",
                );
            }
            abi::CORE_ERROR
        } else {
            abi::OK
        }
    })
}
unsafe extern "C" fn describe<const N: usize>(
    out: *mut abi::Descriptor,
    _: *mut abi::Error,
) -> abi::Status {
    fixture::<N, _>(|v| {
        record(&v.calls, "describe", String::new());
        unsafe {
            out.write(abi::Descriptor {
                header: abi::Header::of::<abi::Descriptor>(),
                family_id: abi::Bytes::new(v.family.as_bytes()),
                plugin_id: abi::Bytes::new(v.plugin.as_bytes()),
                version: abi::Bytes::new(b"1.0.0"),
                capabilities: abi::CAP_CPU_SURFACE,
                formats: v.formats.as_ptr(),
                format_count: v.formats.len(),
                configuration: v.fields.as_ptr(),
                configuration_count: v.fields.len(),
                display_name: abi::Bytes::new(v.plugin.as_bytes()),
                family_name: abi::Bytes::new(v.family.as_bytes()),
                render: abi::RenderExtentSupport::default(),
            });
        }
    });
    abi::OK
}
unsafe extern "C" fn probe<const N: usize>(
    path: abi::Bytes,
    out: *mut abi::ProbeResult,
    _: *mut abi::Error,
) -> abi::Status {
    fixture::<N, _>(|v| {
        record(&v.calls, "probe", String::new());
        let path = unsafe { path.text(4096) }.unwrap();
        if v.marker
            .is_some_and(|marker| !std::path::Path::new(path).join(marker).is_file())
        {
            return abi::NO_MATCH;
        }
        unsafe {
            out.write(abi::ProbeResult {
                game_id: abi::Bytes::new(b"fixture-game"),
                format: abi::Bytes::new(v.format.as_bytes()),
                confidence_permyriad: v.confidence,
                reserved: 0,
            });
        }
        abi::OK
    })
}
fn info() -> abi::SurfaceInfo {
    abi::SurfaceInfo {
        kind: abi::SURFACE_CPU,
        pixel_format: abi::PIXEL_RGBA8,
        color: abi::COLOR_SRGB,
        alpha: abi::ALPHA_OPAQUE,
        rows: abi::ROWS_TOP_DOWN,
        logical: abi::Extent {
            width: 1,
            height: 1,
        },
        raster: abi::Extent {
            width: 1,
            height: 1,
        },
        stride: 4,
    }
}
struct Session {
    epoch: u64,
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    calls: Arc<Mutex<Vec<Call>>>,
    close_error: bool,
}
unsafe extern "C" fn open<const N: usize>(
    raw: *const abi::OpenRequest,
    out: *mut abi::OpenResult,
    error: *mut abi::Error,
) -> abi::Status {
    let raw = unsafe { &*raw };
    fixture::<N, _>(|v| {
        record(&v.calls, "open", String::new());
        if let Some(entered) = &v.entered {
            let _ = entered.send(());
        }
        if let Some(gate) = &v.gate {
            let _ = gate.recv();
        }
        if unsafe {
            raw.host.cancellation.is_cancelled.unwrap()(
                raw.host.cancellation.context,
                raw.session_epoch,
            )
        } != 0
        {
            unsafe {
                (*error).set("ASTRA_TEST_OPEN_CANCELLED", "cancelled while opening");
            }
            return abi::CANCELLED;
        }
        if v.module_owner == raw.module_owner.is_null() {
            unsafe {
                (*error).set(
                    "ASTRA_TEST_MODULE_OWNER",
                    "module owner presence does not match the extension",
                );
            }
            return abi::INVALID_ARGUMENT;
        }
        if abi::RenderExtentSupport::default()
            .validate_request(abi::CAP_CPU_SURFACE, raw.render_extent)
            .is_err()
        {
            unsafe {
                (*error).set(
                    "ASTRA_TEST_RENDER_EXTENT",
                    "render extent requested from a core without render support",
                );
            }
            return abi::INVALID_ARGUMENT;
        }
        if v.module_owner {
            let owner = unsafe { &*raw.module_owner.cast::<Resources>() };
            record(&owner.calls, "module_use", String::new());
        }
        let generation = Arc::new(AtomicU64::new(1));
        let stop = Arc::new(AtomicBool::new(false));
        let producer_generation = generation.clone();
        let producer_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            while !producer_stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
                producer_generation.fetch_add(1, Ordering::AcqRel);
            }
        });
        let session = Box::new(Session {
            epoch: raw.session_epoch,
            generation,
            stop,
            worker: Some(worker),
            calls: v.calls.clone(),
            close_error: v.close_error,
        });
        unsafe {
            out.write(abi::OpenResult {
                session: Box::into_raw(session).cast(),
                context_epoch: 1,
                output_epoch: 1,
                surface: info(),
            });
        }
        abi::OK
    })
}
unsafe extern "C" fn input(
    handle: abi::Session,
    values: *const abi::InputEvent,
    count: usize,
    _: *mut abi::Error,
) -> abi::Status {
    let session = unsafe { &*handle.cast::<Session>() };
    for value in unsafe { abi::borrowed_array(values, count, 4096) }.unwrap() {
        record(
            &session.calls,
            "input",
            unsafe { value.physical_key.text(256) }.unwrap().to_owned(),
        );
    }
    abi::OK
}
unsafe extern "C" fn control(
    handle: abi::Session,
    command: u32,
    error: *mut abi::Error,
) -> abi::Status {
    let session = unsafe { &*handle.cast::<Session>() };
    record(&session.calls, "control", String::new());
    if command == 0x1002 {
        if !error.is_null() {
            unsafe {
                (*error).set(
                    "ASTRA_EMU_TEST_ACTION_UNAVAILABLE",
                    "test action unavailable",
                );
            }
        }
        return abi::INVALID_STATE;
    }
    abi::OK
}
unsafe extern "C" fn state(
    handle: abi::Session,
    out: *mut abi::SessionState,
    _: *mut abi::Error,
) -> abi::Status {
    let session = unsafe { &*handle.cast::<Session>() };
    record(&session.calls, "state", String::new());
    unsafe {
        out.write(abi::SessionState {
            state: abi::SESSION_RUNNING,
            flags: 0,
            context_epoch: 1,
            output_epoch: 1,
            error: abi::Error::default(),
        });
    }
    abi::OK
}
unsafe extern "C" fn release(owner: *mut std::ffi::c_void) {
    drop(unsafe { Box::from_raw(owner.cast::<[u8; 4]>()) });
}
unsafe extern "C" fn frame(
    handle: abi::Session,
    after: abi::FrameIdentity,
    out: *mut abi::FrameSurface,
    _: *mut abi::Error,
) -> abi::Status {
    let session = unsafe { &*handle.cast::<Session>() };
    record(&session.calls, "frame", String::new());
    let identity = abi::FrameIdentity {
        session_epoch: session.epoch,
        output_epoch: 1,
        generation: session.generation.load(Ordering::Acquire),
    };
    let (state, cpu) = if after == identity {
        (
            abi::FRAME_UNCHANGED,
            abi::CpuLease {
                pixels: abi::Bytes::EMPTY,
                owner: std::ptr::null_mut(),
                release: None,
            },
        )
    } else {
        let bytes = Box::new([23, 45, 67, 255]);
        let pixels = abi::Bytes::new(bytes.as_ref());
        (
            abi::FRAME_READY,
            abi::CpuLease {
                pixels,
                owner: Box::into_raw(bytes).cast(),
                release: Some(release),
            },
        )
    };
    unsafe {
        out.write(abi::FrameSurface {
            state,
            reserved: 0,
            identity,
            timestamp_ns: identity.generation * 2_000_000,
            info: info(),
            cpu,
        });
    }
    abi::OK
}
unsafe extern "C" fn cancel(handle: abi::Session) {
    let session = unsafe { &*handle.cast::<Session>() };
    session.stop.store(true, Ordering::Release);
}
unsafe extern "C" fn close(handle: abi::Session, error: *mut abi::Error) -> abi::Status {
    let mut session = unsafe { Box::from_raw(handle.cast::<Session>()) };
    session.stop.store(true, Ordering::Release);
    session.worker.take().unwrap().join().unwrap();
    record(&session.calls, "close", String::new());
    if session.close_error {
        unsafe {
            (*error).set("ASTRA_TEST_CLOSE", "close failed after cleanup");
        }
        abi::CORE_ERROR
    } else {
        abi::OK
    }
}
struct Resources {
    calls: Arc<Mutex<Vec<Call>>>,
    destroy_error: bool,
}
unsafe extern "C" fn create_owner<const N: usize>(
    out: *mut abi::ModuleOwner,
    error: *mut abi::Error,
) -> abi::Status {
    fixture::<N, _>(|fixture| {
        record(&fixture.calls, "module_create", String::new());
        unsafe {
            *out = Box::into_raw(Box::new(Resources {
                calls: fixture.calls.clone(),
                destroy_error: fixture.owner_destroy_error,
            }))
            .cast();
        }
        // Exercise host cleanup for a core that violates the null-on-error rule.
        if fixture.owner_create_error {
            unsafe {
                (*error).set("ASTRA_TEST_MODULE_CREATE", "owner initialization failed");
            }
            abi::CORE_ERROR
        } else {
            abi::OK
        }
    })
}
unsafe extern "C" fn destroy_owner(
    handle: abi::ModuleOwner,
    error: *mut abi::Error,
) -> abi::Status {
    let owner = unsafe { Box::from_raw(handle.cast::<Resources>()) };
    record(&owner.calls, "module_destroy", String::new());
    if owner.destroy_error {
        unsafe {
            (*error).set("ASTRA_TEST_MODULE_DESTROY", "owner destroyed with an error");
        }
        abi::CORE_ERROR
    } else {
        abi::OK
    }
}
struct OwnerTable<const N: usize>;
impl<const N: usize> OwnerTable<N> {
    const API: abi::ModuleOwnerApi = abi::ModuleOwnerApi {
        header: abi::ModuleOwnerApi::HEADER,
        create: Some(create_owner::<N>),
        destroy: Some(destroy_owner),
    };
}
static TEST_MENU: [abi::SessionMenuItem; 1] = [abi::SessionMenuItem {
    control: 0x1001,
    icon: abi::MENU_ICON_GENERIC,
    label: abi::Bytes::from_static("测试动作".as_bytes()),
}];
static TEST_MENU_API: abi::SessionMenuApi = abi::SessionMenuApi {
    header: abi::SessionMenuApi::HEADER,
    items: TEST_MENU.as_ptr(),
    count: 1,
};
unsafe extern "C" fn extension<const N: usize>(
    id: abi::Bytes,
    version: u32,
    out: *mut *const std::ffi::c_void,
) -> abi::Status {
    unsafe {
        *out = std::ptr::null();
    }
    let name = match unsafe { id.as_slice(128) } {
        Ok(name) => name,
        Err(_) => return abi::UNSUPPORTED,
    };
    if name == abi::SESSION_MENU_EXTENSION && version == abi::SESSION_MENU_VERSION {
        unsafe {
            *out = (&TEST_MENU_API as *const abi::SessionMenuApi).cast();
        }
        return abi::OK;
    }
    if version != abi::MODULE_OWNER_VERSION
        || name != abi::MODULE_OWNER_EXTENSION
        || !fixture::<N, _>(|value| value.module_owner)
    {
        return abi::UNSUPPORTED;
    }
    unsafe {
        *out = (&OwnerTable::<N>::API as *const abi::ModuleOwnerApi).cast();
    }
    abi::OK
}
struct Table<const N: usize>;
impl<const N: usize> Table<N> {
    const API: abi::CoreApi = abi::CoreApi {
        header: abi::Header::of::<abi::CoreApi>(),
        initialize: Some(initialize::<N>),
        describe: Some(describe::<N>),
        probe: Some(probe::<N>),
        open: Some(open::<N>),
        input: Some(input),
        control: Some(control),
        state: Some(state),
        frame: Some(frame),
        cancel: Some(cancel),
        close: Some(close),
        extension: Some(extension::<N>),
    };
}
unsafe extern "C" fn query<const N: usize>(
    version: u32,
    out: *mut *const abi::CoreApi,
) -> abi::Status {
    unsafe {
        *out = std::ptr::null();
    }
    if version != abi::ABI_VERSION {
        return abi::UNSUPPORTED;
    }
    unsafe {
        *out = &Table::<N>::API;
    }
    abi::OK
}
