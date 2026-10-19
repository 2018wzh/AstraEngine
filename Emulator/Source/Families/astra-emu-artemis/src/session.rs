//! Dedicated native owner: original VM/GPU/media/save, autonomous scheduling.
use crate::{
    audio::{AudioBridge, MixerCommand},
    error::{self, NativeError, NativeResult},
    events::{self, Input},
    media, provider,
};
use art3m1s_core::{
    backend::BackendSelection, host_events::HostEvents, host_files::HostResources,
    runtime::CoreRuntime,
};
use astra_emu_abi::*;
use parking_lot::Mutex;
use std::{
    ops::ControlFlow,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
pub(crate) const PROJECT_INI: &str = "system.ini";
#[derive(Clone, Copy)]
pub(crate) struct Platform(pub WindowService, pub CancellationService);
// SAFETY: callbacks are thread-safe and valid through close; no runtime crosses threads.
unsafe impl Send for Platform {}
unsafe impl Sync for Platform {}
impl Platform {
    fn cancelled(self, epoch: u64) -> bool {
        self.1
            .is_cancelled
            .is_none_or(|callback| unsafe { callback(self.1.context, epoch) != 0 })
    }
    fn check(self, epoch: u64) -> NativeResult<()> {
        if self.cancelled(epoch) {
            Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_CANCELLED",
                "session was cancelled",
            ))
        } else {
            Ok(())
        }
    }
}
pub(crate) struct Snapshot {
    pub info: SurfaceInfo,
    pub identity: FrameIdentity,
    pub pixels: Option<Arc<Vec<u8>>>,
    pub timestamp_ns: u64,
    pub state: u32,
    pub error: Option<NativeError>,
}
pub(crate) struct SessionOwner {
    commands: SyncSender<Command>,
    pub snapshot: Arc<Mutex<Snapshot>>,
    pub context: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
    platform: Platform,
    epoch: u64,
    wake: std::thread::Thread,
    thread: Option<JoinHandle<()>>,
}
enum Command {
    Input(Vec<Input>, SyncSender<NativeResult<()>>),
    Pause(bool, SyncSender<NativeResult<()>>),
}
struct OwnerState<'a> {
    shared: &'a Mutex<Snapshot>,
    stop: &'a AtomicBool,
    context: &'a AtomicU64,
}
impl SessionOwner {
    pub(crate) fn open(
        path: PathBuf,
        archive: PathBuf,
        extra_archive: String,
        epoch: u64,
        window: WindowState,
        platform: Platform,
    ) -> NativeResult<Box<Self>> {
        let lease = provider::SessionLease::acquire()?;
        let (tx, rx) = mpsc::sync_channel(128);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let context = Arc::new(AtomicU64::new(1));
        let snapshot = Arc::new(Mutex::new(Snapshot {
            info: surface(1, 1),
            identity: FrameIdentity {
                session_epoch: epoch,
                output_epoch: 1,
                generation: 0,
            },
            pixels: None,
            timestamp_ns: 0,
            state: SESSION_RUNNING,
            error: None,
        }));
        let shared = snapshot.clone();
        let stop = cancelled.clone();
        let generation = context.clone();
        let thread = std::thread::Builder::new()
            .name("artemis-native-owner".into())
            .spawn(move || {
                let _lease = lease;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    platform.check(epoch)?;
                    let mut runtime =
                        Runtime::boot(path, archive, extra_archive, stop.clone(), platform, epoch)?;
                    {
                        let mut state = shared.lock();
                        state.info = surface(runtime.rt.stage_width(), runtime.rt.stage_height());
                        state.info.validate().map_err(|_| {
                            error::invalid(
                                "ASTRA_EMU_ARTEMIS_FRAME_SIZE",
                                "invalid native stage dimensions",
                            )
                        })?;
                    }
                    platform.check(epoch)?;
                    let _ = ready_tx.send(Ok(()));
                    runtime.run(
                        rx,
                        OwnerState {
                            shared: &shared,
                            stop: &stop,
                            context: &generation,
                        },
                        epoch,
                        window,
                        platform,
                    )
                }));
                let failure = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(_) => Some(error::invalid(
                        "ASTRA_EMU_ARTEMIS_PANIC",
                        "native owner panicked",
                    )),
                };
                let mut state = shared.lock();
                state.state = if failure.is_some() {
                    SESSION_FAILED
                } else {
                    SESSION_FINISHED
                };
                state.error = failure.clone();
                if let Some(error) = failure {
                    let _ = ready_tx.try_send(Err(error));
                }
            })
            .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_WORKER", e.to_string()))?;
        let wake = thread.thread().clone();
        let mut owner = Box::new(Self {
            commands: tx,
            snapshot,
            context,
            cancelled,
            platform,
            epoch,
            wake,
            thread: Some(thread),
        });
        match ready_rx.recv() {
            Ok(Ok(())) if !platform.cancelled(epoch) => Ok(owner),
            value => {
                let failure = value.ok().and_then(Result::err).unwrap_or_else(|| {
                    error::invalid(
                        "ASTRA_EMU_ARTEMIS_BOOT",
                        "native owner stopped or was cancelled during open",
                    )
                });
                let _ = owner.shutdown();
                Err(failure)
            }
        }
    }
    fn request(
        &self,
        make: impl FnOnce(SyncSender<NativeResult<()>>) -> Command,
    ) -> NativeResult<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_CANCELLED",
                "session is cancelled",
            ));
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.commands.try_send(make(tx)).map_err(|_| {
            error::invalid(
                "ASTRA_EMU_ARTEMIS_COMMAND_QUEUE",
                "native command queue unavailable",
            )
        })?;
        rx.recv().map_err(|_| {
            self.snapshot.lock().error.clone().unwrap_or_else(|| {
                error::invalid("ASTRA_EMU_ARTEMIS_CLOSED", "native owner stopped")
            })
        })?
    }
    pub(crate) fn input(&self, input: Vec<Input>) -> NativeResult<()> {
        self.request(|reply| Command::Input(input, reply))
    }
    pub(crate) fn pause(&self, paused: bool) -> NativeResult<()> {
        self.request(|reply| Command::Pause(paused, reply))
    }
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(invalidate) = self.platform.0.invalidate {
            unsafe {
                invalidate(
                    self.platform.0.context,
                    self.epoch,
                    self.context.load(Ordering::Acquire),
                )
            };
        }
        self.wake.unpark();
    }
    pub(crate) fn shutdown(&mut self) -> NativeResult<()> {
        self.cancel();
        let joined = self.thread.take().map(|thread| thread.join()).transpose();
        if joined.is_err() {
            return Err(error::invalid(
                "ASTRA_EMU_ARTEMIS_PANIC",
                "native owner join failed",
            ));
        }
        let state = self.snapshot.lock();
        state.error.clone().map_or(Ok(()), Err)
    }
}
impl Drop for SessionOwner {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.shutdown();
        }
    }
}
struct Runtime {
    rt: CoreRuntime,
    audio: Option<AudioBridge>,
    resources: HostResources,
    events: HostEvents,
    window: media::WindowRequests,
}

/// Mount native resources without starting a VM, graphics backend or save IO.
pub(crate) fn project_resources(
    archive: &std::path::Path,
    extra_archive: &str,
    mut check: impl FnMut() -> NativeResult<()>,
) -> NativeResult<(HostResources, Vec<u8>)> {
    let resources = HostResources::new();
    let mut mount_error = None;
    for encoding in provider::ENTRY_ENCODINGS {
        check()?;
        match resources.mount_pfs_with_extra(archive, encoding.name(), extra_archive) {
            Ok(()) => {
                if let Ok(ini) = resources.read_file(PROJECT_INI) {
                    return Ok((resources, ini));
                }
            }
            Err(error) => mount_error = Some(error),
        }
    }
    if !extra_archive.is_empty() {
        if let Some(error) = mount_error {
            return Err(error::engine(error));
        }
    }
    Err(error::invalid(
        "ASTRA_EMU_ARTEMIS_PROJECT_INI",
        "PFS has no readable system.ini",
    ))
}

impl Runtime {
    fn boot(
        path: PathBuf,
        archive: PathBuf,
        extra_archive: String,
        stop: Arc<AtomicBool>,
        platform: Platform,
        epoch: u64,
    ) -> NativeResult<Self> {
        let (resources, ini) =
            project_resources(&archive, &extra_archive, || platform.check(epoch))?;
        let save_dir = path.join("savedata");
        std::fs::create_dir_all(&save_dir)
            .map_err(|e| error::invalid("ASTRA_EMU_ARTEMIS_SAVE_DIR", e.to_string()))?;
        resources
            .set_save_dir(Some(&save_dir))
            .map_err(error::engine)?;
        #[cfg(target_os = "macos")]
        let backend = BackendSelection::NativeMetal;
        #[cfg(not(target_os = "macos"))]
        let backend = BackendSelection::NativeVulkanHardware;
        let mut rt = CoreRuntime::create(1280, 720, backend).map_err(error::engine)?;
        platform.check(epoch)?;
        rt.set_resources(resources.clone());
        rt.set_runtime_media_enabled(true);
        let events = HostEvents::new();
        events.set_enabled(true);
        let mut native = Self {
            rt,
            audio: None,
            resources,
            events,
            window: media::WindowRequests::default(),
        };
        native.audio = Some(AudioBridge::start(stop)?);
        platform.check(epoch)?;
        native
            .rt
            .load_project_bytes(&ini, "WINDOWS")
            .map_err(error::engine)?;
        native.rt.take_timeline_reset();
        Ok(native)
    }
    fn drain(
        &mut self,
        platform: Platform,
        epoch: u64,
        context: u64,
        request: &mut u64,
    ) -> NativeResult<ControlFlow<(), bool>> {
        let audio = self.audio.as_ref().unwrap();
        if platform.cancelled(epoch) {
            return Ok(ControlFlow::Break(()));
        }
        for finished in audio.drain_finished() {
            self.rt.notify_sound_finished(finished.id.as_deref());
        }
        for command in media::drain(&self.events, &self.resources, &mut self.window)? {
            if audio.send(command)?.is_break() {
                return Ok(ControlFlow::Break(()));
            }
        }
        let mut serviced = false;
        while let Some(command) = self.window.take() {
            *request = request.checked_add(1).ok_or_else(|| {
                error::invalid("ASTRA_EMU_ARTEMIS_REQUEST_ID", "request IDs exhausted")
            })?;
            if crate::platform::request(platform, command, epoch, context, *request)?.is_break() {
                return Ok(ControlFlow::Break(()));
            }
            serviced = true;
        }
        audio.check_error()?;
        Ok(ControlFlow::Continue(serviced))
    }
    fn run(
        &mut self,
        rx: Receiver<Command>,
        owner: OwnerState<'_>,
        epoch: u64,
        mut window: WindowState,
        platform: Platform,
    ) -> NativeResult<()> {
        let OwnerState {
            shared,
            stop,
            context,
        } = owner;
        let mut explicit_pause = false;
        let mut paused = false;
        let mut last = Instant::now();
        let started = last;
        let mut remainder = 0u128;
        let mut request = 0u64;
        let mut pending = false;
        let mut window_state_dirty = true;
        let mut pool: [Option<Arc<Vec<u8>>>; 3] = [None, None, None];
        'owner: while !stop.load(Ordering::Acquire) && !platform.cancelled(epoch) {
            for command in rx.try_iter() {
                match command {
                    Command::Pause(value, reply) => {
                        explicit_pause = value;
                        let effective = value || window.suspended != 0 || window.focused == 0;
                        let result = if effective != paused {
                            self.audio
                                .as_ref()
                                .unwrap()
                                .send(MixerCommand::Suspend(effective))
                        } else {
                            Ok(ControlFlow::Continue(()))
                        };
                        let result = match result {
                            Ok(ControlFlow::Break(())) => {
                                let _ = reply.send(Err(error::invalid(
                                    "ASTRA_EMU_ARTEMIS_CANCELLED",
                                    "session was cancelled",
                                )));
                                break 'owner;
                            }
                            result => result.map(|_| ()),
                        };
                        if result.is_ok() {
                            if effective != paused {
                                self.rt.notify_lifecycle(if effective { 1 } else { 2 });
                            }
                            paused = effective;
                            remainder = 0;
                            last = Instant::now();
                            shared.lock().state = if paused {
                                SESSION_PAUSED
                            } else {
                                SESSION_RUNNING
                            };
                        }
                        let _ = reply.send(result.clone());
                        result?;
                    }
                    Command::Input(inputs, reply) => {
                        let state = shared.lock();
                        let mut result = Ok(());
                        for input in inputs {
                            if let Input::Window(value) = input {
                                window = value;
                                window_state_dirty = true;
                            } else if let Err(error) =
                                events::apply(&self.rt, input, state.info, state.identity)
                            {
                                result = Err(error);
                                break;
                            }
                        }
                        let _ = reply.send(result);
                    }
                }
            }
            if window_state_dirty {
                request = request.checked_add(1).ok_or_else(|| {
                    error::invalid("ASTRA_EMU_ARTEMIS_REQUEST_ID", "request IDs exhausted")
                })?;
                let flags = match crate::platform::query_state(
                    platform,
                    epoch,
                    context.load(Ordering::Acquire),
                    request,
                )? {
                    ControlFlow::Continue(flags) => flags,
                    ControlFlow::Break(()) => break,
                };
                self.events.set_window_state(flags);
                window_state_dirty = false;
                last = Instant::now();
                remainder = 0;
            }
            let next_pause = explicit_pause || window.suspended != 0 || window.focused == 0;
            if next_pause != paused {
                if self
                    .audio
                    .as_ref()
                    .unwrap()
                    .send(MixerCommand::Suspend(next_pause))?
                    .is_break()
                {
                    break;
                }
                self.rt.notify_lifecycle(if next_pause { 1 } else { 2 });
                paused = next_pause;
                remainder = 0;
                last = Instant::now();
                shared.lock().state = if paused {
                    SESSION_PAUSED
                } else {
                    SESSION_RUNNING
                };
            }
            if paused {
                std::thread::park_timeout(Duration::from_millis(4));
                continue;
            }
            let now = Instant::now();
            remainder += now.duration_since(last).as_nanos();
            last = now;
            let elapsed = (remainder / 1_000_000).min(u64::MAX as u128) as u64;
            remainder %= 1_000_000;
            match self.drain(
                platform,
                epoch,
                context.load(Ordering::Acquire),
                &mut request,
            )? {
                ControlFlow::Break(()) => break,
                ControlFlow::Continue(true) => {
                    last = Instant::now();
                    remainder = 0;
                }
                ControlFlow::Continue(false) => {}
            }
            pending |= self.rt.try_advance_frame(elapsed).map_err(error::engine)?;
            if self.rt.take_timeline_reset() {
                let old = {
                    let mut state = shared.lock();
                    let old = context.load(Ordering::Acquire);
                    let next = old.checked_add(1).ok_or_else(|| {
                        error::invalid("ASTRA_EMU_ARTEMIS_GENERATION", "context epoch exhausted")
                    })?;
                    state.identity.output_epoch =
                        state.identity.output_epoch.checked_add(1).ok_or_else(|| {
                            error::invalid("ASTRA_EMU_ARTEMIS_GENERATION", "output epoch exhausted")
                        })?;
                    state.pixels = None;
                    state.identity.generation = 0;
                    context.store(next, Ordering::Release);
                    old
                };
                if let Some(invalidate) = platform.0.invalidate {
                    unsafe { invalidate(platform.0.context, epoch, old) };
                }
                pending = true;
                remainder = 0;
                last = Instant::now();
            }
            match self.drain(
                platform,
                epoch,
                context.load(Ordering::Acquire),
                &mut request,
            )? {
                ControlFlow::Break(()) => break,
                ControlFlow::Continue(true) => {
                    last = Instant::now();
                    remainder = 0;
                }
                ControlFlow::Continue(false) => {}
            }
            if pending {
                if let Some(slot) = pool.iter_mut().find(|slot| {
                    slot.as_ref()
                        .is_none_or(|pixels| Arc::strong_count(pixels) == 1)
                }) {
                    let pixels = slot.get_or_insert_with(|| Arc::new(Vec::new()));
                    let bytes = Arc::get_mut(pixels).unwrap();
                    bytes.resize(self.rt.pixel_buffer_size(), 0);
                    self.rt
                        .try_capture_frame_into(bytes)
                        .map_err(error::engine)?;
                    let info = surface(self.rt.stage_width(), self.rt.stage_height());
                    info.validate().map_err(|_| {
                        error::invalid("ASTRA_EMU_ARTEMIS_FRAME_SIZE", "invalid complete frame")
                    })?;
                    let mut state = shared.lock();
                    if state.info.logical != info.logical || state.info.raster != info.raster {
                        state.identity.output_epoch =
                            state.identity.output_epoch.checked_add(1).ok_or_else(|| {
                                error::invalid(
                                    "ASTRA_EMU_ARTEMIS_GENERATION",
                                    "output epoch exhausted",
                                )
                            })?;
                    }
                    state.info = info;
                    state.identity.generation =
                        state.identity.generation.checked_add(1).ok_or_else(|| {
                            error::invalid(
                                "ASTRA_EMU_ARTEMIS_GENERATION",
                                "frame generation exhausted",
                            )
                        })?;
                    state.pixels = Some(pixels.clone());
                    state.timestamp_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
                    pending = false;
                }
            }
            if self.rt.is_exit_requested() {
                break;
            }
            std::thread::park_timeout(Duration::from_millis(4));
        }
        self.rt.notify_lifecycle(0);
        if let Some(audio) = self.audio.take() {
            audio.close()?;
        }
        Ok(())
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(audio) = self.audio.take() {
            let _ = audio.close();
        }
        self.events.set_enabled(false);
    }
}
pub(crate) fn surface(width: u32, height: u32) -> SurfaceInfo {
    SurfaceInfo {
        kind: SURFACE_CPU,
        pixel_format: PIXEL_RGBA8,
        color: COLOR_SRGB,
        alpha: ALPHA_IGNORED,
        rows: ROWS_TOP_DOWN,
        logical: Extent { width, height },
        raster: Extent { width, height },
        stride: u64::from(width) * 4,
    }
}
