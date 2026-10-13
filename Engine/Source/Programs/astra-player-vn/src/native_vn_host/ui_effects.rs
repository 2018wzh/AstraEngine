use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub(super) fn activate_locale(&mut self, locale: &str) -> Result<(), NativeVnHostError> {
        let localization = self.localizations.get(locale).cloned().ok_or_else(|| {
            NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_LOCALE_UNDECLARED: locale {locale} is not packaged"
            ))
        })?;
        *self.ui_text_locale.write().map_err(|_| {
            NativeVnHostError::Localization(
                "ASTRA_PLAYER_LOCALE_LOCK: UI text locale state was poisoned".into(),
            )
        })? = locale.to_string();
        let font_families = ordered_ui_font_families(&self.available_font_families, locale);
        *self.ui_text_font_families.write().map_err(|_| {
            NativeVnHostError::Localization(
                "ASTRA_PLAYER_FONT_FALLBACK_LOCK: UI font fallback state was poisoned".into(),
            )
        })? = font_families.clone();
        self.font_families = font_families;
        self.localization = localization;
        self.localization_keys = self.localization.strings.keys().cloned().collect();
        Ok(())
    }

    pub(super) fn apply_ui_controller_effects(
        &mut self,
        origin_controller_id: &str,
        effects: Vec<VnUiControllerEffect>,
        allow_forward: bool,
    ) -> Result<Option<VnUiAction>, NativeVnHostError> {
        let mut queue = effects
            .into_iter()
            .map(|effect| (origin_controller_id.to_string(), effect))
            .collect::<VecDeque<_>>();
        let mut processed = 0usize;
        let mut forwarded = None;
        while let Some((controller_id, effect)) = queue.pop_front() {
            processed += 1;
            if processed > MAX_EFFECTS_PER_CALL {
                return Err(NativeVnHostError::Input(
                    "ASTRA_PLAYER_UI_CONTROLLER_EFFECT_LIMIT: recursive controller effects exceeded the per-call limit"
                        .into(),
                ));
            }
            match effect {
                VnUiControllerEffect::Forward { action } if allow_forward => {
                    if forwarded.replace(action).is_some() {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_CONTROLLER_FORWARD_DUPLICATE: an action callback may forward at most one product action"
                                .into(),
                        ));
                    }
                }
                VnUiControllerEffect::Forward { .. } => {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_UI_CONTROLLER_FORWARD_AUTHORITY: lifecycle callbacks cannot forward product actions"
                            .into(),
                    ));
                }
                VnUiControllerEffect::OpenModal { view_id, model } => {
                    if self.ui_modals.len() >= MAX_MODAL_DEPTH {
                        return Err(NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_DEPTH: modal stack exceeds {MAX_MODAL_DEPTH}"
                        )));
                    }
                    let view = self.ui_blueprints.views.get(&view_id).ok_or_else(|| {
                        NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_VIEW_MISSING: view {view_id} is not packaged"
                        ))
                    })?;
                    if self.base_ui_theme_id.as_deref() != Some(view.theme_id.as_str()) {
                        return Err(NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_THEME: modal view {view_id} must use the active profile theme"
                        )));
                    }
                    let matching = self
                        .ui_controller_host
                        .manifests()
                        .filter(|manifest| manifest.view == view_id)
                        .cloned()
                        .collect::<Vec<_>>();
                    if matching.len() != 1 {
                        return Err(NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_MODAL_CONTROLLER_BINDING: view {view_id} resolves to {} controllers instead of exactly one",
                            matching.len()
                        )));
                    }
                    let manifest = matching.into_iter().next().ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_MODAL_CONTROLLER_BINDING: modal controller disappeared"
                                .into(),
                        )
                    })?;
                    if manifest.model_schema != view.model_schema {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_MODAL_MODEL_SCHEMA: modal controller and view schemas differ"
                                .into(),
                        ));
                    }
                    let instance_id = format!(
                        "modal.{}.{}.{}",
                        self.ui_generation,
                        self.ui_modals.len(),
                        view_id
                    );
                    let modal = ActiveUiModal {
                        instance_id,
                        controller_id: manifest.id.clone(),
                        view_id,
                        model_schema: manifest.model_schema.clone(),
                        model,
                    };
                    self.ui_modals.push(modal.clone());
                    let open_effects = self
                        .ui_controller_host
                        .invoke_open(
                            &modal.controller_id,
                            &modal.model_schema,
                            &modal.model,
                            self.ui_controller_sessions
                                .entry(modal.controller_id.clone())
                                .or_default(),
                        )
                        .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
                    queue.extend(
                        open_effects
                            .into_iter()
                            .map(|effect| (modal.controller_id.clone(), effect)),
                    );
                }
                VnUiControllerEffect::CloseModal => {
                    if self.ui_modals.pop().is_none() {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_MODAL_UNDERFLOW: close_modal requires a live modal"
                                .into(),
                        ));
                    }
                    self.pending_ui_focus = None;
                    self.ui_animations.clear();
                }
                VnUiControllerEffect::Focus { semantic_id } => {
                    if self
                        .pending_ui_focus
                        .as_ref()
                        .is_some_and(|pending| pending != &semantic_id)
                    {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_FOCUS_CONFLICT: one effect transaction requested multiple focus targets"
                                .into(),
                        ));
                    }
                    self.pending_ui_focus = Some(semantic_id);
                }
                VnUiControllerEffect::SetSessionState { .. } => {}
                VnUiControllerEffect::Animation {
                    target_id,
                    preset_id,
                } => {
                    let controller = self
                        .ui_controller_host
                        .manifest(&controller_id)
                        .ok_or_else(|| {
                            NativeVnHostError::Input(
                                "ASTRA_PLAYER_UI_ANIMATION_CONTROLLER: origin controller is not registered"
                                    .into(),
                            )
                        })?;
                    let view = self
                        .ui_blueprints
                        .views
                        .get(&controller.view)
                        .ok_or_else(|| {
                            NativeVnHostError::Input(
                                "ASTRA_PLAYER_UI_ANIMATION_VIEW: origin view is not packaged"
                                    .into(),
                            )
                        })?;
                    let theme = self.ui_themes.get(&view.theme_id).ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ANIMATION_THEME: origin theme is not packaged".into(),
                        )
                    })?;
                    let motion = theme.tokens.get(&preset_id).ok_or_else(|| {
                        NativeVnHostError::Input(format!(
                            "ASTRA_PLAYER_UI_ANIMATION_PRESET: preset {preset_id} is absent from theme {}",
                            theme.id
                        ))
                    })?;
                    let UiThemeValue::Motion { duration_ms, .. } = motion else {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ANIMATION_PRESET: animation preset must reference a motion token"
                                .into(),
                        ));
                    };
                    let fixed_time_ns = self.fixed_step.saturating_mul(16_666_667);
                    self.ui_animations.insert(
                        target_id.clone(),
                        ActiveUiAnimation {
                            target_id,
                            preset_id,
                            started_at_ns: fixed_time_ns,
                            duration_ns: u64::from(*duration_ms).saturating_mul(1_000_000),
                        },
                    );
                }
                VnUiControllerEffect::Trace { event, fields } => {
                    tracing::debug!(
                        target: "astra_player_vn::ui_controller",
                        event = "vn.ui.controller.trace",
                        controller_id = %controller_id,
                        controller_event = %event,
                        field_count = fields.len()
                    );
                }
            }
        }
        Ok(forwarded)
    }

    pub(super) fn queue_ui_save_request(
        &mut self,
        slot_id: String,
        saving: bool,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let slot = self.ui_save_slots.get(&slot_id).ok_or_else(|| {
            NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            ))
        })?;
        if saving && !slot.can_write {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_READ_ONLY: {slot_id}"
            )));
        }
        if !saving && (!slot.occupied || !slot.can_load) {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_EMPTY: {slot_id}"
            )));
        }
        if self.pending_ui_host_request.is_some() {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_HOST_REQUEST_CONFLICT: a host request is already pending".into(),
            ));
        }
        self.pending_ui_host_request = Some(if saving {
            self.pending_save_completion = Some(self.system_ui_policy.save_completion);
            VnUiHostRequest::Save {
                slot_id,
                completion: self.system_ui_policy.save_completion,
            }
        } else {
            VnUiHostRequest::Load { slot_id }
        });
        self.present_current_scene(self.ui_draw.clone())
    }

    pub(super) fn require_system_page(
        &self,
        page: SystemPageKind,
    ) -> Result<(), NativeVnHostError> {
        if page == SystemPageKind::Unknown || !self.system_ui_policy.allowed_pages.contains(&page) {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SYSTEM_PAGE_UNDECLARED: page {page:?} is not allowed by profile {}",
                self.ui_profile
            )));
        }
        Ok(())
    }

    pub(super) fn queue_ui_delete_request(
        &mut self,
        slot_id: String,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let slot = self.ui_save_slots.get(&slot_id).ok_or_else(|| {
            NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_UNKNOWN: UI requested undeclared slot {slot_id}"
            ))
        })?;
        if !slot.occupied {
            return Err(NativeVnHostError::Input(format!(
                "ASTRA_PLAYER_SAVE_SLOT_EMPTY: {slot_id}"
            )));
        }
        if self.pending_ui_host_request.is_some() {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_HOST_REQUEST_BUSY: another save operation is pending".into(),
            ));
        }
        self.pending_ui_host_request = Some(VnUiHostRequest::Delete { slot_id });
        self.present_current_scene(self.ui_draw.clone())
    }

    pub(super) fn present_current_scene(
        &mut self,
        ui_draw: Vec<SceneCommand>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if self.gpu_resources_invalidated {
            self.pending_gpu_lifecycle.extend(
                ui_draw
                    .iter()
                    .filter(|command| is_ui_resource_lifecycle(command))
                    .cloned(),
            );
            self.ui_draw = reusable_ui_draw_commands(&ui_draw);
            return self.render_with_stage_refresh(&[], 0, true);
        }
        let mut commands = vec![SceneCommand::rect(
            "vn.frame.clear",
            0,
            0,
            self.width,
            self.height,
            [8, 10, 16, 255],
        )];
        commands.append(&mut self.pending_gpu_lifecycle);
        commands.extend(self.scene_draw.iter().cloned());
        commands.extend(ui_draw.iter().cloned());
        // Retain only the UI layer. Keeping the fully composed frame here caused
        // every resize/focus repaint to recursively append the previous clear and
        // scene layers, producing duplicate resource identities on the WGPU path.
        self.ui_draw = reusable_ui_draw_commands(&ui_draw);
        Ok(PlayerHostCommandBatch::new(vec![
            PlayerHostCommand::PresentScene {
                sequence: self.next_command_sequence()?,
                surface: self.surface,
                width: self.output_extent.width,
                height: self.output_extent.height,
                clear_rgba: [0, 0, 0, 255],
                commands: self.map_output_scene(commands)?,
                semantics: self.output_semantics()?,
            },
        ])?)
    }

    pub(super) fn accumulate_ui_host_performance(
        &mut self,
        incoming: NativeVnUiHostPerformanceSample,
    ) {
        let Some(sample) = self.last_ui_host_performance_sample.as_mut() else {
            return;
        };
        sample.model_binding_ns = sample
            .model_binding_ns
            .saturating_add(incoming.model_binding_ns);
        sample.controller_ns = sample.controller_ns.saturating_add(incoming.controller_ns);
        sample.frame_model_ns = sample
            .frame_model_ns
            .saturating_add(incoming.frame_model_ns);
        sample.text_scene_ns = sample.text_scene_ns.saturating_add(incoming.text_scene_ns);
        sample.text_layout_ns = sample
            .text_layout_ns
            .saturating_add(incoming.text_layout_ns);
        sample.text_resource_ns = sample
            .text_resource_ns
            .saturating_add(incoming.text_resource_ns);
        sample.text_compose_ns = sample
            .text_compose_ns
            .saturating_add(incoming.text_compose_ns);
        sample.action_dispatch_ns = sample
            .action_dispatch_ns
            .saturating_add(incoming.action_dispatch_ns);
        sample.present_scene_ns = sample
            .present_scene_ns
            .saturating_add(incoming.present_scene_ns);
        sample.runtime_host_step_ns = sample
            .runtime_host_step_ns
            .saturating_add(incoming.runtime_host_step_ns);
        sample.runtime_output_decode_ns = sample
            .runtime_output_decode_ns
            .saturating_add(incoming.runtime_output_decode_ns);
        sample.runtime_render_ns = sample
            .runtime_render_ns
            .saturating_add(incoming.runtime_render_ns);
        sample.stage_prepare_ns = sample
            .stage_prepare_ns
            .saturating_add(incoming.stage_prepare_ns);
        sample.stage_scene_ns = sample
            .stage_scene_ns
            .saturating_add(incoming.stage_scene_ns);
        sample.stage_texture_ns = sample
            .stage_texture_ns
            .saturating_add(incoming.stage_texture_ns);
        sample.stage_command_ns = sample
            .stage_command_ns
            .saturating_add(incoming.stage_command_ns);
        sample.stage_lifecycle_ns = sample
            .stage_lifecycle_ns
            .saturating_add(incoming.stage_lifecycle_ns);
        sample.scene_compose_ns = sample
            .scene_compose_ns
            .saturating_add(incoming.scene_compose_ns);
    }
}
