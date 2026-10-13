use super::stage::*;
use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub(super) fn render_with_stage_refresh(
        &mut self,
        ordered_outputs: &[NativeVnOrderedRuntimeOutput],
        presentation_count: usize,
        refresh_stage: bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.ensure_presentation_active()?;
        let stage_prepare_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let stage_commands = ordered_outputs
            .iter()
            .filter_map(|output| match output {
                NativeVnOrderedRuntimeOutput::Presentation(PresentationCommand::Stage(stage)) => {
                    Some(stage)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let transition_request = stage_commands.iter().rev().find_map(|command| {
            let StageCommand::Transition {
                descriptor_id: Some(descriptor_id),
                ..
            } = command
            else {
                return None;
            };
            Some(descriptor_id.clone())
        });
        let (next_stage_director, stage_outputs) = if stage_commands.is_empty() {
            (None, Vec::new())
        } else {
            let (director, outputs) = self
                .stage_director
                .prepare_batch(stage_commands)
                .map_err(stage_director_error)?;
            (Some(director), outputs)
        };
        if let Some(director) = next_stage_director.as_ref() {
            self.check_wait_fence(director)?;
        }
        let mut stage_outputs = stage_outputs.into_iter();
        let mut next_audio = Vec::new();
        for output in ordered_outputs {
            if let NativeVnOrderedRuntimeOutput::AudioStart(request) = output {
                next_audio.push(NativeVnAudioOutput::Start(request.clone()));
                continue;
            }
            let NativeVnOrderedRuntimeOutput::Presentation(command) = output else {
                continue;
            };
            if let PresentationCommand::Stage(stage) = command {
                let outputs = stage_outputs.next().ok_or_else(|| {
                    NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_STAGE_BATCH_OUTPUT_MISSING: {}",
                        stage.kind()
                    ))
                })?;
                for output in outputs {
                    match output {
                        StageDirectorOutput::Preload { asset } => {
                            if !self.textures.contains_key(&asset)
                                && !self.asset_store.contains_media(&asset)
                                && !self.asset_store.contains_image(&asset)
                            {
                                return Err(NativeVnHostError::Asset(format!(
                                    "ASTRA_PLAYER_PRELOAD_ASSET_MISSING: {asset}"
                                )));
                            }
                        }
                        StageDirectorOutput::Audio(_) => {}
                        StageDirectorOutput::AudioControl(control) => {
                            next_audio.push(NativeVnAudioOutput::Control(
                                NativeVnAudioControlRequest {
                                    command_id: control.id,
                                    action: match &control.action {
                                        VnAudioControlAction::Pause => "pause",
                                        VnAudioControlAction::Resume => "resume",
                                        VnAudioControlAction::Stop => "stop",
                                        VnAudioControlAction::FadeStop { .. } => "fade_stop",
                                    }
                                    .to_string(),
                                    target: control.target,
                                    duration_ms: match &control.action {
                                        VnAudioControlAction::FadeStop { duration_ms, .. } => {
                                            Some(*duration_ms)
                                        }
                                        _ => None,
                                    },
                                    fence: match control.action {
                                        VnAudioControlAction::FadeStop { fence, .. } => Some(fence),
                                        _ => None,
                                    },
                                },
                            ));
                        }
                        StageDirectorOutput::AudioBusEnabled { bus, enabled } => {
                            let target = match bus {
                                VnAudioBus::Bgm => "bgm",
                                VnAudioBus::Se => "se",
                                VnAudioBus::Voice => "voice",
                                VnAudioBus::Movie => "movie_audio",
                            };
                            next_audio.push(NativeVnAudioOutput::Control(
                                NativeVnAudioControlRequest {
                                    command_id: format!("audio.bus.{target}.enabled"),
                                    action: if enabled { "enable_bus" } else { "disable_bus" }
                                        .to_string(),
                                    target: target.to_string(),
                                    duration_ms: None,
                                    fence: None,
                                },
                            ));
                        }
                        StageDirectorOutput::Movie(movie) => {
                            let asset = self.asset_store.load_media(&movie.asset)?;
                            let scope = self.replace_video_scope(&movie.layer);
                            self.pending_video.push(NativeVnVideoRequest {
                                scope,
                                layer: movie.layer,
                                asset_id: movie.asset,
                                codec: asset.codec.clone(),
                                encoded_bytes: asset.bytes.clone(),
                                encoded_length: asset.byte_length,
                                alpha_millionths: movie.alpha.millionths,
                                looping: matches!(movie.loop_mode, MovieLoopMode::Loop),
                                fence: movie.fence,
                            });
                        }
                        StageDirectorOutput::Effect(_) => {}
                        StageDirectorOutput::FenceCompleted { id, .. } => {
                            self.pending_stage_completions.push(id);
                        }
                    }
                }
            }
        }
        if stage_outputs.next().is_some() {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_STAGE_BATCH_OUTPUT_EXCESS: stage output count exceeded command count"
                    .into(),
            ));
        }
        let stage_prepare_ns = performance_phase_duration(stage_prepare_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.stage_prepare_ns = sample.stage_prepare_ns.saturating_add(stage_prepare_ns);
        }
        let stage_scene_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let mut lifecycle = std::mem::take(&mut self.pending_gpu_lifecycle);
        let mut uploaded_texture_ids = Vec::new();
        let next_stage_scene = if next_stage_director.is_some()
            || refresh_stage
            || self.gpu_resources_invalidated
        {
            let stage_texture_started =
                performance_phase_started(self.ui_host_performance_sampling_enabled);
            let (mut required, mut cpu_required) = presentation::stage_texture_requirements(
                next_stage_director
                    .as_ref()
                    .unwrap_or(&self.stage_director)
                    .state(),
                &self.textures,
            );
            let outgoing_state = if transition_request.is_some() {
                Some(self.stage_director.state())
            } else {
                self.director_transition_snapshot
                    .as_ref()
                    .map(|snapshot| &snapshot.source_state)
            };
            if let Some(state) = outgoing_state {
                let (outgoing, outgoing_cpu) =
                    presentation::stage_texture_requirements(state, &self.textures);
                required.extend(outgoing);
                cpu_required.extend(outgoing_cpu);
            }
            self.ensure_stage_texture_assets(required, cpu_required)?;
            let stage_texture_ns = performance_phase_duration(stage_texture_started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.stage_texture_ns = sample.stage_texture_ns.saturating_add(stage_texture_ns);
            }
            let stage_command_started =
                performance_phase_started(self.ui_host_performance_sampling_enabled);
            let scene_draw = stage_scene_commands(
                next_stage_director
                    .as_ref()
                    .unwrap_or(&self.stage_director)
                    .state(),
                &self.textures,
                &self.texture_dimensions,
                self.width,
                self.height,
            )?;
            let stage_command_ns = performance_phase_duration(stage_command_started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.stage_command_ns = sample.stage_command_ns.saturating_add(stage_command_ns);
            }

            let stage_lifecycle_started =
                performance_phase_started(self.ui_host_performance_sampling_enabled);
            let mut texture_ids = scene_texture_ids(&scene_draw);
            // Both halves of a transition must survive residency eviction, including
            // a restored outgoing scene that has never been uploaded by this process.
            if transition_request.is_some() {
                texture_ids.extend(scene_texture_ids(&self.scene_draw));
            } else if next_stage_director
                .as_ref()
                .unwrap_or(&self.stage_director)
                .state()
                .transition
                .as_ref()
                .and_then(|value| value.descriptor_id.as_ref())
                .is_some()
            {
                if let Some(snapshot) = &self.director_transition_snapshot {
                    texture_ids.extend(scene_texture_ids(&snapshot.source_draw));
                }
            }
            for asset_id in &texture_ids {
                self.mark_texture_used(asset_id)?;
            }
            let missing_ids = texture_ids
                .difference(&self.live_texture_ids)
                .cloned()
                .collect::<Vec<_>>();
            let missing_bytes = missing_ids.iter().try_fold(0u64, |total, asset_id| {
                total
                    .checked_add(self.texture(asset_id)?.rgba8.len() as u64)
                    .ok_or_else(|| {
                        NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_BYTES_OVERFLOW".into())
                    })
            })?;
            self.evict_gpu_textures_for(missing_bytes, &texture_ids, &mut lifecycle)?;
            for asset_id in missing_ids {
                let frame = self.texture(&asset_id)?.clone();
                self.resident_texture_bytes = self
                    .resident_texture_bytes
                    .checked_add(frame.rgba8.len() as u64)
                    .ok_or_else(|| {
                        NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_BYTES_OVERFLOW".into())
                    })?;
                self.live_texture_ids.insert(asset_id.clone());
                self.live_texture_bytes
                    .insert(asset_id.clone(), frame.rgba8.len() as u64);
                lifecycle.push(SceneCommand::UploadTexture {
                    resource_id: asset_id.clone(),
                    frame,
                });
                uploaded_texture_ids.push(asset_id);
            }
            let stage_lifecycle_ns = performance_phase_duration(stage_lifecycle_started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.stage_lifecycle_ns =
                    sample.stage_lifecycle_ns.saturating_add(stage_lifecycle_ns);
            }
            Some((scene_draw, texture_ids))
        } else {
            None
        };
        let stage_scene_ns = performance_phase_duration(stage_scene_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.stage_scene_ns = sample.stage_scene_ns.saturating_add(stage_scene_ns);
        }
        let active_transition = next_stage_director
            .as_ref()
            .map(|director| director.state().transition.as_ref())
            .unwrap_or_else(|| self.stage_director.state().transition.as_ref())
            .cloned();
        let next_transition_snapshot = match (transition_request, active_transition.as_ref()) {
            (Some(descriptor_id), Some(transition))
                if transition.descriptor_id.as_deref() == Some(descriptor_id.as_str()) =>
            {
                Some(DirectorTransitionSnapshot {
                    descriptor_id,
                    source_state: self.stage_director.state().clone(),
                    source_draw: self.scene_draw.clone(),
                })
            }
            (Some(_), _) => {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_DIRECTOR_TRANSITION_DESCRIPTOR_UNBOUND".into(),
                ));
            }
            (None, _) => None,
        };

        for command in ordered_outputs.iter().filter_map(|output| match output {
            NativeVnOrderedRuntimeOutput::Presentation(command) => Some(command),
            NativeVnOrderedRuntimeOutput::AudioStart(_) => None,
        }) {
            match command {
                PresentationCommand::Dialogue { .. }
                | PresentationCommand::Choice { .. }
                | PresentationCommand::SystemPage { .. }
                | PresentationCommand::SystemOption { .. } => {}
                PresentationCommand::Stage(_) => {}
                PresentationCommand::Extension(extension) => {
                    return Err(NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_EXTENSION_PROVIDER_UNWIRED: command {} requires provider {}",
                        extension.command, extension.provider_id
                    )));
                }
                PresentationCommand::Marker { .. } => {}
            }
        }
        let ui_frame = if self.runtime_state.as_ref().is_some_and(|state| {
            state.pending_choice.is_some()
                || !state.system_stack.is_empty()
                || state.pending_wait.as_ref().map(|wait| wait.kind) == Some(VnWaitKind::Dialogue)
        }) {
            Some(self.render_ui(Vec::new())?)
        } else {
            None
        };
        let scene_compose_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        if ui_frame.is_none() && !self.live_layout_ids.is_empty() {
            let layout_ids = self.live_layout_ids.iter().cloned().collect::<Vec<_>>();
            let removal_refs = layout_ids.iter().map(String::as_str).collect::<Vec<_>>();
            lifecycle.extend(
                self.text_resources
                    .update_frame(&[], &removal_refs)?
                    .lifecycle,
            );
            self.live_layout_ids.clear();
            self.ui_frame_reuse = None;
            self.ui_text_measurer.begin_frame()?;
        }
        lifecycle.push(SceneCommand::rect(
            "vn.frame.clear",
            0,
            0,
            self.width,
            self.height,
            [8, 10, 16, 255],
        ));
        let stage_draw = next_stage_scene
            .as_ref()
            .map_or(self.scene_draw.as_slice(), |(scene_draw, _)| {
                scene_draw.as_slice()
            });
        let composed_stage_draw = match (
            next_transition_snapshot.as_ref().or_else(|| {
                active_transition
                    .as_ref()
                    .and(self.director_transition_snapshot.as_ref())
            }),
            active_transition.as_ref(),
        ) {
            (Some(snapshot), Some(transition)) => compose_director_transition_scene(
                snapshot,
                stage_draw,
                transition.progress.millionths,
                self.width,
                self.height,
            )?,
            (None, Some(transition)) if transition.descriptor_id.is_some() => {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_DIRECTOR_TRANSITION_SNAPSHOT_MISSING".into(),
                ));
            }
            _ => stage_draw.to_vec(),
        };
        lifecycle.extend(composed_stage_draw);
        let ui_draw = ui_frame.map_or_else(Vec::new, |frame| frame.draw);
        lifecycle.extend(ui_draw.iter().cloned());
        self.command_sequence = self
            .command_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        tracing::trace!(
            event = "player.vn.runtime.command.emit",
            sequence = self.command_sequence,
            presentation_count,
            scene_command_count = lifecycle.len(),
            "emitted AstraVN Player host command"
        );
        if let Some((scene_draw, _texture_ids)) = next_stage_scene {
            self.scene_draw = scene_draw;
        }
        if let Some(stage_director) = next_stage_director {
            self.stage_director = stage_director;
        }
        if let Some(snapshot) = next_transition_snapshot {
            self.director_transition_snapshot = Some(snapshot);
        } else if active_transition.is_none() {
            self.director_transition_snapshot = None;
        }
        self.cancel_removed_video_scopes();
        self.ui_draw = reusable_ui_draw_commands(&ui_draw);
        if !next_audio.is_empty() {
            tracing::trace!(
                event = "player.vn.audio_sequence.queued",
                fixed_step = self.fixed_step,
                media_command_count = next_audio.len(),
                "queued audio starts and controls in Runtime output order"
            );
        }
        self.pending_audio.extend(next_audio);
        let batch = PlayerHostCommandBatch::new(vec![PlayerHostCommand::PresentScene {
            sequence: self.command_sequence,
            surface: self.surface,
            width: self.output_extent.width,
            height: self.output_extent.height,
            clear_rgba: [0, 0, 0, 255],
            commands: self.map_output_scene(lifecycle)?,
            semantics: self.output_semantics()?,
        }])?;
        for asset_id in uploaded_texture_ids {
            self.remove_texture(&asset_id)?;
            self.asset_store.release_uploaded_image(&asset_id)?;
        }
        let scene_compose_ns = performance_phase_duration(scene_compose_started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.scene_compose_ns = sample.scene_compose_ns.saturating_add(scene_compose_ns);
        }
        self.gpu_resources_invalidated = false;
        Ok(batch)
    }
}
