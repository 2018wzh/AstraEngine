use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use abi_stable::std_types::ROption;
use astra_emu_family_api::{
    ConfigEntry, FamilyHostServices, FamilyProvider, FamilyResult, FamilyStatus, FrameAlpha,
    FrameFormat, FrameIdentity, FrameLease, FramePoll, FrameRowOrder, OpenRequest, ProbeRequest,
    WindowState,
};
use astra_emu_manager_core::LoadedFamilyPlugin;
use serde::Deserialize;

use crate::audio_executor::{AudioDeviceKind, HostAudioExecutor};

#[path = "headless_input.rs"]
mod input;

#[path = "headless_captures.rs"]
mod captures;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    plugin: PathBuf,
    game: PathBuf,
    configuration: Vec<ConfigEntry>,
    frames: u32,
    output: PathBuf,
    #[serde(default)]
    inputs: Vec<input::TimedInput>,
    #[serde(default)]
    capture_frames: Vec<u32>,
}

#[derive(Default)]
struct Capture {
    frame: Option<FrameLease>,
}

impl Capture {
    fn identity(&self) -> Option<FrameIdentity> {
        self.frame.as_ref().map(|frame| frame.identity)
    }
    fn read(&mut self, poll: FramePoll) -> FamilyResult<bool> {
        let frame = match poll {
            FramePoll::Ready(frame) => frame,
            FramePoll::Unchanged(identity) if self.identity() == Some(identity) => return Ok(true),
            FramePoll::Pending(_) => return Ok(false),
            FramePoll::Unchanged(_) => {
                return Err(astra_emu_family_api::FamilyError::invalid(
                    "ASTRA_EMU_HEADLESS_FRAME_BASELINE",
                    "unchanged frame has no matching image",
                ))
            }
        };
        frame.validate()?;
        self.frame = Some(frame);
        Ok(true)
    }
}

fn capture_image(frame: &FrameLease) -> FamilyResult<image::RgbaImage> {
    frame.validate()?;
    let info = frame.info;
    let bgra = matches!(
        info.format,
        FrameFormat::Bgra8Srgb { .. } | FrameFormat::Bgra8Linear { .. }
    );
    let linear = matches!(
        info.format,
        FrameFormat::Rgba8Linear { .. } | FrameFormat::Bgra8Linear { .. }
    );
    let mut pixels = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for y in 0..info.height as usize {
        let row = if info.rows == FrameRowOrder::BottomToTop {
            info.height as usize - 1 - y
        } else {
            y
        };
        let begin = row * info.stride as usize;
        for pixel in frame.pixels()[begin..begin + info.width as usize * 4]
            .as_chunks::<4>()
            .0
        {
            let mut value = *pixel;
            if bgra {
                value.swap(0, 2);
            }
            if linear {
                for channel in &mut value[..3] {
                    let linear = f32::from(*channel) / 255.0;
                    let encoded = if linear <= 0.0031308 {
                        12.92 * linear
                    } else {
                        1.055 * linear.powf(1.0 / 2.4) - 0.055
                    };
                    *channel = (encoded * 255.0).round() as u8;
                }
            }
            if info.format.alpha() == FrameAlpha::Ignored {
                value[3] = 255;
            }
            pixels.extend_from_slice(&value);
        }
    }
    image::RgbaImage::from_raw(info.width, info.height, pixels).ok_or_else(|| {
        astra_emu_family_api::FamilyError::invalid(
            "ASTRA_EMU_HEADLESS_FRAME_BYTES",
            "capture pixels have an invalid extent",
        )
    })
}

pub(crate) fn run(path: &Path) -> Result<(), String> {
    let path = path.to_owned();
    std::thread::Builder::new()
        .name("astra-family-headless".into())
        .stack_size(super::FAMILY_OWNER_STACK_BYTES)
        .spawn(move || run_on_owner(&path))
        .map_err(|error| format!("ASTRA_EMU_HEADLESS_OWNER_START: {error}"))?
        .join()
        .map_err(|_| "ASTRA_EMU_HEADLESS_OWNER_PANIC".to_owned())?
}

fn run_on_owner(path: &Path) -> Result<(), String> {
    let filter = match std::env::var("RUST_LOG") {
        Ok(filter) => filter,
        Err(std::env::VarError::NotPresent) => "info".into(),
        Err(_) => return Err("ASTRA_EMU_HEADLESS_LOG_FILTER".into()),
    };
    let bytes = std::fs::read(path).map_err(|_| "ASTRA_EMU_HEADLESS_CONFIG_READ")?;
    let config: Configuration =
        serde_json::from_slice(&bytes).map_err(|_| "ASTRA_EMU_HEADLESS_CONFIG_INVALID")?;
    // The Windows Manager uses the GUI subsystem, where console output may be
    // unavailable. Keep the same bounded host logs beside this run's captures.
    let mut observability = astra_observability::HostObservabilityConfig::for_cli(filter);
    observability.role = astra_observability::HostRole::Test;
    observability.log_dir = Some(config.output.with_extension("diagnostics"));
    let _observability =
        astra_observability::init_host(observability).map_err(|_| "ASTRA_EMU_HEADLESS_LOG_INIT")?;
    let result = run_configuration(config);
    if let Err(cause) = &result {
        tracing::error!(event = "astra.emu.headless.failed", error = %cause);
    }
    result
}

fn run_configuration(config: Configuration) -> Result<(), String> {
    if !(1..=36_000).contains(&config.frames) {
        return Err("ASTRA_EMU_HEADLESS_FRAME_LIMIT".into());
    }
    let inputs = input::prepare(&config.inputs, config.frames)?;
    let mut captures =
        captures::Captures::new(&config.capture_frames, config.frames, &config.output)?;
    let game = config
        .game
        .to_str()
        .ok_or("ASTRA_EMU_HEADLESS_GAME_ENCODING")?;
    let mut plugin = LoadedFamilyPlugin::load(&config.plugin).map_err(|error| error.to_string())?;
    let descriptor = plugin.descriptor().map_err(|error| error.to_string())?;
    if plugin
        .probe(ProbeRequest {
            game_path: game.into(),
        })
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err("ASTRA_EMU_HEADLESS_PROBE_NO_MATCH".into());
    }
    let audio = HostAudioExecutor::new(AudioDeviceKind::Null, None);
    let request = OpenRequest {
        game_path: game.into(),
        session_epoch: astra_emu_family_api::allocate_session_epoch()
            .map_err(|error| error.to_string())?,
        configuration: config.configuration.into(),
        initial_window: WindowState {
            width: 1280,
            height: 720,
            focused: true,
            visible: true,
        },
        host: FamilyHostServices {
            audio_sink: ROption::RSome(audio.sink()),
            text_replacement: ROption::RNone,
            modal: ROption::RNone,
        },
    };
    request
        .validate_for_descriptor(&descriptor)
        .map_err(|error| error.to_string())?;
    tracing::info!(event = "astra.emu.headless.opening");
    let opened = plugin.open(request).map_err(|error| error.to_string())?;
    tracing::info!(event = "astra.emu.headless.opened");
    let mut session = opened.session;
    let result = (|| {
        opened
            .response
            .validate_for_descriptor(&descriptor)
            .map_err(|error| error.to_string())?;
        let start = Instant::now();
        let mut capture = Capture::default();
        for index in 0..config.frames {
            tracing::trace!(event = "astra.emu.headless.frame.begin", frame = index);
            if index % 60 == 0 {
                tracing::info!(event = "astra.emu.headless.advancing", frame = index);
            }
            let previous = u64::from(index) * 1_000_000_000 / 60;
            let next = u64::from(index + 1) * 1_000_000_000 / 60;
            let response = session
                .advance(
                    next - previous,
                    inputs.get(&index).map_or(&[], Vec::as_slice),
                )
                .map_err(|error| error.to_string())?;
            response.validate().map_err(|error| error.to_string())?;
            if matches!(response.window_command, ROption::RSome(_)) {
                return Err("ASTRA_EMU_HEADLESS_WINDOW_COMMAND_UNAVAILABLE".to_owned());
            }
            audio.check_health()?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let poll = session
                    .acquire_frame(capture.identity())
                    .map_err(|error| error.to_string())?;
                if capture.read(poll).map_err(|error| error.to_string())? {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("ASTRA_EMU_HEADLESS_FRAME_TIMEOUT".into());
                }
                // Pending is not unchanged: wait for the real completion,
                // without advancing the VM/events again or saving stale pixels.
                std::thread::yield_now();
            }
            captures.write(index, capture.frame.as_ref())?;
            tracing::trace!(event = "astra.emu.headless.frame.end", frame = index);
            if response.status == FamilyStatus::Finished {
                break;
            }
            if let Some(delay) = Duration::from_nanos(next).checked_sub(start.elapsed()) {
                std::thread::sleep(delay);
            }
        }
        captures.finish()?;
        capture_image(
            capture
                .frame
                .as_ref()
                .ok_or("ASTRA_EMU_HEADLESS_FRAME_MISSING")?,
        )
        .map_err(|error| error.to_string())?
        .save(&config.output)
        .map_err(|_| "ASTRA_EMU_HEADLESS_CAPTURE_WRITE".to_owned())
    })();
    // The family must cancel and join its PCM worker while the host queue is
    // still alive. Release the host device only after the family is closed.
    let session_result = session.close().map_err(|error| error.to_string());
    let audio_result = audio.close();
    let errors: Vec<_> = [result, audio_result, session_result]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        tracing::info!(event = "astra.emu.headless.closed", succeeded = true);
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
