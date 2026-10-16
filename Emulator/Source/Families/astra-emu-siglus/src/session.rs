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
    FrameAlpha, FrameFormat, FrameIdentity, FrameInfo, FramePoll, FramePublisher, FrameRowOrder,
    WindowState,
};
use siglus_scene_vm::audio::kira_hub::hosted_tap;
use siglus_scene_vm::host::{SiglusHost, SiglusHostConfig};
use siglus_scene_vm::render::Renderer;
use siglus_scene_vm::resource;

use crate::{audio, audio::PcmBridge, error, events};

const FRAME_INTERVAL_MS: u32 = 16;

pub(crate) struct SiglusSession {
    host: SiglusHost,
    audio: PcmBridge,
    frame: FramePublisher,
    fatal: Option<FamilyError>,
    frame_dirty: bool,
    elapsed_remainder_ns: u64,
    global_fingerprint: u64,
    render_generation: u64,
}

pub(crate) struct SiglusOpen {
    pub(crate) session: SiglusSession,
    pub(crate) frame_info: FrameInfo,
}

pub(crate) fn boot(
    game_path: &Path,
    _initial_window: WindowState,
    sink: AudioSinkBox,
    session_epoch: u64,
) -> FamilyResult<SiglusOpen> {
    let game_path = game_path.to_owned();
    let (width, height) = resolve_screen_size(&game_path)?;
    let frame = FramePublisher::new(session_epoch)?;

    let audio = PcmBridge::new(sink)?;
    hosted_tap::install(audio.tap_factory(), audio::OUTPUT_FORMAT.sample_rate);

    let boot_result = boot_engine(&game_path, width, height, audio, frame);
    if boot_result.is_err() {
        // Unwind the tap registry so a later session cannot inherit a dead
        // callback; the engine may still own the mixer worker until dropped.
        hosted_tap::clear();
    }
    boot_result
}

fn boot_engine(
    game_path: &Path,
    width: u32,
    height: u32,
    audio: PcmBridge,
    mut frame: FramePublisher,
) -> FamilyResult<SiglusOpen> {
    let renderer = pollster::block_on(Renderer::new_offscreen(width, height))
        .map_err(|error| error::engine("renderer.open", error))?;
    let mut config = SiglusHostConfig::new(game_path.to_owned());
    config.width = Some(width);
    config.height = Some(height);
    let mut host = SiglusHost::new_with_renderer_sync(config, renderer)
        .map_err(|error| error::engine("session.open", error))?;
    let global_fingerprint = host.persist_global_if_changed(None);

    // Compose the first frame before publishing its dimensions. Readback
    // failures must propagate rather than masquerading as an unsettled frame.
    if host
        .step(FRAME_INTERVAL_MS)
        .map_err(|error| error::engine("session.first_frame", error))?
    {
        return Err(error::invalid(
            "ASTRA_EMU_SIGLUS_BOOT_FRAME",
            "the Siglus engine produced no composed frame during boot",
        ));
    }
    let frame_info = pull_frame(&host, &mut frame)?;
    host.take_timeline_reset();
    let render_generation = host.renderer().hosted_frame_generation();

    Ok(SiglusOpen {
        frame_info,
        session: SiglusSession {
            host,
            audio,
            frame,
            fatal: None,
            frame_dirty: false,
            elapsed_remainder_ns: 0,
            global_fingerprint,
            render_generation,
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

fn frame_info(width: u32, height: u32) -> FamilyResult<FrameInfo> {
    if width == 0 || height == 0 {
        return Err(error::invalid(
            "ASTRA_EMU_SIGLUS_FRAME_EMPTY",
            "the engine produced an empty frame",
        ));
    }
    Ok(FrameInfo {
        width,
        height,
        logical_width: width,
        logical_height: height,
        stride: width.checked_mul(4).ok_or_else(|| {
            error::invalid("ASTRA_EMU_SIGLUS_FRAME_SIZE", "frame stride overflows")
        })?,
        // Native swapchain ignores render-target alpha; preserve native RGB.
        format: FrameFormat::Rgba8Srgb {
            alpha: FrameAlpha::Ignored,
        },
        rows: FrameRowOrder::TopToBottom,
    })
}

/// Reads the composed frame back from the offscreen target. A size change
/// resizes the snapshot; the stride stays width * 4.
fn pull_frame(host: &SiglusHost, frame: &mut FramePublisher) -> FamilyResult<FrameInfo> {
    let (width, height, pixels) = host
        .renderer()
        .read_frame_rgba()
        .map_err(|error| error::engine("renderer.readback", error))?;
    let info = frame_info(width, height)?;
    frame.publish(pixels, Vec::as_slice, info)?;
    Ok(info)
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
        // Preserve the upstream player's real-time clock; do not enable its
        // replay-only virtual-clock feature in a hosted game session.
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
        // happens on demand in `acquire_frame`, so hosts that skip frame pulls
        // (headless routes) do not pay a texture-to-buffer copy per advance.
        self.frame_dirty |=
            self.host.renderer().hosted_frame_generation() != self.render_generation;
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

    fn acquire_frame(&mut self, after: Option<FrameIdentity>) -> FamilyResult<FramePoll> {
        self.ensure_live()?;
        if self.frame_dirty {
            pull_frame(&self.host, &mut self.frame)?;
            self.render_generation = self.host.renderer().hosted_frame_generation();
            self.frame_dirty = false;
        }
        Ok(self.frame.poll(after))
    }

    fn close(mut self: Box<Self>) -> FamilyResult<()> {
        // Cancel the host queue first so the kira worker cannot stay blocked
        // in `write`; dropping the host then joins that worker.
        let audio_result = self.audio.close();
        self.host
            .persist_global_if_changed(Some(self.global_fingerprint));
        drop(self.host);
        hosted_tap::clear();
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
    fn native_gpu_readback_preserves_pixels_across_padded_resize() {
        use siglus_scene_vm::{
            assets::RgbaImage,
            image_manager::ImageManager,
            layer::{RenderSprite, Sprite},
        };
        let mut renderer = pollster::block_on(super::Renderer::new_offscreen(63, 47)).unwrap();
        let mut images = ImageManager::new(std::path::PathBuf::new());
        let mut previous_generation = renderer.hosted_frame_generation();
        for (width, height, color) in [
            (63, 47, [23, 101, 211, 255]),
            (65, 49, [199, 37, 61, 255]),
            (63, 47, [23, 101, 211, 255]),
        ] {
            renderer.resize(width, height);
            let image = images.insert_image(RgbaImage {
                width,
                height,
                center_x: 0,
                center_y: 0,
                rgba: color.repeat((width * height) as usize),
            });
            let sprite = RenderSprite::new(
                None,
                None,
                Sprite {
                    image_id: Some(image),
                    visible: true,
                    ..Sprite::default()
                },
            );
            renderer.render_sprites(&images, &[sprite]).unwrap();
            let (actual_width, actual_height, pixels) = renderer.read_frame_rgba().unwrap();
            let generation = renderer.hosted_frame_generation();
            assert!(
                generation > previous_generation,
                "resize must not alias an older CPU frame"
            );
            previous_generation = generation;
            assert_eq!((actual_width, actual_height), (width, height));
            assert_eq!(pixels, color.repeat((width * height) as usize));
        }
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
