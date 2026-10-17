//! C ownership boundary. The native worker owns the existing Musica session;
//! polling and input submission never execute the VM on the platform thread.
use astra_emu_abi as abi;
use astra_emu_family_api::{FamilyError, FamilyEvent};
use std::{ffi::c_void, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}, mpsc::{sync_channel, SyncSender}}, thread::JoinHandle};
mod config;
mod input;
mod worker;
pub(crate) mod text;

#[cfg(test)]
mod tests;
#[derive(Clone)]
struct Failure { status: abi::Status, diagnostic: abi::Error }
impl Failure {
    fn new(status: abi::Status, code: &str, message: &str) -> Self {
        let mut diagnostic = abi::Error::default(); diagnostic.set(code, message); Self { status, diagnostic }
    }
}
impl From<FamilyError> for Failure {
    fn from(cause: FamilyError) -> Self {
        let status = if matches!(cause.code.as_str(), "ASTRA_EMU_MUSICA_CANCELLED" | "ASTRA_EMU_MUSICA_TEXT_CANCELLED") {
            abi::CANCELLED
        } else { abi::CORE_ERROR };
        Self::new(status, &cause.code, &cause.message)
    }
}
fn invalid(status: abi::Status) -> Failure { Failure::new(status, "ASTRA_EMU_MUSICA_ABI", "invalid native ABI request or state") }
fn boundary(error: *mut abi::Error, run: impl FnOnce() -> Result<(), Failure>) -> abi::Status {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
        .unwrap_or_else(|_| Err(Failure::new(abi::PANICKED, "ASTRA_EMU_MUSICA_ABI_PANIC", "native entry panicked")));
    match result { Ok(()) => { if !error.is_null() { unsafe { error.write(abi::Error::default()) }; } abi::OK },
        Err(cause) => { if !error.is_null() { unsafe { error.write(cause.diagnostic) }; } cause.status } }
}
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static API: abi::CoreApi = abi::CoreApi {
    header: abi::Header::of::<abi::CoreApi>(), initialize: Some(initialize), describe: Some(describe), probe: Some(probe),
    open: Some(open), input: Some(input), control: Some(control), state: Some(state), frame: Some(frame),
    cancel: Some(cancel), close: Some(close), extension: Some(extension),
};
/// Static registry entry, identical to the dynamic query protocol.
/// # Safety
/// `output` must be writable for one table pointer.
pub unsafe extern "C" fn musica_query(version: u32, output: *mut *const abi::CoreApi) -> abi::Status {
    if output.is_null() { return abi::INVALID_ARGUMENT; }
    unsafe { output.write(std::ptr::null()) };
    if version != abi::ABI_VERSION { return abi::UNSUPPORTED; }
    unsafe { output.write(&API) }; abi::OK
}
#[cfg(feature = "dynamic-plugin-export")]
#[no_mangle]
pub unsafe extern "C" fn astra_emu_query(version: u32, output: *mut *const abi::CoreApi) -> abi::Status {
    unsafe { musica_query(version, output) }
}
unsafe extern "C" fn initialize(diagnostics: *const abi::Diagnostics, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        let diagnostics = unsafe { diagnostics.as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        if !diagnostics.header.accepts::<abi::Diagnostics>() || diagnostics.emit.is_none() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        #[cfg(feature = "dynamic-plugin-export")]
        unsafe { abi::diagnostic_bridge::install(*diagnostics) }.map_err(|status| Failure::new(status,
            "ASTRA_EMU_FAMILY_DIAGNOSTIC_INIT", "cannot install native diagnostics bridge"))?;
        INITIALIZED.store(true, Ordering::Release); Ok(())
    })
}
fn initialized() -> Result<(), Failure> {
    if INITIALIZED.load(Ordering::Acquire) { Ok(()) } else { Err(invalid(abi::INVALID_STATE)) }
}
unsafe extern "C" fn describe(output: *mut abi::Descriptor, error: *mut abi::Error) -> abi::Status {
    boundary(error, || { initialized()?; if output.is_null() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        unsafe { output.write(config::descriptor()) }; Ok(()) })
}
static PROBE_GAME: Mutex<String> = Mutex::new(String::new());
unsafe extern "C" fn probe(path: abi::Bytes, output: *mut abi::ProbeResult, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        initialized()?; if output.is_null() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        let root = std::path::Path::new(unsafe { path.text(32768) }.map_err(invalid)?);
        if !crate::REQUIRED_ARCHIVE_ROLES.iter().all(|role| root.join(format!("{role}.paz")).is_file()) {
            return Err(Failure::new(abi::NO_MATCH, "ASTRA_EMU_MUSICA_NO_MATCH", "required Musica archives are absent"));
        }
        let name = root.file_name().and_then(|name| name.to_str()).unwrap_or("musica");
        if name.is_empty() || name.len() > astra_emu_family_api::MAX_SYMBOL_BYTES {
            return Err(Failure::new(abi::INVALID_ARGUMENT, "ASTRA_EMU_MUSICA_GAME_ID", "game directory name exceeds the metadata bound"));
        }
        let mut game_id = PROBE_GAME.lock().map_err(|_| invalid(abi::PANICKED))?;
        game_id.clear();
        game_id.push_str(name);
        unsafe { output.write(abi::ProbeResult { game_id: abi::Bytes::new(game_id.as_bytes()), format: abi::Bytes::new(b"musica.paz"), confidence_permyriad: 8000, reserved: 0 }) };
        Ok(())
    })
}
struct Published { info: abi::SurfaceInfo, pixels: Arc<[u8]>, generation: u64, timestamp_ns: u64 }
struct Shared {
    services: Arc<text::ServiceContext>,
    canvas: astra_emu_sdk::StageCanvas,
    frame: Mutex<Published>,
    state: Mutex<abi::SessionState>,
    leases: Arc<AtomicUsize>,
}
enum Command { Input(Vec<FamilyEvent>), Control(u32) }
struct Handle { shared: Arc<Shared>, commands: SyncSender<Command>, worker: Option<JoinHandle<Result<(), Failure>>> }
impl Drop for Handle {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.shared.services.cancel();
            let _ = worker.join();
        }
    }
}
unsafe fn handle<'a>(session: abi::Session) -> Result<&'a Handle, Failure> {
    unsafe { session.cast::<Handle>().as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))
}
unsafe extern "C" fn open(request: *const abi::OpenRequest, output: *mut abi::OpenResult, error: *mut abi::Error) -> abi::Status {
    if !output.is_null() { unsafe { (*output).session = std::ptr::null_mut() }; }
    boundary(error, || {
        initialized()?;
        let request = unsafe { request.as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        if output.is_null() || !request.header.accepts::<abi::OpenRequest>() || !request.host.header.accepts::<abi::HostServices>()
            || request.session_epoch == 0 || request.window.focused > 1 || request.window.suspended > 1 {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        if request.host.cancellation.is_cancelled.is_none() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        let text = request.host.text;
        if text.request.is_some() && (text.acknowledge.is_none() || text.invalidate.is_none()) { return Err(invalid(abi::INVALID_ARGUMENT)); }
        let window = request.host.window;
        if window.request.is_some() && window.invalidate.is_none() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        let services = Arc::new(text::ServiceContext { session: request.session_epoch,
            epoch: std::sync::atomic::AtomicU64::new(1), cancelled: AtomicBool::new(false), waiting: AtomicBool::new(false),
            text: text.request.map(|_| text), window: window.request.map(|_| window),
            host_cancel: Some(request.host.cancellation) });
        if services.is_cancelled() { return Err(invalid(abi::CANCELLED)); }
        let path = unsafe { request.game_path.text(32768) }.map_err(invalid)?.to_owned();
        let configuration = unsafe { config::entries(request) }?;
        let window = request.window;
        let (commands, receiver) = sync_channel(64);
        let (ready, ready_rx) = sync_channel(1);
        let worker_services = services.clone();
        let worker = std::thread::Builder::new().name("musica-native".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker::run(path, configuration, window, worker_services.clone(), receiver, &ready)));
            let result = result.unwrap_or_else(|_| Err(Failure::new(abi::PANICKED, "ASTRA_EMU_MUSICA_WORKER_PANIC", "native worker panicked")));
            if let Err(cause) = &result { let _ = ready.try_send(Err(cause.clone())); }
            worker_services.invalidate();
            result
        }).map_err(|_| Failure::new(abi::CORE_ERROR, "ASTRA_EMU_MUSICA_WORKER", "native worker could not start"))?;
        let shared = match ready_rx.recv() {
            Ok(Ok(shared)) => shared,
            result => {
                services.cancel();
                let joined = worker.join();
                return Err(match result { Ok(Err(cause)) => cause, _ => match joined {
                    Ok(Err(cause)) => cause, _ => Failure::new(abi::CORE_ERROR, "ASTRA_EMU_MUSICA_OPEN", "native worker exited during startup") } });
            }
        };
        let owned = Box::new(Handle { shared, commands, worker: Some(worker) });
        if owned.shared.services.is_cancelled() { return Err(invalid(abi::CANCELLED)); }
        let surface = owned.shared.frame.lock().map_err(|_| invalid(abi::PANICKED))?.info;
        unsafe { output.write(abi::OpenResult { session: Box::into_raw(owned).cast(), context_epoch: 1, output_epoch: 1, surface }) };
        Ok(())
    })
}
unsafe extern "C" fn input(session: abi::Session, events: *const abi::InputEvent, count: usize, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        let handle = unsafe { handle(session) }?;
        if handle.shared.services.is_cancelled() { return Err(invalid(abi::CANCELLED)); }
        let events = unsafe { abi::borrowed_array(events, count, 256) }.map_err(invalid)?;
        let events = unsafe { input::translate(handle, events) }?;
        handle.commands.try_send(Command::Input(events)).map_err(|_| Failure::new(abi::CAPACITY, "ASTRA_EMU_MUSICA_INPUT_QUEUE", "native input queue is full or closed"))
    })
}
unsafe extern "C" fn control(session: abi::Session, command: u32, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        let handle = unsafe { handle(session) }?;
        if !matches!(command, abi::CONTROL_PAUSE | abi::CONTROL_RESUME | abi::CONTROL_RETRY) { return Err(invalid(abi::UNSUPPORTED)); }
        if handle.shared.services.is_cancelled() { return Err(invalid(abi::CANCELLED)); }
        handle.commands.try_send(Command::Control(command)).map_err(|_| invalid(abi::CAPACITY))
    })
}
unsafe extern "C" fn state(session: abi::Session, output: *mut abi::SessionState, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if output.is_null() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        let handle = unsafe { handle(session) }?;
        let mut state = *handle.shared.state.lock().map_err(|_| invalid(abi::PANICKED))?;
        let cancelled = handle.shared.services.is_cancelled();
        state.context_epoch = handle.shared.services.epoch.load(Ordering::Acquire);
        if cancelled { state.state = abi::SESSION_FINISHED; }
        else if handle.shared.services.waiting.load(Ordering::Acquire) { state.state = abi::SESSION_WAITING; }
        unsafe { output.write(state) }; Ok(())
    })
}
struct Lease { pixels: Arc<[u8]>, count: Arc<AtomicUsize> }
unsafe extern "C" fn release(owner: *mut c_void) {
    if !owner.is_null() { let lease = unsafe { Box::from_raw(owner.cast::<Lease>()) }; lease.count.fetch_sub(1, Ordering::AcqRel); }
}
unsafe extern "C" fn frame(session: abi::Session, after: abi::FrameIdentity, output: *mut abi::FrameSurface, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if output.is_null() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        let handle = unsafe { handle(session) }?;
        let published = handle.shared.frame.lock().map_err(|_| invalid(abi::PANICKED))?;
        let identity = abi::FrameIdentity { session_epoch: handle.shared.services.session, output_epoch: 1, generation: published.generation };
        let mut result = abi::FrameSurface { state: abi::FRAME_UNCHANGED, reserved: 0, identity, timestamp_ns: published.timestamp_ns,
            info: published.info, cpu: abi::CpuLease { pixels: abi::Bytes::EMPTY, owner: std::ptr::null_mut(), release: None } };
        if after != identity {
            if handle.shared.leases.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < 3).then_some(n + 1)).is_err() {
                result.state = abi::FRAME_PENDING;
            } else {
                let owner = Box::new(Lease { pixels: published.pixels.clone(), count: handle.shared.leases.clone() });
                result.state = abi::FRAME_READY;
                result.cpu.pixels = abi::Bytes::new(&owner.pixels);
                result.cpu.owner = Box::into_raw(owner).cast(); result.cpu.release = Some(release);
            }
        }
        unsafe { output.write(result) }; Ok(())
    })
}
unsafe extern "C" fn cancel(session: abi::Session) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(handle) = unsafe { handle(session) } { handle.shared.services.cancel(); }
    }));
}
unsafe extern "C" fn close(session: abi::Session, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if session.is_null() { return Err(invalid(abi::INVALID_ARGUMENT)); }
        let mut handle = unsafe { Box::from_raw(session.cast::<Handle>()) };
        handle.shared.services.cancel();
        if let Some(worker) = handle.worker.take() {
            worker.join().map_err(|_| Failure::new(abi::PANICKED, "ASTRA_EMU_MUSICA_JOIN", "native worker panicked during close"))??;
        }
        Ok(())
    })
}
unsafe extern "C" fn extension(_name: abi::Bytes, _version: u32, output: *mut *const c_void) -> abi::Status {
    if output.is_null() { return abi::INVALID_ARGUMENT; }
    unsafe { output.write(std::ptr::null()) }; abi::UNSUPPORTED
}
