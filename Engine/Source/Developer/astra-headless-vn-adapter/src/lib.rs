use astra_core::Hash256;
use astra_headless_protocol::{ButtonState, GamepadControl, PhysicalInput, TouchPhase};
use astra_package::PackageReader;
use astra_platform::{SurfaceHandle, SurfaceRequest, WindowHandle, WindowRequest};
use astra_player_core::{
    PlatformCommandSink, PlayerAction, PlayerActionMap, PlayerHostCommandExecutor,
    PlayerHostCommandResult, PlayerHostResourceId,
};
use astra_product_host::{
    CanonicalAudioSnapshot, Observation, ProductAdapterFactory, ProductFuture, ProductHostError,
    ProductOpenRequest, ProductSession,
};
use astra_vn_core::VnRunConfig;

use astra_player_vn::{NativeVnHostCommandSource, NativeVnProductMediaHost};

#[derive(Debug, Default)]
pub struct NativeVnProductAdapterFactory;

impl ProductAdapterFactory for NativeVnProductAdapterFactory {
    fn binding_id(&self) -> &str {
        "astra.native_vn"
    }

    fn open<'a>(
        &'a self,
        request: ProductOpenRequest,
    ) -> ProductFuture<'a, Result<Box<dyn ProductSession>, ProductHostError>> {
        Box::pin(async move {
            tracing::info!(
                event = "headless.product.native_vn.open",
                target = %request.target,
                profile = %request.profile,
                "opening NativeVN through the generic Headless product host"
            );
            let package = PackageReader::open(&request.package_bytes)
                .map_err(|error| binding("package.open", error))?;
            let locale = match request.locale {
                Some(locale) => locale,
                None => {
                    astra_vn_package::load_player_locale_config(&package)
                        .map_err(|error| binding("locale.load", error))?
                        .default_locale
                }
            };
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
            let surface = request
                .platform
                .create_surface(SurfaceRequest {
                    window,
                    width: request.width,
                    height: request.height,
                })
                .await
                .map_err(|error| binding("surface.create", error))?;
            let logical_surface = PlayerHostResourceId(1);
            let mut sink = PlatformCommandSink::new(request.platform.clone());
            sink.bind_surface(logical_surface, surface)
                .map_err(|error| binding("surface.bind", error))?;
            let mut executor = PlayerHostCommandExecutor::new(sink);
            let mut source = NativeVnHostCommandSource::from_package(
                &package,
                VnRunConfig {
                    profile: request.profile,
                    locale,
                },
                request.width,
                request.height,
                logical_surface,
            )
            .map_err(|error| binding("runtime.open", error))?;
            executor
                .execute_batch(
                    source
                        .launch()
                        .map_err(|error| binding("runtime.launch", error))?,
                )
                .await
                .map_err(|error| binding("host.launch", error))?;
            let mut media = NativeVnProductMediaHost::with_video_limits(
                256,
                request.max_video_frames,
                request.max_decode_output_bytes,
            )
            .map_err(|error| binding("media.create", error))?;
            media
                .initialize(&mut source, &mut executor)
                .await
                .map_err(|error| binding("media.initialize", error))?;
            media
                .process(&mut source, &mut executor, 0, Vec::new())
                .await
                .map_err(|error| binding("media.launch", error))?;
            Ok(Box::new(NativeVnHeadlessSession {
                platform: request.platform,
                window,
                surface,
                source: Some(source),
                executor,
                media,
                action_map: PlayerActionMap::standard(),
                pointer: (0.0, 0.0),
                viewport: (request.width, request.height),
                next_save_transaction: 1_000,
                observations: Vec::new(),
                resumed: false,
                focused: false,
                last_tick: 0,
            }) as Box<dyn ProductSession>)
        })
    }
}

struct NativeVnHeadlessSession {
    platform: astra_platform::PlatformHostClient,
    window: WindowHandle,
    surface: SurfaceHandle,
    source: Option<NativeVnHostCommandSource>,
    executor: PlayerHostCommandExecutor<PlatformCommandSink>,
    media: NativeVnProductMediaHost,
    action_map: PlayerActionMap,
    pointer: (f64, f64),
    viewport: (u32, u32),
    next_save_transaction: u64,
    observations: Vec<Observation>,
    resumed: bool,
    focused: bool,
    last_tick: u64,
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
                PhysicalInput::Focus { focused } => self.focused = *focused,
                PhysicalInput::Keyboard {
                    physical_key,
                    state: ButtonState::Pressed,
                    ..
                } if physical_key == "F5" => self.save("slot-quick").await?,
                PhysicalInput::Keyboard {
                    physical_key,
                    state: ButtonState::Pressed,
                    ..
                } if physical_key == "F9" => self.load("slot-quick").await?,
                PhysicalInput::Keyboard {
                    physical_key,
                    state: ButtonState::Pressed,
                    ..
                } => {
                    if let Some(action) = self.action_map.keyboard(physical_key) {
                        self.dispatch(action).await?;
                    }
                }
                PhysicalInput::PointerMove { x, y } => {
                    self.pointer = (
                        normalized(*x, self.viewport.0),
                        normalized(*y, self.viewport.1),
                    );
                }
                PhysicalInput::PointerButton {
                    button: astra_headless_protocol::PointerButton::Primary,
                    state: ButtonState::Pressed,
                } => self.dispatch_pointer().await?,
                PhysicalInput::Touch {
                    x,
                    y,
                    phase: TouchPhase::Started,
                    ..
                } => {
                    self.pointer = (
                        normalized(*x, self.viewport.0),
                        normalized(*y, self.viewport.1),
                    );
                    self.dispatch_pointer().await?;
                }
                PhysicalInput::GamepadInput {
                    control: GamepadControl::South,
                    value,
                    ..
                } if *value > 0 => self.dispatch(PlayerAction::Advance).await?,
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
    fn source(&mut self) -> Result<&mut NativeVnHostCommandSource, ProductHostError> {
        self.source
            .as_mut()
            .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))
    }

    async fn dispatch(&mut self, action: PlayerAction) -> Result<(), ProductHostError> {
        if matches!(action, PlayerAction::Advance) && self.media.has_active_video() {
            let source = self
                .source
                .as_mut()
                .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))?;
            if self.media.skip_active_videos(source) {
                return Ok(());
            }
        }
        let batch = self
            .source()?
            .dispatch_action(action)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.executor
            .execute_batch(batch)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        Ok(())
    }

    async fn dispatch_pointer(&mut self) -> Result<(), ProductHostError> {
        let pointer = self.pointer;
        let batch = self
            .source()?
            .dispatch_pointer(pointer.0, pointer.1)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.executor
            .execute_batch(batch)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        Ok(())
    }

    async fn advance_through(&mut self, target_tick: u64) -> Result<(), ProductHostError> {
        while self.last_tick < target_tick {
            self.last_tick = self
                .last_tick
                .checked_add(1)
                .ok_or_else(|| ProductHostError::Input("tick sequence overflowed".into()))?;
            let now_ms = canonical_ms(self.last_tick)?;
            let source = self
                .source
                .as_mut()
                .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))?;
            if let Some(present) = source
                .tick_presentation(astra_headless_protocol::TICK_DURATION_NS)
                .map_err(|error| ProductHostError::Output(error.to_string()))?
            {
                self.executor
                    .execute_batch(present)
                    .await
                    .map_err(|error| ProductHostError::Output(error.to_string()))?;
            }
            self.media
                .poll_and_process_with_audio_tick(source, &mut self.executor, now_ms, true)
                .await
                .map_err(|error| ProductHostError::Output(error.to_string()))?;
        }
        Ok(())
    }

    async fn process_without_audio_tick(&mut self, now_ms: u64) -> Result<(), ProductHostError> {
        let source = self
            .source
            .as_mut()
            .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))?;
        self.media
            .poll_and_process_with_audio_tick(source, &mut self.executor, now_ms, false)
            .await
            .map_err(|error| ProductHostError::Output(error.to_string()))
    }

    async fn save(&mut self, slot: &str) -> Result<(), ProductHostError> {
        self.next_save_transaction = self
            .next_save_transaction
            .checked_add(1)
            .ok_or_else(|| ProductHostError::Input("save transaction overflowed".into()))?;
        let transaction = PlayerHostResourceId(self.next_save_transaction);
        let media_snapshot = serde_json::to_vec(&self.media.snapshot())
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
            .map_err(|error| ProductHostError::Input(error.to_string()))
    }

    async fn load(&mut self, slot: &str) -> Result<(), ProductHostError> {
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
        Ok(())
    }

    fn refresh_observations(&mut self) -> Result<(), ProductHostError> {
        let evidence = self
            .source
            .as_ref()
            .and_then(NativeVnHostCommandSource::last_step_evidence)
            .ok_or_else(|| {
                ProductHostError::Output("runtime step has no observation evidence".into())
            })?;
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
            hashed_observation("vn.pending_choices", &evidence.pending_choice_ids)?,
            hashed_observation("vn.terminal_routes", &evidence.terminal_route_ids)?,
            hashed_observation("media.active_video", &self.media.has_active_video())?,
            hashed_observation("media.active_voice", &self.media.has_active_voice())?,
        ];
        Ok(())
    }
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
