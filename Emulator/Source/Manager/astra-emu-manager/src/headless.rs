use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use abi_stable::std_types::ROption;
use astra_emu_family_api::{
    ConfigEntry, FamilyHostServices, FamilyProvider, FamilyResult, FamilyStatus, FrameView,
    FrameVisitor, OpenRequest, ProbeRequest, WindowState,
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

struct Capture(Option<image::RgbaImage>);

impl FrameVisitor for Capture {
    fn accept(&mut self, frame: FrameView<'_>) -> FamilyResult<()> {
        FrameView::from_slice(frame.as_slice(), frame.info)?;
        let mut pixels =
            Vec::with_capacity(frame.info.width as usize * frame.info.height as usize * 4);
        for row in frame
            .as_slice()
            .chunks(frame.info.stride as usize)
            .take(frame.info.height as usize)
        {
            pixels.extend_from_slice(&row[..frame.info.width as usize * 4]);
        }
        self.0 = image::RgbaImage::from_raw(frame.info.width, frame.info.height, pixels);
        Ok(())
    }
}

pub(crate) fn run(path: &Path) -> Result<(), String> {
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
    let _observability = astra_observability::init_host(observability)
        .map_err(|_| "ASTRA_EMU_HEADLESS_LOG_INIT")?;
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
    let mut captures = captures::Captures::new(&config.capture_frames, config.frames, &config.output)?;
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
        let mut capture = Capture(None);
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
            audio.check_health()?;
            session
                .visit_frame(&mut capture)
                .map_err(|error| error.to_string())?;
            captures.write(index, capture.0.as_ref())?;
            tracing::trace!(event = "astra.emu.headless.frame.end", frame = index);
            if response.status == FamilyStatus::Finished {
                break;
            }
            if let Some(delay) = Duration::from_nanos(next).checked_sub(start.elapsed()) {
                std::thread::sleep(delay);
            }
        }
        captures.finish()?;
        capture
            .0
            .ok_or("ASTRA_EMU_HEADLESS_FRAME_MISSING")?
            .save(&config.output)
            .map_err(|_| "ASTRA_EMU_HEADLESS_CAPTURE_WRITE".to_owned())
    })();
    // Cancel blocked PCM writes before joining the family workers, including on failure.
    let audio_result = audio.close();
    let session_result = session.close().map_err(|error| error.to_string());
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
