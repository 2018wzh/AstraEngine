use super::*;
use abi_stable::std_types::ROption;
use astra_emu_family_api::{
    FamilyHostServices, FamilySession, FamilyStatus, FamilyWindowCommand, OpenRequest, WindowState,
};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

pub(super) fn run(
    graphics_owner: owner::GraphicsOwner,
    path: String,
    configuration: Vec<astra_emu_family_api::ConfigEntry>,
    window: abi::WindowState,
    services: Arc<text::ServiceContext>,
    commands: Receiver<Command>,
    ready: &SyncSender<Result<Arc<Shared>, Failure>>,
) -> Result<(), Failure> {
    let request = OpenRequest {
        game_path: path.into(),
        session_epoch: services.session,
        configuration: configuration.into(),
        initial_window: WindowState {
            width: window.drawable_width,
            height: window.drawable_height,
            focused: window.focused != 0,
            visible: window.suspended == 0,
        },
        host: FamilyHostServices {
            audio_sink: ROption::RNone,
            text_replacement: ROption::RNone,
            modal: ROption::RNone,
        },
    };
    let (_, mut session) = {
        let mut owner = graphics_owner.lock().map_err(|_| invalid(abi::PANICKED))?;
        owner.open_native_session(request, Some(text::TextBridge(services.clone())))?
    };
    if window.suspended != 0 {
        session.native_pause(true)?;
    }
    let (info, pixels) = session.native_frame();
    let info = abi::SurfaceInfo {
        kind: abi::SURFACE_CPU,
        pixel_format: abi::PIXEL_RGBA8,
        color: abi::COLOR_SRGB,
        alpha: abi::ALPHA_IGNORED,
        rows: abi::ROWS_TOP_DOWN,
        backend: 0,
        logical: abi::Extent {
            width: info.logical_width,
            height: info.logical_height,
        },
        raster: abi::Extent {
            width: info.width,
            height: info.height,
        },
        stride: u64::from(info.stride),
    };
    info.validate().map_err(invalid)?;
    let canvas = astra_emu_sdk::StageCanvas::new(
        astra_media_core::Extent2D {
            width: info.logical.width,
            height: info.logical.height,
        },
        astra_media_core::Extent2D {
            width: info.raster.width,
            height: info.raster.height,
        },
    )
    .map_err(|cause| {
        Failure::new(
            abi::CORE_ERROR,
            "ASTRA_EMU_MUSICA_CANVAS",
            cause.to_string(),
        )
    })?;
    let shared = Arc::new(Shared {
        services: services.clone(),
        frame: Mutex::new(Published {
            info,
            pixels,
            generation: 1,
            timestamp_ns: 0,
        }),
        state: Mutex::new(abi::SessionState {
            state: abi::SESSION_RUNNING,
            flags: 0,
            context_epoch: 1,
            output_epoch: 1,
            error: abi::Error::default(),
        }),
        canvas,
        leases: Arc::new(AtomicUsize::new(0)),
    });
    if ready.send(Ok(shared.clone())).is_err() {
        services.cancel();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pump(&mut session, &shared, commands)
    }));
    if let Ok(Err(cause)) = &result {
        fail(&shared, cause, false);
    }
    if result.is_err() {
        fail(
            &shared,
            &Failure::new(
                abi::PANICKED,
                "ASTRA_EMU_MUSICA_WORKER_PANIC",
                "native scheduler panicked",
            ),
            false,
        );
        services.cancel();
    }
    // Invalidate first, then close every media/audio owner even after failure.
    services.invalidate();
    let cleanup = Box::new(session).close().map_err(Failure::from);
    match result {
        Ok(result) => {
            result?;
            cleanup
        }
        Err(_) => Err(Failure::new(
            abi::PANICKED,
            "ASTRA_EMU_MUSICA_WORKER_PANIC",
            "native scheduler panicked",
        )),
    }
}
fn fail(shared: &Shared, cause: &Failure, can_retry: bool) {
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    state.state = abi::SESSION_FAILED;
    state.flags = if can_retry { abi::SESSION_CAN_RETRY } else { 0 };
    state.error = cause.diagnostic();
}
fn pump(
    session: &mut crate::session::MusicaSession,
    shared: &Shared,
    commands: Receiver<Command>,
) -> Result<(), Failure> {
    let started = Instant::now();
    let mut previous = Instant::now();
    let mut failed = false;
    let mut paused = session.native_suspended();
    let mut finished = false;
    let mut pending_window = None;
    let mut window_sequence = 0u64;
    while !shared.services.is_cancelled() {
        let command = match commands.recv_timeout(Duration::from_millis(8)) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if shared.services.is_cancelled() {
            break;
        }
        let now = Instant::now();
        let elapsed = now
            .duration_since(previous)
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
        previous = now;
        if let Some(Command::Control(control @ (abi::CONTROL_PAUSE | abi::CONTROL_RESUME))) =
            &command
        {
            paused = *control == abi::CONTROL_PAUSE;
            if let Err(cause) = session.native_pause(paused || failed) {
                failed = true;
                fail(shared, &cause.into(), false);
            }
            if !failed {
                shared
                    .state
                    .lock()
                    .map_err(|_| invalid(abi::PANICKED))?
                    .state = if paused {
                    abi::SESSION_PAUSED
                } else if finished {
                    abi::SESSION_FINISHED
                } else {
                    abi::SESSION_RUNNING
                };
            }
            previous = Instant::now();
            continue;
        }
        let recovering = failed;
        let result = match command {
            Some(Command::Control(abi::CONTROL_RETRY)) if pending_window.is_some() => Ok(None),
            Some(Command::Control(abi::CONTROL_RETRY)) => session.native_retry().map(Some),
            Some(Command::Control(_)) => return Err(invalid(abi::UNSUPPORTED)),
            Some(Command::Input(events)) => {
                // During a failed hook only native restore is actionable; ordinary
                // gameplay input cannot commit choices or run half an operation.
                let restore = events.iter().any(|event| {
                    matches!(
                        event,
                        FamilyEvent::Key {
                            code: astra_emu_family_api::KeyCode::F9,
                            state: astra_emu_family_api::KeyState::Pressed,
                            ..
                        }
                    )
                });
                for event in &events {
                    if let FamilyEvent::WindowSuspended { suspended } = event {
                        paused = *suspended;
                    }
                }
                if failed && !restore {
                    continue;
                }
                session
                    .advance(if failed || paused { 0 } else { elapsed }, &events)
                    .map(Some)
            }
            None if failed || finished || paused => continue,
            None => session.advance(elapsed, &[]).map(Some),
        };
        match result {
            Ok(response) => {
                if recovering {
                    session.native_pause(paused)?;
                    previous = Instant::now();
                }
                failed = false;
                if let Some(response) = response {
                    finished = response.status == FamilyStatus::Finished;
                    if response.reset_clock {
                        previous = Instant::now();
                    }
                    if let ROption::RSome(command) = response.window_command {
                        window_sequence = window_sequence
                            .checked_add(1)
                            .ok_or_else(|| invalid(abi::INVALID_STATE))?;
                        pending_window = Some((window_sequence, command));
                    }
                }
                if let Some((id, command)) = &pending_window {
                    if let Err(cause) = window_request(shared, *id, command) {
                        failed = true;
                        fail(
                            shared,
                            &cause,
                            matches!(cause.status, abi::CORE_ERROR | abi::CAPACITY),
                        );
                        session.native_pause(true)?;
                        previous = Instant::now();
                        continue;
                    }
                    pending_window = None;
                    session.native_pause(paused)?;
                    previous = Instant::now();
                }
                let mut state = shared.state.lock().map_err(|_| invalid(abi::PANICKED))?;
                state.state = if finished {
                    abi::SESSION_FINISHED
                } else if paused {
                    abi::SESSION_PAUSED
                } else {
                    abi::SESSION_RUNNING
                };
                state.flags = 0;
                state.error = abi::Error::default();
                drop(state);
                let (_, pixels) = session.native_frame();
                let mut frame = shared.frame.lock().map_err(|_| invalid(abi::PANICKED))?;
                if !Arc::ptr_eq(&pixels, &frame.pixels) {
                    frame.generation = frame
                        .generation
                        .checked_add(1)
                        .ok_or_else(|| invalid(abi::INVALID_STATE))?;
                    frame.pixels = pixels;
                    frame.timestamp_ns =
                        started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
                }
                if finished {
                    shared.services.invalidate();
                }
            }
            Err(cause) => {
                failed = true;
                fail(shared, &cause.into(), session.native_failed_hook());
                previous = Instant::now();
                // semantic_event already suspended audio before requesting the
                // host. Other fatal errors suspend it here but never resume VM.
                if !session.native_failed_hook() {
                    let _ = session.native_pause(true);
                }
            }
        }
    }
    Ok(())
}
fn window_request(shared: &Shared, id: u64, command: &FamilyWindowCommand) -> Result<(), Failure> {
    let service = shared.services.window.ok_or_else(|| {
        Failure::new(
            abi::UNSUPPORTED,
            "ASTRA_EMU_MUSICA_WINDOW_SERVICE",
            "window operation requires a native host service",
        )
    })?;
    let epoch = shared.services.epoch.load(Ordering::Acquire);
    let mut request = abi::WindowRequest {
        session_epoch: shared.services.session,
        context_epoch: epoch,
        request_id: id,
        kind: 0,
        flags: 0,
        text: abi::Bytes::EMPTY,
        pixels: abi::Bytes::EMPTY,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        autohide_ms: 0,
        buttons: std::ptr::null(),
        button_count: 0,
        caption: abi::Bytes::EMPTY,
        initial_text: abi::Bytes::EMPTY,
    };
    match command {
        FamilyWindowCommand::SetFullscreen(fullscreen) => {
            request.kind = abi::WINDOW_FULLSCREEN;
            request.flags = if *fullscreen {
                abi::FULLSCREEN_BORDERLESS
            } else {
                abi::FULLSCREEN_WINDOWED
            };
        }
        FamilyWindowCommand::SetTitle(title) => {
            request.kind = abi::WINDOW_TITLE;
            request.text = abi::Bytes::new(title.as_bytes());
        }
        FamilyWindowCommand::SetIcon(icon) => {
            request.kind = abi::WINDOW_ICON;
            request.width = icon.width;
            request.height = icon.height;
            request.pixels = abi::Bytes::new(icon.rgba.as_slice());
        }
        FamilyWindowCommand::SetPointer(pointer) => {
            request.kind = abi::WINDOW_POINTER;
            if let ROption::RSome(x) = pointer.logical_x {
                request.flags |= abi::POINTER_SET_X;
                request.x = x;
            }
            if let ROption::RSome(y) = pointer.logical_y {
                request.flags |= abi::POINTER_SET_Y;
                request.y = y;
            }
            if let ROption::RSome(visible) = pointer.visible {
                request.flags |= abi::POINTER_SET_VISIBILITY;
                if visible {
                    request.flags |= abi::POINTER_VISIBLE;
                }
            }
            if let ROption::RSome(ms) = pointer.autohide_ms {
                request.flags |= abi::POINTER_SET_AUTOHIDE;
                request.autohide_ms = ms;
            }
        }
    }
    let mut result = abi::WindowResult {
        selected_button: 0,
        reserved: 0,
        valid_fields: 0,
        fullscreen: 0,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        input: abi::TextBuffer {
            data: std::ptr::null_mut(),
            capacity: 0,
            len: 0,
        },
        visible: 0,
        minimized: 0,
    };
    let mut diagnostic = abi::Error::default();
    shared.services.waiting.store(true, Ordering::Release);
    let status = unsafe {
        service.request.unwrap()(service.context, &request, &mut result, &mut diagnostic)
    };
    shared.services.waiting.store(false, Ordering::Release);
    if shared.services.epoch.load(Ordering::Acquire) != epoch || shared.services.is_cancelled() {
        return Err(invalid(abi::CANCELLED));
    }
    if status != abi::OK {
        let code = diagnostic
            .code
            .get(..diagnostic.code_len as usize)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        let message = diagnostic
            .message
            .get(..diagnostic.message_len as usize)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        return Err(Failure::new(status, code.to_owned(), message.to_owned()));
    }
    if request.kind == abi::WINDOW_FULLSCREEN
        && (result.valid_fields & abi::WINDOW_RESULT_MODE == 0
            || result.fullscreen != request.flags)
    {
        return Err(Failure::new(
            abi::CORE_ERROR,
            "ASTRA_EMU_MUSICA_WINDOW_RESULT",
            "host did not apply the requested window mode",
        ));
    }
    Ok(())
}
