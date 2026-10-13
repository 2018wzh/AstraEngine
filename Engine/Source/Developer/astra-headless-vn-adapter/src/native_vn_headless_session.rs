use super::*;

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
            samples: self
                .media
                .submitted_audio_timeline()
                .map_err(|error| ProductHostError::Output(error.to_string()))?,
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
