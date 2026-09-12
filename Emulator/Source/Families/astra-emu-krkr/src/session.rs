//! Kirikiri family session: owns the process-global engine, the audio bridge,
//! and the CPU frame snapshot handed to the host.
//!
//! Ownership across the language boundary is strictly pull-based:
//! - The engine composes into its own CPU frame buffer during
//!   [`ffi::astra_krkr_tick`]; the session copies it out afterwards.
//! - Mixed PCM is pushed from the engine audio thread through a global tap
//!   registry, because that thread does not share thread-locals with the
//!   session thread.
//! - `boot`, `tick`, and `shutdown` are only ever called from the family
//!   session thread, which the provider serializes.

use std::{
    ffi::CString,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use astra_emu_family_api::{
    AdvanceResponse, AudioSinkBox, FamilyError, FamilyResult, FamilySession, FamilyStatus,
    FrameAlpha, FrameFormat, FrameInfo, FrameView, FrameVisitor,
};

use crate::{
    audio::{engine_output_format, PcmBridge},
    engine_ffi as ffi,
    events::{self, InputState},
};

/// Must match `ASTRA_KRKR_SAMPLE_RATE` in `astra_krkr_host.h`.
pub(crate) const ENGINE_SAMPLE_RATE: u32 = 48_000;

/// How many settle ticks `open` may spend waiting for the first composed
/// frame before failing the boot.
const FIRST_FRAME_SETTLE_TICKS: u32 = 600;
const SETTLE_TICK_SECONDS: f64 = 1.0 / 60.0;

static ENGINE_IN_USE: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct FrameSnapshot {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

pub(crate) struct KrkrSession {
    frame: FrameSnapshot,
    audio: PcmBridge,
    input: InputState,
    fatal: Option<FamilyError>,
    frame_info: FrameInfo,
}

pub(crate) struct KrkrOpen {
    pub(crate) session: KrkrSession,
    pub(crate) frame_info: FrameInfo,
    pub(crate) audio_format: astra_emu_family_api::PcmFormatSpec,
}

fn invalid(code: &str, message: &str) -> FamilyError {
    FamilyError::invalid(format!("ASTRA_EMU_KRKR_{code}").as_str(), message)
}

pub(crate) fn boot(game_path: &Path, sink: AudioSinkBox) -> FamilyResult<KrkrOpen> {
    if ENGINE_IN_USE.swap(true, Ordering::AcqRel) {
        return Err(invalid(
            "ENGINE_BUSY",
            "the Kirikiri engine allows one session per process",
        ));
    }
    let result = boot_locked(game_path, sink);
    if result.is_err() {
        unsafe {
            let _ = ffi::astra_krkr_shutdown();
        }
        audio_registry::clear();
        ENGINE_IN_USE.store(false, Ordering::Release);
    }
    result
}

fn boot_locked(game_path: &Path, sink: AudioSinkBox) -> FamilyResult<KrkrOpen> {
    let game_dir = path_to_cstring(game_path)?;
    let save_dir = path_to_cstring(&game_path.join("savedata"))?;
    let locale = CString::new("zh-CN").expect("static locale is a valid C string");

    let format = engine_output_format(ENGINE_SAMPLE_RATE);
    let audio = PcmBridge::new(sink, format)?;
    let tap = Arc::new(audio.tap());

    let config = ffi::AstraKrkrBootConfig {
        abi: 1,
        game_dir: game_dir.as_ptr(),
        save_dir: save_dir.as_ptr(),
        locale: locale.as_ptr(),
        initial_width: 0,
        initial_height: 0,
        callbacks: ffi::AstraKrkrHostCallbacks {
            user: std::ptr::null_mut(),
            push_pcm: Some(push_pcm_trampoline),
            log: Some(log_trampoline),
        },
    };

    let generation = 0_u32;
    let _ = generation;
    audio_registry::install(tap);

    let status = unsafe { ffi::astra_krkr_boot(&config) };
    if status != ffi::ASTRA_KRKR_OK {
        return Err(invalid(
            "BOOT",
            "the Kirikiri engine failed to boot the game directory",
        ));
    }

    let mut sample_rate = 0_u32;
    let mut channels = 0_u16;
    let audio_status = unsafe { ffi::astra_krkr_audio_format(&mut sample_rate, &mut channels) };
    if audio_status != ffi::ASTRA_KRKR_OK
        || sample_rate != format.sample_rate
        || channels != format.channels
    {
        return Err(invalid(
            "AUDIO_FORMAT",
            "the engine reported an unexpected audio format",
        ));
    }

    // The ABI requires the final frame size in the open response; settle
    // ticks pump the engine until the game created its primary window.
    let mut frame = FrameSnapshot::default();
    for _ in 0..FIRST_FRAME_SETTLE_TICKS {
        let tick = unsafe { ffi::astra_krkr_tick(SETTLE_TICK_SECONDS, std::ptr::null(), 0) };
        if tick == ffi::ASTRA_KRKR_ERR_TERMINATED {
            return Err(invalid(
                "BOOT_TERMINATED",
                "the engine exited before composing a frame",
            ));
        }
        if tick != ffi::ASTRA_KRKR_OK {
            return Err(invalid("TICK", "the engine rejected a settle tick"));
        }
        if pull_frame(&mut frame).is_ok() {
            break;
        }
    }
    let frame_info = frame_info(&frame)?;

    Ok(KrkrOpen {
        audio_format: format,
        frame_info,
        session: KrkrSession {
            frame,
            audio,
            input: InputState::default(),
            fatal: None,
            frame_info,
        },
    })
}

fn frame_info(frame: &FrameSnapshot) -> FamilyResult<FrameInfo> {
    if frame.width == 0 || frame.height == 0 {
        return Err(invalid(
            "FRAME_EMPTY",
            "the engine produced no primary-layer frame during boot",
        ));
    }
    Ok(FrameInfo {
        width: frame.width,
        height: frame.height,
        stride: frame
            .width
            .checked_mul(4)
            .ok_or_else(|| invalid("FRAME_SIZE", "frame stride overflows"))?,
        format: FrameFormat::Rgba8Srgb {
            alpha: FrameAlpha::Opaque,
        },
    })
}

/// Pulls the engine frame buffer into the session snapshot. The first call
/// discovers the current size; `ERR_ARG` means "capacity too small" and is
/// the expected size-discovery result.
fn pull_frame(frame: &mut FrameSnapshot) -> FamilyResult<()> {
    let mut width = 0_u32;
    let mut height = 0_u32;
    let status =
        unsafe { ffi::astra_krkr_copy_frame(std::ptr::null_mut(), 0, &mut width, &mut height) };
    match status {
        ffi::ASTRA_KRKR_OK | ffi::ASTRA_KRKR_ERR_ARG => {}
        ffi::ASTRA_KRKR_ERR_STATE => {
            return Err(invalid("FRAME_EMPTY", "no composed frame is available"));
        }
        _ => return Err(invalid("FRAME", "the engine frame query failed")),
    }
    let required = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("FRAME_SIZE", "frame dimensions overflow"))?;
    if frame.pixels.len() != required {
        frame.pixels.resize(required, 0);
    }
    let status = unsafe {
        ffi::astra_krkr_copy_frame(
            frame.pixels.as_mut_ptr(),
            frame.pixels.len() as u32,
            &mut width,
            &mut height,
        )
    };
    if status != ffi::ASTRA_KRKR_OK {
        return Err(invalid("FRAME", "the engine frame copy failed"));
    }
    frame.width = width;
    frame.height = height;
    Ok(())
}

impl KrkrSession {
    fn fail<T>(&mut self, value: FamilyError) -> FamilyResult<T> {
        self.fatal = Some(value.clone());
        Err(value)
    }

    fn ensure_live(&self) -> FamilyResult<()> {
        if let Some(value) = &self.fatal {
            return Err(value.clone());
        }
        Ok(())
    }
}

impl FamilySession for KrkrSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[astra_emu_family_api::FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        self.ensure_live()?;
        self.audio.check_error()?;
        let input = events::translate(events, &mut self.input);
        let elapsed_seconds = (elapsed_ns as f64) / 1_000_000_000.0;
        let status = unsafe {
            ffi::astra_krkr_tick(
                elapsed_seconds,
                if input.is_empty() {
                    std::ptr::null()
                } else {
                    input.as_ptr()
                },
                input.len() as u32,
            )
        };
        if status == ffi::ASTRA_KRKR_ERR_TERMINATED {
            return Ok(AdvanceResponse {
                status: FamilyStatus::Finished,
            });
        }
        if status != ffi::ASTRA_KRKR_OK {
            return self.fail(invalid("TICK", "the engine rejected a tick request"));
        }
        if let Err(value) = pull_frame(&mut self.frame) {
            // A missing frame mid-game is not fatal (engine may be between
            // windows); keep the previous snapshot.
            tracing::debug!(event = "astra.emu.krkr.frame_missing", diagnostic = %value.code());
        } else {
            self.frame_info = frame_info(&self.frame)?;
        }
        let mut terminated = 0_u8;
        let _ = unsafe { ffi::astra_krkr_terminated(&mut terminated) };
        if terminated != 0 {
            return Ok(AdvanceResponse {
                status: FamilyStatus::Finished,
            });
        }
        Ok(AdvanceResponse::running())
    }

    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        self.ensure_live()?;
        let info = frame_info(&self.frame)?;
        let view = FrameView::from_slice(&self.frame.pixels, info)?;
        visitor.accept(view)
    }

    fn close(self: Box<Self>) -> FamilyResult<()> {
        let audio_result = self.audio.close();
        let shutdown = unsafe { ffi::astra_krkr_shutdown() };
        audio_registry::clear();
        ENGINE_IN_USE.store(false, Ordering::Release);
        audio_result?;
        if shutdown != ffi::ASTRA_KRKR_OK {
            return Err(invalid("SHUTDOWN", "the engine did not shut down cleanly"));
        }
        Ok(())
    }
}

fn path_to_cstring(path: &Path) -> FamilyResult<CString> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid("GAME_PATH", "the game path is not valid UTF-8"))?;
    CString::new(text).map_err(|_| invalid("GAME_PATH", "the game path contains a NUL byte"))
}

// ---------------------------------------------------------------------------
// Engine callback plumbing
// ---------------------------------------------------------------------------

/// The audio tap must be reachable from the engine audio thread, which does
/// not share thread-locals with the session thread.
mod audio_registry {
    use std::sync::{Arc, Mutex};

    static TAP: Mutex<Option<Arc<crate::audio::PcmTap>>> = Mutex::new(None);

    pub(super) fn install(tap: Arc<crate::audio::PcmTap>) {
        *TAP.lock().unwrap() = Some(tap);
    }

    pub(super) fn lookup() -> Option<Arc<crate::audio::PcmTap>> {
        TAP.lock().unwrap().as_ref().map(Arc::clone)
    }

    pub(super) fn clear() {
        *TAP.lock().unwrap() = None;
    }
}

unsafe extern "C" fn push_pcm_trampoline(
    _user: *mut std::ffi::c_void,
    samples: *const i16,
    frame_count: u32,
) {
    let total = usize::try_from(frame_count)
        .ok()
        .and_then(|frames| frames.checked_mul(2));
    let Some(total) = total else { return };
    let samples = unsafe { std::slice::from_raw_parts(samples, total) };
    if let Some(tap) = audio_registry::lookup() {
        tap.push(samples, 2);
    }
}

unsafe extern "C" fn log_trampoline(
    _user: *mut std::ffi::c_void,
    level: u8,
    message: *const std::os::raw::c_char,
) {
    let Ok(text) = unsafe { std::ffi::CStr::from_ptr(message) }.to_str() else {
        return;
    };
    match level {
        3 => tracing::warn!(event = "astra.emu.krkr.engine", "{text}"),
        4 => tracing::error!(event = "astra.emu.krkr.engine", "{text}"),
        _ => tracing::debug!(event = "astra.emu.krkr.engine", "{text}"),
    }
}
