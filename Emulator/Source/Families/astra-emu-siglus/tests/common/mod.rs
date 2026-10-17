#![allow(dead_code)]
use astra_emu_abi as abi;
use std::{mem::MaybeUninit, path::PathBuf, sync::{Once, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Duration, Instant}};
static INIT: Once = Once::new();
static EPOCH: AtomicU64 = AtomicU64::new(1);
unsafe extern "C" fn emit(_: *mut std::ffi::c_void, record: *const abi::LogRecord) {
    use std::io::Write;
    let record = unsafe { &*record };
    let target = unsafe { record.target.text(4096) }.unwrap_or("invalid target");
    let message = unsafe { record.message.text(16384) }.unwrap_or("invalid message");
    let _ = writeln!(std::io::stderr(), "{target}: {message}");
}
unsafe extern "C" fn flush(_: *mut std::ffi::c_void) { use std::io::Write; let _ = std::io::stderr().flush(); }
unsafe extern "C" fn cancelled(context: *mut std::ffi::c_void, _: u64) -> u32 {
    u32::from(unsafe { &*context.cast::<AtomicBool>() }.load(Ordering::Acquire))
}
pub fn api() -> &'static abi::CoreApi {
    let mut pointer = std::ptr::null();
    assert_eq!(unsafe { astra_emu_siglus::query(abi::ABI_VERSION, &mut pointer) }, abi::OK);
    let api = unsafe { &*pointer };
    api.validate().unwrap();
    INIT.call_once(|| {
        let diagnostics = abi::Diagnostics { header: abi::Header::of::<abi::Diagnostics>(), context: std::ptr::null_mut(), emit: Some(emit), flush: Some(flush) };
        let mut error = abi::Error::default();
        success(unsafe { api.initialize.unwrap()(&diagnostics, &mut error) }, &error);
    });
    api
}
pub fn success(status: abi::Status, error: &abi::Error) {
    assert_eq!(status, abi::OK, "{}: {}", String::from_utf8_lossy(&error.code[..error.code_len as usize]), String::from_utf8_lossy(&error.message[..error.message_len as usize]));
}
pub fn game() -> PathBuf {
    assert_eq!(std::env::var("ASTRA_SIGLUS_TEST_WRITABLE_COPY").as_deref(), Ok("1"), "tests write native savedata: use an authorized disposable game copy");
    PathBuf::from(std::env::var_os("ASTRA_SIGLUS_TEST_GAME").expect("ASTRA_SIGLUS_TEST_GAME"))
}
pub fn window() -> abi::WindowState { abi::WindowState { drawable_width: 1280, drawable_height: 720, scale_factor: 1.0, focused: 1, suspended: 0 } }
pub fn services(cancel: &AtomicBool) -> abi::HostServices {
    abi::HostServices { header: abi::Header::of::<abi::HostServices>(), text: abi::TextService { context: std::ptr::null_mut(), request: None, acknowledge: None, invalidate: None }, window: abi::WindowService { context: std::ptr::null_mut(), request: None, invalidate: None }, cancellation: abi::CancellationService { context: (cancel as *const AtomicBool).cast_mut().cast(), is_cancelled: Some(cancelled) } }
}
pub struct Native { pub api: &'static abi::CoreApi, pub handle: abi::Session, pub epoch: u64, pub info: abi::SurfaceInfo, cancel: Box<AtomicBool> }
impl Native {
    pub fn open() -> Self {
        let api = api();
        let cancel = Box::new(AtomicBool::new(false));
        let epoch = EPOCH.fetch_add(1, Ordering::Relaxed);
        let path = game().to_string_lossy().into_owned();
        let request = abi::OpenRequest { header: abi::Header::of::<abi::OpenRequest>(), game_path: abi::Bytes::new(path.as_bytes()), session_epoch: epoch, configuration: std::ptr::null(), configuration_count: 0, window: window(), host: services(&cancel) };
        let mut out = MaybeUninit::uninit(); let mut error = abi::Error::default();
        success(unsafe { api.open.unwrap()(&request, out.as_mut_ptr(), &mut error) }, &error);
        let out = unsafe { out.assume_init() };
        out.surface.validate().unwrap();
        Self { api, handle: out.session, epoch, info: out.surface, cancel }
    }
    pub fn state(&self) -> abi::SessionState {
        let mut out = MaybeUninit::uninit(); let mut error = abi::Error::default();
        success(unsafe { self.api.state.unwrap()(self.handle, out.as_mut_ptr(), &mut error) }, &error);
        let state: abi::SessionState = unsafe { out.assume_init() };
        assert_ne!(state.state, abi::SESSION_FAILED, "{}", String::from_utf8_lossy(&state.error.message[..state.error.message_len as usize]));
        state
    }
    pub fn frame(&self) -> abi::CpuFrame {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut out = MaybeUninit::uninit(); let mut error = abi::Error::default();
            success(unsafe { self.api.frame.unwrap()(self.handle, abi::FrameIdentity::default(), out.as_mut_ptr(), &mut error) }, &error);
            let out: abi::FrameSurface = unsafe { out.assume_init() };
            if out.state == abi::FRAME_READY { return unsafe { abi::CpuFrame::adopt(out, self.epoch) }.unwrap(); }
            assert!(Instant::now() < deadline, "native frame timeout");
            self.state(); std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn control(&self, control: u32) {
        let mut error = abi::Error::default(); success(unsafe { self.api.control.unwrap()(self.handle, control, &mut error) }, &error);
    }
    pub fn wait_state(&self, expected: u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.state().state != expected { assert!(Instant::now() < deadline, "native state timeout"); std::thread::sleep(Duration::from_millis(1)); }
    }
    pub fn wait(&self, duration: Duration) -> bool {
        let deadline = Instant::now() + duration;
        loop {
            if self.state().state == abi::SESSION_FINISHED { return true; }
            if Instant::now() >= deadline { return false; }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn send(&self, events: &[abi::InputEvent]) {
        let mut error = abi::Error::default(); success(unsafe { self.api.input.unwrap()(self.handle, events.as_ptr(), events.len(), &mut error) }, &error);
    }
    pub fn event(&self, kind: u32) -> abi::InputEvent {
        abi::InputEvent { kind, pressed: 0, repeat: 0, button: 0, physical_key: abi::Bytes::EMPTY, logical_key: abi::Bytes::EMPTY, x: 0.0, y: 0.0, displayed: self.frame().identity(), window: window() }
    }
    pub fn key(&self, name: &'static str) {
        let mut press = self.event(abi::INPUT_KEY); press.physical_key = abi::Bytes::new(name.as_bytes()); press.logical_key = press.physical_key; press.pressed = 1;
        let mut release = press; release.pressed = 0; self.send(&[press, release]);
    }
    pub fn click(&self, x: f64, y: f64) {
        let mut motion = self.event(abi::INPUT_POINTER_MOVE); motion.x = x; motion.y = y;
        let mut press = motion; press.kind = abi::INPUT_POINTER_BUTTON; press.button = abi::BUTTON_PRIMARY; press.pressed = 1;
        let mut release = press; release.pressed = 0; self.send(&[motion, press, release]);
    }
    pub fn close(mut self) {
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        let mut error = abi::Error::default(); success(unsafe { self.api.close.unwrap()(handle, &mut error) }, &error);
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            self.cancel.store(true, Ordering::Release);
            unsafe { self.api.cancel.unwrap()(self.handle); self.api.close.unwrap()(self.handle, std::ptr::null_mut()); }
        }
    }
}
pub fn digest(frame: &abi::CpuFrame) -> (u64, usize) {
    frame.pixels().chunks_exact(4).fold((0, 0), |(sum, lit), p| (sum + p[..3].iter().map(|b| u64::from(*b)).sum::<u64>(), lit + usize::from(p[..3].iter().any(|b| *b > 24))))
}
pub fn rewrite_opening(session: &Native) {
    session.wait(Duration::from_secs(12)); session.click(640.0, 400.0);
    session.wait(Duration::from_secs(15)); session.click(452.0, 645.0);
    session.wait(Duration::from_secs(70)); session.click(452.0, 645.0);
    session.wait(Duration::from_secs(7));
    for _ in 0..150 { session.click(640.0, 400.0); if session.wait(Duration::from_millis(640)) { break; } }
}
