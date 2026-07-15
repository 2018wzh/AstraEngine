use astra_platform::{HostLaunchProfile, HostStartFuture, PlatformHostFactory};

#[cfg(not(target_arch = "wasm32"))]
use astra_platform::{PlatformError, PlatformErrorCode};

#[derive(Debug, Clone, Copy, Default)]
pub struct WebPlatformFactory;

pub fn factory() -> WebPlatformFactory {
    WebPlatformFactory
}

impl PlatformHostFactory for WebPlatformFactory {
    fn start(&self, profile: HostLaunchProfile) -> HostStartFuture {
        #[cfg(target_arch = "wasm32")]
        {
            Box::pin(browser::start(profile))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(async move {
                profile.require_platform()?;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Web host requires a wasm32 browser environment",
                ))
            })
        }
    }
}

#[cfg(any(target_arch = "wasm32", feature = "web-code-check"))]
#[cfg_attr(
    all(feature = "web-code-check", not(target_arch = "wasm32")),
    allow(dead_code)
)]
mod browser {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        rc::Rc,
    };

    use astra_platform::{
        host_channel, AudioOutputHandle, CapturedFrame, DecodeSessionHandle, HostCommand,
        HostLaunchProfile, PackageSourceHandle, PlatformBackendChannels, PlatformError,
        PlatformErrorCode, PlatformEventKind, PlatformHostProfile, PlatformHostSession,
        SaveTransactionHandle, SurfaceHandle, WindowHandle,
    };
    use astra_platform_common::ResourceTable;
    use astra_ui_core::{UiSemanticRole, UiSemanticSnapshot, ValidateUi};
    use js_sys::{Function, Promise, Reflect};
    use wasm_bindgen::{closure::Closure, JsCast, JsValue};
    use wasm_bindgen_futures::{spawn_local, JsFuture};
    use web_sys::{
        CompositionEvent, Element, Event, EventTarget, HtmlCanvasElement, HtmlElement,
        KeyboardEvent, PointerEvent, TouchEvent, WheelEvent,
    };

    use crate::services::{
        commit_save, delete_save, preferred_audio_output_format, read_save, PackageBytes,
        SaveTransaction, WebAudioOutput, WebDecodeSession,
    };

    pub async fn start(
        launch_profile: HostLaunchProfile,
    ) -> Result<PlatformHostSession, PlatformError> {
        let profile = launch_profile.require_platform()?.clone();
        tracing::info!(
            event = "platform.web.host.start",
            profile = %profile.id,
            target = %profile.target,
            "Web platform host startup began"
        );
        if profile.platform != astra_platform::PlatformId::Web {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "host.start",
                "Web factory requires a Web profile",
            ));
        }
        let instance_lock = WebInstanceLock::acquire(&profile).await?;
        let (client, backend, events) = host_channel(
            HostLaunchProfile::platform(profile.clone()),
            profile.limits.command_queue_capacity,
            profile.limits.event_queue_capacity,
        )?;
        backend.event_emitter().emit(PlatformEventKind::Resumed)?;
        let lifecycle = BrowserLifecycle::bind(backend.event_emitter())?;
        spawn_local(run_backend(
            backend,
            profile.clone(),
            lifecycle,
            instance_lock,
        ));
        Ok(PlatformHostSession {
            client,
            events,
            profile: launch_profile,
        })
    }

    async fn run_backend(
        mut backend: PlatformBackendChannels,
        profile: PlatformHostProfile,
        _lifecycle: BrowserLifecycle,
        _instance_lock: WebInstanceLock,
    ) {
        let emitter = backend.event_emitter();
        let mut windows = ResourceTable::<CanvasResource, WindowHandle>::new("window");
        let mut surfaces = ResourceTable::<SurfaceResource, SurfaceHandle>::new("surface");
        let mut surface_windows = BTreeMap::<SurfaceHandle, WindowHandle>::new();
        let mut saves = ResourceTable::<SaveTransaction, SaveTransactionHandle>::new("save");
        let mut packages = ResourceTable::<PackageBytes, PackageSourceHandle>::new("package");
        let mut audio = ResourceTable::<WebAudioOutput, AudioOutputHandle>::new("audio");
        let mut decoders = ResourceTable::<WebDecodeSession, DecodeSessionHandle>::new("decode");
        while let Some(command) = backend.next_command().await {
            match command {
                HostCommand::CreateWindow { request, reply } => {
                    let result = CanvasResource::new(
                        request.title,
                        request.width,
                        request.height,
                        request.visible,
                    )
                    .and_then(|resource| windows.insert(resource))
                    .and_then(|handle| {
                        windows
                            .get_mut(handle)?
                            .bind_events(handle, emitter.clone())?;
                        Ok(handle)
                    });
                    let _ = reply.send(result);
                }
                HostCommand::CreateSurface { request, reply } => {
                    let window_handle = request.window;
                    let result = match windows.get(request.window) {
                        Ok(window) => {
                            create_surface(window.canvas.clone(), request.width, request.height)
                                .await
                                .and_then(|surface| surfaces.insert(surface))
                        }
                        Err(error) => Err(error),
                    };
                    if let Ok(surface) = result.as_ref() {
                        surface_windows.insert(*surface, window_handle);
                    }
                    let _ = reply.send(result);
                }
                HostCommand::PresentRgba {
                    surface,
                    frame,
                    reply,
                } => {
                    let result = match surfaces.get_mut(surface) {
                        Ok(surface) => surface.present(frame),
                        Err(error) => Err(error),
                    };
                    if result
                        .as_ref()
                        .is_err_and(|error| error.code == PlatformErrorCode::ContextLost)
                    {
                        let recovered = surfaces
                            .get_mut(surface)
                            .and_then(|surface| surface.reconfigure_after_loss())
                            .is_ok();
                        for event in
                            astra_platform_common::wgpu_recovery_events("webgpu", recovered)
                        {
                            let _ = emitter.emit(event);
                        }
                    }
                    let _ = reply.send(result);
                }
                HostCommand::PresentScene {
                    surface,
                    frame,
                    reply,
                } => {
                    let semantics = frame.semantics.clone();
                    let result = match surfaces.get_mut(surface) {
                        Ok(surface) => surface.present_scene(frame),
                        Err(error) => Err(error),
                    };
                    let result = result.and_then(|()| {
                        let Some(semantics) = semantics.as_ref() else {
                            return Ok(());
                        };
                        let window = *surface_windows
                            .get(&surface)
                            .ok_or_else(|| web_error("accessibility.web.update"))?;
                        windows.get_mut(window)?.update_accessibility(
                            window,
                            semantics,
                            emitter.clone(),
                        )
                    });
                    if result
                        .as_ref()
                        .is_err_and(|error| error.code == PlatformErrorCode::ContextLost)
                    {
                        let recovered = surfaces
                            .get_mut(surface)
                            .and_then(|surface| surface.reconfigure_after_loss())
                            .is_ok();
                        for event in
                            astra_platform_common::wgpu_recovery_events("webgpu", recovered)
                        {
                            let _ = emitter.emit(event);
                        }
                    }
                    let _ = reply.send(result);
                }
                #[cfg(feature = "platform-test-driver")]
                HostCommand::InjectSurfaceDeviceLoss { reply, .. } => {
                    let _ = reply.send(Err(PlatformError::new(
                        PlatformErrorCode::PlatformNotImplemented,
                        "surface.test.inject_device_loss",
                        "Web surface device-loss injection is outside the current implementation scope",
                    )));
                }
                HostCommand::CaptureSurface { surface, reply } => {
                    let result = match surfaces.get_mut(surface) {
                        Ok(surface) => capture_surface(surface).await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::DestroySurface { surface, reply } => {
                    surface_windows.remove(&surface);
                    let _ = reply.send(surfaces.remove(surface).map(|_| ()));
                }
                HostCommand::DestroyWindow { window, reply } => {
                    let _ = reply.send(windows.remove(window).map(|_| ()));
                }
                HostCommand::OpenAudioOutput { request, reply } => {
                    let result = WebAudioOutput::open(request)
                        .await
                        .and_then(|output| audio.insert(output));
                    let _ = reply.send(result);
                }
                HostCommand::QueryAudioOutputFormat { reply } => {
                    let _ = reply.send(preferred_audio_output_format().await);
                }
                HostCommand::QueryAudioDeviceFormat { reply } => {
                    let _ = reply.send(Err(PlatformError::new(
                        PlatformErrorCode::PlatformNotImplemented,
                        "audio.query_device_format",
                        "WebAudio format is selected during user-activated output creation",
                    )));
                }
                HostCommand::SubmitAudio {
                    output,
                    packet,
                    reply,
                } => {
                    let result = audio.get_mut(output).and_then(|audio| audio.submit(packet));
                    let _ = reply.send(result);
                }
                HostCommand::QueryAudio { output, reply } => {
                    let result = match audio.get(output) {
                        Ok(audio) => audio.state().await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::DrainAudio { output, reply } => {
                    let result = match audio.get(output) {
                        Ok(audio) => audio.drain().await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::QueryAudioOutput { output, reply } => {
                    let result = audio.get(output).map(|audio| audio.status());
                    let _ = reply.send(result);
                }
                HostCommand::PauseAudio { output, reply } => {
                    let result = match audio.get(output) {
                        Ok(audio) => audio.pause().await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::ResumeAudio { output, reply } => {
                    let result = match audio.get(output) {
                        Ok(audio) => audio.resume().await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::AbortAudio { output, reply } => {
                    let result = match audio.remove(output) {
                        Ok(audio) => audio.close().await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                #[cfg(feature = "platform-test-driver")]
                HostCommand::InjectAudioDeviceLoss { reply, .. } => {
                    let _ = reply.send(Err(PlatformError::new(
                        PlatformErrorCode::PlatformNotImplemented,
                        "audio.test.inject_device_loss",
                        "Windows-only test injection is unavailable on Web",
                    )));
                }
                HostCommand::CloseAudio { output, reply } => {
                    let result = match audio.remove(output) {
                        Ok(audio) => audio.close().await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::OpenDecode { kind, reply } => {
                    let _ = reply.send(decoders.insert(WebDecodeSession::new(kind)));
                }
                HostCommand::Decode {
                    session,
                    request,
                    reply,
                } => {
                    let result = match decoders.get_mut(session) {
                        Ok(decoder) => decoder.decode(request).await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::CloseDecode { session, reply } => {
                    let _ = reply.send(decoders.remove(session).map(|_| ()));
                }
                HostCommand::BeginSave { slot, reply } => {
                    let _ = reply.send(saves.insert(SaveTransaction {
                        slot,
                        bytes: Vec::new(),
                    }));
                }
                HostCommand::WriteSave {
                    transaction,
                    bytes,
                    reply,
                } => {
                    let result = saves
                        .get_mut(transaction)
                        .map(|save| save.bytes.extend(bytes));
                    let _ = reply.send(result);
                }
                HostCommand::CommitSave { transaction, reply } => {
                    let result = match saves.remove(transaction) {
                        Ok(save) => commit_save(&profile.package_id, &save).await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                HostCommand::AbortSave { transaction, reply } => {
                    let _ = reply.send(saves.remove(transaction).map(|_| ()));
                }
                HostCommand::ReadSave { slot, reply } => {
                    let _ = reply.send(read_save(&profile.package_id, &slot).await);
                }
                HostCommand::DeleteSave { slot, reply } => {
                    let _ = reply.send(delete_save(&profile.package_id, &slot).await);
                }
                HostCommand::OpenPackage { source, reply } => {
                    let result = PackageBytes::open(
                        source,
                        &profile.package_sources,
                        &profile.package_id,
                        &profile.package_cache,
                    )
                    .await
                    .and_then(|source| packages.insert(source));
                    let _ = reply.send(result);
                }
                HostCommand::ReadPackageRange {
                    source,
                    offset,
                    length,
                    reply,
                } => {
                    let result = packages
                        .get(source)
                        .and_then(|source| source.read_range(offset, length));
                    let _ = reply.send(result);
                }
                HostCommand::ClosePackage { source, reply } => {
                    let _ = reply.send(packages.remove(source).map(|_| ()));
                }
                HostCommand::Shutdown { reply } => {
                    let result = surfaces
                        .ensure_empty()
                        .and_then(|_| windows.ensure_empty())
                        .and_then(|_| saves.ensure_empty())
                        .and_then(|_| packages.ensure_empty())
                        .and_then(|_| audio.ensure_empty())
                        .and_then(|_| decoders.ensure_empty());
                    let exit = result.is_ok();
                    let _ = reply.send(result);
                    if exit {
                        tracing::info!(
                            event = "platform.web.host.shutdown",
                            "Web platform host shut down without live resources"
                        );
                        break;
                    }
                }
            }
        }
    }

    struct WebInstanceLock {
        release: Function,
    }

    impl WebInstanceLock {
        async fn acquire(profile: &PlatformHostProfile) -> Result<Self, PlatformError> {
            let identity = format!("{}\n{}\n{}", profile.package_id, profile.target, profile.id);
            let hash = astra_core::Hash256::from_sha256(identity.as_bytes()).to_string();
            let name = format!("astra-player-{}", hash.trim_start_matches("sha256:"));
            let acquire = Function::new_with_args(
                "name",
                "return new Promise((resolve, reject) => { if (!navigator.locks) { reject(new Error('Web Locks unavailable')); return; } let release; const held = new Promise(done => { release = done; }); navigator.locks.request(name, {mode: 'exclusive', ifAvailable: true}, lock => { if (!lock) { resolve(null); return; } resolve({release}); return held; }).catch(reject); });",
            );
            let promise: Promise = acquire
                .call1(&JsValue::NULL, &JsValue::from_str(&name))
                .map_err(|_| web_error("host.instance.acquire"))?
                .dyn_into()
                .map_err(|_| web_error("host.instance.acquire"))?;
            let value = JsFuture::from(promise)
                .await
                .map_err(|_| web_error("host.instance.acquire"))?;
            if value.is_null() {
                return Err(PlatformError::new(
                    PlatformErrorCode::AlreadyInUse,
                    "host.instance.acquire",
                    "the same game target and profile is already running",
                ));
            }
            let release = Reflect::get(&value, &JsValue::from_str("release"))
                .map_err(|_| web_error("host.instance.acquire"))?
                .dyn_into::<Function>()
                .map_err(|_| web_error("host.instance.acquire"))?;
            Ok(Self { release })
        }
    }

    impl Drop for WebInstanceLock {
        fn drop(&mut self) {
            let _ = self.release.call0(&JsValue::NULL);
        }
    }

    struct BrowserLifecycle {
        target: EventTarget,
        visibility: Closure<dyn FnMut(Event)>,
        gamepad_timer: i32,
        _gamepad_callback: Closure<dyn FnMut()>,
    }

    impl BrowserLifecycle {
        fn bind(emitter: astra_platform::PlatformEventEmitter) -> Result<Self, PlatformError> {
            let document = web_sys::window()
                .and_then(|window| window.document())
                .ok_or_else(|| web_error("lifecycle.bind"))?;
            let target: EventTarget = document.clone().into();
            let visibility_emitter = emitter.clone();
            let visibility = Closure::wrap(Box::new(move |_| {
                let event = if document.hidden() {
                    PlatformEventKind::Suspended
                } else {
                    PlatformEventKind::Resumed
                };
                let _ = visibility_emitter.emit(event);
            }) as Box<dyn FnMut(Event)>);
            target
                .add_event_listener_with_callback(
                    "visibilitychange",
                    visibility.as_ref().unchecked_ref(),
                )
                .map_err(|_| web_error("lifecycle.bind"))?;
            let gamepads = gilrs::Gilrs::new().map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "input.gamepad.open",
                    "browser Gamepad API initialization failed",
                )
            })?;
            let mapper = astra_platform_common::GamepadMapper::new(0.2)?;
            let state = Rc::new(RefCell::new((gamepads, mapper)));
            let gamepad_emitter = emitter.clone();
            let gamepad_callback = Closure::wrap(Box::new(move || {
                let mut state = state.borrow_mut();
                while let Some(event) = state.0.next_event() {
                    let Some(event) = raw_gamepad_event(event) else {
                        continue;
                    };
                    match state.1.apply_checked(event) {
                        Ok(events) => {
                            for event in events {
                                let _ = gamepad_emitter.emit(event);
                            }
                        }
                        Err(error) => tracing::warn!(
                            event = "platform.web.gamepad.invalid_event",
                            diagnostic_code = ?error.code,
                            operation = %error.operation,
                            "browser Gamepad event was rejected"
                        ),
                    }
                }
            }) as Box<dyn FnMut()>);
            let gamepad_timer = web_sys::window()
                .ok_or_else(|| web_error("input.gamepad.bind"))?
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    gamepad_callback.as_ref().unchecked_ref(),
                    16,
                )
                .map_err(|_| web_error("input.gamepad.bind"))?;
            Ok(Self {
                target,
                visibility,
                gamepad_timer,
                _gamepad_callback: gamepad_callback,
            })
        }
    }

    impl Drop for BrowserLifecycle {
        fn drop(&mut self) {
            let _ = self.target.remove_event_listener_with_callback(
                "visibilitychange",
                self.visibility.as_ref().unchecked_ref(),
            );
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(self.gamepad_timer);
            }
        }
    }

    fn raw_gamepad_event(event: gilrs::Event) -> Option<astra_platform_common::RawGamepadEvent> {
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
            EventType::AxisChanged(axis, value, _) => {
                map_axis(axis).map(|control| RawGamepadEvent::Axis {
                    raw_device_id,
                    control,
                    value,
                })
            }
            EventType::Dropped | EventType::ForceFeedbackEffectCompleted => None,
            _ => None,
        }
    }

    type GlobalListener = (EventTarget, &'static str, Closure<dyn FnMut(Event)>);

    struct AriaListener {
        target: EventTarget,
        event: &'static str,
        callback: Closure<dyn FnMut(Event)>,
    }

    impl Drop for AriaListener {
        fn drop(&mut self) {
            let _ = self.target.remove_event_listener_with_callback(
                self.event,
                self.callback.as_ref().unchecked_ref(),
            );
        }
    }

    struct CanvasResource {
        canvas: HtmlCanvasElement,
        aria_root: HtmlElement,
        aria_listeners: Vec<AriaListener>,
        aria_programmatic_focus: Rc<Cell<bool>>,
        listeners: BTreeMap<&'static str, Closure<dyn FnMut(Event)>>,
        global_listeners: Vec<GlobalListener>,
    }

    impl CanvasResource {
        fn new(
            title: String,
            width: u32,
            height: u32,
            visible: bool,
        ) -> Result<Self, PlatformError> {
            let window = web_sys::window().ok_or_else(|| web_error("window.create"))?;
            let document = window
                .document()
                .ok_or_else(|| web_error("window.create"))?;
            let canvas = document
                .create_element("canvas")
                .map_err(|_| web_error("window.create"))?
                .dyn_into::<HtmlCanvasElement>()
                .map_err(|_| web_error("window.create"))?;
            let aria_root = document
                .create_element("div")
                .map_err(|_| web_error("accessibility.web.create"))?
                .dyn_into::<HtmlElement>()
                .map_err(|_| web_error("accessibility.web.create"))?;
            aria_root.set_id("astra-accessibility-root");
            aria_root
                .set_attribute("role", "application")
                .map_err(|_| web_error("accessibility.web.create"))?;
            let aria_style = aria_root.style();
            aria_style
                .set_property("position", "fixed")
                .map_err(|_| web_error("accessibility.web.create"))?;
            aria_style
                .set_property("inset", "0")
                .map_err(|_| web_error("accessibility.web.create"))?;
            aria_style
                .set_property("pointer-events", "none")
                .map_err(|_| web_error("accessibility.web.create"))?;
            canvas.set_width(width);
            canvas.set_height(height);
            canvas.set_tab_index(0);
            canvas.set_id("astra-player-canvas");
            canvas
                .set_attribute("aria-label", &title)
                .map_err(|_| web_error("window.create"))?;
            if !visible {
                canvas
                    .style()
                    .set_property("display", "none")
                    .map_err(|_| web_error("window.create"))?;
            }
            document
                .body()
                .ok_or_else(|| web_error("window.create"))?
                .append_child(&canvas)
                .map_err(|_| web_error("window.create"))?;
            document
                .body()
                .ok_or_else(|| web_error("accessibility.web.create"))?
                .append_child(&aria_root)
                .map_err(|_| web_error("accessibility.web.create"))?;
            Ok(Self {
                canvas,
                aria_root,
                aria_listeners: Vec::new(),
                aria_programmatic_focus: Rc::new(Cell::new(false)),
                listeners: BTreeMap::new(),
                global_listeners: Vec::new(),
            })
        }

        fn update_accessibility(
            &mut self,
            window: WindowHandle,
            snapshot: &UiSemanticSnapshot,
            emitter: astra_platform::PlatformEventEmitter,
        ) -> Result<(), PlatformError> {
            snapshot
                .validate()
                .map_err(|_| web_error("accessibility.web.update"))?;
            self.aria_listeners.clear();
            self.aria_root.set_text_content(None);
            let document = self
                .aria_root
                .owner_document()
                .ok_or_else(|| web_error("accessibility.web.update"))?;
            let mut elements = BTreeMap::<String, Element>::new();
            let mut focused = None;
            for node in &snapshot.nodes {
                let element = document
                    .create_element("div")
                    .map_err(|_| web_error("accessibility.web.update"))?;
                element
                    .set_attribute("role", aria_role(node.role))
                    .map_err(|_| web_error("accessibility.web.update"))?;
                element
                    .set_attribute("data-astra-semantic-id", &node.id)
                    .map_err(|_| web_error("accessibility.web.update"))?;
                if let Some(name) = node.name.as_deref() {
                    element
                        .set_attribute("aria-label", name)
                        .map_err(|_| web_error("accessibility.web.update"))?;
                }
                if let Some(description) = node.description.as_deref() {
                    element
                        .set_attribute("aria-description", description)
                        .map_err(|_| web_error("accessibility.web.update"))?;
                }
                if let Some(value) = node.value.as_deref() {
                    element
                        .set_attribute("aria-valuetext", value)
                        .map_err(|_| web_error("accessibility.web.update"))?;
                }
                for (property, attribute) in [
                    ("range.value", "aria-valuenow"),
                    ("range.min", "aria-valuemin"),
                    ("range.max", "aria-valuemax"),
                ] {
                    if let Some(value) = node.properties.get(property) {
                        element
                            .set_attribute(attribute, value)
                            .map_err(|_| web_error("accessibility.web.update"))?;
                    }
                }
                element
                    .set_attribute("aria-disabled", if node.enabled { "false" } else { "true" })
                    .map_err(|_| web_error("accessibility.web.update"))?;
                element
                    .set_attribute("aria-hidden", if node.hidden { "true" } else { "false" })
                    .map_err(|_| web_error("accessibility.web.update"))?;
                element
                    .set_attribute(
                        "aria-selected",
                        if node.selected { "true" } else { "false" },
                    )
                    .map_err(|_| web_error("accessibility.web.update"))?;
                if let Some(checked) = node.checked {
                    element
                        .set_attribute("aria-checked", if checked { "true" } else { "false" })
                        .map_err(|_| web_error("accessibility.web.update"))?;
                }
                let style = element
                    .dyn_ref::<HtmlElement>()
                    .ok_or_else(|| web_error("accessibility.web.update"))?
                    .style();
                style
                    .set_property("position", "absolute")
                    .map_err(|_| web_error("accessibility.web.update"))?;
                style
                    .set_property("left", &format!("{}px", node.bounds_points.min.x))
                    .map_err(|_| web_error("accessibility.web.update"))?;
                style
                    .set_property("top", &format!("{}px", node.bounds_points.min.y))
                    .map_err(|_| web_error("accessibility.web.update"))?;
                style
                    .set_property(
                        "width",
                        &format!("{}px", node.bounds_points.max.x - node.bounds_points.min.x),
                    )
                    .map_err(|_| web_error("accessibility.web.update"))?;
                style
                    .set_property(
                        "height",
                        &format!("{}px", node.bounds_points.max.y - node.bounds_points.min.y),
                    )
                    .map_err(|_| web_error("accessibility.web.update"))?;
                style
                    .set_property("opacity", "0")
                    .map_err(|_| web_error("accessibility.web.update"))?;
                if !node.actions.is_empty() {
                    element
                        .set_attribute("tabindex", "0")
                        .map_err(|_| web_error("accessibility.web.update"))?;
                }
                if node
                    .actions
                    .contains(&astra_ui_core::UiSemanticAction::Activate)
                {
                    self.bind_aria_action(
                        &element,
                        "click",
                        window,
                        node.id.clone(),
                        "invoke",
                        false,
                        emitter.clone(),
                    )?;
                }
                if node
                    .actions
                    .contains(&astra_ui_core::UiSemanticAction::Focus)
                {
                    self.bind_aria_action(
                        &element,
                        "focus",
                        window,
                        node.id.clone(),
                        "focus",
                        false,
                        emitter.clone(),
                    )?;
                }
                if node
                    .actions
                    .contains(&astra_ui_core::UiSemanticAction::SetValue)
                {
                    self.bind_aria_action(
                        &element,
                        "change",
                        window,
                        node.id.clone(),
                        "set_value",
                        true,
                        emitter.clone(),
                    )?;
                }
                if node.role == UiSemanticRole::Slider {
                    self.bind_aria_slider_keys(&element, window, node.id.clone(), emitter.clone())?;
                }
                if node.focused {
                    focused = Some(element.clone());
                }
                elements.insert(node.id.clone(), element);
            }
            for node in &snapshot.nodes {
                let element = elements
                    .get(&node.id)
                    .ok_or_else(|| web_error("accessibility.web.update"))?;
                if let Some(parent) = node.parent_id.as_ref() {
                    elements
                        .get(parent)
                        .ok_or_else(|| web_error("accessibility.web.update"))?
                        .append_child(element)
                        .map_err(|_| web_error("accessibility.web.update"))?;
                } else {
                    self.aria_root
                        .append_child(element)
                        .map_err(|_| web_error("accessibility.web.update"))?;
                }
            }
            if let Some(focused) =
                focused.and_then(|element| element.dyn_into::<HtmlElement>().ok())
            {
                self.aria_programmatic_focus.set(true);
                let focus_result = focused.focus();
                self.aria_programmatic_focus.set(false);
                focus_result.map_err(|_| web_error("accessibility.web.focus"))?;
            }
            Ok(())
        }

        fn bind_aria_action(
            &mut self,
            element: &Element,
            event: &'static str,
            window: WindowHandle,
            semantic_id: String,
            action: &'static str,
            read_value: bool,
            emitter: astra_platform::PlatformEventEmitter,
        ) -> Result<(), PlatformError> {
            let target: EventTarget = element.clone().into();
            let focus_guard = self.aria_programmatic_focus.clone();
            let callback = Closure::wrap(Box::new(move |event: Event| {
                if action == "focus" && focus_guard.get() {
                    return;
                }
                let value = read_value
                    .then(|| {
                        event.target().and_then(|target| {
                            Reflect::get(&target, &JsValue::from_str("value"))
                                .ok()
                                .and_then(|value| value.as_string())
                        })
                    })
                    .flatten();
                let _ = emitter.emit(PlatformEventKind::AccessibilityAction {
                    window,
                    semantic_id: semantic_id.clone(),
                    action: action.to_string(),
                    value,
                });
            }) as Box<dyn FnMut(Event)>);
            target
                .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
                .map_err(|_| web_error("accessibility.web.bind"))?;
            self.aria_listeners.push(AriaListener {
                target,
                event,
                callback,
            });
            Ok(())
        }

        fn bind_aria_slider_keys(
            &mut self,
            element: &Element,
            window: WindowHandle,
            semantic_id: String,
            emitter: astra_platform::PlatformEventEmitter,
        ) -> Result<(), PlatformError> {
            let target: EventTarget = element.clone().into();
            let callback = Closure::wrap(Box::new(move |event: Event| {
                let Ok(event) = event.dyn_into::<KeyboardEvent>() else {
                    return;
                };
                let action = match event.key().as_str() {
                    "ArrowRight" | "ArrowUp" => "increment",
                    "ArrowLeft" | "ArrowDown" => "decrement",
                    _ => return,
                };
                event.prevent_default();
                let _ = emitter.emit(PlatformEventKind::AccessibilityAction {
                    window,
                    semantic_id: semantic_id.clone(),
                    action: action.to_string(),
                    value: None,
                });
            }) as Box<dyn FnMut(Event)>);
            target
                .add_event_listener_with_callback("keydown", callback.as_ref().unchecked_ref())
                .map_err(|_| web_error("accessibility.web.bind"))?;
            self.aria_listeners.push(AriaListener {
                target,
                event: "keydown",
                callback,
            });
            Ok(())
        }

        fn bind_events(
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

        fn add_listener(
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

        fn add_global_listener(
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
                let _ = target
                    .remove_event_listener_with_callback(name, callback.as_ref().unchecked_ref());
            }
            self.aria_listeners.clear();
            self.aria_root.remove();
            self.canvas.remove();
        }
    }

    fn aria_role(role: UiSemanticRole) -> &'static str {
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

    fn pointer_button(button: i16) -> astra_platform::PointerButton {
        match button {
            0 => astra_platform::PointerButton::Primary,
            1 => astra_platform::PointerButton::Middle,
            2 => astra_platform::PointerButton::Secondary,
            3 => astra_platform::PointerButton::Back,
            4 => astra_platform::PointerButton::Forward,
            other => astra_platform::PointerButton::Other(other.max(0) as u16),
        }
    }

    type SurfaceResource = astra_platform_common::WgpuPresentationCore;

    #[cfg(target_arch = "wasm32")]
    async fn create_surface(
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
    async fn create_surface(
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

    async fn capture_surface(
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

    #[cfg(any())]
    struct LegacySurfaceResource {
        _instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        _adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        bind_group_layout: wgpu::BindGroupLayout,
        sampler: wgpu::Sampler,
        pipeline: wgpu::RenderPipeline,
        last_upload: Option<UploadFrame>,
    }

    #[cfg(any())]
    impl LegacySurfaceResource {
        async fn new(
            canvas: HtmlCanvasElement,
            width: u32,
            height: u32,
        ) -> Result<Self, PlatformError> {
            let instance = wgpu::Instance::default();
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|_| web_error("surface.create"))?;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                })
                .await
                .map_err(|_| web_error("surface.create"))?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .map_err(|_| web_error("surface.create"))?;
            let mut config = surface
                .get_default_config(&adapter, width, height)
                .ok_or_else(|| web_error("surface.create"))?;
            config.present_mode = wgpu::PresentMode::Fifo;
            surface.configure(&device, &config);
            let (bind_group_layout, sampler, pipeline) = create_pipeline(&device, config.format);
            Ok(Self {
                _instance: instance,
                surface,
                _adapter: adapter,
                device,
                queue,
                config,
                bind_group_layout,
                sampler,
                pipeline,
                last_upload: None,
            })
        }

        fn present(&mut self, frame: RgbaFrame) -> Result<(), PlatformError> {
            if frame.width != self.config.width || frame.height != self.config.height {
                self.config.width = frame.width;
                self.config.height = frame.height;
                self.surface.configure(&self.device, &self.config);
            }
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("astra-web-frame-upload"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba8,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("astra-web-frame-bind-group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let output = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(output)
                | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
                wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                    return Err(PlatformError::new(
                        PlatformErrorCode::ContextLost,
                        "surface.present_rgba",
                        "WebGPU canvas surface was lost",
                    ));
                }
                _ => return Err(web_error("surface.present_rgba")),
            };
            let output_view = output
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("astra-web-frame-encoder"),
                });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("astra-web-frame-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &output_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            self.queue.submit([encoder.finish()]);
            self.queue.present(output);
            self.last_upload = Some(UploadFrame {
                texture,
                width: frame.width,
                height: frame.height,
            });
            Ok(())
        }

        async fn capture(&mut self) -> Result<CapturedFrame, PlatformError> {
            let upload = self
                .last_upload
                .as_ref()
                .ok_or_else(|| web_error("surface.capture"))?;
            let row = upload.width * 4;
            let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("astra-web-frame-readback"),
                size: u64::from(padded) * u64::from(upload.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("astra-web-frame-readback-encoder"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &upload.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(upload.height),
                    },
                },
                wgpu::Extent3d {
                    width: upload.width,
                    height: upload.height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit([encoder.finish()]);
            let (mapped_tx, mapped_rx) = tokio::sync::oneshot::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = mapped_tx.send(result);
                });
            mapped_rx
                .await
                .map_err(|_| web_error("surface.capture"))?
                .map_err(|_| web_error("surface.capture"))?;
            let mapped = buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|_| web_error("surface.capture"))?;
            let mut rgba8 = Vec::with_capacity((row * upload.height) as usize);
            for bytes in mapped
                .chunks_exact(padded as usize)
                .take(upload.height as usize)
            {
                rgba8.extend_from_slice(&bytes[..row as usize]);
            }
            drop(mapped);
            buffer.unmap();
            Ok(CapturedFrame {
                width: upload.width,
                height: upload.height,
                rgba8,
            })
        }
    }

    #[cfg(any())]
    struct UploadFrame {
        texture: wgpu::Texture,
        width: u32,
        height: u32,
    }

    #[cfg(any())]
    fn create_pipeline(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> (wgpu::BindGroupLayout, wgpu::Sampler, wgpu::RenderPipeline) {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("astra-web-frame-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("astra-web-frame-shader"),
            source: wgpu::ShaderSource::Wgsl(FRAME_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("astra-web-frame-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("astra-web-frame-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        (layout, sampler, pipeline)
    }

    fn web_error(operation: &'static str) -> PlatformError {
        PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            operation,
            "browser platform operation failed",
        )
    }

    #[cfg(any())]
    const FRAME_SHADER: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0)
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}
@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(frame_texture));
    return textureSample(frame_texture, frame_sampler, position.xy / dimensions);
}
"#;
}
