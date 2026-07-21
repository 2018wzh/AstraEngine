use astra_core::Hash256;
use astra_headless_protocol::{ButtonState, GamepadControl, PhysicalInput, TouchPhase};
use astra_package::{
    AstraContainerReader, ContainerCryptoProvider, PackageReader, SourceUnlockPolicy,
};
use astra_platform::{
    SurfaceHandle, SurfaceRequest, WindowHandle, WindowRequest, HEADLESS_PRESENTATION_RATE_HZ,
};
use astra_player_core::{
    PlatformCommandSink, PlayerHostCommandExecutor, PlayerHostCommandResult, PlayerHostResourceId,
};
use astra_product_host::{
    CanonicalAudioSnapshot, Observation, ProductAdapterFactory, ProductFuture, ProductHostError,
    ProductOpenRequest, ProductPackageSource, ProductPerformanceObserver, ProductPerformanceSample,
    ProductSession,
};
use astra_ui_core::{
    UiButtonState, UiInputEventKind, UiNavigationAction, UiPoint, UiPointerButton, UiTouchPhase,
};
use astra_vn_core::VnRunConfig;
use std::{sync::Arc, time::Instant};

use astra_player_vn::{NativeVnHostCommandSource, NativeVnProductMediaHost, VnUiHostRequest};

#[derive(Default)]
pub struct NativeVnProductAdapterFactory {
    package_crypto: Option<Arc<dyn ContainerCryptoProvider>>,
}

impl NativeVnProductAdapterFactory {
    pub fn with_package_crypto(package_crypto: Arc<dyn ContainerCryptoProvider>) -> Self {
        Self {
            package_crypto: Some(package_crypto),
        }
    }
}

impl ProductAdapterFactory for NativeVnProductAdapterFactory {
    fn binding_id(&self) -> &str {
        "astra.native_vn"
    }

    fn open<'a>(
        &'a self,
        request: ProductOpenRequest,
    ) -> ProductFuture<'a, Result<Box<dyn ProductSession>, ProductHostError>> {
        let package_crypto = self.package_crypto.clone();
        Box::pin(async move {
            let performance_observer = request.performance_observer.clone();
            if request.presentation_rate_hz < HEADLESS_PRESENTATION_RATE_HZ
                || !request
                    .presentation_rate_hz
                    .is_multiple_of(HEADLESS_PRESENTATION_RATE_HZ)
            {
                return Err(ProductHostError::Binding(
                    "presentation rate must be an integer multiple of the authoritative Runtime tick rate"
                        .into(),
                ));
            }
            let presentation_substeps =
                request.presentation_rate_hz / HEADLESS_PRESENTATION_RATE_HZ;
            tracing::info!(
                event = "headless.product.native_vn.open",
                target = %request.target,
                profile = %request.profile,
                "opening NativeVN through the generic Headless product host"
            );
            let raw = match request.package {
                ProductPackageSource::InMemory(bytes) => AstraContainerReader::new(&bytes),
                ProductPackageSource::VerifiedContainer(container) => Ok(container),
                ProductPackageSource::StorageVerified {
                    source,
                    storage_hash,
                } => AstraContainerReader::open_storage_verified_source(source, storage_hash),
            }
            .map_err(|error| binding("package.inspect", error))?;
            let source_locked = raw.has_section("source.unlock");
            let package = match (source_locked, package_crypto) {
                (false, None) => PackageReader::open_verified_container(raw),
                (false, Some(_)) => {
                    return Err(ProductHostError::Binding(
                        "package.unlock: plaintext package received unexpected crypto provider"
                            .into(),
                    ));
                }
                (true, None) => {
                    return Err(ProductHostError::Binding(
                        "package.unlock: source-locked package requires verified source input"
                            .into(),
                    ));
                }
                (true, Some(crypto)) => {
                    let policy: SourceUnlockPolicy = raw
                        .decode_postcard("source.unlock")
                        .map_err(|error| binding("package.unlock_policy", error))?;
                    PackageReader::open_source_locked_container(
                        raw,
                        &policy,
                        "source.unlock",
                        crypto,
                    )
                }
            }
            .map_err(|error| binding("package.open", error))?;
            record_open_phase(&performance_observer, "product.package_opened")?;
            let locale = match request.locale {
                Some(locale) => locale,
                None => {
                    astra_vn_package::load_player_locale_config(&package)
                        .map_err(|error| binding("locale.load", error))?
                        .default_locale
                }
            };
            record_open_phase(&performance_observer, "product.locale_loaded")?;
            let window = request
                .platform
                .create_window(WindowRequest {
                    title: "Astra Headless Product".into(),
                    width: request.width,
                    height: request.height,
                    visible: false,
                })
                .await
                .map_err(|error| binding("window.create", error))?;
            record_open_phase(&performance_observer, "product.window_created")?;
            let surface = request
                .platform
                .create_surface(SurfaceRequest {
                    window,
                    width: request.width,
                    height: request.height,
                })
                .await
                .map_err(|error| binding("surface.create", error))?;
            record_open_phase(&performance_observer, "product.surface_created")?;
            let logical_surface = PlayerHostResourceId(1);
            let asset_cache_bytes = request.max_decoded_cache_bytes.saturating_mul(2) / 3;
            let audio_cache_bytes = request
                .max_decoded_cache_bytes
                .saturating_sub(asset_cache_bytes);
            if asset_cache_bytes == 0 || audio_cache_bytes == 0 {
                return Err(ProductHostError::Binding(
                    "decoded cache budget is too small to partition by resource domain".into(),
                ));
            }
            let mut sink = PlatformCommandSink::new(request.platform.clone());
            sink.bind_surface(logical_surface, surface)
                .map_err(|error| binding("surface.bind", error))?;
            let mut executor = PlayerHostCommandExecutor::new(sink);
            let mut source = NativeVnHostCommandSource::from_package_with_asset_cache(
                &package,
                VnRunConfig {
                    profile: request.profile,
                    locale,
                },
                request.width,
                request.height,
                logical_surface,
                asset_cache_bytes,
            )
            .map_err(|error| binding("runtime.open", error))?;
            source.set_ui_host_performance_sampling_enabled(performance_observer.is_some());
            record_open_phase(&performance_observer, "product.runtime_opened")?;
            hydrate_save_catalog(&mut source, &mut executor).await?;
            record_open_phase(&performance_observer, "product.save_catalog_hydrated")?;
            executor
                .execute_batch(
                    source
                        .launch()
                        .map_err(|error| binding("runtime.launch", error))?,
                )
                .await
                .map_err(|error| binding("host.launch", error))?;
            record_open_phase(&performance_observer, "product.runtime_launched")?;
            let mut media = NativeVnProductMediaHost::with_video_limits(
                256,
                request.max_video_frames,
                request.max_decode_output_bytes,
                audio_cache_bytes,
                request.retain_audio_timeline,
            )
            .map_err(|error| binding("media.create", error))?;
            media.set_performance_profiling(performance_observer.is_some());
            record_open_phase(&performance_observer, "product.media_created")?;
            media
                .initialize(&mut source, &mut executor)
                .await
                .map_err(|error| binding("media.initialize", error))?;
            record_open_phase(&performance_observer, "product.media_initialized")?;
            media
                .process(&mut source, &mut executor, 0, Vec::new())
                .await
                .map_err(|error| binding("media.launch", error))?;
            record_open_phase(&performance_observer, "product.media_launched")?;
            Ok(Box::new(NativeVnHeadlessSession {
                platform: request.platform,
                window,
                surface,
                source: Some(source),
                executor,
                media,
                pointer: (0.0, 0.0),
                viewport: (request.width, request.height),
                next_save_transaction: 1_000,
                observations: Vec::new(),
                resumed: false,
                focused: false,
                last_tick: 0,
                presentation_substeps,
                performance_observer: performance_observer.clone(),
                performance: performance_observer
                    .is_some()
                    .then(ProductPerformanceSample::default),
            }) as Box<dyn ProductSession>)
        })
    }
}

fn record_open_phase(
    observer: &Option<Arc<dyn ProductPerformanceObserver>>,
    name: &str,
) -> Result<(), ProductHostError> {
    observer
        .as_ref()
        .map(|observer| observer.record_phase(name))
        .transpose()
        .map_err(ProductHostError::Output)?;
    Ok(())
}

async fn hydrate_save_catalog(
    source: &mut NativeVnHostCommandSource,
    executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
) -> Result<(), ProductHostError> {
    let results = executor
        .execute_batch(
            source
                .list_saves()
                .map_err(|error| binding("save.list.prepare", error))?,
        )
        .await
        .map_err(|error| binding("save.list", error))?;
    let slots = match results.as_slice() {
        [PlayerHostCommandResult::SaveList { slots }] => slots.clone(),
        _ => {
            return Err(ProductHostError::Binding(
                "ASTRA_HEADLESS_SAVE_LIST_RESULT_INVALID".into(),
            ));
        }
    };
    for slot in &slots {
        let results = executor
            .execute_batch(
                source
                    .read_save(slot)
                    .map_err(|error| binding("save.catalog.read.prepare", error))?,
            )
            .await
            .map_err(|error| binding("save.catalog.read", error))?;
        let bytes = match results.as_slice() {
            [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
            _ => {
                return Err(ProductHostError::Binding(
                    "ASTRA_HEADLESS_SAVE_CATALOG_RESULT_INVALID".into(),
                ));
            }
        };
        source
            .ingest_save_catalog_entry(slot, bytes)
            .map_err(|error| binding("save.catalog.ingest", error))?;
    }
    tracing::info!(
        event = "headless.product.native_vn.save_catalog_hydrated",
        slot_count = slots.len(),
        "hydrated validated save metadata before launching NativeVN"
    );
    Ok(())
}

struct NativeVnHeadlessSession {
    platform: astra_platform::PlatformHostClient,
    window: WindowHandle,
    surface: SurfaceHandle,
    source: Option<NativeVnHostCommandSource>,
    executor: PlayerHostCommandExecutor<PlatformCommandSink>,
    media: NativeVnProductMediaHost,
    pointer: (f64, f64),
    viewport: (u32, u32),
    next_save_transaction: u64,
    observations: Vec<Observation>,
    resumed: bool,
    focused: bool,
    last_tick: u64,
    presentation_substeps: u32,
    performance_observer: Option<Arc<dyn ProductPerformanceObserver>>,
    performance: Option<ProductPerformanceSample>,
}

impl ProductSession for NativeVnHeadlessSession {
    fn consume<'a>(
        &'a mut self,
        tick: u64,
        input: &'a PhysicalInput,
    ) -> ProductFuture<'a, Result<Vec<Observation>, ProductHostError>> {
        Box::pin(async move {
            let now_ms = canonical_ms(tick)?;
            if tick < self.last_tick {
                return Err(ProductHostError::Input(
                    "input tick moved behind the product media timeline".into(),
                ));
            }
            if tick > self.last_tick.saturating_add(1) {
                self.advance_through(tick - 1).await?;
            }
            if requires_active_input(input) && (!self.resumed || !self.focused) {
                return Err(ProductHostError::Input(
                    "physical input requires a resumed and focused product session".into(),
                ));
            }
            match input {
                PhysicalInput::Resume => self.resumed = true,
                PhysicalInput::Focus { focused } => {
                    self.focused = *focused;
                    self.dispatch_ui(UiInputEventKind::Focus { focused: *focused })
                        .await?;
                }
                PhysicalInput::Keyboard {
                    physical_key,
                    state: ButtonState::Pressed,
                    ..
                } if physical_key == "F5" => self.save("slot.quick").await?,
                PhysicalInput::Keyboard {
                    physical_key,
                    state: ButtonState::Pressed,
                    ..
                } if physical_key == "F9" => self.load("slot.quick").await?,
                PhysicalInput::Keyboard {
                    physical_key,
                    logical_key,
                    state,
                    repeat,
                } => {
                    self.dispatch_ui(UiInputEventKind::Keyboard {
                        logical_key: logical_key.clone().unwrap_or_else(|| physical_key.clone()),
                        physical_key: physical_key.clone(),
                        state: ui_button_state(*state),
                        repeat: *repeat,
                        modifiers: 0,
                    })
                    .await?
                }
                PhysicalInput::PointerMove { x, y } => {
                    self.pointer = (
                        normalized(*x, self.viewport.0),
                        normalized(*y, self.viewport.1),
                    );
                    self.dispatch_ui(UiInputEventKind::PointerMove {
                        position: ui_point(self.pointer),
                    })
                    .await?;
                }
                PhysicalInput::PointerButton { button, state } => {
                    self.dispatch_ui(UiInputEventKind::PointerButton {
                        position: ui_point(self.pointer),
                        button: ui_pointer_button(*button),
                        state: ui_button_state(*state),
                    })
                    .await?
                }
                PhysicalInput::Wheel { delta_x, delta_y } => {
                    self.dispatch_ui(UiInputEventKind::Wheel {
                        delta_points: UiPoint {
                            x: *delta_x as f32,
                            y: *delta_y as f32,
                        },
                    })
                    .await?
                }
                PhysicalInput::Touch {
                    id, x, y, phase, ..
                } => {
                    self.pointer = (
                        normalized(*x, self.viewport.0),
                        normalized(*y, self.viewport.1),
                    );
                    self.dispatch_ui(UiInputEventKind::Touch {
                        device_id: 0,
                        contact_id: *id,
                        position: ui_point(self.pointer),
                        phase: ui_touch_phase(*phase),
                    })
                    .await?;
                }
                PhysicalInput::GamepadInput { control, value, .. } if *value > 0 => {
                    if let Some(action) = ui_navigation_action(*control) {
                        self.dispatch_ui(UiInputEventKind::Navigation { action })
                            .await?;
                    }
                }
                PhysicalInput::ImePreedit {
                    text,
                    cursor_start,
                    cursor_end,
                } => {
                    self.dispatch_ui(UiInputEventKind::ImePreedit {
                        text: text.clone(),
                        cursor_start: *cursor_start,
                        cursor_end: *cursor_end,
                    })
                    .await?
                }
                PhysicalInput::ImeCommit { text } => {
                    self.dispatch_ui(UiInputEventKind::ImeCommit { text: text.clone() })
                        .await?
                }
                PhysicalInput::AdvanceTicks { .. } => {}
                PhysicalInput::Shutdown => {
                    return Err(ProductHostError::Input(
                        "shutdown must be handled by the protocol session".into(),
                    ));
                }
                _ => {}
            }
            let target_tick = match input {
                PhysicalInput::AdvanceTicks { count } => tick
                    .checked_add(u64::from(*count) - 1)
                    .ok_or_else(|| ProductHostError::Input("tick sequence overflowed".into()))?,
                _ => tick,
            };
            if target_tick > self.last_tick {
                self.advance_through(target_tick).await?;
            } else {
                self.process_without_audio_tick(now_ms).await?;
            }
            self.refresh_observations()?;
            Ok(self.observations.clone())
        })
    }

    fn observations(&self) -> Vec<Observation> {
        self.observations.clone()
    }

    fn capture_frame<'a>(
        &'a self,
    ) -> ProductFuture<'a, Result<astra_platform::CapturedFrame, ProductHostError>> {
        Box::pin(async move {
            self.platform
                .capture_surface(self.surface)
                .await
                .map_err(|error| ProductHostError::Output(error.to_string()))
        })
    }

    fn capture_audio(&self) -> Result<CanonicalAudioSnapshot, ProductHostError> {
        Ok(CanonicalAudioSnapshot {
            sample_rate: astra_media::CANONICAL_SAMPLE_RATE,
            channels: astra_media::CANONICAL_CHANNELS,
            samples: self.media.submitted_audio_timeline().to_vec(),
        })
    }

    fn decoded_cache_bytes(&self) -> u64 {
        self.media.decoded_cache_bytes().saturating_add(
            self.source
                .as_ref()
                .map_or(0, NativeVnHostCommandSource::decoded_asset_cache_bytes),
        )
    }

    fn take_performance_sample(&mut self) -> ProductPerformanceSample {
        self.performance
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    fn shutdown<'a>(&'a mut self) -> ProductFuture<'a, Result<(), ProductHostError>> {
        Box::pin(async move {
            let source = self
                .source
                .as_mut()
                .ok_or_else(|| ProductHostError::Shutdown("session already shut down".into()))?;
            self.media
                .shutdown(source, &mut self.executor)
                .await
                .map_err(|error| ProductHostError::Shutdown(error.to_string()))?;
            let release = source
                .release_resources()
                .map_err(|error| ProductHostError::Shutdown(error.to_string()))?;
            self.executor
                .execute_batch(release)
                .await
                .map_err(|error| ProductHostError::Shutdown(error.to_string()))?;
            self.source
                .take()
                .expect("checked above")
                .shutdown()
                .map_err(|error| ProductHostError::Shutdown(error.to_string()))?;
            self.platform
                .destroy_surface(self.surface)
                .await
                .map_err(|error| ProductHostError::Shutdown(error.to_string()))?;
            self.platform
                .destroy_window(self.window)
                .await
                .map_err(|error| ProductHostError::Shutdown(error.to_string()))?;
            tracing::info!(
                event = "headless.product.native_vn.shutdown",
                "shut down NativeVN Headless product session without retained resources"
            );
            Ok(())
        })
    }
}

impl NativeVnHeadlessSession {
    fn profile_started(&self) -> Option<Instant> {
        self.performance.as_ref().map(|_| Instant::now())
    }

    fn add_profile_duration(
        &mut self,
        started: Option<Instant>,
        update: impl FnOnce(&mut ProductPerformanceSample, u64),
    ) -> Result<(), ProductHostError> {
        let Some(duration_ns) = self.profile_duration(started)? else {
            return Ok(());
        };
        let Some(sample) = self.performance.as_mut() else {
            return Err(ProductHostError::Output(
                "performance timer exists without profiling state".into(),
            ));
        };
        update(sample, duration_ns);
        Ok(())
    }

    fn flush_performance_sample(&mut self) -> Result<(), ProductHostError> {
        let Some(observer) = self.performance_observer.as_ref() else {
            return Ok(());
        };
        let sample = self
            .performance
            .as_mut()
            .map(std::mem::take)
            .ok_or_else(|| {
                ProductHostError::Output(
                    "performance observer exists without profiling state".into(),
                )
            })?;
        observer
            .record_sample(sample)
            .map_err(ProductHostError::Output)
    }

    fn profile_duration(&self, started: Option<Instant>) -> Result<Option<u64>, ProductHostError> {
        let Some(started) = started else {
            return Ok(None);
        };
        started
            .elapsed()
            .as_nanos()
            .try_into()
            .map(Some)
            .map_err(|_| ProductHostError::Output("performance duration overflowed".into()))
    }

    fn source(&mut self) -> Result<&mut NativeVnHostCommandSource, ProductHostError> {
        self.source
            .as_mut()
            .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))
    }

    async fn dispatch_ui(&mut self, event: UiInputEventKind) -> Result<(), ProductHostError> {
        let activates = matches!(
            &event,
            UiInputEventKind::Keyboard {
                logical_key,
                state: UiButtonState::Pressed,
                ..
            } if matches!(logical_key.as_str(), "Enter" | " " | "Space")
        ) || matches!(
            &event,
            UiInputEventKind::Navigation {
                action: UiNavigationAction::Activate
            }
        );
        if activates && self.media.has_active_video() {
            let source = self
                .source
                .as_mut()
                .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))?;
            if self.media.skip_active_videos(source) {
                return Ok(());
            }
        }
        if self.source()?.should_capture_gameplay_surface(&event) {
            self.capture_gameplay_surface().await?;
        }
        let profile_started = self.profile_started();
        let batch = self
            .source()?
            .dispatch_ui_event(event)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let ui_sample = self.source()?.take_last_ui_performance_sample();
        let ui_host_sample = self.source()?.take_last_ui_host_performance_sample();
        self.add_profile_duration(profile_started, |sample, duration| {
            sample.ui_layout_paint_ns = sample.ui_layout_paint_ns.saturating_add(duration);
            if let Some(ui_sample) = ui_sample {
                sample.ui_update_layout_ns = sample
                    .ui_update_layout_ns
                    .saturating_add(ui_sample.update_layout_ns);
                sample.ui_paint_conversion_ns = sample
                    .ui_paint_conversion_ns
                    .saturating_add(ui_sample.paint_conversion_ns);
                sample.ui_host_scene_ns = sample.ui_host_scene_ns.saturating_add(
                    duration.saturating_sub(
                        ui_sample
                            .update_layout_ns
                            .saturating_add(ui_sample.paint_conversion_ns),
                    ),
                );
            }
            if let Some(ui_host_sample) = ui_host_sample {
                sample.ui_model_binding_ns = sample
                    .ui_model_binding_ns
                    .saturating_add(ui_host_sample.model_binding_ns);
                sample.ui_controller_ns = sample
                    .ui_controller_ns
                    .saturating_add(ui_host_sample.controller_ns);
                sample.ui_frame_model_ns = sample
                    .ui_frame_model_ns
                    .saturating_add(ui_host_sample.frame_model_ns);
                sample.ui_text_scene_ns = sample
                    .ui_text_scene_ns
                    .saturating_add(ui_host_sample.text_scene_ns);
            }
        })?;
        self.flush_performance_sample()?;
        self.executor
            .execute_batch(batch)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.process_ui_host_request().await?;
        Ok(())
    }

    async fn process_ui_host_request(&mut self) -> Result<(), ProductHostError> {
        let Some(request) = self.source()?.take_ui_host_request() else {
            return Ok(());
        };
        match request {
            VnUiHostRequest::Save { slot_id, .. } => {
                if let Err(error) = self.save(&slot_id).await {
                    self.source()?
                        .mark_save_failed(&slot_id)
                        .map_err(|cleanup_error| {
                            ProductHostError::Input(cleanup_error.to_string())
                        })?;
                    return Err(error);
                }
                if let Some(batch) = self
                    .source()?
                    .mark_save_committed(&slot_id)
                    .map_err(|error| ProductHostError::Input(error.to_string()))?
                {
                    self.executor
                        .execute_batch(batch)
                        .await
                        .map_err(|error| ProductHostError::Input(error.to_string()))?;
                }
            }
            VnUiHostRequest::Load { slot_id } => self.load(&slot_id).await?,
            VnUiHostRequest::Delete { slot_id } => {
                let batch = self
                    .source()?
                    .delete_save(&slot_id)
                    .map_err(|error| ProductHostError::Input(error.to_string()))?;
                self.executor
                    .execute_batch(batch)
                    .await
                    .map_err(|error| ProductHostError::Input(error.to_string()))?;
                self.source()?
                    .mark_save_deleted(&slot_id)
                    .map_err(|error| ProductHostError::Input(error.to_string()))?;
            }
        }
        Ok(())
    }

    async fn advance_through(&mut self, target_tick: u64) -> Result<(), ProductHostError> {
        while self.last_tick < target_tick {
            let tick_started = self.profile_started();
            self.last_tick = self
                .last_tick
                .checked_add(1)
                .ok_or_else(|| ProductHostError::Input("tick sequence overflowed".into()))?;
            let now_ms = canonical_ms(self.last_tick)?;
            self.add_profile_duration(tick_started, |sample, duration| {
                sample.runtime_tick_ns = sample.runtime_tick_ns.saturating_add(duration);
            })?;
            for substep in 0..self.presentation_substeps {
                let delta_ns = presentation_substep_duration_ns(
                    astra_headless_protocol::TICK_DURATION_NS,
                    self.presentation_substeps,
                    substep,
                );
                let vn_started = self.profile_started();
                let present = self
                    .source
                    .as_mut()
                    .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))?
                    .tick_presentation(delta_ns)
                    .map_err(|error| ProductHostError::Output(error.to_string()))?;
                let vn_duration = self.profile_duration(vn_started)?;
                if let Some(duration) = vn_duration {
                    let sample = self.performance.as_mut().ok_or_else(|| {
                        ProductHostError::Output(
                            "performance timer exists without profiling state".into(),
                        )
                    })?;
                    sample.vn_step_ns = sample.vn_step_ns.saturating_add(duration);
                    sample.runtime_tick_ns = sample.runtime_tick_ns.saturating_add(duration);
                }
                if let Some(present) = present {
                    self.flush_performance_sample()?;
                    self.executor
                        .execute_batch(present)
                        .await
                        .map_err(|error| ProductHostError::Output(error.to_string()))?;
                }
            }
            let media_started = self.profile_started();
            let source = self
                .source
                .as_mut()
                .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))?;
            self.media
                .poll_and_process_with_audio_tick(source, &mut self.executor, now_ms, true)
                .await
                .map_err(|error| ProductHostError::Output(error.to_string()))?;
            let media_sample = self.media.take_performance_sample();
            self.add_profile_duration(media_started, |sample, duration| {
                sample.media_decode_ns = sample.media_decode_ns.saturating_add(duration);
                sample.media_provider_decode_ns = sample
                    .media_provider_decode_ns
                    .saturating_add(media_sample.provider_decode_ns);
                sample.media_parse_convert_ns = sample
                    .media_parse_convert_ns
                    .saturating_add(media_sample.parse_convert_ns);
                sample.media_mixer_ns = sample.media_mixer_ns.saturating_add(media_sample.mixer_ns);
            })?;
            self.flush_performance_sample()?;
        }
        Ok(())
    }

    async fn process_without_audio_tick(&mut self, now_ms: u64) -> Result<(), ProductHostError> {
        let media_started = self.profile_started();
        let source = self
            .source
            .as_mut()
            .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))?;
        self.media
            .poll_and_process_with_audio_tick(source, &mut self.executor, now_ms, false)
            .await
            .map_err(|error| ProductHostError::Output(error.to_string()))?;
        let media_sample = self.media.take_performance_sample();
        self.add_profile_duration(media_started, |sample, duration| {
            sample.media_decode_ns = sample.media_decode_ns.saturating_add(duration);
            sample.media_provider_decode_ns = sample
                .media_provider_decode_ns
                .saturating_add(media_sample.provider_decode_ns);
            sample.media_parse_convert_ns = sample
                .media_parse_convert_ns
                .saturating_add(media_sample.parse_convert_ns);
            sample.media_mixer_ns = sample.media_mixer_ns.saturating_add(media_sample.mixer_ns);
        })?;
        self.flush_performance_sample()
    }

    async fn save(&mut self, slot: &str) -> Result<(), ProductHostError> {
        let profile_started = self.profile_started();
        self.next_save_transaction = self
            .next_save_transaction
            .checked_add(1)
            .ok_or_else(|| ProductHostError::Input("save transaction overflowed".into()))?;
        let transaction = PlayerHostResourceId(self.next_save_transaction);
        let media_snapshot = serde_json::to_vec(&self.media.snapshot())
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        if !self.source()?.has_gameplay_thumbnail_capture() {
            self.capture_gameplay_surface().await?;
        }
        let last_tick = self.last_tick;
        let playtime_ms = canonical_ms(last_tick)?;
        let total_seconds = playtime_ms / 1_000;
        self.source()?
            .prepare_save_metadata(
                slot,
                format!(
                    "T+{:02}:{:02}:{:02}",
                    total_seconds / 3_600,
                    (total_seconds / 60) % 60,
                    total_seconds % 60
                ),
                playtime_ms,
            )
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let plan = self
            .source()?
            .prepare_save_transaction_with_product_media_snapshot(
                slot,
                transaction,
                Some(media_snapshot),
            )
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.executor
            .execute_save_transaction(plan)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.add_profile_duration(profile_started, |sample, duration| {
            sample.save_load_ns = sample.save_load_ns.saturating_add(duration);
        })?;
        self.flush_performance_sample()
    }

    async fn capture_gameplay_surface(&mut self) -> Result<(), ProductHostError> {
        let batch = self
            .source()?
            .prepare_surface_capture()
            .map_err(|error| ProductHostError::Output(error.to_string()))?;
        let results = self
            .executor
            .execute_batch(batch)
            .await
            .map_err(|error| ProductHostError::Output(error.to_string()))?;
        let (width, height, rgba8) = match results.as_slice() {
            [PlayerHostCommandResult::Captured {
                width,
                height,
                rgba8,
                ..
            }] => (*width, *height, rgba8.clone()),
            _ => {
                return Err(ProductHostError::Output(
                    "captured gameplay surface result is invalid".into(),
                ));
            }
        };
        self.source()?
            .cache_gameplay_surface(width, height, rgba8)
            .map_err(|error| ProductHostError::Output(error.to_string()))
    }

    async fn load(&mut self, slot: &str) -> Result<(), ProductHostError> {
        let profile_started = self.profile_started();
        let read = self
            .source()?
            .read_save(slot)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let result = self
            .executor
            .execute_batch(read)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let bytes = match result.as_slice() {
            [PlayerHostCommandResult::SaveRead { bytes }] => bytes.clone(),
            _ => {
                return Err(ProductHostError::Output(
                    "save read result is invalid".into(),
                ))
            }
        };
        let present = self
            .source()?
            .restore(&bytes)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.executor
            .execute_batch(present)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let media_snapshot = self
            .source()?
            .take_restored_product_media_snapshot()
            .ok_or_else(|| {
                ProductHostError::Input(
                    "ASTRA_PLAYER_SAVE_MEDIA_SNAPSHOT_MISSING: save has no product media state"
                        .into(),
                )
            })?;
        let media_snapshot = serde_json::from_slice(&media_snapshot)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.media
            .restore(media_snapshot)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.add_profile_duration(profile_started, |sample, duration| {
            sample.save_load_ns = sample.save_load_ns.saturating_add(duration);
        })?;
        self.flush_performance_sample()
    }

    fn refresh_observations(&mut self) -> Result<(), ProductHostError> {
        let source = self.source.as_ref().ok_or_else(|| {
            ProductHostError::Output("runtime source is unavailable for observation".into())
        })?;
        let evidence = source.last_step_evidence().ok_or_else(|| {
            ProductHostError::Output("runtime step has no observation evidence".into())
        })?;
        let product = source
            .product_observation_evidence()
            .map_err(|error| binding("product.observe", error))?;
        self.observations = vec![
            Observation {
                key: "runtime.state_hash".into(),
                value_hash: evidence.runtime_state_hash.clone(),
            },
            Observation {
                key: "runtime.event_hash".into(),
                value_hash: evidence.runtime_event_hash.clone(),
            },
            Observation {
                key: "runtime.presentation_hash".into(),
                value_hash: evidence.runtime_presentation_hash.clone(),
            },
            hashed_observation("vn.current_state", &evidence.current_state_id)?,
            hashed_observation("vn.pending_wait_command", &evidence.pending_wait_command_id)?,
            hashed_observation("vn.pending_wait_await_id", &evidence.pending_wait_await_id)?,
            hashed_observation("vn.pending_choices", &evidence.pending_choice_ids)?,
            hashed_observation("vn.terminal_routes", &evidence.terminal_route_ids)?,
            hashed_observation("vn.ui_profile", &product.ui_profile)?,
            hashed_observation("vn.locale", &product.locale)?,
            hashed_observation("vn.system_page", &product.active_system_page)?,
            hashed_observation("vn.focused_semantic_id", &product.focused_semantic_id)?,
            hashed_observation("vn.auto_enabled", &product.auto_enabled)?,
            hashed_observation("vn.skip_mode", &product.skip_mode)?,
            hashed_observation("vn.reading_mode", &product.reading_mode)?,
            hashed_observation("vn.audio_enabled", &product.audio_enabled)?,
            hashed_observation("vn.skip_allowed", &product.skip_allowed)?,
            hashed_observation("vn.system_config", &product.system_config)?,
            hashed_observation("vn.backlog_count", &product.backlog_count)?,
            hashed_observation(
                "vn.occupied_save_slot_count",
                &product.occupied_save_slot_count,
            )?,
            hashed_observation("media.active_video", &self.media.has_active_video())?,
            hashed_observation("media.active_voice", &self.media.has_active_voice())?,
        ];
        Ok(())
    }
}

fn presentation_substep_duration_ns(tick_duration_ns: u64, substeps: u32, substep: u32) -> u64 {
    let base = tick_duration_ns / u64::from(substeps);
    let remainder = tick_duration_ns % u64::from(substeps);
    base + u64::from(substep + 1 == substeps) * remainder
}

fn hashed_observation(
    key: &str,
    value: &impl serde::Serialize,
) -> Result<Observation, ProductHostError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| ProductHostError::Output(error.to_string()))?;
    Ok(Observation {
        key: key.into(),
        value_hash: Hash256::from_sha256(&bytes).to_string(),
    })
}

fn normalized(value: u16, extent: u32) -> f64 {
    f64::from(value) * f64::from(extent.saturating_sub(1)) / 65_535.0
}

fn ui_point(pointer: (f64, f64)) -> UiPoint {
    UiPoint {
        x: pointer.0 as f32,
        y: pointer.1 as f32,
    }
}

fn ui_button_state(state: ButtonState) -> UiButtonState {
    match state {
        ButtonState::Pressed => UiButtonState::Pressed,
        ButtonState::Released => UiButtonState::Released,
    }
}

fn ui_pointer_button(button: astra_headless_protocol::PointerButton) -> UiPointerButton {
    match button {
        astra_headless_protocol::PointerButton::Primary => UiPointerButton::Primary,
        astra_headless_protocol::PointerButton::Secondary => UiPointerButton::Secondary,
        astra_headless_protocol::PointerButton::Middle => UiPointerButton::Middle,
        astra_headless_protocol::PointerButton::Back => UiPointerButton::Back,
        astra_headless_protocol::PointerButton::Forward => UiPointerButton::Forward,
        astra_headless_protocol::PointerButton::Other => UiPointerButton::Other(0),
    }
}

fn ui_touch_phase(phase: TouchPhase) -> UiTouchPhase {
    match phase {
        TouchPhase::Started => UiTouchPhase::Started,
        TouchPhase::Moved => UiTouchPhase::Moved,
        TouchPhase::Ended => UiTouchPhase::Ended,
        TouchPhase::Cancelled => UiTouchPhase::Cancelled,
    }
}

fn ui_navigation_action(control: GamepadControl) -> Option<UiNavigationAction> {
    match control {
        GamepadControl::South => Some(UiNavigationAction::Activate),
        GamepadControl::East => Some(UiNavigationAction::Cancel),
        GamepadControl::DpadUp => Some(UiNavigationAction::Up),
        GamepadControl::DpadDown => Some(UiNavigationAction::Down),
        GamepadControl::DpadLeft => Some(UiNavigationAction::Left),
        GamepadControl::DpadRight => Some(UiNavigationAction::Right),
        GamepadControl::LeftShoulder => Some(UiNavigationAction::PagePrevious),
        GamepadControl::RightShoulder => Some(UiNavigationAction::PageNext),
        _ => None,
    }
}

fn canonical_ms(tick: u64) -> Result<u64, ProductHostError> {
    tick.checked_mul(astra_headless_protocol::TICK_DURATION_NS)
        .and_then(|value| value.checked_div(1_000_000))
        .ok_or_else(|| ProductHostError::Input("tick time overflowed".into()))
}

fn requires_active_input(input: &PhysicalInput) -> bool {
    matches!(
        input,
        PhysicalInput::Keyboard { .. }
            | PhysicalInput::ImePreedit { .. }
            | PhysicalInput::ImeCommit { .. }
            | PhysicalInput::PointerMove { .. }
            | PhysicalInput::PointerButton { .. }
            | PhysicalInput::Wheel { .. }
            | PhysicalInput::Touch { .. }
            | PhysicalInput::GamepadInput { .. }
    )
}

fn binding(operation: &str, error: impl std::fmt::Display) -> ProductHostError {
    ProductHostError::Binding(format!("{operation}: {error}"))
}

#[cfg(test)]
mod cadence_tests {
    use super::presentation_substep_duration_ns;

    #[test]
    fn presentation_substeps_preserve_the_authoritative_tick_duration() {
        let tick = astra_headless_protocol::TICK_DURATION_NS;
        let first = presentation_substep_duration_ns(tick, 2, 0);
        let second = presentation_substep_duration_ns(tick, 2, 1);
        assert_eq!(first, 8_333_333);
        assert_eq!(second, 8_333_334);
        assert_eq!(first + second, tick);
    }
}
