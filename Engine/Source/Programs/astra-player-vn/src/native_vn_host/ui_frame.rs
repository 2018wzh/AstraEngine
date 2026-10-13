use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub(super) fn render_ui(
        &mut self,
        events: Vec<UiInputEvent>,
    ) -> Result<NativeVnUiFrameResult, NativeVnHostError> {
        let model_binding_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let mut host_performance = NativeVnUiHostPerformanceSample::default();
        let state = self.runtime_state.as_ref().ok_or_else(|| {
            NativeVnHostError::Input("ASTRA_PLAYER_STATE: runtime has not launched".into())
        })?;
        let text_layout_command = (state.system_stack.is_empty()
            && state.pending_choice.is_none()
            && state.pending_wait.as_ref().map(|wait| wait.kind) == Some(VnWaitKind::Dialogue))
        .then(|| {
            self.stage_director
                .text_reveal_state()
                .map(|text| text.command_id)
        })
        .flatten();
        let save_slots = self.ui_save_slots.values().cloned().collect::<Vec<_>>();
        let text_reveal = self.stage_director.text_reveal_state();
        let context = VnUiModelContext {
            runtime: state,
            text_reveal: text_reveal.as_ref(),
            story: &self.story,
            save_slots: &save_slots,
            localization_keys: &self.localization_keys,
        };
        let (surface, system_page, model) = if state.pending_choice.is_some() {
            (
                "choice".to_string(),
                None,
                context.build_choice()?.to_ui_value()?,
            )
        } else if let Some(frame) = state.system_stack.last() {
            let page_model = context.build_system_page(frame.page)?;
            (
                "system".to_string(),
                Some(system_page_binding_key(frame.page)?),
                page_model.to_ui_value()?,
            )
        } else {
            let message = context.build_message()?;
            let surface = message
                .window
                .clone()
                .unwrap_or_else(|| "message".to_string());
            (surface, None, message.to_ui_value()?)
        };
        let binding = resolve_binding(
            &self.ui_bindings,
            VnUiBindingRequest {
                command_id: state
                    .pending_wait
                    .as_ref()
                    .map(|wait| wait.command_id.as_str()),
                system_page,
                surface: Some(&surface),
                profile: &self.ui_profile,
            },
        )?
        .clone();
        let (active_view_id, active_model_schema) = self
            .ui_blueprints
            .views
            .get(&binding.view_id)
            .map(|view| (view.id.clone(), view.model_schema.clone()))
            .ok_or_else(|| {
                NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_VIEW_MISSING: view {} is not packaged",
                    binding.view_id
                ))
            })?;
        let theme = self
            .ui_themes
            .get(&binding.theme_id)
            .cloned()
            .ok_or_else(|| {
                NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_THEME_MISSING: binding references unpackaged theme {}",
                    binding.theme_id
                ))
            })?;
        let controller_manifest = self
            .ui_controller_host
            .manifest(&binding.controller_id)
            .ok_or_else(|| {
                NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_CONTROLLER_MISSING: controller {} is not packaged",
                    binding.controller_id
                ))
            })?;
        if controller_manifest.view != binding.view_id
            || controller_manifest.model_schema != active_model_schema
        {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_CONTROLLER_BINDING: controller manifest does not match the selected view"
                .into(),
            ));
        }
        // A dialogue wait changes its model but does not create a new UI
        // surface. Keeping the controller instance scoped to the surface lets
        // its retained state, font resources, and layout caches survive a
        // normal `advance` action. Choice and system-page instances remain
        // distinct because their semantic action sets can change structurally.
        let instance_source = if state.pending_choice.is_some() {
            state
                .pending_wait
                .as_ref()
                .map(|wait| {
                    format!(
                        "{}.{}",
                        wait.command_id,
                        wait.await_id.as_deref().unwrap_or("unbound")
                    )
                })
                .ok_or_else(|| {
                    NativeVnHostError::Input("ASTRA_PLAYER_UI_CHOICE_INSTANCE_WAIT_MISSING".into())
                })?
        } else {
            system_page
                .map(str::to_owned)
                .unwrap_or_else(|| surface.to_owned())
        };
        let instance_id = format!(
            "{}.{}.{}.{}",
            self.ui_generation, binding.view_id, binding.controller_id, instance_source
        );
        let active_model = model.clone();
        let active_controller_id = binding.controller_id.clone();
        let active_changed = self
            .base_ui_instance_id
            .as_ref()
            .is_none_or(|active| active != &instance_id);
        if active_changed {
            self.ui_modals.clear();
            self.pending_ui_focus = None;
            self.ui_animations.clear();
            self.base_ui_instance_id = Some(instance_id.clone());
        }
        self.base_ui_theme_id = Some(binding.theme_id.clone());
        host_performance.model_binding_ns = performance_phase_duration(model_binding_started)?;
        let controller_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let lifecycle_effects = if active_changed {
            self.ui_controller_host.invoke_open(
                &active_controller_id,
                &active_model_schema,
                &active_model,
                self.ui_controller_sessions
                    .entry(active_controller_id.clone())
                    .or_default(),
            )
        } else {
            self.ui_controller_host.invoke_update(
                &active_controller_id,
                &active_model_schema,
                &active_model,
                &VnUiControllerUpdate {
                    fixed_time_ns: self.fixed_step.saturating_mul(16_666_667),
                    delta_ns: 16_666_667,
                    generation: self.ui_generation,
                },
                self.ui_controller_sessions
                    .entry(active_controller_id.clone())
                    .or_default(),
            )
        }
        .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
        self.apply_ui_controller_effects(&active_controller_id, lifecycle_effects, false)?;

        let modal_updates = self.ui_modals.clone();
        for modal in modal_updates {
            if !self
                .ui_modals
                .iter()
                .any(|live| live.instance_id == modal.instance_id)
            {
                continue;
            }
            let effects = self
                .ui_controller_host
                .invoke_update(
                    &modal.controller_id,
                    &modal.model_schema,
                    &modal.model,
                    &VnUiControllerUpdate {
                        fixed_time_ns: self.fixed_step.saturating_mul(16_666_667),
                        delta_ns: 16_666_667,
                        generation: self.ui_generation,
                    },
                    self.ui_controller_sessions
                        .entry(modal.controller_id.clone())
                        .or_default(),
                )
                .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
            self.apply_ui_controller_effects(&modal.controller_id, effects, false)?;
        }

        host_performance.controller_ns = performance_phase_duration(controller_started)?;
        let frame_model_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let fixed_time_ns = self.fixed_step.saturating_mul(16_666_667);
        let state_value = controller_state_value(
            self.ui_controller_sessions
                .entry(active_controller_id.clone())
                .or_default()
                .values(),
            self.ui_animations.values().map(|animation| {
                (
                    animation.target_id.clone(),
                    animation.progress_millionths(fixed_time_ns),
                )
            }),
        );
        let modal_frames = self
            .ui_modals
            .iter()
            .map(|modal| {
                let state = self
                    .ui_controller_sessions
                    .get(&modal.controller_id)
                    .map(VnUiSessionState::values)
                    .cloned()
                    .unwrap_or_default();
                UiBlueprintModalFrameModel {
                    view_id: modal.view_id.clone(),
                    model_schema: modal.model_schema.clone(),
                    model: modal.model.clone(),
                    state: controller_state_value(&state, std::iter::empty()),
                }
            })
            .collect::<Vec<_>>();
        let localization = frame_localization_subset(
            &self.localization.strings,
            &self.ui_view_localization_keys,
            &binding.view_id,
            &model,
            &state_value,
            &modal_frames,
        )?;
        let frame = UiBlueprintFrameModel {
            schema: "astra.ui_blueprint_frame_model.v1".to_string(),
            view_id: binding.view_id,
            model,
            state: state_value,
            modals: modal_frames,
            focus_request: self.pending_ui_focus.take(),
            localization,
        };
        let request = UiFrameRequest {
            schema: "astra.ui_frame_request.v1".to_string(),
            session_id: format!("vn.ui.{}", self.session_id),
            generation: self.ui_generation,
            viewport: self.ui_viewport.clone(),
            fixed_time_ns: self.fixed_step.saturating_mul(16_666_667),
            input: UiInputFrame {
                schema: "astra.ui_input_frame.v1".to_string(),
                events,
            },
            theme,
            model_schema: active_model_schema.clone(),
            model_revision: self.fixed_step,
            model: frame,
        };
        let active = self.ui_modals.last().map_or(
            ActiveUiController {
                instance_id: instance_id.clone(),
                controller_id: active_controller_id.clone(),
                view_id: active_view_id.clone(),
                model_schema: active_model_schema.clone(),
                model: active_model.clone(),
            },
            |modal| ActiveUiController {
                instance_id: modal.instance_id.clone(),
                controller_id: modal.controller_id.clone(),
                view_id: modal.view_id.clone(),
                model_schema: modal.model_schema.clone(),
                model: modal.model.clone(),
            },
        );
        self.active_ui_controller = Some(active);
        // Presentation reveal advances independently of the 60 Hz runtime step.
        // Reuse only when the visible text model is unchanged as well.
        let text_reveal_progress = text_reveal
            .as_ref()
            .map(|text| (text.visible_graphemes, text.text_graphemes));
        let stable_frame = request.input.events.is_empty() && !active_changed;
        host_performance.frame_model_ns = performance_phase_duration(frame_model_started)?;
        if !active_changed {
            if let (Some(reuse), Some(semantics)) =
                (self.ui_frame_reuse.as_mut(), self.ui_semantics.as_ref())
            {
                if reuse
                    .key
                    .matches(&request, &instance_id, text_reveal_progress)
                {
                    if let Some(pointer_position) = reusable_pointer_position(
                        reuse.pointer_position,
                        &request.input.events,
                        semantics,
                    ) {
                        reuse.pointer_position = pointer_position;
                        let mut performance = reuse.performance.clone();
                        performance.update_layout_ns = 0;
                        performance.paint_conversion_ns = 0;
                        performance.texture_update_bytes = 0;
                        self.ui_performance.record(performance.clone(), true)?;
                        self.last_ui_performance_sample = Some(performance);
                        tracing::trace!(
                            event = "player.vn.ui.frame.reused",
                            view_id = %active_view_id,
                            generation = self.ui_generation,
                            input_count = request.input.events.len(),
                            semantic_generation = semantics.generation,
                            "reused an unchanged NativeVN UI frame"
                        );
                        let result = NativeVnUiFrameResult {
                            actions: Vec::new(),
                            dispositions: request
                                .input
                                .events
                                .iter()
                                .map(|event| UiInputDisposition {
                                    sequence: event.sequence,
                                    disposition: UiInputDispositionKind::Bubble,
                                    semantic_target_id: None,
                                })
                                .collect(),
                            semantics: semantics.clone(),
                            // `ui_draw` is the last complete submission and can contain
                            // one-shot resource lifecycle commands. Replaying those commands
                            // on a pointer-only frame would re-upload already live glyphs or
                            // textures, which the renderer correctly rejects. The retained
                            // resource owner remains authoritative until a new UI layout emits
                            // an explicit lifecycle delta.
                            draw: reusable_ui_draw_commands(&self.ui_draw),
                        };
                        self.accumulate_ui_host_performance(host_performance);
                        return Ok(result);
                    }
                }
            }
        }
        let reuse_key =
            NativeVnUiFrameReuseKey::from_request(&request, &instance_id, text_reveal_progress);
        let pointer_position = pointer_position_after_events(
            self.ui_frame_reuse
                .as_ref()
                .and_then(|reuse| reuse.pointer_position),
            &request.input.events,
        );
        self.ui_text_measurer.begin_frame()?;
        let mut output = self.ui_backend.render_frame(request)?;
        self.ui_backend_started = true;
        let measured_text_layouts = self.ui_text_measurer.take_frame_layouts()?;
        let text_scene_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        if !output.diagnostics.is_empty() {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_UI_DIAGNOSTIC: UI frame returned {} diagnostics",
                output.diagnostics.len()
            )));
        }
        self.ui_performance
            .record(output.performance.clone(), stable_frame)?;
        self.last_ui_performance_sample = Some(output.performance.clone());
        let root_bounds = output
            .semantics
            .nodes
            .iter()
            .find(|node| node.id == "root")
            .map(|node| node.bounds_points);
        tracing::debug!(
            event = "player.vn.ui.frame_layout",
            view_id = %active_view_id,
            generation = self.ui_generation,
            active_changed,
            semantic_count = output.semantics.nodes.len(),
            primitive_count = output.render.primitives.len(),
            texture_upload_count = output.render.textures.uploads.len(),
            root_min_x = root_bounds.map_or(-1.0, |bounds| bounds.min.x),
            root_min_y = root_bounds.map_or(-1.0, |bounds| bounds.min.y),
            root_max_x = root_bounds.map_or(-1.0, |bounds| bounds.max.x),
            root_max_y = root_bounds.map_or(-1.0, |bounds| bounds.max.y),
            "rendered a traceable AstraVN UI semantic generation"
        );
        let draw = ui_frame_to_scene_commands(&output.render)?;
        let mut next_layout_ids = BTreeSet::new();
        let mut pending_text = Vec::new();
        // Must remain identical to Yakui's default when an authored text node
        // omits `font_size`; otherwise shaped layout identity breaks at the
        // retained-text boundary.
        let body_font_size = 20.0;
        let text_layout_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        append_ui_semantic_text(
            &self.localization,
            &measured_text_layouts,
            &mut next_layout_ids,
            &mut pending_text,
            &output.semantics,
            body_font_size,
        )?;
        host_performance.text_layout_ns = performance_phase_duration(text_layout_started)?;
        let text_resource_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let removals = self
            .live_layout_ids
            .difference(&next_layout_ids)
            .map(String::as_str)
            .collect::<Vec<_>>();
        let updates = pending_text
            .iter()
            .map(|pending| TextRenderLayoutUpdate {
                layout_id: &pending.layout_id,
                layout: pending.layout.as_ref(),
                shared_layout: Some(&pending.layout),
                rgba: pending.rgba,
                translation: (pending.aligned_x, pending.aligned_y),
            })
            .collect::<Vec<_>>();
        let text_frame = self.text_resources.update_frame(&updates, &removals)?;
        host_performance.text_resource_ns = performance_phase_duration(text_resource_started)?;
        let text_compose_started =
            performance_phase_started(self.ui_host_performance_sampling_enabled);
        let mut text_draw = Vec::new();
        for (pending, draw) in pending_text.iter().zip(text_frame.layouts) {
            if pending.layout_id != draw.layout_id {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_LAYOUT_ORDER: text resource owner changed frame draw order"
                        .into(),
                ));
            }
            if let Some(rect) = pending.clip {
                text_draw.push(SceneCommand::PushClip { rect });
            }
            text_draw.extend(draw.commands);
            if pending.clip.is_some() {
                text_draw.push(SceneCommand::PopClip);
            }
        }
        let mut composed_draw = text_frame.lifecycle;
        composed_draw.extend(draw);
        composed_draw.extend(text_draw);
        self.live_layout_ids = next_layout_ids;
        host_performance.text_compose_ns = performance_phase_duration(text_compose_started)?;
        if self
            .ui_animations
            .values()
            .any(|animation| animation.progress_millionths(fixed_time_ns) < 1_000_000)
        {
            output.repaint_after_ns = Some(
                output
                    .repaint_after_ns
                    .unwrap_or(16_666_667)
                    .min(16_666_667),
            );
        }
        self.ui_semantics = Some(output.semantics.clone());
        self.ui_frame_reuse = if output.repaint_after_ns.is_none() && output.actions.is_empty() {
            Some(NativeVnUiFrameReuse {
                key: reuse_key,
                pointer_position,
                performance: output.performance.clone(),
            })
        } else {
            None
        };
        host_performance.text_scene_ns = performance_phase_duration(text_scene_started)?;
        if let Some(command_id) = text_layout_command {
            self.stage_director
                .complete_text_layout(&command_id)
                .map_err(stage_director_error)?;
        }
        self.accumulate_ui_host_performance(host_performance);
        Ok(NativeVnUiFrameResult {
            actions: output.actions,
            dispositions: output.dispositions,
            semantics: output.semantics,
            draw: composed_draw,
        })
    }
}
