use super::*;

impl ApplicationHandler for AndroidHostApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.resumed {
            return;
        }
        self.resumed = true;
        if let Err(error) = self.restore_surfaces() {
            self.player_result = Some(Err(error));
            return;
        }
        for handle in self.audio_outputs.handles().collect::<Vec<_>>() {
            if let Err(error) = self
                .audio_outputs
                .get_mut(handle)
                .and_then(|audio| audio.set_suspended(false))
            {
                self.player_result = Some(Err(error));
                return;
            }
        }
        self.emit(PlatformEventKind::Resumed);
        if let Some(player) = self.player.take() {
            match thread::Builder::new()
                .name("astra-player-android".to_string())
                .spawn(player)
            {
                Ok(thread) => self.player_thread = Some(thread),
                Err(_) => {
                    self.player_result = Some(Err(host_error(
                        "player.start",
                        "Android Player thread could not be started",
                    )))
                }
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if !self.resumed {
            return;
        }
        self.resumed = false;
        for handle in self.audio_outputs.handles().collect::<Vec<_>>() {
            if let Err(error) = self
                .audio_outputs
                .get_mut(handle)
                .and_then(|audio| audio.set_suspended(true))
            {
                self.player_result = Some(Err(error));
            }
        }
        self.suspend_surfaces();
        self.emit(PlatformEventKind::Suspended);
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        self.emit(PlatformEventKind::MemoryPressure {
            level: MemoryPressureLevel::Critical,
        });
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
        let accessibility_actions = match self.accessibility.as_mut() {
            Some(accessibility) => match accessibility.drain_actions() {
                Ok(actions) => actions,
                Err(error) => {
                    self.player_result = Some(Err(error));
                    event_loop.exit();
                    return;
                }
            },
            None => Vec::new(),
        };
        for action in accessibility_actions {
            self.emit(PlatformEventKind::AccessibilityAction {
                window: action.window,
                semantic_id: action.semantic_id,
                action: action.action,
                value: action.value,
            });
        }
        let window = self.window_ids.values().next().copied();
        let bridge_events = match super::super::drain_bridge_events(window) {
            Ok(events) => events,
            Err(error) => {
                self.player_result = Some(Err(error));
                event_loop.exit();
                return;
            }
        };
        for event in bridge_events {
            match event {
                PlatformEventKind::AudioFocusChanged { state } => {
                    let handles = self.audio_outputs.handles().collect::<Vec<_>>();
                    for handle in handles {
                        if let Err(error) = self
                            .audio_outputs
                            .get_mut(handle)
                            .and_then(|audio| audio.apply_focus(state))
                        {
                            tracing::error!(
                                event = "platform.android.audio.focus_apply_failed",
                                diagnostic_code = ?error.code,
                                operation = %error.operation,
                                "Android audio focus change could not be applied"
                            );
                        }
                    }
                    self.emit(PlatformEventKind::AudioFocusChanged { state });
                }
                other => self.emit(other),
            }
        }
        if self.player_result.is_none() {
            if let Ok(result) = self.player_result_rx.try_recv() {
                let failed = result.is_err();
                self.player_result = Some(result);
                if failed {
                    event_loop.exit();
                }
            }
        }
        if self.player_result.as_ref().is_some_and(Result::is_err) {
            event_loop.exit();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: ()) {
        self.process_commands(event_loop);
        self.process_package_completions();
    }
}

pub(super) fn capture_surface(surface: &mut SurfaceCore) -> Result<CapturedFrame, PlatformError> {
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

pub(super) fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}
