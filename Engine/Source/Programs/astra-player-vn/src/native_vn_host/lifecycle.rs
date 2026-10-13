use super::stage::*;
use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub fn release_resources(&mut self) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if self.shutdown_started {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_SHUTDOWN_REPEATED: resource shutdown already started".to_string(),
            ));
        }
        self.reset_pending_work();
        self.media_scope.cancel();
        let completions = self.image_prefetcher.shutdown()?;
        self.image_prefetch_inflight.clear();
        for completion in completions {
            if self.image_prefetcher.is_current(&completion.scope) {
                if let Err(error) = completion.result {
                    self.image_prefetch_failure =
                        Some(format!("ASTRA_PLAYER_IMAGE_PREFETCH_FAILED: {error}"));
                }
            }
        }
        self.ui_text_measurer.begin_frame()?;
        self.ui_frame_reuse = None;
        let mut commands = std::mem::take(&mut self.pending_gpu_lifecycle);
        commands.extend(self.text_resources.shutdown());
        commands.extend(self.live_texture_ids.iter().map(|resource_id| {
            SceneCommand::ReleaseResource {
                resource_id: resource_id.clone(),
            }
        }));
        commands.push(SceneCommand::rect(
            "vn.shutdown.clear",
            0,
            0,
            self.width,
            self.height,
            [0, 0, 0, 255],
        ));
        self.live_texture_ids.clear();
        self.live_texture_bytes.clear();
        self.texture_last_used.clear();
        self.texture_cpu_last_used.clear();
        self.textures.clear();
        self.texture_dimensions.clear();
        self.texture_cpu_bytes = 0;
        self.resident_texture_bytes = 0;
        self.live_layout_ids.clear();
        self.scene_draw.clear();
        self.shutdown_started = true;
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentScene {
                sequence: self.command_sequence,
                surface: self.surface,
                width: self.output_extent.width,
                height: self.output_extent.height,
                clear_rgba: [0, 0, 0, 255],
                commands: self.map_output_scene(commands)?,
                semantics: None,
            },
        ])?)
    }

    pub fn shutdown(mut self) -> Result<(), NativeVnHostError> {
        if !self.shutdown_started {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_SHUTDOWN_ORDER: release_resources must execute before provider shutdown"
                    .to_string(),
            ));
        }
        self.host.shutdown()?;
        self.host.destroy()?;
        if let Some(error) = self.image_prefetch_failure.take() {
            return Err(NativeVnHostError::Asset(error));
        }
        Ok(())
    }

    pub(super) fn command(
        &mut self,
        command: VnPlayerCommand,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.ensure_presentation_active()?;
        if matches!(&command, VnPlayerCommand::Advance) {
            if self.skip_waiting_video()? {
                return self.present_current_scene(self.ui_draw.clone());
            }
            match self.stage_director.request_text_advance() {
                TextAdvanceDisposition::RevealCompleted => {
                    let ui = self.render_ui(Vec::new())?;
                    self.ui_semantics = Some(ui.semantics);
                    return self.present_current_scene(ui.draw);
                }
                TextAdvanceDisposition::StoryAdvanceRequested => {
                    let previous = self.stage_director.clone();
                    self.stage_director
                        .acknowledge_story_advance()
                        .map_err(stage_director_error)?;
                    return match self.step(NativeVnStepCommand::Execute(command)) {
                        Ok(batch) => Ok(batch),
                        Err(error) => {
                            self.stage_director = previous;
                            Err(error)
                        }
                    };
                }
                TextAdvanceDisposition::NoActiveText => {}
            }
        }
        self.step(NativeVnStepCommand::Execute(command))
    }

    pub(super) fn ensure_text_region(&mut self) -> Result<(), NativeVnHostError> {
        let Some(state) = self.runtime_state.as_ref() else {
            return Ok(());
        };
        if state.pending_wait.as_ref().map(|wait| wait.kind) != Some(VnWaitKind::Dialogue) {
            return Ok(());
        }
        let Some(entry) = state.backlog.last() else {
            return Err(NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_TEXT_REVEAL_BACKLOG: dialogue wait has no backlog entry".into(),
            ));
        };
        let text = resolve_localized(&self.localization, &entry.key)?;
        let text_graphemes = u32::try_from(text.graphemes(true).count()).map_err(|_| {
            NativeVnHostError::Localization(
                "ASTRA_PLAYER_TEXT_REVEAL_TOO_LONG: localized dialogue exceeds grapheme limit"
                    .into(),
            )
        })?;
        let configured = match state.system.config.get("text.speed") {
            Some(value) => value.parse::<i64>().map_err(|_| {
                NativeVnHostError::Input(
                    "ASTRA_PLAYER_TEXT_REVEAL_SPEED: text.speed must be an integer".into(),
                )
            })?,
            None => 50,
        }
        .clamp(0, 100);
        let graphemes_per_second = match configured {
            0..=20 => 18,
            21..=40 => 30,
            41..=60 => 45,
            61..=80 => 65,
            _ => 90,
        };
        if self
            .stage_director
            .text_reveal_state()
            .as_ref()
            .is_some_and(|reveal| reveal.command_id == entry.command_id)
        {
            return Ok(());
        }
        let command_id = entry.command_id.clone();
        let text_key = entry.key.clone();
        let speaker = entry.speaker.clone();
        let window = entry.layout.window.clone();
        self.stage_director
            .configure_text(
                command_id,
                text_key,
                speaker,
                window,
                text_graphemes,
                graphemes_per_second,
                PresentationInterruptPolicy::ReplaceFromCurrent,
            )
            .map_err(stage_director_error)?;
        Ok(())
    }

    pub(super) fn step(
        &mut self,
        command: NativeVnStepCommand,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.ensure_presentation_active()?;
        if self.shutdown_started {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_SHUTDOWN_STATE: runtime input arrived after shutdown started"
                    .to_string(),
            ));
        }
        self.poll_image_prefetch()?;
        let fixed_step = self
            .fixed_step
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        let runtime_step_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let policy_poll = matches!(command, NativeVnStepCommand::PollPolicy);
        let previous_wait = self.pending_wait().cloned();
        let output = self.host.step(NativeVnStepInput {
            timing: astra_runtime::TickInput {
                fixed_step,
                delta_ns: 16_666_667,
                seed: self.session_seed,
            },
            mode: self.next_step_mode,
            command,
        })?;
        let runtime_host_step_ns = performance_phase_duration(runtime_step_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.runtime_host_step_ns = sample
                .runtime_host_step_ns
                .saturating_add(runtime_host_step_ns);
        }
        let output_decode_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        self.fixed_step = fixed_step;
        self.next_step_mode = astra_runtime::TickMode::Live;
        let runtime_view = output.vn_state;
        self.runtime_backlog_count = runtime_view.backlog_count;
        self.runtime_state = Some(runtime_view.into_display_state());
        self.ensure_text_region()?;
        self.schedule_current_image_prefetch()?;
        self.queue_current_story_audio_preloads()?;
        let runtime_state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_VN_STATE_MISSING: runtime state trace is required".into(),
            )
        })?;
        self.last_step_evidence = Some(NativeVnStepEvidence {
            schema: "astra.player_vn_step_evidence.v2".to_string(),
            fixed_step,
            coverage_reached: unique_string_set(
                output.coverage_reached,
                "ASTRA_PLAYER_VN_COVERAGE_DUPLICATE",
            )?,
            presentation_count: output.presentations.len(),
            current_state_id: runtime_state
                .cursor
                .as_ref()
                .map(|cursor| cursor.state_id.clone()),
            pending_wait_command_id: runtime_state
                .pending_wait
                .as_ref()
                .map(|wait| wait.command_id.clone()),
            pending_wait_await_id: runtime_state
                .pending_wait
                .as_ref()
                .and_then(|wait| wait.await_id.clone()),
            pending_choice_ids: runtime_state
                .pending_choice
                .as_ref()
                .map(|choice| {
                    choice
                        .options
                        .iter()
                        .map(|option| option.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
            terminal_route_ids: if runtime_state.cursor.is_none() {
                runtime_state
                    .route_coverage
                    .intersection(&self.terminal_routes)
                    .cloned()
                    .collect()
            } else {
                Default::default()
            },
        });
        for task in output.timeline {
            self.pending_timeline.push(player_timeline_task(task)?);
        }
        let mut live_audio_cues = output.audio.into_iter();
        let mut ordered_outputs = Vec::new();
        let mut presentation_count = 0_usize;
        for command in output.presentations {
            presentation_count = presentation_count.saturating_add(1);
            if matches!(&command, PresentationCommand::Stage(StageCommand::Audio(_))) {
                let cue = live_audio_cues.next().ok_or_else(|| {
                            NativeVnHostError::RuntimeEvidence(
                                "ASTRA_PLAYER_VN_AUDIO_LIVE_MISSING: stage audio presentation has no live cue".into(),
                            )
                        })?;
                let command_id = cue.command_id;
                let cue = cue.cue;
                let asset_id = cue.asset.clone();
                let asset = self.asset_store.load_media(&asset_id)?;
                let command_kind = match cue.bus {
                    VnAudioBus::Voice => "voice",
                    VnAudioBus::Bgm => "bgm",
                    VnAudioBus::Se => "se",
                    VnAudioBus::Movie => "movie",
                };
                let mut attributes = BTreeMap::from([
                    ("asset".to_string(), asset_id.clone()),
                    ("loop".to_string(), cue.looped.to_string()),
                    ("fade".to_string(), cue.fade_ms.to_string()),
                ]);
                match &cue.sync {
                    astra_vn_core::VnAudioSync::None => {}
                    astra_vn_core::VnAudioSync::Text => {
                        attributes.insert("sync".to_string(), "text".to_string());
                    }
                    astra_vn_core::VnAudioSync::Fence(fence) => {
                        attributes.insert("sync".to_string(), "fence".to_string());
                        attributes.insert("fence".to_string(), fence.clone());
                    }
                }
                ordered_outputs.push(NativeVnOrderedRuntimeOutput::AudioStart(
                    NativeVnAudioRequest {
                        command_id: command_id.clone(),
                        target_id: command_id,
                        command: command_kind.to_string(),
                        attributes,
                        asset_id,
                        codec: asset.codec.clone(),
                        encoded_bytes: asset.bytes.clone(),
                        encoded_length: asset.byte_length,
                    },
                ));
            }
            ordered_outputs.push(NativeVnOrderedRuntimeOutput::Presentation(command));
        }
        if live_audio_cues.next().is_some() {
            return Err(NativeVnHostError::RuntimeEvidence(
                "ASTRA_PLAYER_VN_AUDIO_LIVE_EXTRA: live audio cue has no stage audio presentation"
                    .into(),
            ));
        }
        tracing::trace!(
            event = "player.vn.runtime.step_applied",
            fixed_step,
            presentation_count,
            pending_wait_command_id = runtime_state
                .pending_wait
                .as_ref()
                .map(|wait| wait.command_id.as_str())
                .unwrap_or("none"),
            pending_choice_count = runtime_state
                .pending_choice
                .as_ref()
                .map_or(0, |choice| choice.options.len()),
            pending_choice_enabled_count = runtime_state
                .pending_choice
                .as_ref()
                .map_or(0, |choice| choice.enabled_option_ids.len()),
            "applied runtime output before presentation rendering"
        );
        let runtime_output_decode_ns = performance_phase_duration(output_decode_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.runtime_output_decode_ns = sample
                .runtime_output_decode_ns
                .saturating_add(runtime_output_decode_ns);
        }
        if policy_poll && presentation_count == 0 && previous_wait == self.pending_wait().cloned() {
            return PlayerHostCommandBatch::new(Vec::new()).map_err(Into::into);
        }
        let render_started = performance_phase_started(self.ui_host_performance_sampling_enabled);
        let batch = self.render(&ordered_outputs, presentation_count)?;
        let runtime_render_ns = performance_phase_duration(render_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.runtime_render_ns = sample.runtime_render_ns.saturating_add(runtime_render_ns);
        }

        Ok(batch)
    }

    pub(super) fn render(
        &mut self,
        ordered_outputs: &[NativeVnOrderedRuntimeOutput],
        presentation_count: usize,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.render_with_stage_refresh(ordered_outputs, presentation_count, false)
    }
}
