use std::{
    collections::BTreeMap,
    sync::{mpsc as std_mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

use android_activity::AndroidApp;
use astra_core::Hash256;
use astra_platform::{
    host_channel, AudioOutputHandle, CapturedFrame, DecodeSessionHandle, HostCommand,
    HostLaunchProfile, InputState, MemoryPressureLevel, PackageCachePolicy, PackageSourceHandle,
    PackageSourcePolicy, PackageSourceRequest, PlatformBackendChannels, PlatformError,
    PlatformErrorCode, PlatformEvent, PlatformEventKind, PlatformHostSession, PointerButton,
    SaveTransactionHandle, SurfaceHandle, TouchPhase, WindowHandle,
};
use astra_platform_common::{
    AtomicSaveStore, CachedPackageSource, ResourceTable, SaveTransaction, VerifiedPackageCache,
};
use winit::{
    application::ApplicationHandler,
    event::{
        ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase as WinitTouchPhase,
        WindowEvent,
    },
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::android::EventLoopBuilderExtAndroid,
    window::{Window, WindowAttributes, WindowId},
};

use crate::accessibility::AndroidAccessibilityBridge;
use crate::audio::AndroidAudioResource;
use crate::decode::AndroidDecodeWorker;

type SurfaceCore = astra_platform_common::WgpuPresentationCore;

pub(super) fn run<F>(
    app: AndroidApp,
    launch_profile: HostLaunchProfile,
    player: F,
) -> Result<(), PlatformError>
where
    F: FnOnce(PlatformHostSession) -> Result<(), PlatformError> + Send + 'static,
{
    let profile = launch_profile.require_platform()?.clone();
    if profile.platform != astra_platform::PlatformId::Android {
        return Err(host_error(
            "host.start",
            "Android activity requires an Android profile",
        ));
    }
    astra_platform::validate_host_profile(&profile)?;
    let data_root = app.internal_data_path().ok_or_else(|| {
        host_error(
            "host.start",
            "Android app-private storage directory is unavailable",
        )
    })?;
    let save_store = AtomicSaveStore::new(data_root.join("saves"), &profile.package_id)?;
    let imports_root = data_root.join("files").join("package-imports");
    let package_cache_root = data_root.join("cache").join("packages");
    let decode_root = data_root.join("cache").join("mediacodec");
    crate::decode::prepare_scratch_root(&decode_root)?;
    let max_decode_output_bytes = profile
        .limits
        .max_frame_bytes
        .checked_mul(256)
        .ok_or_else(|| host_error("host.start", "Android decode output budget overflows"))?;
    let bundled_package = read_bundled_package(&app)?;
    let (client, backend, events) = host_channel(
        launch_profile.clone(),
        profile.limits.command_queue_capacity,
        profile.limits.event_queue_capacity,
    )?;
    let session = PlatformHostSession {
        client,
        events,
        profile: launch_profile,
    };
    let mut event_loop = EventLoop::builder();
    event_loop.with_android_app(app.clone());
    let event_loop = event_loop
        .build()
        .map_err(|_| host_error("host.start", "Android event loop creation failed"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let (player_result_tx, player_result_rx) = std_mpsc::sync_channel(1);
    let (package_completion_tx, package_completion_rx) = std_mpsc::channel();
    let mut host = AndroidHostApp {
        backend,
        player: Some(Box::new(move || {
            let result = player(session);
            let _ = player_result_tx.send(result);
        })),
        player_result_rx,
        player_result: None,
        app,
        resumed: false,
        windows: ResourceTable::new("window"),
        window_ids: BTreeMap::new(),
        surfaces: ResourceTable::new("surface"),
        audio_outputs: ResourceTable::new("audio_output"),
        decoders: ResourceTable::new("decode_session"),
        decode_root,
        max_decode_output_bytes,
        accessibility: None,
        accessibility_action_capacity: profile.limits.event_queue_capacity,
        save_store,
        save_transactions: ResourceTable::new("save_transaction"),
        package_sources: ResourceTable::new("package_source"),
        package_source_policies: profile.package_sources.clone(),
        package_cache_policy: profile.package_cache.clone(),
        package_cache_root,
        package_completion_tx,
        package_completion_rx,
        pending_package_opens: 0,
        bundled_package: Arc::new(bundled_package),
        imports_root,
        max_package_entry_bytes: profile.package_cache.max_entry_bytes,
        event_sequence: 0,
    };
    event_loop
        .run_app(&mut host)
        .map_err(|_| host_error("host.run", "Android event loop failed"))?;
    if host.player_result.is_none() {
        host.player_result = host
            .player_result_rx
            .recv_timeout(Duration::from_secs(5))
            .ok();
    }
    host.player_result.take().unwrap_or_else(|| {
        Err(host_error(
            "player.session",
            "Android Activity was destroyed before Player shutdown completed",
        ))
    })
}

type PlayerStart = Box<dyn FnOnce() + Send>;

struct AndroidHostApp {
    backend: PlatformBackendChannels,
    player: Option<PlayerStart>,
    player_result_rx: std_mpsc::Receiver<Result<(), PlatformError>>,
    player_result: Option<Result<(), PlatformError>>,
    app: AndroidApp,
    resumed: bool,
    windows: ResourceTable<Arc<Window>, WindowHandle>,
    window_ids: BTreeMap<WindowId, WindowHandle>,
    surfaces: ResourceTable<SurfaceSlot, SurfaceHandle>,
    audio_outputs: ResourceTable<AndroidAudioResource, AudioOutputHandle>,
    decoders: ResourceTable<AndroidDecodeWorker, DecodeSessionHandle>,
    decode_root: std::path::PathBuf,
    max_decode_output_bytes: usize,
    accessibility: Option<AndroidAccessibilityBridge>,
    accessibility_action_capacity: usize,
    save_store: AtomicSaveStore,
    save_transactions: ResourceTable<SaveTransaction, SaveTransactionHandle>,
    package_sources: ResourceTable<PackageSourceResource, PackageSourceHandle>,
    package_source_policies: Vec<PackageSourcePolicy>,
    package_cache_policy: PackageCachePolicy,
    package_cache_root: std::path::PathBuf,
    package_completion_tx: std_mpsc::Sender<PackageCompletion>,
    package_completion_rx: std_mpsc::Receiver<PackageCompletion>,
    pending_package_opens: usize,
    bundled_package: Arc<Vec<u8>>,
    imports_root: std::path::PathBuf,
    max_package_entry_bytes: u64,
    event_sequence: u64,
}

struct SurfaceSlot {
    window: WindowHandle,
    width: u32,
    height: u32,
    core: Option<SurfaceCore>,
}

struct MemoryPackageSource {
    bytes: Arc<Vec<u8>>,
}

enum PackageSourceResource {
    Memory(MemoryPackageSource),
    Cached(CachedPackageSource),
}

impl PackageSourceResource {
    fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        match self {
            Self::Memory(source) => source.read_range(offset, length),
            Self::Cached(source) => source.read_range(offset, length),
        }
    }
}

struct PackageCompletion {
    reply: tokio::sync::oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    result: Result<CachedPackageSource, PlatformError>,
}

impl MemoryPackageSource {
    fn read_range(&self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        let offset = usize::try_from(offset)
            .map_err(|_| host_error("package.read_range", "package offset is too large"))?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| host_error("package.read_range", "package range overflows"))?;
        let range = self.bytes.get(offset..end).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.read_range",
                "package range is outside the verified source",
            )
        })?;
        Ok(range.to_vec())
    }
}

impl AndroidHostApp {
    fn next_sequence(&mut self) -> u64 {
        self.event_sequence = self.event_sequence.saturating_add(1);
        self.event_sequence
    }

    fn emit(&mut self, kind: PlatformEventKind) {
        let event = PlatformEvent::new(self.next_sequence(), kind);
        if let Err(error) = self.backend.emit_event(event) {
            tracing::error!(
                event = "platform.android.event.emit_failed",
                diagnostic_code = ?error.code,
                operation = %error.operation,
                "Android platform event could not be emitted"
            );
        }
    }

    fn create_surface_core(&self, slot: &SurfaceSlot) -> Result<SurfaceCore, PlatformError> {
        let window = self.windows.get(slot.window)?.clone();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|_| host_error("surface.create", "Android Vulkan surface creation failed"))?;
        pollster::block_on(SurfaceCore::new(
            instance,
            surface,
            slot.width,
            slot.height,
            true,
        ))
    }

    fn restore_surfaces(&mut self) -> Result<(), PlatformError> {
        let handles = self.surfaces.handles().collect::<Vec<_>>();
        for handle in handles {
            let needs_restore = self.surfaces.get(handle)?.core.is_none();
            if needs_restore {
                let core = {
                    let slot = self.surfaces.get(handle)?;
                    self.create_surface_core(slot)?
                };
                self.surfaces.get_mut(handle)?.core = Some(core);
            }
        }
        Ok(())
    }

    fn suspend_surfaces(&mut self) {
        let handles = self.surfaces.handles().collect::<Vec<_>>();
        for handle in handles {
            if let Ok(slot) = self.surfaces.get_mut(handle) {
                slot.core = None;
            }
        }
    }

    fn surface_core_mut(
        &mut self,
        handle: SurfaceHandle,
    ) -> Result<&mut SurfaceCore, PlatformError> {
        self.surfaces.get_mut(handle)?.core.as_mut().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::ContextLost,
                "surface.access",
                "Android surface is suspended",
            )
        })
    }

    fn recover_surface_error(
        &mut self,
        surface: SurfaceHandle,
        result: &Result<(), PlatformError>,
    ) {
        if result
            .as_ref()
            .is_err_and(|error| error.code == PlatformErrorCode::ContextLost)
        {
            let recovered = self
                .surface_core_mut(surface)
                .and_then(SurfaceCore::reconfigure_after_loss)
                .is_ok();
            for event in astra_platform_common::wgpu_recovery_events("wgpu_vulkan", recovered) {
                self.emit(event);
            }
        } else if result
            .as_ref()
            .is_err_and(|error| error.code == PlatformErrorCode::DeviceLost)
        {
            let recovered = self
                .surface_core_mut(surface)
                .and_then(|core| pollster::block_on(core.recover_device()).map(|_| ()))
                .is_ok();
            for event in
                astra_platform_common::wgpu_device_recovery_events("wgpu_vulkan", recovered)
            {
                self.emit(event);
            }
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
            self.process_command(event_loop, command);
        }
    }

    fn process_command(&mut self, event_loop: &ActiveEventLoop, command: HostCommand) {
        match command {
            HostCommand::CreateWindow { request, reply } => {
                let result = if !self.resumed {
                    Err(host_error(
                        "window.create",
                        "Android Activity must be resumed before window creation",
                    ))
                } else if !self.window_ids.is_empty() {
                    Err(PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "window.create",
                        "Android Activity owns exactly one main window",
                    ))
                } else {
                    event_loop
                        .create_window(
                            WindowAttributes::default()
                                .with_title(request.title)
                                .with_visible(request.visible)
                                .with_inner_size(winit::dpi::PhysicalSize::new(
                                    request.width,
                                    request.height,
                                )),
                        )
                        .map_err(|_| host_error("window.create", "Android window creation failed"))
                        .and_then(|window| {
                            let window = Arc::new(window);
                            window.set_ime_allowed(true);
                            let id = window.id();
                            let handle = self.windows.insert(window)?;
                            self.window_ids.insert(id, handle);
                            match AndroidAccessibilityBridge::new(
                                &self.app,
                                handle,
                                self.accessibility_action_capacity,
                            ) {
                                Ok(bridge) => {
                                    self.accessibility = Some(bridge);
                                    Ok(handle)
                                }
                                Err(error) => {
                                    self.window_ids.remove(&id);
                                    let _ = self.windows.remove(handle);
                                    Err(error)
                                }
                            }
                        })
                };
                let _ = reply.send(result);
            }
            HostCommand::CreateSurface { request, reply } => {
                let result = (|| {
                    if request.width == 0 || request.height == 0 {
                        return Err(host_error(
                            "surface.create",
                            "Android surface dimensions must be non-zero",
                        ));
                    }
                    let mut slot = SurfaceSlot {
                        window: request.window,
                        width: request.width,
                        height: request.height,
                        core: None,
                    };
                    slot.core = Some(self.create_surface_core(&slot)?);
                    self.surfaces.insert(slot)
                })();
                let _ = reply.send(result);
            }
            HostCommand::PresentRgba {
                surface,
                frame,
                reply,
            } => {
                let result = self
                    .surface_core_mut(surface)
                    .and_then(|core| core.present(frame));
                self.recover_surface_error(surface, &result);
                let _ = reply.send(result);
            }
            HostCommand::PresentScene {
                surface,
                frame,
                reply,
            } => {
                let semantics = frame.semantics.clone();
                let result = self
                    .surface_core_mut(surface)
                    .and_then(|core| core.present_scene(frame))
                    .and_then(|()| {
                        let Some(semantics) = semantics.as_ref() else {
                            return Ok(());
                        };
                        self.accessibility
                            .as_mut()
                            .ok_or_else(|| {
                                host_error(
                                    "accessibility.android.update",
                                    "Android accessibility bridge is unavailable",
                                )
                            })?
                            .update(semantics)
                    });
                self.recover_surface_error(surface, &result);
                let _ = reply.send(result);
            }
            HostCommand::CaptureSurface { surface, reply } => {
                let _ = reply.send(self.surface_core_mut(surface).and_then(capture_surface));
            }
            #[cfg(feature = "platform-test-driver")]
            HostCommand::InjectSurfaceDeviceLoss { surface, reply } => {
                let result = self.surface_core_mut(surface).map(|core| {
                    core.inject_device_loss_for_test();
                });
                let _ = reply.send(result);
            }
            HostCommand::DestroySurface { surface, reply } => {
                let _ = reply.send(self.surfaces.remove(surface).map(|_| ()));
            }
            HostCommand::DestroyWindow { window, reply } => {
                let result = self.windows.remove(window).map(|window| {
                    self.window_ids.remove(&window.id());
                    self.accessibility = None;
                });
                let _ = reply.send(result);
            }
            HostCommand::OpenAudioOutput { request, reply } => {
                let result = AndroidAudioResource::new(request)
                    .and_then(|resource| self.audio_outputs.insert(resource));
                let _ = reply.send(result);
            }
            HostCommand::QueryAudioOutputFormat { reply }
            | HostCommand::QueryAudioDeviceFormat { reply } => {
                let _ = reply.send(crate::audio::preferred_output_format());
            }
            HostCommand::SubmitAudio {
                output,
                packet,
                reply,
            } => {
                let result = self
                    .audio_outputs
                    .get_mut(output)
                    .and_then(|audio| audio.submit(packet));
                let _ = reply.send(result);
            }
            HostCommand::QueryAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .get(output)
                    .and_then(|audio| audio.state());
                let _ = reply.send(result);
            }
            HostCommand::QueryAudioOutput { output, reply } => {
                let result = self
                    .audio_outputs
                    .get(output)
                    .and_then(|audio| audio.status());
                let _ = reply.send(result);
            }
            HostCommand::DrainAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .get(output)
                    .and_then(|audio| audio.drain());
                let _ = reply.send(result);
            }
            HostCommand::PauseAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .get_mut(output)
                    .and_then(AndroidAudioResource::pause);
                let _ = reply.send(result);
            }
            HostCommand::ResumeAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .get_mut(output)
                    .and_then(AndroidAudioResource::resume);
                let _ = reply.send(result);
            }
            HostCommand::AbortAudio { output, reply } => {
                let result = self
                    .audio_outputs
                    .remove(output)
                    .and_then(|mut audio| audio.stop());
                let _ = reply.send(result);
            }
            #[cfg(feature = "platform-test-driver")]
            HostCommand::InjectAudioDeviceLoss { output: _, reply } => {
                let _ = reply.send(Err(PlatformError::new(
                    PlatformErrorCode::ProviderUnavailable,
                    "audio.test.inject_device_loss",
                    "Android test device loss requires live route disconnection",
                )));
            }
            HostCommand::CloseAudio { output, reply } => {
                let result = self.audio_outputs.remove(output).and_then(|mut audio| {
                    audio.drain()?;
                    audio.stop()
                });
                let _ = reply.send(result);
            }
            HostCommand::OpenDecode { kind, reply } => {
                let result = AndroidDecodeWorker::new(
                    kind,
                    self.decode_root.clone(),
                    self.max_decode_output_bytes,
                )
                .and_then(|worker| self.decoders.insert(worker));
                let _ = reply.send(result);
            }
            HostCommand::Decode {
                session,
                request,
                reply,
            } => match self.decoders.get(session) {
                Ok(worker) => worker.submit(request, reply),
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            HostCommand::CloseDecode { session, reply } => match self.decoders.remove(session) {
                Ok(worker) => worker.close(reply),
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
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
            HostCommand::DeleteSave { slot, reply } => {
                let _ = reply.send(self.save_store.delete(&slot));
            }
            HostCommand::OpenPackage { source, reply } => {
                let result = match source {
                    PackageSourceRequest::Bundled {
                        relative_path,
                        expected_hash,
                    } if relative_path == "game.astrapkg" => {
                        let actual =
                            Hash256::from_sha256(self.bundled_package.as_slice()).to_string();
                        if actual != expected_hash {
                            Err(PlatformError::new(
                                PlatformErrorCode::InvalidState,
                                "package.open",
                                "bundled Android package hash does not match",
                            ))
                        } else {
                            self.package_sources.insert(PackageSourceResource::Memory(
                                MemoryPackageSource {
                                    bytes: Arc::clone(&self.bundled_package),
                                },
                            ))
                        }
                    }
                    PackageSourceRequest::Bundled { .. } => Err(host_error(
                        "package.open",
                        "Android bundled package path must be game.astrapkg",
                    )),
                    PackageSourceRequest::UserAuthorized { expected_hash } => {
                        self.open_saf_import(&expected_hash)
                    }
                    PackageSourceRequest::HttpsRange { url, expected_hash } => {
                        self.start_https_package_open(url, expected_hash, reply);
                        return;
                    }
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
                let _ = reply.send(self.package_sources.remove(source).map(|_| ()));
            }
            HostCommand::Shutdown { reply } => {
                let result = self
                    .surfaces
                    .ensure_empty()
                    .and_then(|_| self.windows.ensure_empty())
                    .and_then(|_| self.audio_outputs.ensure_empty())
                    .and_then(|_| self.decoders.ensure_empty())
                    .and_then(|_| {
                        if self.accessibility.is_none() {
                            Ok(())
                        } else {
                            Err(host_error(
                                "host.shutdown",
                                "Android accessibility bridge is still live",
                            ))
                        }
                    })
                    .and_then(|_| self.save_transactions.ensure_empty())
                    .and_then(|_| self.package_sources.ensure_empty())
                    .and_then(|_| {
                        if self.pending_package_opens == 0 {
                            Ok(())
                        } else {
                            Err(host_error(
                                "host.shutdown",
                                "Android HTTPS package opens are still pending",
                            ))
                        }
                    });
                let exit = result.is_ok();
                let _ = reply.send(result);
                if exit {
                    event_loop.exit();
                }
            }
        }
    }

    fn open_saf_import(
        &mut self,
        expected_hash: &str,
    ) -> Result<PackageSourceHandle, PlatformError> {
        let import = super::take_saf_import().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::Cancelled,
                "package.open_user_authorized",
                "no completed SAF import is available",
            )
        })?;
        if !import.permission_persisted {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "package.open_user_authorized",
                "SAF read permission was not persisted",
            ));
        }
        if import.sha256 != expected_hash || import.size > self.max_package_entry_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open_user_authorized",
                "SAF package identity or size violates the active profile",
            ));
        }
        if import.token.is_empty()
            || !import.token.ends_with(".astrapkg")
            || !import
                .token
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_' | b'.'))
        {
            return Err(host_error(
                "package.open_user_authorized",
                "SAF package token is invalid",
            ));
        }
        let path = self.imports_root.join(&import.token);
        let metadata = std::fs::metadata(&path).map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::Io,
                "package.open_user_authorized",
                "SAF imported package is unavailable",
            )
        })?;
        if metadata.len() != import.size || !metadata.is_file() {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open_user_authorized",
                "SAF imported package size changed",
            ));
        }
        let bytes = std::fs::read(path).map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::Io,
                "package.open_user_authorized",
                "SAF imported package could not be read",
            )
        })?;
        let actual = Hash256::from_sha256(&bytes).to_string();
        if actual != expected_hash {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open_user_authorized",
                "SAF imported package hash changed",
            ));
        }
        self.package_sources
            .insert(PackageSourceResource::Memory(MemoryPackageSource {
                bytes: Arc::new(bytes),
            }))
    }

    fn start_https_package_open(
        &mut self,
        url: String,
        expected_hash: String,
        reply: tokio::sync::oneshot::Sender<Result<PackageSourceHandle, PlatformError>>,
    ) {
        let policies = self.package_source_policies.clone();
        let policy = self.package_cache_policy.clone();
        let cache_root = self.package_cache_root.clone();
        let completion_tx = self.package_completion_tx.clone();
        self.pending_package_opens += 1;
        let spawn = thread::Builder::new()
            .name("astra-android-package".to_string())
            .spawn(move || {
                let result = (|| {
                    let mut cache = VerifiedPackageCache::open(cache_root, policy)?;
                    let client = astra_platform_common::HttpRangeClient::from_policies(&policies)?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| {
                            host_error(
                                "package.https.open",
                                "Android HTTPS runtime could not start",
                            )
                        })?;
                    runtime.block_on(client.fetch_into_cache(&url, &expected_hash, &mut cache))?;
                    cache.open_source(&expected_hash)
                })();
                let _ = completion_tx.send(PackageCompletion { reply, result });
            });
        if spawn.is_err() {
            self.pending_package_opens = self.pending_package_opens.saturating_sub(1);
        }
    }

    fn process_package_completions(&mut self) {
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
        self.emit(PlatformEventKind::Resumed);
        if let Some(player) = self.player.take() {
            if thread::Builder::new()
                .name("astra-player-android".to_string())
                .spawn(player)
                .is_err()
            {
                self.player_result = Some(Err(host_error(
                    "player.start",
                    "Android Player thread could not be started",
                )));
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if !self.resumed {
            return;
        }
        self.resumed = false;
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
                physical_key: format!("{:?}", event.physical_key),
                logical_key: event.logical_key.to_text().map(str::to_string),
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
        self.process_commands(event_loop);
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
        let bridge_events = match super::drain_bridge_events(window) {
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
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(4),
        ));
    }
}

fn read_bundled_package(app: &AndroidApp) -> Result<Vec<u8>, PlatformError> {
    use std::{ffi::CString, io::Read};
    let name = CString::new("game.astrapkg").map_err(|_| {
        host_error(
            "package.asset.open",
            "bundled package asset name is invalid",
        )
    })?;
    let mut asset = app.asset_manager().open(&name).ok_or_else(|| {
        host_error(
            "package.asset.open",
            "bundled game.astrapkg asset is missing",
        )
    })?;
    let length = asset.length();
    if length == 0 {
        return Err(host_error(
            "package.asset.open",
            "bundled package asset is empty",
        ));
    }
    let mut bytes = Vec::with_capacity(length);
    asset.read_to_end(&mut bytes).map_err(|_| {
        host_error(
            "package.asset.read",
            "bundled package asset could not be read",
        )
    })?;
    if bytes.len() != length {
        return Err(host_error(
            "package.asset.read",
            "bundled package asset length changed during read",
        ));
    }
    Ok(bytes)
}

fn capture_surface(surface: &mut SurfaceCore) -> Result<CapturedFrame, PlatformError> {
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

fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}
