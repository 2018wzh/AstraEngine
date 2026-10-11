//! Siglus family session: owns the engine host, the audio bridge, and the
//! CPU frame snapshot handed to the host.
//!
//! Ownership is pull-based and single-threaded: the engine composes into the
//! offscreen wgpu target during `SiglusHost::step`, the session reads the
//! frame back afterwards, and mixed PCM arrives from the kira tap worker
//! thread. `boot`, `advance`, and shutdown all run on the family session
//! thread, which the provider serializes.

use std::path::Path;

use astra_emu_family_api::{
    AdvanceResponse, AudioSinkBox, FamilyError, FamilyResult, FamilySession, FamilyStatus,
    FrameAlpha, FrameFormat, FrameInfo, FrameView, FrameVisitor, WindowState,
};
use siglus_scene_vm::audio::kira_hub::hosted_tap;
use siglus_scene_vm::host::{SiglusHost, SiglusHostConfig};
use siglus_scene_vm::render::Renderer;
use siglus_scene_vm::resource;

use crate::{audio, audio::PcmBridge, error, events};

const FRAME_INTERVAL_MS: u32 = 16;

/// Interior-mutable frame snapshot so an immutable `visit_frame` can refresh
/// the CPU copy on demand. Only the family session thread touches these.
#[derive(Default)]
struct FrameSnapshot {
    pixels: std::cell::RefCell<Vec<u8>>,
    width: std::cell::Cell<u32>,
    height: std::cell::Cell<u32>,
}

pub(crate) struct SiglusSession {
    host: SiglusHost,
    audio: PcmBridge,
    frame: FrameSnapshot,
    frame_info: std::cell::Cell<FrameInfo>,
    fatal: Option<FamilyError>,
    frame_dirty: std::cell::Cell<bool>,
    elapsed_remainder_ns: u64,
}

pub(crate) struct SiglusOpen {
    pub(crate) session: SiglusSession,
    pub(crate) frame_info: FrameInfo,
}

pub(crate) fn boot(
    game_path: &Path,
    _initial_window: WindowState,
    sink: AudioSinkBox,
) -> FamilyResult<SiglusOpen> {
    let game_path = game_path.to_owned();
    let (width, height) = resolve_screen_size(&game_path)?;

    let audio = PcmBridge::new(sink)?;
    hosted_tap::install(audio.tap_factory(), audio::OUTPUT_FORMAT.sample_rate);
    siglus_scene_vm::platform_time::hosted_clock::enable();

    let boot_result = boot_engine(&game_path, width, height, audio);
    if boot_result.is_err() {
        // Unwind the tap registry so a later session cannot inherit a dead
        // callback; the engine may still own the mixer worker until dropped.
        hosted_tap::clear();
        siglus_scene_vm::platform_time::hosted_clock::disable();
    }
    boot_result
}

fn boot_engine(
    game_path: &Path,
    width: u32,
    height: u32,
    audio: PcmBridge,
) -> FamilyResult<SiglusOpen> {
    let renderer = pollster::block_on(Renderer::new_offscreen(width, height))
        .map_err(|error| error::engine("renderer.open", error))?;
    let mut config = SiglusHostConfig::new(game_path.to_owned());
    config.width = Some(width);
    config.height = Some(height);
    config.deterministic_frame_clock = true;
    let mut host = pollster::block_on(SiglusHost::new_with_renderer(config, renderer))
        .map_err(|error| error::engine("session.open", error))?;

    // Compose the first frame before publishing its dimensions. Readback
    // failures must propagate rather than masquerading as an unsettled frame.
    let frame = FrameSnapshot::default();
    if host
        .step(FRAME_INTERVAL_MS)
        .map_err(|error| error::engine("session.first_frame", error))?
    {
        return Err(error::invalid(
            "ASTRA_EMU_SIGLUS_BOOT_FRAME",
            "the Siglus engine produced no composed frame during boot",
        ));
    }
    pull_frame(&host, &frame)?;
    let frame_info = frame_info(&frame)?;
    host.take_timeline_reset();

    Ok(SiglusOpen {
        frame_info,
        session: SiglusSession {
            host,
            audio,
            frame,
            frame_info: std::cell::Cell::new(frame_info),
            fatal: None,
            frame_dirty: std::cell::Cell::new(true),
            elapsed_remainder_ns: 0,
        },
    })
}

/// Resolves the game's native `#SCREEN_SIZE` with the same discovery and
/// default the engine host uses, so the offscreen target matches the VM
/// layout size exactly.
fn resolve_screen_size(game_path: &Path) -> FamilyResult<(u32, u32)> {
    let path = resource::find_initial_gameexe_path(game_path)
        .map_err(|error| error::engine("configuration.discover", error))?;
    let raw = std::fs::read(&path).map_err(|_| {
        error::invalid(
            "ASTRA_EMU_SIGLUS_GAMEEXE_READ",
            "cannot read the game configuration",
        )
    })?;
    let text = if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ini"))
    {
        String::from_utf8(raw).map_err(|_| {
            error::invalid(
                "ASTRA_EMU_SIGLUS_GAMEEXE_ENCODING",
                "the game configuration is not UTF-8",
            )
        })?
    } else {
        let options = resource::load_gameexe_decode_options(game_path)
            .map_err(|error| error::engine("configuration.options", error))?;
        let (text, _report) =
            siglus_scene_vm::formats::gameexe::decode_gameexe_dat_bytes(&raw, &options)
                .map_err(|error| error::engine("configuration.decode", error))?;
        text
    };
    let config = siglus_scene_vm::formats::gameexe::GameexeConfig::from_text(&text);
    let Some(entry) = config.get_entry("SCREEN_SIZE") else {
        tracing::info!(
            event = "astra.emu.siglus.screen_size.default",
            width = 1280,
            height = 720
        );
        return Ok((1280, 720));
    };
    let dimension = |index| {
        entry
            .item_unquoted(index)
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                error::invalid(
                    "ASTRA_EMU_SIGLUS_SCREEN_SIZE",
                    "SCREEN_SIZE must contain two positive dimensions",
                )
            })
    };
    Ok((dimension(0)?, dimension(1)?))
}

fn frame_info(frame: &FrameSnapshot) -> FamilyResult<FrameInfo> {
    if frame.width.get() == 0 || frame.height.get() == 0 {
        return Err(error::invalid(
            "ASTRA_EMU_SIGLUS_FRAME_EMPTY",
            "the engine produced an empty frame",
        ));
    }
    Ok(FrameInfo {
        width: frame.width.get(),
        height: frame.height.get(),
        logical_width: frame.width.get(),
        logical_height: frame.height.get(),
        stride: frame.width.get().checked_mul(4).ok_or_else(|| {
            error::invalid("ASTRA_EMU_SIGLUS_FRAME_SIZE", "frame stride overflows")
        })?,
        format: FrameFormat::Rgba8Srgb {
            alpha: FrameAlpha::Opaque,
        },
    })
}

/// Reads the composed frame back from the offscreen target. A size change
/// resizes the snapshot; the stride stays width * 4.
fn pull_frame(host: &SiglusHost, frame: &FrameSnapshot) -> FamilyResult<()> {
    let (width, height, mut pixels) = {
        let renderer = host.renderer();
        renderer
            .read_frame_rgba()
            .map_err(|error| error::engine("renderer.readback", error))?
    };
    if width == 0 || height == 0 {
        return Err(error::invalid(
            "ASTRA_EMU_SIGLUS_FRAME_EMPTY",
            "the engine reported an empty frame size",
        ));
    }
    // The native swapchain displays RGB over its black clear; its alpha
    // channel is render-target bookkeeping, not presentation opacity. Keep
    // the composed RGB unchanged and publish an opaque final Family frame.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    frame.width.set(width);
    frame.height.set(height);
    *frame.pixels.borrow_mut() = pixels;
    Ok(())
}

impl SiglusSession {
    fn ensure_live(&self) -> FamilyResult<()> {
        if let Some(value) = &self.fatal {
            return Err(value.clone());
        }
        Ok(())
    }
}

fn elapsed_millis(elapsed_ns: u64, remainder_ns: &mut u64) -> FamilyResult<u32> {
    let total_ns = elapsed_ns.checked_add(*remainder_ns).ok_or_else(|| {
        error::invalid("ASTRA_EMU_SIGLUS_ELAPSED_RANGE", "elapsed time overflows")
    })?;
    let millis = i32::try_from(total_ns / 1_000_000).map_err(|_| {
        error::invalid(
            "ASTRA_EMU_SIGLUS_ELAPSED_RANGE",
            "elapsed time exceeds the native millisecond clock range",
        )
    })?;
    *remainder_ns = total_ns % 1_000_000;
    Ok(millis as u32)
}

impl FamilySession for SiglusSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[astra_emu_family_api::FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        self.ensure_live()?;
        self.audio.check_error()?;
        events::apply(&mut self.host, events);
        // The engine runs one VM frame per advance; the deterministic clock
        // consumes this elapsed value instead of the wall clock.
        let elapsed_ms = elapsed_millis(elapsed_ns, &mut self.elapsed_remainder_ns)?;
        let exit = self
            .host
            .step(elapsed_ms)
            .map_err(|error| error::engine("session.advance", error))?;
        let reset_clock = self.host.take_timeline_reset();
        if reset_clock {
            self.elapsed_remainder_ns = 0;
        }
        tracing::trace!(
            event = "astra.emu.siglus.frame.advanced",
            elapsed_ms,
            finished = exit
        );
        // The composed frame stays in the offscreen target; the CPU readback
        // happens on demand in `visit_frame`, so hosts that skip frame pulls
        // (headless routes) do not pay a texture-to-buffer copy per advance.
        self.frame_dirty.set(true);
        Ok(AdvanceResponse {
            status: if exit {
                FamilyStatus::Finished
            } else {
                FamilyStatus::Running
            },
            reset_clock,
            ..AdvanceResponse::running()
        })
    }

    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        self.ensure_live()?;
        if self.frame_dirty.get() {
            pull_frame(&self.host, &self.frame)?;
            let info = frame_info(&self.frame)?;
            self.frame_info.set(info);
            self.frame_dirty.set(false);
        }
        let info = self.frame_info.get();
        let pixels = self.frame.pixels.borrow();
        let view = FrameView::from_slice(&pixels, info)?;
        visitor.accept(view)
    }

    fn close(self: Box<Self>) -> FamilyResult<()> {
        // Cancel the host queue first so the kira worker cannot stay blocked
        // in `write`; dropping the host then joins that worker.
        let audio_result = self.audio.close();
        drop(self.host);
        hosted_tap::clear();
        siglus_scene_vm::platform_time::hosted_clock::disable();
        audio_result
    }
}

#[cfg(test)]
mod tests {
    use super::{elapsed_millis, resolve_screen_size};

    #[test]
    fn elapsed_clock_preserves_fractional_time_and_event_only_advances() {
        let mut remainder = 0;
        let mut total_ms = 0;
        for _ in 0..60 {
            total_ms += elapsed_millis(16_666_666, &mut remainder).unwrap();
            assert_eq!(elapsed_millis(0, &mut remainder).unwrap(), 0);
        }
        total_ms += elapsed_millis(40, &mut remainder).unwrap();
        assert_eq!(total_ms, 1_000);
        assert_eq!(remainder, 0);
        assert_eq!(
            elapsed_millis(2_500_000_000, &mut remainder).unwrap(),
            2_500
        );
        assert!(elapsed_millis((i32::MAX as u64 + 1) * 1_000_000, &mut remainder).is_err());
        remainder = 1;
        assert!(elapsed_millis(u64::MAX, &mut remainder).is_err());
        assert_eq!(remainder, 1);
    }

    #[test]
    #[ignore = "requires a hardware GPU adapter"]
    fn native_gpu_offscreen_readback() {
        let renderer = pollster::block_on(super::Renderer::new_offscreen(64, 48)).unwrap();
        let (width, height, pixels) = renderer.read_frame_rgba().unwrap();
        assert_eq!((width, height), (64, 48));
        assert_eq!(pixels.len(), 64 * 48 * 4);
    }

    #[test]
    fn screen_size_distinguishes_native_default_from_broken_configuration() {
        let game = tempfile::tempdir().unwrap();
        assert!(resolve_screen_size(game.path()).is_err());
        let config = game.path().join("Gameexe.ini");
        std::fs::write(&config, "#TITLE = \"fixture\"\n").unwrap();
        assert_eq!(resolve_screen_size(game.path()).unwrap(), (1280, 720));
        std::fs::write(&config, "#SCREEN_SIZE = 800, 600\n").unwrap();
        assert_eq!(resolve_screen_size(game.path()).unwrap(), (800, 600));
        for invalid in [
            "#SCREEN_SIZE = 0, 600\n",
            "#SCREEN_SIZE = invalid, 600\n",
            "#SCREEN_SIZE = 800\n",
        ] {
            std::fs::write(&config, invalid).unwrap();
            assert!(resolve_screen_size(game.path()).is_err());
        }
        std::fs::write(&config, [0xff, 0xfe]).unwrap();
        assert!(resolve_screen_size(game.path()).is_err());
    }
}
