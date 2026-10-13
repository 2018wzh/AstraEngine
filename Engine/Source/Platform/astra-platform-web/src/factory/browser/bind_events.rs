use super::*;

impl CanvasResource {
    pub(super) fn bind_events(
        &mut self,
        window: WindowHandle,
        emitter: astra_platform::PlatformEventEmitter,
    ) -> Result<(), PlatformError> {
        self.add_listener("keydown", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<KeyboardEvent>() {
                    let _ = emitter.emit(PlatformEventKind::Keyboard {
                        window,
                        physical_key: event.code(),
                        logical_key: Some(event.key()),
                        state: astra_platform::InputState::Pressed,
                        repeat: event.repeat(),
                    });
                }
            }
        })?;
        self.add_listener("keyup", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<KeyboardEvent>() {
                    let _ = emitter.emit(PlatformEventKind::Keyboard {
                        window,
                        physical_key: event.code(),
                        logical_key: Some(event.key()),
                        state: astra_platform::InputState::Released,
                        repeat: event.repeat(),
                    });
                }
            }
        })?;
        self.add_listener("pointermove", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<PointerEvent>() {
                    let _ = emitter.emit(PlatformEventKind::PointerMoved {
                        window,
                        x: event.offset_x() as f64,
                        y: event.offset_y() as f64,
                    });
                }
            }
        })?;
        self.add_listener("pointerdown", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<PointerEvent>() {
                    let _ = emitter.emit(PlatformEventKind::PointerButton {
                        window,
                        button: pointer_button(event.button()),
                        state: astra_platform::InputState::Pressed,
                    });
                }
            }
        })?;
        self.add_listener("pointerup", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<PointerEvent>() {
                    let _ = emitter.emit(PlatformEventKind::PointerButton {
                        window,
                        button: pointer_button(event.button()),
                        state: astra_platform::InputState::Released,
                    });
                }
            }
        })?;
        self.add_listener("focus", {
            let emitter = emitter.clone();
            move |_| {
                let _ = emitter.emit(PlatformEventKind::WindowFocused {
                    window,
                    focused: true,
                });
            }
        })?;
        self.add_listener("blur", {
            let emitter = emitter.clone();
            move |_| {
                let _ = emitter.emit(PlatformEventKind::WindowFocused {
                    window,
                    focused: false,
                });
            }
        })?;
        self.add_listener("compositionupdate", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<CompositionEvent>() {
                    let _ = emitter.emit(PlatformEventKind::ImePreedit {
                        window,
                        text: event.data().unwrap_or_default(),
                        cursor: None,
                    });
                }
            }
        })?;
        self.add_listener("compositionend", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<CompositionEvent>() {
                    let _ = emitter.emit(PlatformEventKind::ImeCommit {
                        window,
                        text: event.data().unwrap_or_default(),
                    });
                }
            }
        })?;
        for (name, phase) in [
            ("touchstart", astra_platform::TouchPhase::Started),
            ("touchmove", astra_platform::TouchPhase::Moved),
            ("touchend", astra_platform::TouchPhase::Ended),
            ("touchcancel", astra_platform::TouchPhase::Cancelled),
        ] {
            let emitter = emitter.clone();
            self.add_listener(name, move |event| {
                if let Ok(event) = event.dyn_into::<TouchEvent>() {
                    let touches = event.changed_touches();
                    for index in 0..touches.length() {
                        if let Some(touch) = touches.item(index) {
                            let _ = emitter.emit(PlatformEventKind::Touch {
                                window,
                                id: touch.identifier() as u64,
                                x: f64::from(touch.client_x()),
                                y: f64::from(touch.client_y()),
                                phase,
                            });
                        }
                    }
                }
            })?;
        }
        self.add_listener("wheel", {
            let emitter = emitter.clone();
            move |event| {
                if let Ok(event) = event.dyn_into::<WheelEvent>() {
                    let _ = emitter.emit(PlatformEventKind::MouseWheel {
                        window,
                        delta_x: event.delta_x() as f32,
                        delta_y: event.delta_y() as f32,
                    });
                }
            }
        })?;
        let browser_window = web_sys::window().ok_or_else(|| web_error("input.bind"))?;
        let canvas = self.canvas.clone();
        self.add_global_listener(browser_window.into(), "resize", move |_| {
            let width = u32::try_from(canvas.client_width().max(1)).unwrap_or(1);
            let height = u32::try_from(canvas.client_height().max(1)).unwrap_or(1);
            canvas.set_width(width);
            canvas.set_height(height);
            let scale_factor = web_sys::window()
                .map(|window| window.device_pixel_ratio())
                .unwrap_or(1.0);
            let _ = emitter.emit(PlatformEventKind::WindowResized {
                window,
                width,
                height,
                scale_factor,
            });
        })?;
        Ok(())
    }

    pub(super) fn add_listener(
        &mut self,
        name: &'static str,
        callback: impl FnMut(Event) + 'static,
    ) -> Result<(), PlatformError> {
        let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
        self.canvas
            .add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())
            .map_err(|_| web_error("input.bind"))?;
        self.listeners.insert(name, callback);
        Ok(())
    }

    pub(super) fn add_global_listener(
        &mut self,
        target: EventTarget,
        name: &'static str,
        callback: impl FnMut(Event) + 'static,
    ) -> Result<(), PlatformError> {
        let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
        target
            .add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())
            .map_err(|_| web_error("input.bind"))?;
        self.global_listeners.push((target, name, callback));
        Ok(())
    }
}

impl Drop for CanvasResource {
    fn drop(&mut self) {
        for (name, callback) in &self.listeners {
            let _ = self
                .canvas
                .remove_event_listener_with_callback(name, callback.as_ref().unchecked_ref());
        }
        for (target, name, callback) in &self.global_listeners {
            let _ =
                target.remove_event_listener_with_callback(name, callback.as_ref().unchecked_ref());
        }
        self.aria_listeners.clear();
        self.aria_root.remove();
        self.canvas.remove();
    }
}

pub(super) fn aria_role(role: UiSemanticRole) -> &'static str {
    match role {
        UiSemanticRole::Application => "application",
        UiSemanticRole::Window => "region",
        UiSemanticRole::Dialog => "dialog",
        UiSemanticRole::Group => "group",
        UiSemanticRole::Text => "paragraph",
        UiSemanticRole::Image => "img",
        UiSemanticRole::Button => "button",
        UiSemanticRole::Toggle => "checkbox",
        UiSemanticRole::Slider => "slider",
        UiSemanticRole::Select => "combobox",
        UiSemanticRole::List => "list",
        UiSemanticRole::ListItem => "listitem",
        UiSemanticRole::Grid => "grid",
        UiSemanticRole::GridCell => "gridcell",
        UiSemanticRole::TextInput => "textbox",
        UiSemanticRole::Link => "link",
        UiSemanticRole::Canvas => "img",
    }
}

pub(super) fn pointer_button(button: i16) -> astra_platform::PointerButton {
    match button {
        0 => astra_platform::PointerButton::Primary,
        1 => astra_platform::PointerButton::Middle,
        2 => astra_platform::PointerButton::Secondary,
        3 => astra_platform::PointerButton::Back,
        4 => astra_platform::PointerButton::Forward,
        other => astra_platform::PointerButton::Other(other.max(0) as u16),
    }
}

pub(super) type SurfaceResource = astra_platform_common::WgpuPresentationCore;

#[cfg(target_arch = "wasm32")]
pub(super) async fn create_surface(
    canvas: HtmlCanvasElement,
    width: u32,
    height: u32,
) -> Result<SurfaceResource, PlatformError> {
    let instance = wgpu::Instance::default();
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|_| web_error("surface.create"))?;
    SurfaceResource::new(instance, surface, width, height, true).await
}

#[cfg(all(feature = "web-code-check", not(target_arch = "wasm32")))]
pub(super) async fn create_surface(
    _canvas: HtmlCanvasElement,
    _width: u32,
    _height: u32,
) -> Result<SurfaceResource, PlatformError> {
    Err(PlatformError::new(
        PlatformErrorCode::UnsupportedPlatform,
        "surface.create",
        "web-code-check validates browser control flow without constructing a native canvas surface",
    ))
}

pub(super) async fn capture_surface(
    surface: &mut SurfaceResource,
) -> Result<CapturedFrame, PlatformError> {
    let readback = surface.begin_capture()?;
    let (mapped_tx, mapped_rx) = tokio::sync::oneshot::channel();
    readback.map_async(move |result| {
        let _ = mapped_tx.send(result);
    });
    mapped_rx
        .await
        .map_err(|_| web_error("surface.capture"))?
        .map_err(|_| web_error("surface.capture"))?;
    readback.finish()
}

pub(super) fn web_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::ProviderUnavailable,
        operation,
        "browser platform operation failed",
    )
}
