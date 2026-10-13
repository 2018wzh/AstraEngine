use super::*;

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

pub(super) async fn run_backend(
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
                    for event in astra_platform_common::wgpu_recovery_events("webgpu", recovered) {
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
                    for event in astra_platform_common::wgpu_recovery_events("webgpu", recovered) {
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
                let result = WebAudioOutput::open(request, backend.audio_wake())
                    .await
                    .and_then(|(output, lane, format)| {
                        audio.insert(output).map(|handle| OpenedAudioOutput {
                            handle,
                            format,
                            lane: Box::new(lane),
                            capture: None,
                        })
                    });
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
            HostCommand::ListSaves { reply } => {
                let _ = reply.send(list_saves(&profile.package_id).await);
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
            #[cfg(not(feature = "platform-test-driver"))]
            command @ (HostCommand::InjectSurfaceDeviceLoss { .. }
            | HostCommand::InjectAudioDeviceLoss { .. }) => {
                let operation = command.operation();
                let _ = command.reply_error(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    operation,
                    "device fault injection requires platform-test-driver",
                ));
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

pub(super) struct WebInstanceLock {
    pub(super) release: Function,
}

impl WebInstanceLock {
    pub(super) async fn acquire(profile: &PlatformHostProfile) -> Result<Self, PlatformError> {
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

pub(super) type AnimationFrameCallback = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

pub(super) struct BrowserLifecycle {
    pub(super) target: EventTarget,
    pub(super) visibility: Closure<dyn FnMut(Event)>,
    pub(super) gamepad_frame: i32,
    pub(super) _gamepad_callback: AnimationFrameCallback,
}
