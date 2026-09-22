//! Artemis family session: owns the engine runtime, the host-event queue, the
//! audio bridge, and the CPU frame snapshot handed to the host.
//!
//! Ownership is pull-based and single-threaded: the session thread mounts the
//! game archive, boots the runtime on the offscreen GPU backend, and drives
//! one engine tick per `advance`; mixed PCM comes from the mixer worker
//! thread through the bounded host sink queue.
//!
//! Save data stays in the game directory (`<game>/savedata`), owned entirely
//! by the engine; the host provides only the game path.

use std::cell::{Cell, RefCell};
use std::path::Path;

use art3m1s_core::backend::BackendSelection;
use art3m1s_core::host_events::HostEvents;
use art3m1s_core::host_files::HostResources;
use astra_emu_family_api::{
    AdvanceResponse, AudioSinkBox, FamilyError, FamilyEvent, FamilyResult, FamilySession,
    FamilyStatus, FrameAlpha, FrameFormat, FrameInfo, FrameView, FrameVisitor, WindowState,
};

use art3m1s_core::runtime::CoreRuntime;

use crate::audio::AudioBridge;
use crate::{error, events, media, provider};

/// The Artemis boot configuration entry inside the base archive.
pub(crate) const PROJECT_INI: &str = "system.ini";
/// How many settle frames `open` may spend waiting for the first composed
/// frame before failing the boot.
const FIRST_FRAME_SETTLE_FRAMES: u32 = 600;
const FRAME_INTERVAL_MS: u64 = 16;

pub(crate) struct ArtemisOpen {
    pub(crate) session: ArtemisSession,
    pub(crate) frame_info: FrameInfo,
}

pub(crate) struct ArtemisSession {
    audio: AudioBridge,
    rt: CoreRuntime,
    resources: HostResources,
    events: HostEvents,
    _lease: provider::SessionLease,
    suspended: bool,
    pixels: RefCell<Vec<u8>>,
    frame_info: Cell<FrameInfo>,
    elapsed_remainder_ns: u64,
    fatal: Option<FamilyError>,
}

pub(crate) fn boot(
    game_path: &Path,
    base_archive: &Path,
    initial_window: WindowState,
    sink: AudioSinkBox,
    lease: provider::SessionLease,
) -> FamilyResult<ArtemisOpen> {
    // Mount with the first entry-name encoding that exposes the boot
    // configuration; a failed attempt is replaced by the next one.
    let resources = HostResources::new();
    let mut ini = None;
    for encoding in provider::ENTRY_ENCODINGS {
        if resources.mount_pfs(base_archive, encoding.name()).is_err() {
            continue;
        }
        match resources.read_file(PROJECT_INI) {
            Ok(bytes) => {
                ini = Some(bytes);
                break;
            }
            Err(_) => continue,
        }
    }
    let Some(ini) = ini else {
        return Err(error::invalid(
            "ASTRA_EMU_ARTEMIS_PROJECT_INI",
            "the PFS archive has no readable system.ini entry",
        ));
    };

    let save_dir = game_path.join("savedata");
    std::fs::create_dir_all(&save_dir).map_err(|_| {
        error::invalid(
            "ASTRA_EMU_ARTEMIS_SAVE_DIR",
            "the game save directory is not creatable",
        )
    })?;
    resources
        .set_save_dir(Some(&save_dir))
        .map_err(|save_error| {
            error::invalid(
                "ASTRA_EMU_ARTEMIS_SAVE_DIR",
                format!("set save dir: {save_error}"),
            )
        })?;

    // One active session per process: the host-events queue routes
    // process-wide core output to the currently enabled handle only.
    let host_events = HostEvents::new();
    host_events.set_enabled(true);
    host_events.set_window_state(if initial_window.focused { 1 } else { 0 });

    let boot_result = boot_engine(&resources, &host_events, &ini, initial_window, sink, lease);
    if boot_result.is_err() {
        // Unwind the process-global host state so a later session cannot
        // inherit a dead handle; the audio bridge cancels its worker.
        host_events.set_enabled(false);
    }
    boot_result
}

fn boot_engine(
    resources: &HostResources,
    host_events: &HostEvents,
    ini: &[u8],
    initial_window: WindowState,
    sink: AudioSinkBox,
    lease: provider::SessionLease,
) -> FamilyResult<ArtemisOpen> {
    let audio = AudioBridge::start(sink)?;
    let mut rt = match build_runtime(resources, ini, initial_window) {
        Ok(rt) => rt,
        Err(boot_error) => {
            let _ = audio.close();
            host_events.set_enabled(false);
            return Err(boot_error);
        }
    };

    let width = rt.stage_width();
    let height = rt.stage_height();
    let frame_info = match frame_info_of(width, height) {
        Ok(info) => info,
        Err(frame_error) => {
            let _ = audio.close();
            host_events.set_enabled(false);
            return Err(frame_error);
        }
    };
    let mut pixels = vec![0_u8; rt.pixel_buffer_size()];

    let mut settled = false;
    for _ in 0..FIRST_FRAME_SETTLE_FRAMES {
        let mut tick = TickContext {
            host_events,
            resources,
            audio: &audio,
        };
        match pump(
            &mut rt,
            &mut tick,
            pixels.as_mut_slice(),
            FRAME_INTERVAL_MS,
            &[],
        ) {
            Ok(written) if written > 0 => {
                settled = true;
                break;
            }
            Ok(_) => {}
            Err(step_error) => {
                let _ = audio.close();
                host_events.set_enabled(false);
                return Err(step_error);
            }
        }
    }
    if !settled {
        let _ = audio.close();
        host_events.set_enabled(false);
        return Err(error::invalid(
            "ASTRA_EMU_ARTEMIS_BOOT_FRAME",
            "the Artemis engine produced no composed frame during boot",
        ));
    }

    Ok(ArtemisOpen {
        frame_info,
        session: ArtemisSession {
            rt,
            resources: resources.clone(),
            events: host_events.clone(),
            _lease: lease,
            suspended: false,
            audio,
            pixels: RefCell::new(pixels),
            frame_info: Cell::new(frame_info),
            elapsed_remainder_ns: 0,
            fatal: None,
        },
    })
}

fn build_runtime(
    resources: &HostResources,
    ini: &[u8],
    initial_window: WindowState,
) -> FamilyResult<CoreRuntime> {
    let mut rt = CoreRuntime::create(
        initial_window.width.max(1),
        initial_window.height.max(1),
        BackendSelection::NativeVulkan,
    )
    .map_err(error::engine)?;
    rt.set_resources(resources.clone());
    rt.load_project_bytes(ini, "WINDOWS")
        .map_err(error::engine)?;
    Ok(rt)
}

fn frame_info_of(width: u32, height: u32) -> FamilyResult<FrameInfo> {
    if width == 0 || height == 0 {
        return Err(error::invalid(
            "ASTRA_EMU_ARTEMIS_FRAME_EMPTY",
            "the engine reported an empty stage size",
        ));
    }
    Ok(FrameInfo {
        width,
        height,
        logical_width: width,
        logical_height: height,
        stride: width.checked_mul(4).ok_or_else(|| {
            error::invalid("ASTRA_EMU_ARTEMIS_FRAME_SIZE", "frame stride overflows")
        })?,
        format: FrameFormat::Rgba8Srgb {
            alpha: FrameAlpha::Opaque,
        },
    })
}

/// Applies finished-sound notifications and queued media commands, injects
/// input, and runs one engine tick. Returns the number of rendered bytes.
/// Per-tick session state the pump consumes and updates.
struct TickContext<'a> {
    host_events: &'a HostEvents,
    resources: &'a HostResources,
    audio: &'a AudioBridge,
}

fn pump(
    rt: &mut CoreRuntime,
    tick: &mut TickContext<'_>,
    pixels: &mut [u8],
    elapsed_ms: u64,
    input: &[FamilyEvent],
) -> FamilyResult<usize> {
    for finished in tick.audio.drain_finished() {
        rt.notify_sound_finished(finished.id.as_deref());
    }
    for command in media::drain(tick.host_events, tick.resources)? {
        tick.audio.send(command)?;
    }
    events::apply(rt, input);
    let written = rt.advance_and_render_into(elapsed_ms, pixels);
    for command in media::drain(tick.host_events, tick.resources)? {
        tick.audio.send(command)?;
    }
    tick.audio.check_error()?;
    Ok(written)
}

impl ArtemisSession {
    fn ensure_live(&self) -> FamilyResult<()> {
        if let Some(value) = &self.fatal {
            return Err(value.clone());
        }
        Ok(())
    }
}

impl FamilySession for ArtemisSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        self.ensure_live()?;
        self.audio.check_error()?;
        let mut changed = false;
        for event in events {
            if let FamilyEvent::WindowSuspended { suspended } = event {
                if self.suspended != *suspended {
                    self.suspended = *suspended;
                    changed = true;
                }
            }
            if matches!(event, FamilyEvent::WindowCloseRequested) {
                return Ok(AdvanceResponse {
                    status: FamilyStatus::Finished,
                    ..AdvanceResponse::running()
                });
            }
        }
        if changed {
            self.audio
                .send(crate::audio::MixerCommand::Suspend(self.suspended))?;
        }
        if self.suspended || changed {
            self.elapsed_remainder_ns = 0;
            return Ok(AdvanceResponse {
                reset_clock: true,
                ..AdvanceResponse::running()
            });
        }
        let elapsed = self
            .elapsed_remainder_ns
            .checked_add(elapsed_ns)
            .ok_or_else(|| error::invalid("ASTRA_EMU_ARTEMIS_TIME", "elapsed time overflow"))?;
        let elapsed_ms = elapsed / 1_000_000;
        self.elapsed_remainder_ns = elapsed % 1_000_000;
        let mut pixels = self.pixels.borrow_mut();
        let mut tick = TickContext {
            host_events: &self.events,
            resources: &self.resources,
            audio: &self.audio,
        };
        if let Err(failure) = pump(&mut self.rt, &mut tick, &mut pixels, elapsed_ms, events) {
            self.fatal = Some(failure.clone());
            return Err(failure);
        }
        drop(pixels);
        if self.rt.is_exit_requested() {
            return Ok(AdvanceResponse {
                status: FamilyStatus::Finished,
                ..AdvanceResponse::running()
            });
        }
        Ok(AdvanceResponse::running())
    }

    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        self.ensure_live()?;
        let pixels = self.pixels.borrow();
        let view = FrameView::from_slice(&pixels, self.frame_info.get())?;
        visitor.accept(view)
    }

    fn close(self: Box<Self>) -> FamilyResult<()> {
        let this = *self;
        // Cancel the host queue and stop the mixer first so no worker outlives
        // the session, then release the process-global host-events handle.
        let audio_result = this.audio.close();
        drop(this.rt);
        this.events.set_enabled(false);
        audio_result
    }
}
