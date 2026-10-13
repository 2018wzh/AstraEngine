use super::*;

impl NativeVnHeadlessSession {
    pub(super) fn profile_started(&self) -> Option<Instant> {
        self.performance.as_ref().map(|_| Instant::now())
    }

    pub(super) fn add_profile_duration(
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

    pub(super) fn flush_performance_sample(
        &mut self,
        bind_to_next_presentation: bool,
    ) -> Result<(), ProductHostError> {
        let Some(observer) = self.performance_observer.as_ref() else {
            return Ok(());
        };
        let mut sample = self
            .performance
            .as_mut()
            .map(std::mem::take)
            .ok_or_else(|| {
                ProductHostError::Output(
                    "performance observer exists without profiling state".into(),
                )
            })?;
        sample.bind_to_next_presentation = bind_to_next_presentation;
        observer
            .record_sample(sample)
            .map_err(ProductHostError::Output)
    }

    pub(super) fn profile_duration(
        &self,
        started: Option<Instant>,
    ) -> Result<Option<u64>, ProductHostError> {
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

    pub(super) fn source(&mut self) -> Result<&mut NativeVnHostCommandSource, ProductHostError> {
        self.source
            .as_mut()
            .ok_or_else(|| ProductHostError::Input("product session is shut down".into()))
    }

    pub(super) async fn dispatch_ui(
        &mut self,
        event: UiInputEventKind,
    ) -> Result<(), ProductHostError> {
        let profile_started = self.profile_started();
        let batch = self
            .source()?
            .prepare_ui_input(event)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        let ui_sample = self.source()?.take_last_ui_performance_sample();
        let ui_host_sample = self.source()?.take_last_ui_host_performance_sample();
        self.add_profile_duration(profile_started, |sample, duration| {
            sample.ui_layout_paint_ns = sample.ui_layout_paint_ns.saturating_add(duration);
            if let Some(ui_sample) = ui_sample {
                sample.ui_request_validation_ns = sample
                    .ui_request_validation_ns
                    .saturating_add(ui_sample.request_validation_ns);
                sample.ui_input_routing_ns = sample
                    .ui_input_routing_ns
                    .saturating_add(ui_sample.input_routing_ns);
                sample.ui_tree_build_ns = sample
                    .ui_tree_build_ns
                    .saturating_add(ui_sample.tree_build_ns);
                sample.ui_layout_finalize_ns = sample
                    .ui_layout_finalize_ns
                    .saturating_add(ui_sample.layout_finalize_ns);
                sample.ui_semantics_ns = sample
                    .ui_semantics_ns
                    .saturating_add(ui_sample.semantics_ns);
                sample.ui_update_layout_ns = sample
                    .ui_update_layout_ns
                    .saturating_add(ui_sample.update_layout_ns);
                sample.ui_paint_conversion_ns = sample
                    .ui_paint_conversion_ns
                    .saturating_add(ui_sample.paint_conversion_ns);
                sample.ui_output_validation_ns = sample
                    .ui_output_validation_ns
                    .saturating_add(ui_sample.output_validation_ns);
                sample.ui_host_scene_ns = sample.ui_host_scene_ns.saturating_add(
                    duration.saturating_sub(
                        ui_sample
                            .request_validation_ns
                            .saturating_add(ui_sample.input_routing_ns)
                            .saturating_add(ui_sample.tree_build_ns)
                            .saturating_add(ui_sample.layout_finalize_ns)
                            .saturating_add(ui_sample.semantics_ns)
                            .saturating_add(ui_sample.update_layout_ns)
                            .saturating_add(ui_sample.paint_conversion_ns)
                            .saturating_add(ui_sample.output_validation_ns),
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
                sample.ui_text_layout_ns = sample
                    .ui_text_layout_ns
                    .saturating_add(ui_host_sample.text_layout_ns);
                sample.ui_text_resource_ns = sample
                    .ui_text_resource_ns
                    .saturating_add(ui_host_sample.text_resource_ns);
                sample.ui_text_compose_ns = sample
                    .ui_text_compose_ns
                    .saturating_add(ui_host_sample.text_compose_ns);
                sample.ui_action_dispatch_ns = sample
                    .ui_action_dispatch_ns
                    .saturating_add(ui_host_sample.action_dispatch_ns);
                sample.ui_present_scene_ns = sample
                    .ui_present_scene_ns
                    .saturating_add(ui_host_sample.present_scene_ns);
                sample.ui_runtime_host_step_ns = sample
                    .ui_runtime_host_step_ns
                    .saturating_add(ui_host_sample.runtime_host_step_ns);
                sample.ui_runtime_output_decode_ns = sample
                    .ui_runtime_output_decode_ns
                    .saturating_add(ui_host_sample.runtime_output_decode_ns);
                sample.ui_runtime_render_ns = sample
                    .ui_runtime_render_ns
                    .saturating_add(ui_host_sample.runtime_render_ns);
                sample.vn_stage_prepare_ns = sample
                    .vn_stage_prepare_ns
                    .saturating_add(ui_host_sample.stage_prepare_ns);
                sample.vn_stage_scene_ns = sample
                    .vn_stage_scene_ns
                    .saturating_add(ui_host_sample.stage_scene_ns);
                sample.vn_stage_texture_ns = sample
                    .vn_stage_texture_ns
                    .saturating_add(ui_host_sample.stage_texture_ns);
                sample.vn_stage_command_ns = sample
                    .vn_stage_command_ns
                    .saturating_add(ui_host_sample.stage_command_ns);
                sample.vn_stage_lifecycle_ns = sample
                    .vn_stage_lifecycle_ns
                    .saturating_add(ui_host_sample.stage_lifecycle_ns);
                sample.vn_scene_compose_ns = sample
                    .vn_scene_compose_ns
                    .saturating_add(ui_host_sample.scene_compose_ns);
            }
        })?;
        let binds_presentation = batch_presents_scene(&batch);
        self.flush_performance_sample(binds_presentation)?;
        let execution = self
            .executor
            .execute_batch(batch)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()));
        if binds_presentation {
            if let Some(observer) = &self.performance_observer {
                observer
                    .finish_presentation_sample()
                    .map_err(ProductHostError::Output)?;
            }
        }
        for result in execution? {
            if let PlayerHostCommandResult::Captured {
                width,
                height,
                rgba8,
                ..
            } = result
            {
                self.source()?
                    .cache_gameplay_surface(width, height, rgba8)
                    .map_err(|error| ProductHostError::Input(error.to_string()))?;
            }
        }
        self.process_ui_host_request().await?;
        Ok(())
    }

    pub(super) async fn process_ui_host_request(&mut self) -> Result<(), ProductHostError> {
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

    pub(super) async fn advance_through(
        &mut self,
        target_tick: u64,
    ) -> Result<(), ProductHostError> {
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
            if let Some(batch) = self
                .source()?
                .tick_policy()
                .map_err(|error| ProductHostError::Output(error.to_string()))?
            {
                self.executor
                    .execute_batch(batch)
                    .await
                    .map_err(|error| ProductHostError::Output(error.to_string()))?;
            }
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
                    let binds_presentation = batch_presents_scene(&present);
                    self.flush_performance_sample(binds_presentation)?;
                    let execution = self
                        .executor
                        .execute_batch(present)
                        .await
                        .map_err(|error| ProductHostError::Output(error.to_string()));
                    if binds_presentation {
                        if let Some(observer) = &self.performance_observer {
                            observer
                                .finish_presentation_sample()
                                .map_err(ProductHostError::Output)?;
                        }
                    }
                    execution?;
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
                sample.media_prewarm_ns = sample
                    .media_prewarm_ns
                    .saturating_add(media_sample.prewarm_ns);
                sample.media_provider_decode_ns = sample
                    .media_provider_decode_ns
                    .saturating_add(media_sample.provider_decode_ns);
                sample.media_parse_convert_ns = sample
                    .media_parse_convert_ns
                    .saturating_add(media_sample.parse_convert_ns);
                sample.media_mixer_ns = sample.media_mixer_ns.saturating_add(media_sample.mixer_ns);
                sample.media_audio_query_ns = sample
                    .media_audio_query_ns
                    .saturating_add(media_sample.audio_query_ns);
                sample.media_audio_render_ns = sample
                    .media_audio_render_ns
                    .saturating_add(media_sample.audio_render_ns);
                sample.media_audio_submit_ns = sample
                    .media_audio_submit_ns
                    .saturating_add(media_sample.audio_submit_ns);
                sample.media_audio_completion_ns = sample
                    .media_audio_completion_ns
                    .saturating_add(media_sample.audio_completion_ns);
            })?;
            self.flush_performance_sample(false)?;
        }
        Ok(())
    }

    pub(super) async fn process_without_audio_tick(
        &mut self,
        now_ms: u64,
    ) -> Result<(), ProductHostError> {
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
            sample.media_prewarm_ns = sample
                .media_prewarm_ns
                .saturating_add(media_sample.prewarm_ns);
            sample.media_provider_decode_ns = sample
                .media_provider_decode_ns
                .saturating_add(media_sample.provider_decode_ns);
            sample.media_parse_convert_ns = sample
                .media_parse_convert_ns
                .saturating_add(media_sample.parse_convert_ns);
            sample.media_mixer_ns = sample.media_mixer_ns.saturating_add(media_sample.mixer_ns);
            sample.media_audio_query_ns = sample
                .media_audio_query_ns
                .saturating_add(media_sample.audio_query_ns);
            sample.media_audio_render_ns = sample
                .media_audio_render_ns
                .saturating_add(media_sample.audio_render_ns);
            sample.media_audio_submit_ns = sample
                .media_audio_submit_ns
                .saturating_add(media_sample.audio_submit_ns);
            sample.media_audio_completion_ns = sample
                .media_audio_completion_ns
                .saturating_add(media_sample.audio_completion_ns);
        })?;
        self.flush_performance_sample(false)
    }

    pub(super) async fn save(&mut self, slot: &str) -> Result<(), ProductHostError> {
        let profile_started = self.profile_started();
        self.next_save_transaction = self
            .next_save_transaction
            .checked_add(1)
            .ok_or_else(|| ProductHostError::Input("save transaction overflowed".into()))?;
        let transaction = PlayerHostResourceId(self.next_save_transaction);
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
            .source
            .as_mut()
            .ok_or_else(|| ProductHostError::Input("runtime source is unavailable".into()))?
            .prepare_product_save_transaction(slot, transaction, &self.media)
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.executor
            .execute_save_transaction(plan)
            .await
            .map_err(|error| ProductHostError::Input(error.to_string()))?;
        self.add_profile_duration(profile_started, |sample, duration| {
            sample.save_load_ns = sample.save_load_ns.saturating_add(duration);
        })?;
        self.flush_performance_sample(false)
    }

    pub(super) async fn capture_gameplay_surface(&mut self) -> Result<(), ProductHostError> {
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
}
