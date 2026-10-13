use super::*;

impl ApplicationHandler for MacosHostApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        let sequence = self.next_sequence();
        let result = self
            .backend
            .emit_event(PlatformEvent::new(sequence, PlatformEventKind::Resumed));
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(result);
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.emit(PlatformEventKind::Suspended);
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
        let actions = self
            .accessibility
            .values_mut()
            .flat_map(MacosAccessibilityBridge::drain_actions)
            .collect::<Vec<_>>();
        for request in actions {
            self.emit(PlatformEventKind::AccessibilityAction {
                window: request.window,
                semantic_id: request.semantic_id,
                action: request.action,
                value: request.value,
            });
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

pub(super) struct PackageCompletion {
    pub(super) reply: oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    pub(super) result: Result<CachedPackageSource, PlatformError>,
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

impl Drop for MacosHostApp {
    fn drop(&mut self) {
        self.gamepad_stop.store(true, Ordering::Release);
        if let Some(worker) = self.gamepad_thread.take() {
            if worker.join().is_err() {
                tracing::error!(
                    event = "platform.macos.gamepad.worker_panic",
                    "macOS gamepad worker panicked during shutdown"
                );
            }
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
