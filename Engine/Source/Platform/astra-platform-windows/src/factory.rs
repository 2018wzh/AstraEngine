use astra_platform::{HostStartFuture, PlatformHostFactory, PlatformHostProfile};

#[cfg(not(target_os = "windows"))]
use astra_platform::{PlatformError, PlatformErrorCode, PlatformId};

#[derive(Debug, Clone, Default)]
pub struct WindowsPlatformFactory {
    #[cfg(target_os = "windows")]
    roots: Option<HostRoots>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct HostRoots {
    save_base: std::path::PathBuf,
    bundle_root: std::path::PathBuf,
}

pub fn factory() -> WindowsPlatformFactory {
    WindowsPlatformFactory::default()
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
pub fn factory_with_test_roots(
    save_base: impl AsRef<std::path::Path>,
    bundle_root: impl AsRef<std::path::Path>,
) -> WindowsPlatformFactory {
    WindowsPlatformFactory {
        roots: Some(HostRoots {
            save_base: save_base.as_ref().to_path_buf(),
            bundle_root: bundle_root.as_ref().to_path_buf(),
        }),
    }
}

impl PlatformHostFactory for WindowsPlatformFactory {
    fn start(&self, profile: PlatformHostProfile) -> HostStartFuture {
        #[cfg(target_os = "windows")]
        {
            Box::pin(crate::factory::windows::start(profile, self.roots.clone()))
        }
        #[cfg(not(target_os = "windows"))]
        {
            Box::pin(async move {
                let _ = profile;
                Err(PlatformError::new(
                    PlatformErrorCode::UnsupportedPlatform,
                    "host.start",
                    "Windows host can only start on Windows",
                )
                .with_field("platform", PlatformId::Windows.as_str()))
            })
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        collections::BTreeMap,
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
            mpsc as std_mpsc, Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use astra_media::{DecodeOutput as MediaDecodeOutput, DecodeProvider};
    use astra_platform::{
        host_channel, AudioMeter, AudioOutputHandle, AudioOutputRequest, AudioPacket,
        CapturedFrame, DecodeKind, DecodeOutput, DecodeSessionHandle, HostCommand, InputState,
        PackageSourceHandle, PackageSourceRequest, PlatformBackendChannels, PlatformDecodeRequest,
        PlatformError, PlatformErrorCode, PlatformEvent, PlatformEventKind, PlatformHostProfile,
        PlatformHostSession, PointerButton, RgbaFrame, SaveTransactionHandle, SurfaceHandle,
        TouchPhase, WindowHandle,
    };
    use astra_platform_general::{
        AtomicSaveStore, FilePackageSource, ResourceTable, SaveTransaction,
    };
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use winit::{
        application::ApplicationHandler,
        event::{
            ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase as WinitTouchPhase,
            WindowEvent,
        },
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        platform::windows::EventLoopBuilderExtWindows,
        window::{Window, WindowAttributes, WindowId},
    };

    pub async fn start(
        profile: PlatformHostProfile,
        roots: Option<super::HostRoots>,
    ) -> Result<PlatformHostSession, PlatformError> {
        if profile.platform != astra_platform::PlatformId::Windows {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "host.start",
                "Windows factory requires a Windows profile",
            ));
        }
        let command_capacity = profile.limits.command_queue_capacity;
        let event_capacity = profile.limits.event_queue_capacity;
        let (client, backend, events) =
            host_channel(profile.clone(), command_capacity, event_capacity)?;
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let backend_profile = profile.clone();
        thread::Builder::new()
            .name("astra-platform-windows".to_string())
            .spawn(move || run_backend(backend, ready_tx, backend_profile, roots))
            .map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "host.start",
                    "Windows platform thread could not be started",
                )
            })?;
        ready_rx.recv().map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::QueueClosed,
                "host.start",
                "Windows platform thread stopped during startup",
            )
        })??;
        Ok(PlatformHostSession {
            client,
            events,
            profile,
        })
    }

    fn run_backend(
        backend: PlatformBackendChannels,
        ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
        profile: PlatformHostProfile,
        roots: Option<super::HostRoots>,
    ) {
        let roots = match roots.or_else(default_roots) {
            Some(roots) => roots,
            None => {
                let _ = ready.send(Err(host_error(
                    "host.start",
                    "Windows save or bundle root is unavailable",
                )));
                return;
            }
        };
        let save_store = match AtomicSaveStore::new(&roots.save_base, &profile.package_id) {
            Ok(store) => store,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        let event_loop = match EventLoop::builder().with_any_thread(true).build() {
            Ok(event_loop) => event_loop,
            Err(_) => {
                let _ = ready.send(Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "host.start",
                    "Windows event loop could not be created",
                )));
                return;
            }
        };
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = match WindowsHostApp::new(backend, ready, save_store, roots.bundle_root) {
            Ok(app) => app,
            Err(_) => return,
        };
        if let Err(error) = event_loop.run_app(&mut app) {
            tracing::error!(
                event = "platform.windows.event_loop.failed",
                diagnostic_code = "ASTRA_PLATFORM_EVENT_LOOP",
                error = %error,
                "Windows platform event loop failed"
            );
        }
    }

    struct WindowsHostApp {
        backend: PlatformBackendChannels,
        ready: Option<std_mpsc::SyncSender<Result<(), PlatformError>>>,
        windows: ResourceTable<Arc<Window>, WindowHandle>,
        window_ids: BTreeMap<WindowId, WindowHandle>,
        surfaces: ResourceTable<SurfaceResource, SurfaceHandle>,
        audio_outputs: ResourceTable<AudioResource, AudioOutputHandle>,
        decode_sessions: ResourceTable<DecodeResource, DecodeSessionHandle>,
        save_store: AtomicSaveStore,
        save_transactions: ResourceTable<SaveTransaction, SaveTransactionHandle>,
        bundle_root: std::path::PathBuf,
        package_sources: ResourceTable<FilePackageSource, PackageSourceHandle>,
        event_sequence: u64,
        gamepads: gilrs::Gilrs,
        gamepad_mapper: astra_platform_general::GamepadMapper,
    }

    impl WindowsHostApp {
        fn new(
            backend: PlatformBackendChannels,
            ready: std_mpsc::SyncSender<Result<(), PlatformError>>,
            save_store: AtomicSaveStore,
            bundle_root: std::path::PathBuf,
        ) -> Result<Self, PlatformError> {
            let gamepads = gilrs::Gilrs::new().map_err(|_| {
                let error = host_error(
                    "input.gamepad.open",
                    "Windows Gaming Input initialization failed",
                );
                let _ = ready.send(Err(error.clone()));
                error
            })?;
            let gamepad_mapper = astra_platform_general::GamepadMapper::new(0.2)?;
            Ok(Self {
                backend,
                ready: Some(ready),
                windows: ResourceTable::new("window"),
                window_ids: BTreeMap::new(),
                surfaces: ResourceTable::new("surface"),
                audio_outputs: ResourceTable::new("audio_output"),
                decode_sessions: ResourceTable::new("decode_session"),
                save_store,
                save_transactions: ResourceTable::new("save_transaction"),
                bundle_root,
                package_sources: ResourceTable::new("package_source"),
                event_sequence: 0,
                gamepads,
                gamepad_mapper,
            })
        }

        fn next_sequence(&mut self) -> u64 {
            self.event_sequence += 1;
            self.event_sequence
        }

        fn emit(&mut self, kind: PlatformEventKind) {
            let sequence = self.next_sequence();
            if let Err(error) = self.backend.emit_event(PlatformEvent::new(sequence, kind)) {
                tracing::error!(
                    event = "platform.windows.event.emit_failed",
                    diagnostic_code = ?error.code,
                    operation = %error.operation,
                    "Windows platform event could not be emitted"
                );
            }
        }

        fn process_commands(&mut self, event_loop: &ActiveEventLoop) {
            loop {
                let command = match self.backend.try_next_command() {
                    Ok(Some(command)) => command,
                    Ok(None) => break,
                    Err(_) => {
                        event_loop.exit();
                        break;
                    }
                };
                match command {
                    HostCommand::CreateWindow { request, reply } => {
                        let attributes = WindowAttributes::default()
                            .with_title(request.title)
                            .with_visible(request.visible)
                            .with_inner_size(winit::dpi::PhysicalSize::new(
                                request.width,
                                request.height,
                            ));
                        let result = event_loop
                            .create_window(attributes)
                            .map_err(|_| host_error("window.create", "window creation failed"))
                            .and_then(|window| {
                                let window = Arc::new(window);
                                window.set_ime_allowed(true);
                                let id = window.id();
                                let handle = self.windows.insert(window)?;
                                self.window_ids.insert(id, handle);
                                Ok(handle)
                            });
                        let _ = reply.send(result);
                    }
                    HostCommand::CreateSurface { request, reply } => {
                        let result = self
                            .windows
                            .get(request.window)
                            .cloned()
                            .and_then(|window| {
                                SurfaceResource::new(window, request.width, request.height)
                            })
                            .and_then(|surface| self.surfaces.insert(surface));
                        let _ = reply.send(result);
                    }
                    HostCommand::PresentRgba {
                        surface,
                        frame,
                        reply,
                    } => {
                        let result = self
                            .surfaces
                            .get_mut(surface)
                            .and_then(|surface| surface.present(frame));
                        let _ = reply.send(result);
                    }
                    HostCommand::CaptureSurface { surface, reply } => {
                        let result = self
                            .surfaces
                            .get_mut(surface)
                            .and_then(SurfaceResource::capture);
                        let _ = reply.send(result);
                    }
                    HostCommand::DestroySurface { surface, reply } => {
                        let result = self.surfaces.remove(surface).map(|_| ());
                        let _ = reply.send(result);
                    }
                    HostCommand::DestroyWindow { window, reply } => {
                        let result = self.windows.remove(window).map(|window| {
                            self.window_ids.remove(&window.id());
                        });
                        let _ = reply.send(result);
                    }
                    HostCommand::OpenAudioOutput { request, reply } => {
                        let result = AudioResource::new(request)
                            .and_then(|resource| self.audio_outputs.insert(resource));
                        let _ = reply.send(result);
                    }
                    HostCommand::SubmitAudio {
                        output,
                        packet,
                        reply,
                    } => {
                        let result = self
                            .audio_outputs
                            .get_mut(output)
                            .and_then(|resource| resource.submit(packet));
                        let _ = reply.send(result);
                    }
                    HostCommand::DrainAudio { output, reply } => {
                        let result = self
                            .audio_outputs
                            .get_mut(output)
                            .and_then(AudioResource::drain);
                        let _ = reply.send(result);
                    }
                    HostCommand::CloseAudio { output, reply } => {
                        let result = self.audio_outputs.remove(output).map(|_| ());
                        let _ = reply.send(result);
                    }
                    HostCommand::OpenDecode { kind, reply } => {
                        let result = DecodeResource::new(kind)
                            .and_then(|resource| self.decode_sessions.insert(resource));
                        let _ = reply.send(result);
                    }
                    HostCommand::Decode {
                        session,
                        request,
                        reply,
                    } => {
                        let result = self
                            .decode_sessions
                            .get_mut(session)
                            .and_then(|resource| resource.decode(request));
                        let _ = reply.send(result);
                    }
                    HostCommand::CloseDecode { session, reply } => {
                        let result = self.decode_sessions.remove(session).map(|_| ());
                        let _ = reply.send(result);
                    }
                    HostCommand::BeginSave { slot, reply } => {
                        let result = self
                            .save_store
                            .begin(&slot)
                            .and_then(|transaction| self.save_transactions.insert(transaction));
                        let _ = reply.send(result);
                    }
                    HostCommand::WriteSave {
                        transaction,
                        bytes,
                        reply,
                    } => {
                        let result = self
                            .save_transactions
                            .get_mut(transaction)
                            .and_then(|transaction| transaction.write(&bytes));
                        let _ = reply.send(result);
                    }
                    HostCommand::CommitSave { transaction, reply } => {
                        let result = self
                            .save_transactions
                            .remove(transaction)
                            .and_then(SaveTransaction::commit);
                        let _ = reply.send(result);
                    }
                    HostCommand::AbortSave { transaction, reply } => {
                        let result = self
                            .save_transactions
                            .remove(transaction)
                            .and_then(SaveTransaction::abort);
                        let _ = reply.send(result);
                    }
                    HostCommand::ReadSave { slot, reply } => {
                        let _ = reply.send(self.save_store.read(&slot));
                    }
                    HostCommand::OpenPackage { source, reply } => {
                        let result = match source {
                            PackageSourceRequest::Bundled {
                                relative_path,
                                expected_hash,
                            } => FilePackageSource::open(
                                self.bundle_root.join(relative_path),
                                &expected_hash,
                            )
                            .and_then(|source| self.package_sources.insert(source)),
                            PackageSourceRequest::UserAuthorized { .. }
                            | PackageSourceRequest::HttpsRange { .. } => Err(host_error(
                                "package.open",
                                "requested package source requires an unavailable permission provider",
                            )),
                        };
                        let _ = reply.send(result);
                    }
                    HostCommand::ReadPackageRange {
                        source,
                        offset,
                        length,
                        reply,
                    } => {
                        let result = self
                            .package_sources
                            .get_mut(source)
                            .and_then(|source| source.read_range(offset, length));
                        let _ = reply.send(result);
                    }
                    HostCommand::ClosePackage { source, reply } => {
                        let result = self.package_sources.remove(source).map(|_| ());
                        let _ = reply.send(result);
                    }
                    HostCommand::Shutdown { reply } => {
                        let result = self
                            .surfaces
                            .ensure_empty()
                            .and_then(|_| self.windows.ensure_empty())
                            .and_then(|_| self.audio_outputs.ensure_empty())
                            .and_then(|_| self.decode_sessions.ensure_empty())
                            .and_then(|_| self.save_transactions.ensure_empty())
                            .and_then(|_| self.package_sources.ensure_empty());
                        let should_exit = result.is_ok();
                        let _ = reply.send(result);
                        if should_exit {
                            event_loop.exit();
                        }
                    }
                }
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

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            window_id: WindowId,
            event: WindowEvent,
        ) {
            let Some(window) = self.window_ids.get(&window_id).copied() else {
                return;
            };
            let kind =
                match event {
                    WindowEvent::Focused(focused) => {
                        Some(PlatformEventKind::WindowFocused { window, focused })
                    }
                    WindowEvent::CloseRequested => Some(PlatformEventKind::WindowClosed { window }),
                    WindowEvent::Resized(size) => self.windows.get(window).ok().map(|native| {
                        PlatformEventKind::WindowResized {
                            window,
                            width: size.width,
                            height: size.height,
                            scale_factor: native.scale_factor(),
                        }
                    }),
                    WindowEvent::KeyboardInput { event, .. } => Some(PlatformEventKind::Keyboard {
                        window,
                        physical_key: format!("{:?}", event.physical_key),
                        logical_key: event.logical_key.to_text().map(str::to_string),
                        state: input_state(event.state),
                        repeat: event.repeat,
                    }),
                    WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                        Some(PlatformEventKind::ImePreedit {
                            window,
                            text,
                            cursor,
                        })
                    }
                    WindowEvent::Ime(Ime::Commit(text)) => {
                        Some(PlatformEventKind::ImeCommit { window, text })
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        Some(PlatformEventKind::PointerMoved {
                            window,
                            x: position.x,
                            y: position.y,
                        })
                    }
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
            self.process_commands(event_loop);
            self.poll_gamepad();
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(4),
            ));
        }
    }

    impl WindowsHostApp {
        fn poll_gamepad(&mut self) {
            while let Some(event) = self.gamepads.next_event() {
                let Some(event) = raw_gamepad_event(event) else {
                    continue;
                };
                match self.gamepad_mapper.apply_checked(event) {
                    Ok(events) => {
                        for event in events {
                            self.emit(event);
                        }
                    }
                    Err(error) => tracing::warn!(
                        event = "platform.windows.gamepad.invalid_event",
                        diagnostic_code = ?error.code,
                        operation = %error.operation,
                        "Windows Gaming Input event was rejected"
                    ),
                }
            }
        }
    }

    fn raw_gamepad_event(event: gilrs::Event) -> Option<astra_platform_general::RawGamepadEvent> {
        use astra_platform::GamepadControl;
        use astra_platform_general::RawGamepadEvent;
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

    fn input_state(state: ElementState) -> InputState {
        match state {
            ElementState::Pressed => InputState::Pressed,
            ElementState::Released => InputState::Released,
        }
    }

    fn pointer_button(button: MouseButton) -> PointerButton {
        match button {
            MouseButton::Left => PointerButton::Primary,
            MouseButton::Right => PointerButton::Secondary,
            MouseButton::Middle => PointerButton::Middle,
            MouseButton::Back => PointerButton::Back,
            MouseButton::Forward => PointerButton::Forward,
            MouseButton::Other(value) => PointerButton::Other(value),
        }
    }

    fn touch_phase(phase: WinitTouchPhase) -> TouchPhase {
        match phase {
            WinitTouchPhase::Started => TouchPhase::Started,
            WinitTouchPhase::Moved => TouchPhase::Moved,
            WinitTouchPhase::Ended => TouchPhase::Ended,
            WinitTouchPhase::Cancelled => TouchPhase::Cancelled,
        }
    }

    struct SurfaceResource {
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

    impl SurfaceResource {
        fn new(window: Arc<Window>, width: u32, height: u32) -> Result<Self, PlatformError> {
            let instance = wgpu::Instance::default();
            let surface = instance
                .create_surface(window)
                .map_err(|_| host_error("surface.create", "wgpu surface creation failed"))?;
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                }))
                .map_err(|_| host_error("surface.create", "hardware adapter is unavailable"))?;
            if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
                return Err(host_error(
                    "surface.create",
                    "software adapters are forbidden by the Windows release profile",
                ));
            }
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                    .map_err(|_| host_error("surface.create", "wgpu device creation failed"))?;
            let mut config = surface
                .get_default_config(&adapter, width, height)
                .ok_or_else(|| {
                    host_error("surface.create", "surface configuration is unavailable")
                })?;
            let capabilities = surface.get_capabilities(&adapter);
            if !capabilities
                .present_modes
                .contains(&wgpu::PresentMode::Fifo)
            {
                return Err(host_error(
                    "surface.create",
                    "required FIFO present mode is unavailable",
                ));
            }
            config.present_mode = wgpu::PresentMode::Fifo;
            surface.configure(&device, &config);
            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("astra-platform-frame-layout"),
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
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("astra-platform-frame-sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("astra-platform-frame-shader"),
                source: wgpu::ShaderSource::Wgsl(FRAME_SHADER.into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("astra-platform-frame-pipeline-layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("astra-platform-frame-pipeline"),
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
                        format: config.format,
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
                label: Some("astra-platform-frame-upload"),
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
                label: Some("astra-platform-frame-bind-group"),
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
                        "wgpu surface was lost",
                    ));
                }
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(host_error(
                        "surface.present_rgba",
                        "surface frame acquisition failed",
                    ));
                }
            };
            let output_view = output
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("astra-platform-frame-encoder"),
                });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("astra-platform-frame-pass"),
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

        fn capture(&mut self) -> Result<CapturedFrame, PlatformError> {
            let upload = self.last_upload.as_ref().ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "surface.capture",
                    "surface has not presented a frame",
                )
            })?;
            let unpadded_bytes_per_row = upload.width * 4;
            let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(alignment) * alignment;
            let buffer_size = u64::from(padded_bytes_per_row) * u64::from(upload.height);
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("astra-platform-frame-readback"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("astra-platform-frame-readback-encoder"),
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
                        bytes_per_row: Some(padded_bytes_per_row),
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
            let (mapped_tx, mapped_rx) = std_mpsc::sync_channel(1);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = mapped_tx.send(result);
                });
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| host_error("surface.capture", "GPU readback poll failed"))?;
            mapped_rx
                .recv()
                .map_err(|_| host_error("surface.capture", "GPU readback callback was lost"))?
                .map_err(|_| host_error("surface.capture", "GPU readback mapping failed"))?;
            let mapped = buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|_| host_error("surface.capture", "GPU readback range is unavailable"))?;
            let row_bytes = usize::try_from(unpadded_bytes_per_row)
                .map_err(|_| host_error("surface.capture", "frame row is too large"))?;
            let padded_row_bytes = usize::try_from(padded_bytes_per_row)
                .map_err(|_| host_error("surface.capture", "padded frame row is too large"))?;
            let mut rgba8 = Vec::with_capacity(row_bytes * upload.height as usize);
            for row in mapped
                .chunks_exact(padded_row_bytes)
                .take(upload.height as usize)
            {
                rgba8.extend_from_slice(&row[..row_bytes]);
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

    struct UploadFrame {
        texture: wgpu::Texture,
        width: u32,
        height: u32,
    }

    struct AudioResource {
        _stream: cpal::Stream,
        producer: astra_platform_general::NativeAudioProducer,
        meter: Arc<CallbackMeter>,
        stream_error: Arc<AtomicBool>,
        channels: u16,
        next_sequence: u64,
        submitted_samples: u64,
    }

    impl AudioResource {
        fn new(request: AudioOutputRequest) -> Result<Self, PlatformError> {
            let host = cpal::default_host();
            let device = host.default_output_device().ok_or_else(|| {
                host_error("audio.open", "WASAPI default output device is unavailable")
            })?;
            let supported = device.default_output_config().map_err(|_| {
                host_error("audio.open", "WASAPI default output config is unavailable")
            })?;
            if supported.sample_rate() != request.sample_rate
                || supported.channels() != request.channels
            {
                return Err(host_error(
                    "audio.open",
                    "WASAPI default format does not match the requested explicit format",
                ));
            }
            let config: cpal::StreamConfig = supported.clone().into();
            let capacity = request.max_buffered_frames * usize::from(request.channels);
            let (producer, consumer, _queue_telemetry) =
                astra_platform_general::NativeAudioQueue::new(capacity)?;
            let meter = Arc::new(CallbackMeter::default());
            let stream_error = Arc::new(AtomicBool::new(false));
            let stream = match supported.sample_format() {
                cpal::SampleFormat::F32 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [f32], _| fill_f32(output, &mut consumer, &meter),
                        move |stream_error_value| set_stream_error(&error, stream_error_value),
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [i16], _| fill_i16(output, &mut consumer, &meter),
                        move |stream_error_value| set_stream_error(&error, stream_error_value),
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let meter = Arc::clone(&meter);
                    let error = Arc::clone(&stream_error);
                    let mut consumer = consumer;
                    device.build_output_stream(
                        &config,
                        move |output: &mut [u16], _| fill_u16(output, &mut consumer, &meter),
                        move |stream_error_value| set_stream_error(&error, stream_error_value),
                        None,
                    )
                }
                _ => {
                    return Err(host_error(
                        "audio.open",
                        "WASAPI sample format is unsupported",
                    ));
                }
            }
            .map_err(|_| host_error("audio.open", "WASAPI output stream creation failed"))?;
            stream
                .play()
                .map_err(|_| host_error("audio.open", "WASAPI output stream could not start"))?;
            Ok(Self {
                _stream: stream,
                producer,
                meter,
                stream_error,
                channels: request.channels,
                next_sequence: 1,
                submitted_samples: 0,
            })
        }

        fn submit(&mut self, packet: AudioPacket) -> Result<(), PlatformError> {
            if packet.sequence != self.next_sequence || packet.channels != self.channels {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "audio.submit",
                    "audio packet sequence or channel count is invalid",
                ));
            }
            self.producer.push_samples(&packet.samples)?;
            self.next_sequence += 1;
            self.submitted_samples = self
                .submitted_samples
                .checked_add(packet.samples.len() as u64)
                .ok_or_else(|| host_error("audio.submit", "audio sample counter overflowed"))?;
            Ok(())
        }

        fn drain(&mut self) -> Result<AudioMeter, PlatformError> {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if self.stream_error.load(Ordering::Acquire) {
                    return Err(PlatformError::new(
                        PlatformErrorCode::DeviceLost,
                        "audio.drain",
                        "WASAPI output stream reported a device error",
                    ));
                }
                if self.meter.sample_count.load(Ordering::Acquire) >= self.submitted_samples {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(host_error("audio.drain", "WASAPI output drain timed out"));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Ok(self.meter.snapshot())
        }
    }

    #[derive(Default)]
    struct CallbackMeter {
        sample_count: AtomicU64,
        peak_bits: AtomicU32,
        sum_squares_bits: AtomicU64,
    }

    impl CallbackMeter {
        fn record(&self, sample: f32) {
            let magnitude = sample.abs();
            let magnitude_bits = magnitude.to_bits();
            let mut peak_bits = self.peak_bits.load(Ordering::Relaxed);
            while magnitude_bits > peak_bits {
                match self.peak_bits.compare_exchange_weak(
                    peak_bits,
                    magnitude_bits,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => peak_bits = actual,
                }
            }
            let contribution = f64::from(sample) * f64::from(sample);
            let mut sum_bits = self.sum_squares_bits.load(Ordering::Relaxed);
            loop {
                let next = f64::from_bits(sum_bits) + contribution;
                match self.sum_squares_bits.compare_exchange_weak(
                    sum_bits,
                    next.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => sum_bits = actual,
                }
            }
            self.sample_count.fetch_add(1, Ordering::Release);
        }

        fn snapshot(&self) -> AudioMeter {
            let sample_count = self.sample_count.load(Ordering::Acquire);
            let rms = if sample_count == 0 {
                0.0
            } else {
                (f64::from_bits(self.sum_squares_bits.load(Ordering::Acquire))
                    / sample_count as f64)
                    .sqrt() as f32
            };
            AudioMeter {
                sample_count,
                peak_dbfs: amplitude_dbfs(f32::from_bits(self.peak_bits.load(Ordering::Acquire))),
                rms_dbfs: amplitude_dbfs(rms),
            }
        }
    }

    fn amplitude_dbfs(value: f32) -> f32 {
        if value <= 0.0 {
            -120.0
        } else {
            20.0 * value.log10()
        }
    }

    fn pop_sample(
        consumer: &mut astra_platform_general::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) -> f32 {
        let sample = consumer.pop_sample().unwrap_or(0.0);
        meter.record(sample);
        sample
    }

    fn fill_f32(
        output: &mut [f32],
        consumer: &mut astra_platform_general::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) {
        for target in output {
            *target = pop_sample(consumer, meter);
        }
    }

    fn fill_i16(
        output: &mut [i16],
        consumer: &mut astra_platform_general::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) {
        for target in output {
            *target = (pop_sample(consumer, meter).clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        }
    }

    fn fill_u16(
        output: &mut [u16],
        consumer: &mut astra_platform_general::NativeAudioConsumer,
        meter: &CallbackMeter,
    ) {
        for target in output {
            let sample = pop_sample(consumer, meter).clamp(-1.0, 1.0);
            *target = ((sample * 0.5 + 0.5) * f32::from(u16::MAX)) as u16;
        }
    }

    fn set_stream_error(error: &AtomicBool, _value: cpal::StreamError) {
        error.store(true, Ordering::Release);
    }

    struct DecodeResource {
        kind: DecodeKind,
        provider: astra_media::WindowsMediaFoundationDecodeProvider,
        next_sequence: u64,
    }

    impl DecodeResource {
        fn new(kind: DecodeKind) -> Result<Self, PlatformError> {
            let provider = astra_media::WindowsMediaFoundationDecodeProvider::probe()
                .map_err(|_| host_error("decode.open", "WMF provider initialization failed"))?;
            Ok(Self {
                kind,
                provider,
                next_sequence: 1,
            })
        }

        fn decode(
            &mut self,
            request: PlatformDecodeRequest,
        ) -> Result<DecodeOutput, PlatformError> {
            if request.sequence != self.next_sequence || request.kind != self.kind {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "decode.submit",
                    "decode request sequence or media kind is invalid",
                ));
            }
            let kind = match request.kind {
                DecodeKind::Audio => astra_media::DecodeKind::Audio,
                DecodeKind::Video => astra_media::DecodeKind::Video,
            };
            let result = self
                .provider
                .decode(&astra_media::DecodeRequest {
                    kind,
                    codec: request.codec,
                    bytes: request.bytes,
                    profile: "desktop-release".to_string(),
                })
                .map_err(|_| host_error("decode.submit", "WMF decode failed"))?;
            self.next_sequence += 1;
            match result.output {
                MediaDecodeOutput::CpuBuffer {
                    bytes,
                    format,
                    hash,
                } => Ok(DecodeOutput::CpuBuffer {
                    format,
                    bytes,
                    hash: hash.to_string(),
                }),
                MediaDecodeOutput::MediaSurfaceToken(_) => Err(host_error(
                    "decode.submit",
                    "WMF returned an unsupported external media surface",
                )),
            }
        }
    }

    fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
        PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
    }

    fn default_roots() -> Option<super::HostRoots> {
        Some(super::HostRoots {
            save_base: saved_games_root().ok()?,
            bundle_root: std::env::current_exe().ok()?.parent()?.to_path_buf(),
        })
    }

    fn saved_games_root() -> Result<std::path::PathBuf, PlatformError> {
        use std::ffi::c_void;
        use windows::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{FOLDERID_SavedGames, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
        };
        unsafe {
            let path = SHGetKnownFolderPath(&FOLDERID_SavedGames, KF_FLAG_DEFAULT, None)
                .map_err(|_| host_error("save.store.open", "Saved Games folder is unavailable"))?;
            let root = path
                .to_string()
                .map_err(|_| host_error("save.store.open", "Saved Games path is invalid"))?;
            CoTaskMemFree(Some(path.as_ptr() as *const c_void));
            Ok(std::path::PathBuf::from(root))
        }
    }

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
    let uv = position.xy / dimensions;
    return textureSample(frame_texture, frame_sampler, uv);
}
"#;
}
