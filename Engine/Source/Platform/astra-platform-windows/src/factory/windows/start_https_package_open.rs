use super::*;

impl WindowsHostApp {
    pub(super) fn start_https_package_open(
        &mut self,
        url: String,
        expected_hash: String,
        reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    ) {
        let completion_tx = self.package_completion_tx.clone();
        let policies = self.package_source_policies.clone();
        let package_id = self.package_id.clone();
        let policy = self.package_cache_policy.clone();
        let event_loop_proxy = self.event_loop_proxy.clone();
        self.pending_package_opens += 1;
        thread::spawn(move || {
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
                runtime.block_on(client.fetch_into_cache(&url, &expected_hash, &mut cache))?;
                cache.open_source(&expected_hash)
            })();
            if completion_tx
                .send(PackageCompletion { reply, result })
                .is_ok()
                && event_loop_proxy.send_event(()).is_err()
            {
                tracing::error!(
                    event = "platform.windows.package_completion_wake.failed",
                    diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                    "Windows package completion could not wake the event loop"
                );
            }
        });
    }

    pub(super) fn start_audio_output_open(
        &mut self,
        request: AudioOutputRequest,
        reply: oneshot::Sender<Result<OpenedAudioOutput, PlatformError>>,
    ) {
        let completion_tx = self.audio_completion_tx.clone();
        let event_loop_proxy = self.event_loop_proxy.clone();
        let audio_wake = self.backend.audio_wake();
        let test_null_audio = self.test_null_audio;
        self.pending_audio_opens += 1;
        thread::spawn(move || {
            let result = AudioResource::new(request, audio_wake, test_null_audio);
            if completion_tx
                .send(AudioCompletion { reply, result })
                .is_ok()
                && event_loop_proxy.send_event(()).is_err()
            {
                tracing::error!(
                    event = "platform.windows.audio_completion_wake.failed",
                    diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                    "Windows audio completion could not wake the event loop"
                );
            }
        });
    }

    pub(super) fn process_audio_completions(&mut self) {
        while let Ok(completion) = self.audio_completion_rx.try_recv() {
            self.pending_audio_opens = self.pending_audio_opens.saturating_sub(1);
            let result = completion.result.and_then(|(resource, lane, format)| {
                let handle = self.audio_outputs.insert(resource)?;
                Ok(OpenedAudioOutput {
                    handle,
                    format,
                    lane: Box::new(lane),
                    capture: None,
                })
            });
            let _ = completion.reply.send(result);
        }
    }

    pub(super) fn process_package_completions(&mut self) {
        while let Ok(completion) = self.package_completion_rx.try_recv() {
            self.pending_package_opens = self.pending_package_opens.saturating_sub(1);
            let result = completion.result.and_then(|source| {
                self.package_sources
                    .insert(PackageSourceResource::Cached(source))
            });
            let _ = completion.reply.send(result);
        }
    }
}

impl ApplicationHandler for WindowsHostApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        let sequence = self.next_sequence();
        let result = self
            .backend
            .emit_event(PlatformEvent::new(sequence, PlatformEventKind::Resumed));
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(result);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
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
        if let (Ok(native), Some(bridge)) = (
            self.windows.get(window),
            self.accessibility.get_mut(&window_id),
        ) {
            bridge.process_event(native.as_ref(), &event);
        }
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
        self.process_audio_completions();
        self.process_commands(event_loop);
        let accessibility_actions = self
            .accessibility
            .values_mut()
            .flat_map(WindowsAccessibilityBridge::drain_actions)
            .collect::<Vec<_>>();
        for request in accessibility_actions {
            self.emit(PlatformEventKind::AccessibilityAction {
                window: request.window,
                semantic_id: request.semantic_id,
                action: request.action,
                value: request.value,
            });
        }
        while let Ok(events) = self.gamepad_events.try_recv() {
            for event in events {
                self.emit(event);
            }
        }
        // The gamepad worker owns the backend and wakes this event loop
        // when it has a bounded batch. There is intentionally no render-
        // or input-driven timer here; gilrs' internal 250 ms wait is only
        // a low-frequency discovery fallback on platforms without a
        // hotplug handle.
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

pub(super) struct PackageCompletion {
    pub(super) reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    pub(super) result: Result<CachedPackageSource, PlatformError>,
}

pub(super) struct AudioCompletion {
    pub(super) reply: oneshot::Sender<Result<OpenedAudioOutput, PlatformError>>,
    pub(super) result: Result<
        (
            AudioResource,
            astra_platform_common::NativeAudioProducer,
            AudioDeviceFormat,
        ),
        PlatformError,
    >,
}

pub(super) enum PackageSourceResource {
    Bundled(FilePackageSource),
    Cached(CachedPackageSource),
}

impl PackageSourceResource {
    pub(super) fn read_range(
        &mut self,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, PlatformError> {
        match self {
            Self::Bundled(source) => source.read_range(offset, length),
            Self::Cached(source) => source.read_range(offset, length),
        }
    }
}

impl Drop for WindowsHostApp {
    fn drop(&mut self) {
        self.gamepad_stop.store(true, Ordering::Release);
        self.gamepad_events = std_mpsc::sync_channel(0).1;
        if let Some(worker) = self.gamepad_thread.take() {
            if worker.join().is_err() {
                tracing::error!(
                    event = "platform.windows.gamepad.worker_panic",
                    diagnostic_code = "ASTRA_PLATFORM_GAMEPAD_WORKER_PANIC",
                    "Windows Gaming Input worker panicked during shutdown"
                );
            }
        }
    }
}

pub(super) fn windows_gamepad_worker(
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
                    event = "platform.windows.gamepad.invalid_event",
                    diagnostic_code = ?error.code,
                    operation = %error.operation,
                    "Windows Gaming Input event was rejected"
                );
                continue;
            }
        };
        if mapped.is_empty() || tx.send(mapped).is_err() {
            return;
        }
        if event_loop_proxy.send_event(()).is_err() {
            tracing::debug!(
                event = "platform.windows.gamepad.wake.closed",
                diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP_CLOSED",
                "Windows event loop was already closed after a gamepad batch"
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
