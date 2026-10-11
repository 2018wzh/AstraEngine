//! Consumer of the fixed private NativeOwner.h ABI 1 (core 021a30516).
//! The real DLL owns VM/GL/input and native worker cleanup. This adapter
//! supplies no substitute frames or native-ready implementation.
mod abi;
mod input;
mod output;

use super::{AudioCallbacks, NativeOwner};
use crate::wire::{self, Failure, FrameInfo, Reply, WindowState, WriteStatus};
use std::{
    ffi::c_void,
    marker::PhantomData,
    mem::ManuallyDrop,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

struct CallbackContext {
    audio: AudioCallbacks,
    failed: AtomicBool,
}
impl CallbackContext {
    fn failed(&self) {
        self.failed.store(true, Ordering::Release);
        self.audio.cancel();
    }
    fn callbacks(&mut self) -> abi::Callbacks {
        abi::Callbacks {
            context: std::ptr::from_mut(self).cast(),
            write_i16,
            progress,
            cancel_audio,
            is_cancelled,
        }
    }
}

unsafe extern "C" fn write_i16(context: *mut c_void, samples: *const i16, frames: u32) -> u32 {
    let Some(context) = (unsafe { context.cast::<CallbackContext>().as_ref() }) else {
        return abi::WRITE_FAILED;
    };
    if samples.is_null() || frames as usize != wire::PCM_FRAMES {
        context.failed();
        return abi::WRITE_FAILED;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // NativeOwner.h guarantees this borrowed exact 480-frame block is
        // readable through the callback. Host copies it before Accepted.
        context
            .audio
            .write(unsafe { std::slice::from_raw_parts(samples, wire::PCM_SAMPLES) })
    }));
    match result {
        Ok(Ok(WriteStatus::Accepted)) => abi::WRITE_ACCEPTED,
        Ok(Ok(WriteStatus::Cancelled)) => abi::WRITE_CANCELLED,
        Ok(Ok(WriteStatus::Closed)) => abi::WRITE_CLOSED,
        Ok(Ok(WriteStatus::Failed(_))) | Ok(Err(_)) | Err(_) => {
            context.failed();
            abi::WRITE_FAILED
        }
    }
}

unsafe extern "C" fn progress(context: *mut c_void, output: *mut abi::Progress) -> u32 {
    let Some(context) = (unsafe { context.cast::<CallbackContext>().as_ref() }) else {
        return 0;
    };
    if output.is_null() {
        context.failed();
        return 0;
    }
    match catch_unwind(AssertUnwindSafe(|| context.audio.progress())) {
        Ok(Some(observed)) => {
            // Paired real Host observation. No mixer lock, RPC or estimate.
            unsafe {
                output.write(abi::Progress {
                    accepted_source_frames: observed.accepted_source_frames,
                    played_source_frames: observed.played_source_frames,
                });
            }
            1
        }
        Ok(None) => 0,
        Err(_) => {
            context.failed();
            0
        }
    }
}

unsafe extern "C" fn cancel_audio(context: *mut c_void) {
    if let Some(context) = unsafe { context.cast::<CallbackContext>().as_ref() } {
        if catch_unwind(AssertUnwindSafe(|| context.audio.cancel())).is_err() {
            context.failed.store(true, Ordering::Release);
        }
    }
}

unsafe extern "C" fn is_cancelled(context: *mut c_void) -> u32 {
    match unsafe { context.cast::<CallbackContext>().as_ref() } {
        Some(context) => {
            u32::from(context.failed.load(Ordering::Acquire) || context.audio.is_cancelled())
        }
        None => 1,
    }
}

/// One private native owner in one fresh process. It cannot move to another
/// thread; the DLL remains resident until process exit, including boot errors.
pub struct DllOwner {
    api: Option<abi::Api>,
    owner: *mut c_void,
    context: Option<Box<CallbackContext>>,
    resources: Option<PathBuf>,
    _resident: Option<ManuallyDrop<libloading::Library>>,
    boot_started: bool,
    _owner_thread: PhantomData<Rc<()>>,
}
impl Default for DllOwner {
    fn default() -> Self {
        Self {
            api: None,
            owner: std::ptr::null_mut(),
            context: None,
            resources: None,
            _resident: None,
            boot_started: false,
            _owner_thread: PhantomData,
        }
    }
}

impl DllOwner {
    #[cfg(windows)]
    fn load(&mut self) -> Result<(), Failure> {
        use libloading::os::windows::{
            Library, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        };
        let executable = std::env::current_exe().map_err(|_| Failure::NativeUnavailable)?;
        let directory = executable.parent().ok_or(Failure::NativeUnavailable)?;
        let path = directory
            .join("astra_krkr_native.dll")
            .canonicalize()
            .map_err(|_| Failure::NativeUnavailable)?;
        let resources = directory.join("Resources");
        if !resources.is_dir() {
            return Err(Failure::NativeUnavailable);
        }
        // Fixed build layout. Native/dependency DLLs come from this directory
        // or system/default DLL directories, never an arbitrary CWD.
        let library: libloading::Library = unsafe {
            Library::load_with_flags(
                path,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        }
        .map_err(|error| {
            eprintln!("KrKr native library load failed: {error}");
            Failure::NativeUnavailable
        })?
        .into();
        self._resident = Some(ManuallyDrop::new(library));
        let library = self._resident.as_ref().unwrap();
        // Types and calling convention are copied from NativeOwner.h ABI 1.
        unsafe {
            let version = library
                .get::<unsafe extern "C" fn() -> u32>(b"astra_krkr_native_version\0")
                .map_err(|_| Failure::NativeUnavailable)?;
            if version() != abi::VERSION {
                return Err(Failure::NativeUnavailable);
            }
            self.api = Some(abi::Api {
                boot: *library
                    .get(b"astra_krkr_owner_boot\0")
                    .map_err(|_| Failure::NativeUnavailable)?,
                advance: *library
                    .get(b"astra_krkr_owner_advance\0")
                    .map_err(|_| Failure::NativeUnavailable)?,
                frame_info: *library
                    .get(b"astra_krkr_owner_frame_info\0")
                    .map_err(|_| Failure::NativeUnavailable)?,
                copy_frame: *library
                    .get(b"astra_krkr_owner_copy_frame\0")
                    .map_err(|_| Failure::NativeUnavailable)?,
                shutdown: *library
                    .get(b"astra_krkr_owner_shutdown\0")
                    .map_err(|_| Failure::NativeUnavailable)?,
            });
        }
        self.resources = Some(resources);
        Ok(())
    }
    #[cfg(not(windows))]
    fn load(&mut self) -> Result<(), Failure> {
        Err(Failure::NativeUnavailable)
    }

    fn active(&self) -> Result<(abi::Api, *mut c_void), Failure> {
        if self.owner.is_null() {
            return Err(Failure::Protocol);
        }
        if self
            .context
            .as_ref()
            .is_some_and(|context| context.failed.load(Ordering::Acquire))
        {
            return Err(Failure::Audio);
        }
        Ok((self.api.ok_or(Failure::NativeUnavailable)?, self.owner))
    }
}

fn path_bytes(path: &Path) -> Result<&str, Failure> {
    let text = path.to_str().ok_or(Failure::Boot)?;
    if !path.is_absolute() || text.is_empty() || text.len() > 4096 || text.contains('\0') {
        return Err(Failure::Boot);
    }
    Ok(text)
}

fn result(code: i32, error: &abi::Error, failure: Failure, operation: &str) -> Result<(), Failure> {
    let end = error
        .message
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Failure::Protocol)?;
    let message = std::str::from_utf8(&error.message[..end]).map_err(|_| Failure::Protocol)?;
    if code == abi::OK {
        if error.code != 0 || end != 0 {
            return Err(Failure::Protocol);
        }
        return Ok(());
    }
    if !(1..=5).contains(&code) {
        return Err(Failure::Protocol);
    }
    // Keep the control failure typed while forwarding the bounded native
    // message through the child's existing diagnostic bridge.
    eprintln!("KrKr native {operation} failed (code {code}): {message}");
    Err(failure)
}

impl NativeOwner for DllOwner {
    fn boot(
        &mut self,
        game_path: &str,
        window: WindowState,
        audio: AudioCallbacks,
    ) -> Result<FrameInfo, Failure> {
        if self.boot_started {
            return Err(Failure::Protocol);
        }
        self.boot_started = true;
        if self.api.is_none() {
            self.load()?;
        }
        let game = path_bytes(Path::new(game_path))?;
        let resources = path_bytes(
            self.resources
                .as_deref()
                .ok_or(Failure::NativeUnavailable)?,
        )?;
        if window.width == 0 || window.height == 0 {
            eprintln!(
                "KrKr native boot rejected window size {}x{}",
                window.width, window.height
            );
            return Err(Failure::Boot);
        }
        if audio.is_cancelled() {
            eprintln!("KrKr native boot rejected cancelled audio sink");
            return Err(Failure::Boot);
        }
        let mut context = Box::new(CallbackContext {
            audio,
            failed: AtomicBool::new(false),
        });
        let options = abi::BootOptions {
            game_path_utf8: game.as_ptr(),
            game_path_bytes: game.len() as u64,
            resource_root_utf8: resources.as_ptr(),
            resource_root_bytes: resources.len() as u64,
            window: abi::WindowState {
                width: window.width,
                height: window.height,
                focused: u32::from(window.focused),
                visible: u32::from(window.visible),
            },
            callbacks: context.callbacks(),
        };
        self.context = Some(context);
        let mut frame = abi::FrameInfo::default();
        let mut error = abi::Error::default();
        // Store the handle even when boot fails. The core has already begun
        // initialization and must receive shutdown before callbacks are freed.
        let code =
            unsafe { (self.api.unwrap().boot)(&options, &mut self.owner, &mut frame, &mut error) };
        result(code, &error, Failure::Boot, "boot")?;
        self.active()?;
        let frame = output::frame(frame)?;
        // A successful boot must have produced an actual completed frame.
        // Verify synchronous copy before emitting NativeReady, without
        // replacing native output with a synthetic buffer on failure.
        let (copied, _) = self.capture()?;
        if copied != frame {
            return Err(Failure::Frame);
        }
        Ok(frame)
    }

    fn advance(&mut self, elapsed_ns: u64, events: &[wire::Event]) -> Result<Reply, Failure> {
        let (api, owner) = self.active()?;
        let events = input::events(events)?;
        let mut output = abi::AdvanceResult::default();
        let mut error = abi::Error::default();
        let code = unsafe {
            (api.advance)(
                owner,
                elapsed_ns,
                events.as_ptr(),
                events.len() as u32,
                &mut output,
                &mut error,
            )
        };
        result(code, &error, Failure::Advance, "advance")?;
        self.active()?;
        // Borrowed output is copied before another advance or shutdown.
        unsafe { output::advance(&output) }
    }

    fn capture(&mut self) -> Result<(FrameInfo, Vec<u8>), Failure> {
        let (api, owner) = self.active()?;
        let mut native = abi::FrameInfo::default();
        let mut error = abi::Error::default();
        let code = unsafe { (api.frame_info)(owner, &mut native, &mut error) };
        result(code, &error, Failure::Frame, "frame_info")?;
        let info = output::frame(native)?;
        let size = info.required_bytes().map_err(|_| Failure::Frame)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(size).map_err(|_| Failure::Frame)?;
        pixels.resize(size, 0);
        let code = unsafe {
            (api.copy_frame)(owner, &native, pixels.as_mut_ptr(), size as u64, &mut error)
        };
        result(code, &error, Failure::Frame, "copy_frame")?;
        self.active()?;
        for row in pixels.chunks_exact(info.stride as usize) {
            if row[..info.width as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 255)
            {
                return Err(Failure::Frame);
            }
        }
        Ok((info, pixels))
    }

    fn shutdown(&mut self) -> Result<(), Failure> {
        let callback_failed = self
            .context
            .as_ref()
            .is_some_and(|context| context.failed.load(Ordering::Acquire));
        if let Some(context) = &self.context {
            context.audio.cancel();
        }
        let owner = std::mem::replace(&mut self.owner, std::ptr::null_mut());
        let outcome = if owner.is_null() {
            Ok(())
        } else {
            let mut error = abi::Error::default();
            let code = unsafe {
                (self.api.ok_or(Failure::NativeUnavailable)?.shutdown)(owner, &mut error)
            };
            result(code, &error, Failure::Shutdown, "shutdown")
        };
        // The consuming native call has completed every native cleanup/join
        // even when it reports failure. Only now may the context be dropped.
        let context = self.context.take();
        if callback_failed
            || context
                .as_ref()
                .is_some_and(|context| context.failed.load(Ordering::Acquire))
                && outcome.is_ok()
        {
            return Err(Failure::Audio);
        }
        outcome
    }
}

impl Drop for DllOwner {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child::audio::AudioState;
    use crossbeam_channel::bounded;
    use std::sync::{atomic::AtomicUsize, Arc};

    struct Partial {
        callbacks: abi::Callbacks,
        shutdowns: Arc<AtomicUsize>,
    }
    unsafe extern "C" fn partial_boot(
        options: *const abi::BootOptions,
        owner: *mut *mut c_void,
        _: *mut abi::FrameInfo,
        error: *mut abi::Error,
    ) -> i32 {
        let callbacks = unsafe { (*options).callbacks };
        let context = unsafe { &*callbacks.context.cast::<CallbackContext>() };
        // Test-only partial-initialization owner. It never returns success,
        // produces a frame, starts GL/VM or advertises NativeReady.
        let partial = Box::new(Partial {
            callbacks,
            shutdowns: Arc::new(AtomicUsize::new(0)),
        });
        assert!(!context.audio.is_cancelled());
        unsafe {
            *owner = Box::into_raw(partial).cast();
            (*error).code = 3;
        }
        3
    }
    unsafe extern "C" fn shutdown(owner: *mut c_void, error: *mut abi::Error) -> i32 {
        let partial = unsafe { Box::from_raw(owner.cast::<Partial>()) };
        // The context is still readable after cancel and until native join.
        assert_eq!(
            unsafe { (partial.callbacks.is_cancelled)(partial.callbacks.context) },
            1
        );
        let mut progress = abi::Progress {
            accepted_source_frames: 99,
            played_source_frames: 99,
        };
        assert_eq!(
            unsafe { (partial.callbacks.progress)(partial.callbacks.context, &mut progress) },
            0
        );
        partial.shutdowns.fetch_add(1, Ordering::Release);
        unsafe {
            (*error).code = 3;
        }
        3 // Cleanup is complete but native shutdown reported a real failure.
    }
    unsafe extern "C" fn unavailable_advance(
        _: *mut c_void,
        _: u64,
        _: *const abi::Event,
        _: u32,
        _: *mut abi::AdvanceResult,
        _: *mut abi::Error,
    ) -> i32 {
        3
    }
    unsafe extern "C" fn unavailable_frame(
        _: *mut c_void,
        _: *mut abi::FrameInfo,
        _: *mut abi::Error,
    ) -> i32 {
        3
    }
    unsafe extern "C" fn unavailable_copy(
        _: *mut c_void,
        _: *const abi::FrameInfo,
        _: *mut u8,
        _: u64,
        _: *mut abi::Error,
    ) -> i32 {
        3
    }

    #[test]
    fn partial_boot_keeps_callback_context_until_consuming_shutdown_even_on_error() {
        let (pcm, _requests) = bounded(1);
        let (_acks, replies) = bounded(1);
        let (socket, _peer) = crate::sockets::pair();
        let state = AudioState::new(17, pcm, replies, socket).unwrap();
        let weak = Arc::downgrade(&state);
        let callbacks = AudioCallbacks(state);
        let mut owner = DllOwner::default();
        owner.api = Some(abi::Api {
            boot: partial_boot,
            advance: unavailable_advance,
            frame_info: unavailable_frame,
            copy_frame: unavailable_copy,
            shutdown,
        });
        owner.resources = Some(std::env::temp_dir());
        let game = std::env::temp_dir();
        assert_eq!(
            owner.boot(
                game.to_str().unwrap(),
                WindowState {
                    width: 640,
                    height: 480,
                    focused: true,
                    visible: true
                },
                callbacks
            ),
            Err(Failure::Boot)
        );
        assert!(weak.upgrade().is_some());
        let shutdowns = Arc::clone(&unsafe { &*owner.owner.cast::<Partial>() }.shutdowns);
        assert_eq!(owner.shutdown(), Err(Failure::Shutdown));
        assert!(weak.upgrade().is_none());
        assert_eq!(shutdowns.load(Ordering::Acquire), 1);
        drop(owner);
        assert_eq!(shutdowns.load(Ordering::Acquire), 1);
    }

    #[test]
    fn unknown_status_and_unterminated_error_are_rejected() {
        let mut error = abi::Error::default();
        assert_eq!(
            result(99, &error, Failure::Boot, "boot"),
            Err(Failure::Protocol)
        );
        error.message.fill(b'x');
        assert_eq!(
            result(3, &error, Failure::Boot, "boot"),
            Err(Failure::Protocol)
        );
        error.message = [0; 512];
        error.code = 3;
        assert_eq!(
            result(0, &error, Failure::Boot, "boot"),
            Err(Failure::Protocol)
        );
    }

    #[test]
    fn callback_failure_remains_first_when_consuming_cleanup_also_reports_failure() {
        let (pcm, _requests) = bounded(1);
        let (_acks, replies) = bounded(1);
        let (socket, _peer) = crate::sockets::pair();
        let state = AudioState::new(18, pcm, replies, socket).unwrap();
        let mut owner = DllOwner::default();
        owner.api = Some(abi::Api {
            boot: partial_boot,
            advance: unavailable_advance,
            frame_info: unavailable_frame,
            copy_frame: unavailable_copy,
            shutdown,
        });
        owner.resources = Some(std::env::temp_dir());
        let game = std::env::temp_dir();
        assert_eq!(
            owner.boot(
                game.to_str().unwrap(),
                WindowState {
                    width: 640,
                    height: 480,
                    focused: true,
                    visible: true
                },
                AudioCallbacks(state)
            ),
            Err(Failure::Boot)
        );
        owner.context.as_ref().unwrap().failed();
        assert_eq!(owner.shutdown(), Err(Failure::Audio));
        assert!(owner.owner.is_null());
        assert!(owner.context.is_none());
    }
}
