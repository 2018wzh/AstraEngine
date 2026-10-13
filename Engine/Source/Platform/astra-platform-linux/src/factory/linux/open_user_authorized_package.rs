use super::*;

impl LinuxHostApp {
    pub(super) fn open_user_authorized_package(
        &mut self,
        expected_hash: &str,
    ) -> Result<CachedPackageSource, PlatformError> {
        use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| {
                host_error(
                    "package.open_user_authorized",
                    "portal runtime could not start",
                )
            })?;
        let selected = runtime
            .block_on(async {
                SelectedFiles::open_file()
                    .title("Open Astra package")
                    .modal(true)
                    .filter(FileFilter::new("Astra package").glob("*.astrapkg"))
                    .send()
                    .await?
                    .response()
            })
            .map_err(|error| {
                let code = if error.to_string().contains("cancel") {
                    PlatformErrorCode::Cancelled
                } else {
                    PlatformErrorCode::ProviderUnavailable
                };
                PlatformError::new(
                    code,
                    "package.open_user_authorized",
                    "XDG portal package selection failed",
                )
            })?;
        let uri = selected.uris().first().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::Cancelled,
                "package.open_user_authorized",
                "user cancelled package selection",
            )
        })?;
        let path = uri.to_file_path().map_err(|_| {
            host_error(
                "package.open_user_authorized",
                "portal returned a non-file package URI",
            )
        })?;
        let bytes = std::fs::read(path).map_err(|_| {
            host_error(
                "package.open_user_authorized",
                "portal-authorized package could not be read",
            )
        })?;
        self.package_cache.store_verified(expected_hash, &bytes)?;
        self.package_cache.open_source(expected_hash)
    }

    pub(super) fn start_https_package_open(
        &mut self,
        url: String,
        expected_hash: String,
        mut reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    ) {
        let completion_tx = self.package_completion_tx.clone();
        let completion_proxy = self.event_loop_proxy.clone();
        let policies = self.package_source_policies.clone();
        let package_id = self.package_id.clone();
        let policy = self.package_cache_policy.clone();
        self.pending_package_opens += 1;
        self.package_workers
            .push(PackageWorker::spawn(move |cancelled| {
                let result = (|| {
                    let cache_root = VerifiedPackageCache::platform_cache_root(&package_id)?;
                    let mut cache = VerifiedPackageCache::open(cache_root, policy)?;
                    let client = astra_platform_common::HttpRangeClient::from_policies(&policies)?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| {
                            host_error("package.https.open", "HTTPS runtime could not start")
                        })?;
                    runtime.block_on(cancellable_fetch(
                        &mut reply,
                        cancelled,
                        client.fetch_into_cache(&url, &expected_hash, &mut cache),
                    ))?;
                    cache.open_source(&expected_hash)
                })();
                PackageCompletion { reply, result }.publish(&completion_tx, || {
                    let _ = completion_proxy.send_event(());
                });
            }));
    }

    pub(super) fn process_package_completions(&mut self) {
        self.package_workers.retain(|worker| !worker.is_finished());
        while let Ok(completion) = self.package_completion_rx.try_recv() {
            self.pending_package_opens = self.pending_package_opens.saturating_sub(1);
            completion.deliver(&mut self.package_sources);
        }
    }
}

impl ApplicationHandler for LinuxHostApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let sequence = self.next_sequence();
        let result = self
            .backend
            .emit_event(PlatformEvent::new(sequence, PlatformEventKind::Resumed));
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(result);
        }
        self.process_commands(event_loop);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window_ids.get(&window_id).copied() else {
            return;
        };
        let kind = match event {
            WindowEvent::Focused(focused) => {
                Some(PlatformEventKind::WindowFocused { window, focused })
            }
            WindowEvent::CloseRequested => Some(PlatformEventKind::WindowClosed { window }),
            WindowEvent::Resized(size) => {
                self.windows
                    .get(window)
                    .ok()
                    .map(|native| PlatformEventKind::WindowResized {
                        window,
                        width: size.width,
                        height: size.height,
                        scale_factor: native.scale_factor(),
                    })
            }
            WindowEvent::KeyboardInput { event, .. } => Some(PlatformEventKind::Keyboard {
                window,
                physical_key: match event.physical_key {
                    winit::keyboard::PhysicalKey::Code(code) => format!("{code:?}"),
                    winit::keyboard::PhysicalKey::Unidentified(code) => format!("{code:?}"),
                },
                logical_key: match event.logical_key {
                    winit::keyboard::Key::Named(key) => Some(format!("{key:?}")),
                    winit::keyboard::Key::Character(text) => Some(text.to_string()),
                    winit::keyboard::Key::Dead(_) | winit::keyboard::Key::Unidentified(_) => None,
                },
                state: input_state(event.state),
                repeat: event.repeat,
            }),
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => Some(PlatformEventKind::ImePreedit {
                window,
                text,
                cursor,
            }),
            WindowEvent::Ime(Ime::Commit(text)) => {
                Some(PlatformEventKind::ImeCommit { window, text })
            }
            WindowEvent::CursorMoved { position, .. } => Some(PlatformEventKind::PointerMoved {
                window,
                x: position.x,
                y: position.y,
            }),
            WindowEvent::MouseInput { state, button, .. } => {
                Some(PlatformEventKind::PointerButton {
                    window,
                    button: pointer_button(button),
                    state: input_state(state),
                })
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (delta_x, delta_y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x as f32, position.y as f32)
                    }
                };
                Some(PlatformEventKind::MouseWheel {
                    window,
                    delta_x,
                    delta_y,
                })
            }
            WindowEvent::Touch(touch) => Some(PlatformEventKind::Touch {
                window,
                id: touch.id,
                x: touch.location.x,
                y: touch.location.y,
                phase: touch_phase(touch.phase),
            }),
            _ => None,
        };
        if let Some(kind) = kind {
            self.emit(kind);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.process_package_completions();
        while let Ok(events) = self.gamepad_events.try_recv() {
            for event in events {
                self.emit(event);
            }
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: ()) {
        self.process_package_completions();
        self.process_commands(event_loop);
    }
}

impl Drop for LinuxHostApp {
    fn drop(&mut self) {
        // Signal every request before joining any worker. Dropping the
        // fetch future also drops incomplete cache staging files.
        for worker in &mut self.package_workers {
            worker.cancel();
        }
        self.package_workers.clear();
        self.gamepad_stop.store(true, Ordering::Release);
        self.gamepad_events = std_mpsc::sync_channel(0).1;
        if let Some(worker) = self.gamepad_thread.take() {
            if worker.join().is_err() {
                tracing::error!(
                    event = "platform.linux.gamepad.worker_panic",
                    diagnostic_code = "ASTRA_PLATFORM_GAMEPAD_WORKER_PANIC",
                    "Linux Gaming Input worker panicked during shutdown"
                );
            }
        }
    }
}

pub(super) fn linux_gamepad_worker(
    mut gamepads: gilrs::Gilrs,
    mut gamepad_mapper: astra_platform_common::GamepadMapper,
    stop: Arc<AtomicBool>,
    event_loop_proxy: EventLoopProxy<()>,
    tx: std_mpsc::SyncSender<Vec<PlatformEventKind>>,
) {
    while !stop.load(Ordering::Acquire) {
        let Some(event) = gamepads.next_event_blocking(Some(Duration::from_millis(250))) else {
            continue;
        };
        let Some(raw_event) = raw_gamepad_event(event) else {
            continue;
        };
        let mapped = match gamepad_mapper.apply_checked(raw_event) {
            Ok(events) => events,
            Err(error) => {
                tracing::warn!(
                    event = "platform.linux.gamepad.invalid_event",
                    diagnostic_code = ?error.code,
                    operation = %error.operation,
                    "Linux Gaming Input event was rejected"
                );
                continue;
            }
        };
        if mapped.is_empty() || tx.send(mapped).is_err() {
            return;
        }
        if event_loop_proxy.send_event(()).is_err() {
            tracing::debug!(
                event = "platform.linux.gamepad.wake.closed",
                diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                "Linux event loop was already closed after a gamepad batch"
            );
            return;
        }
    }
}

pub(super) fn raw_gamepad_event(
    event: gilrs::Event,
) -> Option<astra_platform_common::RawGamepadEvent> {
    use astra_platform::GamepadControl;
    use astra_platform_common::RawGamepadEvent;
    use gilrs::{Axis, Button, EventType};

    let raw_device_id = u32::try_from(usize::from(event.id)).ok()?;
    let map_button = |button| match button {
        Button::South => Some(GamepadControl::South),
        Button::East => Some(GamepadControl::East),
        Button::West => Some(GamepadControl::West),
        Button::North => Some(GamepadControl::North),
        Button::DPadUp => Some(GamepadControl::DpadUp),
        Button::DPadDown => Some(GamepadControl::DpadDown),
        Button::DPadLeft => Some(GamepadControl::DpadLeft),
        Button::DPadRight => Some(GamepadControl::DpadRight),
        Button::LeftTrigger => Some(GamepadControl::LeftShoulder),
        Button::RightTrigger => Some(GamepadControl::RightShoulder),
        Button::LeftTrigger2 => Some(GamepadControl::LeftTrigger),
        Button::RightTrigger2 => Some(GamepadControl::RightTrigger),
        Button::LeftThumb => Some(GamepadControl::LeftStickButton),
        Button::RightThumb => Some(GamepadControl::RightStickButton),
        Button::Start => Some(GamepadControl::Start),
        Button::Select => Some(GamepadControl::Select),
        _ => None,
    };
    let map_axis = |axis| match axis {
        Axis::LeftStickX => Some(GamepadControl::LeftStickX),
        Axis::LeftStickY => Some(GamepadControl::LeftStickY),
        Axis::RightStickX => Some(GamepadControl::RightStickX),
        Axis::RightStickY => Some(GamepadControl::RightStickY),
        _ => None,
    };
    match event.event {
        EventType::Connected => Some(RawGamepadEvent::Connected { raw_device_id }),
        EventType::Disconnected => Some(RawGamepadEvent::Disconnected { raw_device_id }),
        EventType::ButtonPressed(button, _) | EventType::ButtonRepeated(button, _) => {
            map_button(button).map(|control| RawGamepadEvent::Button {
                raw_device_id,
                control,
                pressed: true,
            })
        }
        EventType::ButtonReleased(button, _) => {
            map_button(button).map(|control| RawGamepadEvent::Button {
                raw_device_id,
                control,
                pressed: false,
            })
        }
        EventType::ButtonChanged(button, value, _) => map_button(button).map(|control| {
            if matches!(
                control,
                GamepadControl::LeftTrigger | GamepadControl::RightTrigger
            ) {
                RawGamepadEvent::Axis {
                    raw_device_id,
                    control,
                    value,
                }
            } else {
                RawGamepadEvent::Button {
                    raw_device_id,
                    control,
                    pressed: value >= 0.5,
                }
            }
        }),
        EventType::AxisChanged(axis_value, value, _) => {
            map_axis(axis_value).map(|control| RawGamepadEvent::Axis {
                raw_device_id,
                control,
                value,
            })
        }
        EventType::Dropped | EventType::ForceFeedbackEffectCompleted => None,
        _ => None,
    }
}

pub(super) fn input_state(state: ElementState) -> InputState {
    match state {
        ElementState::Pressed => InputState::Pressed,
        ElementState::Released => InputState::Released,
    }
}

pub(super) fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Back,
        MouseButton::Forward => PointerButton::Forward,
        MouseButton::Other(value) => PointerButton::Other(value),
    }
}

pub(super) fn touch_phase(phase: WinitTouchPhase) -> TouchPhase {
    match phase {
        WinitTouchPhase::Started => TouchPhase::Started,
        WinitTouchPhase::Moved => TouchPhase::Moved,
        WinitTouchPhase::Ended => TouchPhase::Ended,
        WinitTouchPhase::Cancelled => TouchPhase::Cancelled,
    }
}

pub(super) type SurfaceResource = astra_platform_common::WgpuPresentationCore;

pub(super) fn create_surface(
    window: Arc<Window>,
    width: u32,
    height: u32,
) -> Result<SurfaceResource, PlatformError> {
    let instance = astra_platform_common::native_wgpu_instance()?;
    let surface = instance
        .create_surface(window)
        .map_err(|_| host_error("surface.create", "wgpu surface creation failed"))?;
    pollster::block_on(SurfaceResource::new(instance, surface, width, height, true))
}

pub(super) fn capture_surface(
    surface: &mut SurfaceResource,
) -> Result<CapturedFrame, PlatformError> {
    let readback = surface.begin_capture()?;
    let (mapped_tx, mapped_rx) = std_mpsc::sync_channel(1);
    readback.map_async(move |result| {
        let _ = mapped_tx.send(result);
    });
    surface.poll(wgpu::PollType::wait_indefinitely())?;
    mapped_rx
        .recv()
        .map_err(|_| host_error("surface.capture", "GPU readback callback was lost"))?
        .map_err(|_| host_error("surface.capture", "GPU readback mapping failed"))?;
    readback.finish()
}

pub(super) struct AudioResource {
    pub(super) stream: cpal::Stream,
    #[cfg(feature = "platform-test-driver")]
    pub(super) stream_error: Arc<AtomicBool>,
    pub(super) paused: bool,
}
