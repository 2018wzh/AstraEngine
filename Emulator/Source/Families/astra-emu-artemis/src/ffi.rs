use std::{ffi::c_void, path::Path, ptr, sync::{Arc, LazyLock, atomic::{AtomicBool, Ordering}}};
use parking_lot::Mutex;
use astra_emu_abi::*;
use crate::{error::{self, NativeResult}, provider, session::{Platform, SessionOwner}};
static INITIALIZED: AtomicBool = AtomicBool::new(false);
struct Metadata { fields: [ConfigField; 1], formats: [Bytes; 1] }
// SAFETY: spans point only at static byte literals; metadata never changes.
unsafe impl Send for Metadata {}
unsafe impl Sync for Metadata {}
static METADATA: LazyLock<Metadata> = LazyLock::new(|| Metadata { fields: provider::fields(), formats: [Bytes::new(b"artemis.pfs")] });
static PROBE_ID: Mutex<String> = Mutex::new(String::new());
fn failure(out: *mut Error, status: Status, code: &str, message: &str) -> Status {
    if !out.is_null() { unsafe { (*out).set(code, message) }; }
    status
}
fn boundary(out: *mut Error, action: impl FnOnce() -> NativeResult<()>) -> Status {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(())) => OK,
        Ok(Err(error)) => failure(out, CORE_ERROR, error.code, &error.message),
        Err(_) => failure(out, PANICKED, "ASTRA_EMU_ARTEMIS_PANIC", "panic contained at native ABI boundary"),
    }
}
fn initialized() -> NativeResult<()> {
    if INITIALIZED.load(Ordering::Acquire) { Ok(()) } else { Err(error::invalid("ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT", "initialize diagnostics before using the core")) }
}
unsafe fn initialize_impl(diagnostics: *const Diagnostics, out: *mut Error) -> Status {
    if diagnostics.is_null() { return failure(out, INVALID_ARGUMENT, "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT", "missing diagnostics"); }
    let sink = unsafe { *diagnostics };
    if !sink.header.accepts::<Diagnostics>() || sink.emit.is_none() || sink.flush.is_none() {
        return failure(out, INVALID_ARGUMENT, "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT", "invalid diagnostics header or callbacks");
    }
    #[cfg(not(feature = "static-core"))]
    if let Err(status) = unsafe { diagnostic_bridge::install(sink) } {
        return failure(out, status, "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT", "diagnostics bridge installation failed");
    }
    INITIALIZED.store(true, Ordering::Release);
    OK
}
unsafe extern "C" fn initialize(diagnostics: *const Diagnostics, out: *mut Error) -> Status {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { initialize_impl(diagnostics, out) }))
        .unwrap_or_else(|_| failure(out, PANICKED, "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT", "diagnostic initialization panicked"))
}
unsafe extern "C" fn describe(out: *mut Descriptor, error: *mut Error) -> Status {
    if out.is_null() { return failure(error, INVALID_ARGUMENT, "ASTRA_EMU_ARTEMIS_ARGUMENT", "null descriptor output"); }
    boundary(error, || {
        initialized()?;
        unsafe { *out = Descriptor { header: Header::of::<Descriptor>(), family_id: Bytes::new(b"artemis"), plugin_id: Bytes::new(b"astra.emu.artemis"), version: Bytes::new(env!("CARGO_PKG_VERSION").as_bytes()), capabilities: CAP_CPU_SURFACE, formats: METADATA.formats.as_ptr(), format_count: 1, configuration: METADATA.fields.as_ptr(), configuration_count: 1 }; }
        Ok(())
    })
}
unsafe extern "C" fn probe(path: Bytes, out: *mut ProbeResult, error: *mut Error) -> Status {
    if out.is_null() { return failure(error, INVALID_ARGUMENT, "ASTRA_EMU_ARTEMIS_ARGUMENT", "null probe output"); }
    let mut matched = false;
    let status = boundary(error, || {
        initialized()?;
        let path = unsafe { path.text(32768) }.map_err(|_| crate::error::invalid("ASTRA_EMU_ARTEMIS_PROBE_PATH", "invalid UTF-8 game path"))?;
        if let Some(id) = provider::probe(Path::new(path))? {
            let mut storage = PROBE_ID.lock(); *storage = id;
            unsafe { *out = ProbeResult { game_id: Bytes::new(storage.as_bytes()), format: Bytes::new(b"artemis.pfs"), confidence_permyriad: 10000, reserved: 0 }; }
            matched = true;
        }
        Ok(())
    });
    if status == OK && !matched { NO_MATCH } else { status }
}
unsafe extern "C" fn open(request: *const OpenRequest, out: *mut OpenResult, error: *mut Error) -> Status {
    if out.is_null() { return failure(error, INVALID_ARGUMENT, "ASTRA_EMU_ARTEMIS_ARGUMENT", "null open output"); }
    unsafe { *out = OpenResult { session: ptr::null_mut(), context_epoch: 0, output_epoch: 0, surface: crate::session::surface(1, 1) }; }
    if request.is_null() { return failure(error, INVALID_ARGUMENT, "ASTRA_EMU_ARTEMIS_ARGUMENT", "null open request"); }
    boundary(error, || {
        initialized()?;
        let request = unsafe { &*request };
        if !request.header.accepts::<OpenRequest>() || !request.host.header.accepts::<HostServices>() || request.session_epoch == 0
            || request.window.focused > 1 || request.window.suspended > 1 || !request.window.scale_factor.is_finite() || request.window.scale_factor <= 0.0
            || request.host.cancellation.is_cancelled.is_none()
            || (request.host.window.request.is_some() && request.host.window.invalidate.is_none()) {
            return Err(crate::error::invalid("ASTRA_EMU_ARTEMIS_OPEN", "invalid open header, identity, window or cancellation service"));
        }
        let path = unsafe { request.game_path.text(32768) }.map_err(|_| crate::error::invalid("ASTRA_EMU_ARTEMIS_OPEN", "invalid UTF-8 game path"))?;
        let entries = unsafe { borrowed_array(request.configuration, request.configuration_count, MAX_CONFIG_FIELDS) }.map_err(|_| crate::error::invalid("ASTRA_EMU_ARTEMIS_CONFIG", "invalid configuration array"))?;
        unsafe { validate_config(&METADATA.fields, entries) }.map_err(|e| crate::error::invalid("ASTRA_EMU_ARTEMIS_CONFIG", format!("configuration entry {}: {}", e.index, e.reason)))?;
        let archive = if let Some(entry) = entries.first() { unsafe { entry.value.text.text(255) }.map_err(|_| crate::error::invalid("ASTRA_EMU_ARTEMIS_CONFIG", "invalid archive filename"))? } else { "" };
        let archive = provider::find_base_archive(Path::new(path), archive)?;
        let owner = SessionOwner::open(Path::new(path).to_owned(), archive, request.session_epoch, request.window, Platform(request.host.window, request.host.cancellation))?;
        let (surface, output_epoch, context_epoch) = {
            let snapshot = owner.snapshot.lock();
            (snapshot.info, snapshot.identity.output_epoch, owner.context.load(Ordering::Acquire))
        };
        unsafe { *out = OpenResult { session: Box::into_raw(owner).cast(), context_epoch, output_epoch, surface }; }
        Ok(())
    })
}
unsafe fn owner<'a>(session: Session) -> NativeResult<&'a SessionOwner> {
    if session.is_null() { return Err(crate::error::invalid("ASTRA_EMU_ARTEMIS_SESSION", "null session handle")); }
    Ok(unsafe { &*session.cast::<SessionOwner>() })
}
unsafe extern "C" fn input(session: Session, data: *const InputEvent, count: usize, error: *mut Error) -> Status {
    boundary(error, || {
        let owner = unsafe { owner(session)? };
        let values = unsafe { borrowed_array(data, count, 1024) }.map_err(|_| crate::error::invalid("ASTRA_EMU_ARTEMIS_INPUT", "invalid input array"))?;
        let inputs = values.iter().map(|value| unsafe { crate::events::decode(value) }).collect::<NativeResult<Vec<_>>>()?;
        owner.input(inputs)
    })
}
unsafe extern "C" fn control(session: Session, command: u32, error: *mut Error) -> Status {
    if !matches!(command, CONTROL_PAUSE | CONTROL_RESUME) { return failure(error, UNSUPPORTED, "ASTRA_EMU_ARTEMIS_CONTROL", "unsupported native control"); }
    boundary(error, || unsafe { owner(session)? }.pause(command == CONTROL_PAUSE))
}
unsafe extern "C" fn state(session: Session, out: *mut SessionState, error: *mut Error) -> Status {
    if out.is_null() { return failure(error, INVALID_ARGUMENT, "ASTRA_EMU_ARTEMIS_ARGUMENT", "null state output"); }
    boundary(error, || {
        let owner = unsafe { owner(session)? };
        let snapshot = owner.snapshot.lock();
        let mut value = SessionState { state: snapshot.state, reserved: 0, context_epoch: owner.context.load(Ordering::Acquire), output_epoch: snapshot.identity.output_epoch, error: Error::default() };
        if let Some(error) = &snapshot.error { value.error.set(error.code, &error.message); }
        unsafe { *out = value; }
        Ok(())
    })
}
unsafe extern "C" fn release(owner: *mut c_void) { if !owner.is_null() { drop(unsafe { Box::from_raw(owner.cast::<Arc<Vec<u8>>>()) }); } }
unsafe extern "C" fn frame(session: Session, after: FrameIdentity, out: *mut FrameSurface, error: *mut Error) -> Status {
    if out.is_null() { return failure(error, INVALID_ARGUMENT, "ASTRA_EMU_ARTEMIS_ARGUMENT", "null surface output"); }
    boundary(error, || {
        let owner = unsafe { owner(session)? };
        let snapshot = owner.snapshot.lock();
        if let Some(error) = &snapshot.error { return Err(error.clone()); }
        let mut value = FrameSurface { state: FRAME_PENDING, reserved: 0, identity: snapshot.identity, timestamp_ns: snapshot.timestamp_ns, info: snapshot.info, cpu: CpuLease { pixels: Bytes::EMPTY, owner: ptr::null_mut(), release: None } };
        if let Some(pixels) = &snapshot.pixels {
            if after == snapshot.identity { value.state = FRAME_UNCHANGED; }
            else {
                value.state = FRAME_READY; value.cpu.pixels = Bytes::new(pixels.as_slice());
                value.cpu.owner = Box::into_raw(Box::new(pixels.clone())).cast(); value.cpu.release = Some(release);
            }
        }
        unsafe { *out = value; }
        Ok(())
    })
}
unsafe extern "C" fn cancel(session: Session) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { if let Ok(owner) = unsafe { owner(session) } { owner.cancel(); } }));
}
unsafe extern "C" fn close(session: Session, error: *mut Error) -> Status {
    boundary(error, || { if session.is_null() { return Err(crate::error::invalid("ASTRA_EMU_ARTEMIS_SESSION", "null session handle")); } let mut owner = unsafe { Box::from_raw(session.cast::<SessionOwner>()) }; owner.shutdown() })
}
unsafe extern "C" fn extension(_: Bytes, _: u32, out: *mut *const c_void) -> Status { if out.is_null() { return INVALID_ARGUMENT; } unsafe { *out = ptr::null(); } UNSUPPORTED }
static API: CoreApi = CoreApi { header: Header::of::<CoreApi>(), initialize: Some(initialize), describe: Some(describe), probe: Some(probe), open: Some(open), input: Some(input), control: Some(control), state: Some(state), frame: Some(frame), cancel: Some(cancel), close: Some(close), extension: Some(extension) };
/// Static registration and the dynamic symbol return the identical protocol table.
pub unsafe extern "C" fn query(version: u32, out: *mut *const CoreApi) -> Status {
    if out.is_null() { return INVALID_ARGUMENT; }
    unsafe { *out = ptr::null(); }
    if version != ABI_VERSION { return UNSUPPORTED; }
    unsafe { *out = &API; }
    OK
}
#[cfg(not(feature = "static-core"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn astra_emu_query(version: u32, out: *mut *const CoreApi) -> Status { unsafe { query(version, out) } }
