use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use astra_emu_manager_core::model::{
    ConfigEntry, FamilyResult, FrameAlpha, FrameFormat, FrameIdentity, FrameLease, FramePoll,
    FrameRowOrder, ProbeRequest, WindowState,
};
use astra_emu_manager_core::{no_host_services, LoadedFamilyPlugin, NativeOpenRequest};
use serde::Deserialize;

#[path = "headless_input.rs"]
mod input;

#[path = "headless_captures.rs"]
mod captures;
#[path = "headless_presenter.rs"]
mod presenter;

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
    #[serde(default = "default_sample_interval")]
    sample_interval_ms: u64,
    #[serde(default = "default_reopen_count")]
    reopen_count: u32,
    #[serde(default)]
    manager_data: Option<PathBuf>,
    #[serde(default)]
    translation_enabled: bool,
}
fn default_sample_interval() -> u64 {
    10
}
fn default_reopen_count() -> u32 {
    1
}

fn failure_message(state: &astra_emu_abi::SessionState) -> String {
    format!(
        "{}: {}",
        String::from_utf8_lossy(&state.error.code[..(state.error.code_len as usize).min(128)]),
        String::from_utf8_lossy(
            &state.error.message[..(state.error.message_len as usize).min(512)]
        )
    )
}

fn waiting_for_scheduled_retry(
    state: &astra_emu_abi::SessionState,
    index: u32,
    inputs: &std::collections::BTreeMap<u32, Vec<astra_emu_manager_core::model::FamilyEvent>>,
) -> Result<bool, String> {
    if state.state != astra_emu_abi::SESSION_FAILED {
        return Ok(false);
    }
    let retry = state.flags & astra_emu_abi::SESSION_CAN_RETRY != 0
        && inputs.range((index + 1)..).any(|(_, events)| {
            events.iter().any(|event| {
                matches!(
                    event,
                    astra_emu_manager_core::model::FamilyEvent::Control {
                        command: astra_emu_abi::CONTROL_RETRY
                    }
                )
            })
        });
    if retry {
        Ok(true)
    } else {
        Err(failure_message(state))
    }
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
                return Err(astra_emu_manager_core::model::FamilyError::invalid(
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
    let source = frame.pixels().ok_or_else(|| {
        astra_emu_manager_core::model::FamilyError::invalid(
            "ASTRA_EMU_HEADLESS_GPU_CAPTURE",
            "GPU checkpoints must be captured at the Host presenter boundary",
        )
    })?;
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
        for pixel in source[begin..begin + info.width as usize * 4]
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
        astra_emu_manager_core::model::FamilyError::invalid(
            "ASTRA_EMU_HEADLESS_FRAME_BYTES",
            "capture pixels have an invalid extent",
        )
    })
}

fn output_image(
    capture: &Capture,
    presenter: Option<&presenter::Presenter>,
) -> Result<image::RgbaImage, String> {
    let frame = capture
        .frame
        .as_ref()
        .ok_or("ASTRA_EMU_HEADLESS_FRAME_MISSING")?;
    if frame.d3d12().is_some() {
        presenter
            .ok_or("ASTRA_EMU_HEADLESS_GPU_PRESENTER_MISSING")?
            .capture()
    } else {
        capture_image(frame).map_err(|error| error.to_string())
    }
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
    let _observability = std::sync::Arc::new(
        astra_observability::init_host(observability).map_err(|_| "ASTRA_EMU_HEADLESS_LOG_INIT")?,
    );
    let filter_guard = std::sync::Arc::downgrade(&_observability);
    astra_emu_manager_core::set_diagnostic_enabled(move |level, target| {
        filter_guard
            .upgrade()
            .is_some_and(|guard| guard.external_enabled(level, target))
    });
    let emit_guard = std::sync::Arc::downgrade(&_observability);
    astra_emu_manager_core::set_diagnostic_emit(move |record| {
        if let Some(guard) = emit_guard.upgrade() {
            if let Err(error) =
                guard.record_external(record.level, &record.target, &record.event, record.fields)
            {
                tracing::error!(event = "astra.emu.diagnostics.record_failed", error = %error);
            }
        }
    });
    let flush_guard = std::sync::Arc::downgrade(&_observability);
    astra_emu_manager_core::set_diagnostic_flush(move || {
        if let Some(guard) = flush_guard.upgrade() {
            if let Err(error) = guard.flush() {
                tracing::error!(event = "astra.emu.diagnostics.flush_failed", error = %error);
            }
        }
    });
    let result = run_configuration(config);
    if let Err(cause) = &result {
        tracing::error!(event = "astra.emu.headless.failed", error = %cause);
    }
    result
}

fn run_configuration(config: Configuration) -> Result<(), String> {
    let _extensions =
        crate::text_service::ExtensionRuntime::new().map_err(|error| error.to_string())?;
    if !(1..=36_000).contains(&config.frames) {
        return Err("ASTRA_EMU_HEADLESS_FRAME_LIMIT".into());
    }
    let game = config
        .game
        .to_str()
        .ok_or("ASTRA_EMU_HEADLESS_GAME_ENCODING")?;
    let mut plugin = LoadedFamilyPlugin::load(&config.plugin).map_err(|error| error.to_string())?;
    let run_result = (|| {
        if plugin
            .probe(ProbeRequest {
                game_path: game.into(),
            })
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("ASTRA_EMU_HEADLESS_PROBE_NO_MATCH".into());
        }
        if !(1..=1000).contains(&config.sample_interval_ms) {
            return Err("ASTRA_EMU_HEADLESS_SAMPLE_INTERVAL".into());
        }
        if !(1..=32).contains(&config.reopen_count) {
            return Err("ASTRA_EMU_HEADLESS_REOPEN_LIMIT".into());
        }
        for cycle in 0..config.reopen_count {
            let output = if cycle == 0 {
                config.output.clone()
            } else {
                let stem = config
                    .output
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .ok_or("ASTRA_EMU_HEADLESS_OUTPUT")?;
                config
                    .output
                    .with_file_name(format!("{stem}.reopen-{cycle:02}.png"))
            };
            tracing::info!(event = "astra.emu.headless.reopen", cycle);
            run_session(&config, &mut plugin, game, &output)?;
        }
        Ok(())
    })();
    let owner_result = plugin.shutdown().map_err(|error| error.to_string());
    match (run_result, owner_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(primary), Err(cleanup)) => Err(format!("{primary}; {cleanup}")),
    }
}

fn run_session(
    config: &Configuration,
    plugin: &mut LoadedFamilyPlugin,
    game: &str,
    output: &Path,
) -> Result<(), String> {
    let inputs = input::prepare(&config.inputs, config.frames)?;
    let mut captures = captures::Captures::new(&config.capture_frames, config.frames, output)?;
    let epoch = astra_emu_manager_core::model::allocate_session_epoch()
        .map_err(|error| error.to_string())?;
    let cancellation = astra_emu_manager_core::NativeCancellation::new(epoch);
    let mut presenter = if plugin.supports_d3d12() {
        presenter::Presenter::new(epoch)?
    } else {
        None
    };
    let text = if let Some(data) = &config.manager_data {
        let extensions = astra_emu_manager_core::extensions::ExtensionStore::new(data).list()?;
        let profile = if config.translation_enabled {
            let database = data.join("library.sqlite3");
            if !database.is_file() {
                return Err("ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED".into());
            }
            Some(
                astra_emu_manager_core::Library::open(database)
                    .map_err(|e| e.to_string())?
                    .translation_profile()
                    .map_err(|e| e.to_string())?
                    .ok_or("ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED")?,
            )
        } else {
            None
        };
        if profile.is_some() || extensions.iter().any(|extension| extension.enabled) {
            if !plugin
                .manager_descriptor()
                .has_capability(astra_emu_manager_core::FamilyCapability::TextReplacement)
            {
                return Err("ASTRA_EMU_TEXT_HOOK_UNSUPPORTED".into());
            }
            let secrets = std::sync::Arc::new(
                crate::platform_secret::ManagerSecretStore::open().map_err(|e| e.to_string())?,
            );
            Some(
                crate::text_service::TextReplacementBridge::new(
                    epoch, profile, secrets, extensions,
                )
                .map_err(|e| e.to_string())?,
            )
        } else {
            None
        }
    } else {
        if config.translation_enabled {
            return Err("ASTRA_EMU_TRANSLATION_PROFILE_REQUIRED".into());
        }
        None
    };
    let mut host = no_host_services(&cancellation);
    if let Some(text) = &text {
        host.text = text.sink();
    }
    let request = NativeOpenRequest {
        game_path: game.into(),
        session_epoch: epoch,
        configuration: config.configuration.clone(),
        initial_window: WindowState {
            width: 1280,
            height: 720,
            scale_factor: 1.0,
            focused: true,
            visible: true,
            suspended: false,
        },
        host,
        cancellation,
        d3d12: presenter
            .as_ref()
            .map(|presenter| presenter.host())
            .transpose()?
            .flatten(),
    };
    tracing::info!(event = "astra.emu.headless.opening");
    let opening = Instant::now();
    let mut session = plugin.open(request).map_err(|error| error.to_string())?;
    tracing::info!(
        event = "astra.emu.headless.opened",
        open_us = opening.elapsed().as_micros() as u64
    );
    let mut capture = Capture::default();
    let result = (|| {
        let start = Instant::now();
        let first_frame_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if capture
                .read(
                    session
                        .acquire_frame(None)
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?
            {
                break;
            }
            let state = session.state().map_err(|error| error.to_string())?;
            if state.state == astra_emu_abi::SESSION_FAILED {
                return Err(failure_message(&state));
            }
            if Instant::now() >= first_frame_deadline {
                return Err("ASTRA_EMU_HEADLESS_FRAME_TIMEOUT".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(frame) = capture
            .frame
            .as_ref()
            .filter(|frame| frame.d3d12().is_some())
        {
            presenter
                .as_mut()
                .ok_or("ASTRA_EMU_HEADLESS_GPU_PRESENTER_MISSING")?
                .present(frame)?;
        }
        tracing::info!(
            event = "astra.emu.headless.first_frame",
            first_frame_wait_us = start.elapsed().as_micros() as u64
        );
        let start = Instant::now();
        let mut failure_reported = false;
        for index in 0..config.frames {
            let scheduled = Duration::from_millis(u64::from(index) * config.sample_interval_ms);
            if let Some(delay) = scheduled.checked_sub(start.elapsed()) {
                std::thread::sleep(delay);
            }
            let input_start = Instant::now();
            if let Some(events) = inputs.get(&index) {
                if failure_reported {
                    let state = session.state().map_err(|error| error.to_string())?;
                    if state.state == astra_emu_abi::SESSION_FAILED
                        && events.iter().any(|event| {
                            !matches!(
                                event,
                                astra_emu_manager_core::model::FamilyEvent::Control {
                                    command: astra_emu_abi::CONTROL_RETRY
                                }
                            )
                        })
                    {
                        return Err(failure_message(&state));
                    }
                }
                session
                    .input(events, capture.identity())
                    .map_err(|error| error.to_string())?;
            }
            let input_us = input_start.elapsed().as_micros() as u64;
            let deadline = Instant::now() + Duration::from_secs(5);
            let poll_start = Instant::now();
            loop {
                let poll = session
                    .acquire_frame(capture.identity())
                    .map_err(|error| error.to_string())?;
                if capture.read(poll).map_err(|error| error.to_string())? {
                    break;
                }
                let state = session.state().map_err(|error| error.to_string())?;
                if state.state == astra_emu_abi::SESSION_FAILED {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("ASTRA_EMU_HEADLESS_FRAME_TIMEOUT".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            if let Some(frame) = capture
                .frame
                .as_ref()
                .filter(|frame| frame.d3d12().is_some())
            {
                presenter
                    .as_mut()
                    .ok_or("ASTRA_EMU_HEADLESS_GPU_PRESENTER_MISSING")?
                    .present(frame)?;
            }
            let state = session.state().map_err(|error| error.to_string())?;
            if waiting_for_scheduled_retry(&state, index, &inputs)? {
                if !failure_reported {
                    tracing::error!(
                        event = "astra.emu.headless.awaiting_scheduled_retry",
                        sample = index,
                        error = %failure_message(&state)
                    );
                }
                failure_reported = true;
                captures.write(index, || output_image(&capture, presenter.as_ref()))?;
                continue;
            }
            failure_reported = false;
            let frame_poll_us = poll_start.elapsed().as_micros() as u64;
            let capture_start = Instant::now();
            captures.write(index, || output_image(&capture, presenter.as_ref()))?;
            tracing::info!(
                event = "astra.emu.headless.sample",
                sample = index,
                input_us,
                frame_poll_us,
                capture_us = capture_start.elapsed().as_micros() as u64,
                native_frame_timestamp_ns =
                    capture.frame.as_ref().map_or(0, |frame| frame.timestamp_ns),
                generation = capture.identity().map_or(0, |id| id.generation),
                output_epoch = capture.identity().map_or(0, |id| id.output_epoch)
            );
            if state.state == astra_emu_abi::SESSION_FINISHED {
                break;
            }
        }
        captures.finish()?;
        output_image(&capture, presenter.as_ref())?
            .save(output)
            .map_err(|_| "ASTRA_EMU_HEADLESS_CAPTURE_WRITE".to_owned())
    })();
    session.cancel();
    let text_result = text
        .as_ref()
        .map_or(Ok(()), |text| text.close().map_err(|e| e.to_string()));
    drop(capture);
    let session_result = session.close().map_err(|error| error.to_string());
    let graphics_result = presenter
        .as_mut()
        .map_or(Ok(()), |presenter| presenter.close());
    drop(presenter);
    let errors: Vec<_> = [result, text_result, session_result, graphics_result]
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

#[cfg(test)]
mod failure_tests {
    use super::*;
    use astra_emu_abi as abi;
    use astra_emu_manager_core::model::FamilyEvent;

    #[test]
    fn only_explicit_future_retry_can_hold_a_retryable_failure() {
        let mut state = abi::SessionState {
            state: abi::SESSION_FAILED,
            flags: abi::SESSION_CAN_RETRY,
            context_epoch: 1,
            output_epoch: 1,
            error: abi::Error::default(),
        };
        state
            .error
            .set("NATIVE_HOOK_FAILED", "retained text operation");
        let mut inputs = std::collections::BTreeMap::new();
        let expected = "NATIVE_HOOK_FAILED: retained text operation";
        assert_eq!(
            waiting_for_scheduled_retry(&state, 4, &inputs).unwrap_err(),
            expected
        );
        inputs.insert(
            4,
            vec![FamilyEvent::Control {
                command: abi::CONTROL_RETRY,
            }],
        );
        assert_eq!(
            waiting_for_scheduled_retry(&state, 4, &inputs).unwrap_err(),
            expected
        );
        inputs.insert(
            5,
            vec![FamilyEvent::Control {
                command: abi::CONTROL_RESUME,
            }],
        );
        assert_eq!(
            waiting_for_scheduled_retry(&state, 4, &inputs).unwrap_err(),
            expected
        );
        inputs.insert(
            5,
            vec![FamilyEvent::Control {
                command: abi::CONTROL_RETRY,
            }],
        );
        assert!(waiting_for_scheduled_retry(&state, 4, &inputs).unwrap());
        state.flags = 0;
        assert_eq!(
            waiting_for_scheduled_retry(&state, 4, &inputs).unwrap_err(),
            expected
        );
        state.state = abi::SESSION_RUNNING;
        assert!(!waiting_for_scheduled_retry(&state, 4, &inputs).unwrap());
    }
}
