use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub fn dispatch_ui_event(
        &mut self,
        kind: UiInputEventKind,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        let event = self.next_ui_event(kind)?;
        self.dispatch_ui_events(vec![event])
    }

    pub(super) fn next_ui_event(
        &mut self,
        kind: UiInputEventKind,
    ) -> Result<UiInputEvent, NativeVnHostError> {
        self.ui_input_sequence = self
            .ui_input_sequence
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        Ok(UiInputEvent {
            sequence: self.ui_input_sequence,
            kind,
        })
    }

    pub(super) fn dispatch_ui_events(
        &mut self,
        events: Vec<UiInputEvent>,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        self.ensure_presentation_active()?;
        let events = self.map_window_input(events)?;
        if self.handle_quick_slot_input(&events)? {
            return self.present_current_scene(self.ui_draw.clone());
        }
        self.last_ui_performance_sample = None;
        self.last_ui_host_performance_sample = self
            .ui_host_performance_sampling_enabled
            .then(NativeVnUiHostPerformanceSample::default);
        if events
            .iter()
            .any(|event| matches!(event.kind, UiInputEventKind::FixedTime { .. }))
        {
            self.ensure_text_region()?;
        }
        let fallback_events = events.clone();
        if !self.has_active_ui_surface() {
            let semantic_generation = self
                .ui_semantics
                .as_ref()
                .map(|snapshot| snapshot.generation)
                .unwrap_or(self.ui_generation);
            return if let Some(action) =
                self.bubbled_ui_action(&fallback_events, semantic_generation)
            {
                self.dispatch_ui_action(&action)
            } else if let Some(command) = self.bubbled_ui_command(&fallback_events) {
                self.command(command)
            } else {
                self.present_current_scene(self.ui_draw.clone())
            };
        }
        if let Some(action) = self.retained_ui_activation(&events)? {
            let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
            let result = self.dispatch_ui_action(&action);
            let duration = performance_phase_duration(started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.action_dispatch_ns = sample.action_dispatch_ns.saturating_add(duration);
            }
            return result;
        }
        let mut frame = self.render_ui(events)?;
        if frame.actions.len() > 1 {
            return Err(NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_ACTION_AMBIGUOUS: one input frame emitted multiple actions".into(),
            ));
        }
        // Layout has already updated the resource owner. Preserve its one-shot
        // mutations even when an action replaces this frame before presentation.
        frame.draw.retain(|command| {
            if is_ui_resource_lifecycle(command) {
                self.pending_gpu_lifecycle.push(command.clone());
                false
            } else {
                true
            }
        });
        self.ui_draw = frame.draw.clone();
        if let Some(action) = frame.actions.first() {
            let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
            let result = self.dispatch_ui_action(action);
            let duration = performance_phase_duration(started)?;
            if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
                sample.action_dispatch_ns = sample.action_dispatch_ns.saturating_add(duration);
            }
            return result;
        }
        let consumed = frame
            .dispositions
            .iter()
            .any(|item| item.disposition == UiInputDispositionKind::Consumed);
        if !consumed {
            if let Some(action) =
                self.bubbled_ui_action(&fallback_events, frame.semantics.generation)
            {
                return self.dispatch_ui_action(&action);
            }
            if let Some(command) = self.bubbled_ui_command(&fallback_events) {
                return self.command(command);
            }
        }
        let started = performance_phase_started(self.ui_host_performance_sampling_enabled);
        let result = self.present_current_scene(frame.draw);
        let duration = performance_phase_duration(started)?;
        if let Some(sample) = self.last_ui_host_performance_sample.as_mut() {
            sample.present_scene_ns = sample.present_scene_ns.saturating_add(duration);
        }
        result
    }

    pub(super) fn retained_ui_activation(
        &mut self,
        events: &[UiInputEvent],
    ) -> Result<Option<astra_ui_core::UiActionEnvelope>, NativeVnHostError> {
        let [event] = events else {
            return Ok(None);
        };
        let explicit_target = match &event.kind {
            UiInputEventKind::AccessibilityAction {
                semantic_id,
                action,
                ..
            } if matches!(action.as_str(), "activate" | "invoke") => Some(semantic_id.as_str()),
            UiInputEventKind::Keyboard {
                logical_key,
                state: UiButtonState::Pressed,
                repeat: false,
                ..
            } if matches!(logical_key.as_str(), "Enter" | " " | "Space") => None,
            UiInputEventKind::Navigation {
                action: astra_ui_core::UiNavigationAction::Activate,
            } => None,
            UiInputEventKind::PointerButton {
                button: astra_ui_core::UiPointerButton::Primary,
                state: UiButtonState::Pressed,
                position,
            } if self.ui_frame_reuse.is_some() => {
                let semantics = self.ui_semantics.as_ref().ok_or_else(|| {
                    NativeVnHostError::Input(
                        "ASTRA_PLAYER_UI_RETAINED_SEMANTICS_MISSING: active UI surface has no semantic snapshot"
                            .into(),
                    )
                })?;
                let target = retained_pointer_activation_target(semantics, *position);
                let Some(target) = target else {
                    return Ok(None);
                };
                Some(target.id.as_str())
            }
            _ => return Ok(None),
        };
        let semantics = self.ui_semantics.as_ref().ok_or_else(|| {
            NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_RETAINED_SEMANTICS_MISSING: active UI surface has no semantic snapshot"
                    .into(),
            )
        })?;
        let target = if let Some(explicit_target) = explicit_target {
            semantics
                .nodes
                .iter()
                .find(|node| node.id == explicit_target)
        } else {
            semantics.nodes.iter().find(|node| node.focused)
        };
        let Some(target) = target else {
            return Ok(None);
        };
        if !target.enabled
            || target.hidden
            || !target
                .actions
                .contains(&astra_ui_core::UiSemanticAction::Activate)
        {
            return Ok(None);
        }
        self.ui_backend
            .renderer_mut()
            .resolve_retained_activation(semantics.generation, &target.id, event.sequence)
            .map(Some)
            .map_err(NativeVnHostError::Ui)
    }

    pub(super) fn has_active_ui_surface(&self) -> bool {
        self.runtime_state.as_ref().is_some_and(|state| {
            state.pending_choice.is_some()
                || !state.system_stack.is_empty()
                || state.pending_wait.as_ref().map(|wait| wait.kind) == Some(VnWaitKind::Dialogue)
        })
    }

    pub(super) fn bubbled_ui_command(&self, events: &[UiInputEvent]) -> Option<VnPlayerCommand> {
        let state = self.runtime_state.as_ref()?;
        for event in events.iter().rev() {
            match &event.kind {
                UiInputEventKind::Keyboard {
                    logical_key,
                    state: UiButtonState::Pressed,
                    ..
                } if state.pending_choice.is_none()
                    && state.system_stack.is_empty()
                    && matches!(logical_key.as_str(), "Enter" | " " | "Space") =>
                {
                    return Some(VnPlayerCommand::Advance);
                }
                UiInputEventKind::Navigation {
                    action: astra_ui_core::UiNavigationAction::Activate,
                } if state.pending_choice.is_none() && state.system_stack.is_empty() => {
                    return Some(VnPlayerCommand::Advance);
                }
                UiInputEventKind::PointerButton {
                    button: astra_ui_core::UiPointerButton::Primary,
                    state: UiButtonState::Pressed,
                    ..
                } if state.pending_choice.is_none() && state.system_stack.is_empty() => {
                    return Some(VnPlayerCommand::Advance);
                }
                UiInputEventKind::Navigation {
                    action: astra_ui_core::UiNavigationAction::Cancel,
                } if !state.system_stack.is_empty() => {
                    return Some(VnPlayerCommand::ReturnSystem);
                }
                UiInputEventKind::Keyboard {
                    logical_key,
                    state: UiButtonState::Pressed,
                    ..
                } if !state.system_stack.is_empty()
                    && matches!(logical_key.as_str(), "Escape" | "Esc") =>
                {
                    return Some(VnPlayerCommand::ReturnSystem);
                }
                _ => {}
            }
        }
        None
    }

    pub(super) fn bubbled_ui_action(
        &self,
        events: &[UiInputEvent],
        semantic_generation: u64,
    ) -> Option<astra_ui_core::UiActionEnvelope> {
        let state = self.runtime_state.as_ref()?;
        if !state.system_stack.is_empty() {
            return None;
        }
        let event = events.iter().rev().find(|event| {
            matches!(
                event.kind,
                UiInputEventKind::PointerButton {
                    button: astra_ui_core::UiPointerButton::Secondary,
                    state: UiButtonState::Pressed,
                    ..
                }
            )
        })?;
        let page = "quick_panel";
        tracing::debug!(
            event = "vn.ui.secondary_shortcut",
            input_sequence = event.sequence,
            semantic_target_id = "root",
            page,
            profile = %self.ui_profile,
            "secondary pointer input resolved through the UI action router"
        );
        Some(astra_ui_core::UiActionEnvelope {
            schema: "astra.ui_action_envelope.v1".into(),
            input_sequence: event.sequence,
            semantic_target_id: "root".into(),
            action_id: "vn.open_system".into(),
            arguments: BTreeMap::from([("page".into(), UiValue::String(page.into()))]),
            semantic_generation,
        })
    }

    pub(super) fn dispatch_ui_action(
        &mut self,
        action: &astra_ui_core::UiActionEnvelope,
    ) -> Result<PlayerHostCommandBatch, NativeVnHostError> {
        if action.action_id.starts_with("ui.") {
            let controller = self.active_ui_controller.clone().ok_or_else(|| {
                NativeVnHostError::Input(
                    "ASTRA_PLAYER_UI_CONTROLLER_CONTEXT: local action arrived without a live controller context"
                        .into(),
                )
            })?;
            let effect = match action.action_id.as_str() {
                "ui.open_modal" => VnUiControllerEffect::OpenModal {
                    view_id: ui_string_argument(action, "view_id")?.to_string(),
                    model: action
                        .arguments
                        .get("model")
                        .cloned()
                        .unwrap_or_else(|| UiValue::Map(BTreeMap::new())),
                },
                "ui.close_modal" => VnUiControllerEffect::CloseModal,
                "ui.set_state" => VnUiControllerEffect::SetSessionState {
                    key: ui_string_argument(action, "key")?.to_string(),
                    value: action.arguments.get("value").cloned().ok_or_else(|| {
                        NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: value is missing".into(),
                        )
                    })?,
                },
                unsupported => {
                    return Err(NativeVnHostError::Input(format!(
                        "ASTRA_PLAYER_UI_LOCAL_ACTION_UNSUPPORTED: action {unsupported} is not registered"
                    )))
                }
            };
            self.ui_controller_sessions
                .entry(controller.controller_id.clone())
                .or_default()
                .apply(std::slice::from_ref(&effect))
                .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
            self.apply_ui_controller_effects(&controller.controller_id, vec![effect], false)?;
            return self.present_current_scene(self.ui_draw.clone());
        }
        let typed_action = match action.action_id.as_str() {
            "vn.advance" => VnUiAction::Advance,
            "vn.choose" => VnUiAction::Choose {
                option_id: ui_string_argument(action, "option_id")?.to_string(),
            },
            "vn.open_system" => VnUiAction::OpenSystem {
                page: SystemPageKind::parse(ui_string_argument(action, "page")?),
            },
            "vn.switch_system" => VnUiAction::SwitchSystemPage {
                page: SystemPageKind::parse(ui_string_argument(action, "page")?),
            },
            "vn.return_system" => VnUiAction::ReturnSystem,
            "vn.request_exit" => VnUiAction::RequestExit,
            "vn.set_config" => VnUiAction::SetConfig {
                key: ui_string_argument(action, "key")?.to_string(),
                value: action.arguments.get("value").cloned().ok_or_else(|| {
                    NativeVnHostError::Input(
                        "ASTRA_PLAYER_UI_ACTION_ARGUMENT: value is missing".into(),
                    )
                })?,
            },
            "vn.set_auto" => VnUiAction::SetAuto {
                enabled: ui_bool_argument(action, "enabled")?,
            },
            "vn.set_skip" => VnUiAction::SetSkip {
                mode: match ui_string_argument(action, "mode")? {
                    "none" => astra_vn_core::SkipMode::None,
                    "read" => astra_vn_core::SkipMode::Read,
                    "all" => astra_vn_core::SkipMode::All,
                    _ => {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: skip mode must be none, read or all"
                                .into(),
                        ))
                    }
                },
            },
            "vn.set_reading_mode" => VnUiAction::SetReadingMode {
                mode: match ui_string_argument(action, "mode")? {
                    "hidden" => ReadingMode::Hidden,
                    "manual" => ReadingMode::Manual,
                    "fast_forward" => ReadingMode::FastForward,
                    _ => {
                        return Err(NativeVnHostError::Input(
                            "ASTRA_PLAYER_READING_MODE_UNKNOWN: reading mode must be hidden, manual or fast_forward".into(),
                        ))
                    }
                },
            },
            "vn.set_audio_enabled" => VnUiAction::SetAudioEnabled {
                enabled: ui_bool_argument(action, "enabled")?,
            },
            "vn.invoke_system_action" => VnUiAction::InvokeSystemAction {
                action_id: ui_string_argument(action, "action_id")?.to_string(),
            },
            "vn.replay_voice" => VnUiAction::ReplayVoice {
                voice_id: ui_string_argument(action, "voice_id")?.to_string(),
            },
            "vn.request_save" => VnUiAction::RequestSave {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.request_save_confirmed" => VnUiAction::RequestSaveConfirmed {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.request_load" => VnUiAction::RequestLoad {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.request_delete_save" => VnUiAction::RequestDeleteSave {
                slot_id: ui_string_argument(action, "slot_id")?.to_string(),
            },
            "vn.start_replay" => VnUiAction::StartReplay {
                replay_id: ui_string_argument(action, "replay_id")?.to_string(),
            },
            "vn.preview_gallery" => VnUiAction::PreviewGallery {
                item_id: ui_string_argument(action, "item_id")?.to_string(),
            },
            "vn.request_route_jump" => VnUiAction::RequestRouteJump {
                node_id: ui_string_argument(action, "node_id")?.to_string(),
            },
            "vn.request_backlog_jump" => VnUiAction::RequestBacklogJump {
                command_id: ui_string_argument(action, "command_id")?.to_string(),
            },
            "vn.submit_text" => VnUiAction::SubmitText {
                input_id: ui_string_argument(action, "input_id")?.to_string(),
                value: ui_string_argument(action, "value")?.to_string(),
            },
            unsupported => {
                return Err(NativeVnHostError::Input(format!(
                    "ASTRA_PLAYER_UI_ACTION_UNSUPPORTED: action {unsupported} is not routed by the product host"
                )))
            }
        };
        if self.active_ui_controller.is_none() {
            return match typed_action {
                VnUiAction::OpenSystem { page } => {
                    self.require_system_page(page)?;
                    tracing::debug!(
                        event = "vn.ui.global_action.forwarded",
                        action_id = "vn.open_system",
                        semantic_target_id = %action.semantic_target_id,
                        input_sequence = action.input_sequence,
                        "forwarded an explicitly allowed global UI action without a page controller"
                    );
                    self.command(VnPlayerCommand::OpenSystem { page })
                }
                _ => Err(NativeVnHostError::Input(
                    "ASTRA_PLAYER_UI_CONTROLLER_CONTEXT: only the global open-system action is allowed without a live controller context"
                        .into(),
                )),
            };
        }
        let controller = self.active_ui_controller.clone().ok_or_else(|| {
            NativeVnHostError::Input(
                "ASTRA_PLAYER_UI_CONTROLLER_CONTEXT: action arrived without a live controller context"
                    .into(),
            )
        })?;
        let effects = self
            .ui_controller_host
            .invoke_action(
                &controller.controller_id,
                &controller.model_schema,
                &controller.model,
                &typed_action,
                self.ui_controller_sessions
                    .entry(controller.controller_id.clone())
                    .or_default(),
            )
            .map_err(|error| NativeVnHostError::Input(error.to_string()))?;
        let forwarded =
            self.apply_ui_controller_effects(&controller.controller_id, effects, true)?;
        let Some(product_action) = forwarded else {
            return self.present_current_scene(self.ui_draw.clone());
        };
        tracing::trace!(
            event = "vn.ui.action.forwarded",
            action_id = %action.action_id,
            semantic_target_id = %action.semantic_target_id,
            stable_target_id = %product_action.stable_target(),
            controller_id = %controller.controller_id,
            input_sequence = action.input_sequence,
            "forwarded a typed UI action to the product command router"
        );
        let requested_locale = match &product_action {
            VnUiAction::SetConfig { key, value } if key == "display.language" => {
                Some(ui_value_to_scalar(value)?)
            }
            _ => None,
        };
        if matches!(product_action, VnUiAction::RequestExit) {
            self.exit_requested = true;
            tracing::info!(
                event = "player.vn.exit.requested",
                controller_id = %controller.controller_id,
                input_sequence = action.input_sequence,
                "accepted a typed UI request to close the Player session"
            );
            return self.present_current_scene(self.ui_draw.clone());
        }
        let command = match product_action {
            VnUiAction::Advance => VnPlayerCommand::Advance,
            VnUiAction::Choose { option_id } => VnPlayerCommand::Choose { option_id },
            VnUiAction::OpenSystem { page } => {
                self.require_system_page(page)?;
                VnPlayerCommand::OpenSystem { page }
            }
            VnUiAction::SwitchSystemPage { page } => {
                self.require_system_page(page)?;
                VnPlayerCommand::SwitchSystemPage { page }
            }
            VnUiAction::ReturnSystem => VnPlayerCommand::ReturnSystem,
            VnUiAction::RequestExit => {
                unreachable!("exit requests are handled before runtime routing")
            }
            VnUiAction::SetConfig { key, value } => VnPlayerCommand::SetConfig {
                key,
                value: ui_value_to_scalar(&value)?,
            },
            VnUiAction::SetAuto { enabled } => VnPlayerCommand::SetAuto { enabled },
            VnUiAction::SetSkip { mode } => VnPlayerCommand::SetSkip { mode },
            VnUiAction::SetReadingMode { mode } => {
                if !self.system_ui_policy.reading_modes.contains(&mode) {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_READING_MODE_UNDECLARED: UI requested a reading mode outside the bound profile"
                            .into(),
                    ));
                }
                VnPlayerCommand::SetReadingMode { mode }
            }
            VnUiAction::SetAudioEnabled { enabled } => {
                if !self.system_ui_policy.audio_toggle {
                    return Err(NativeVnHostError::Input(
                        "ASTRA_PLAYER_AUDIO_TOGGLE_UNDECLARED: UI requested audio enable state outside the bound profile"
                            .into(),
                    ));
                }
                VnPlayerCommand::SetAudioEnabled { enabled }
            }
            VnUiAction::InvokeSystemAction { action_id } => {
                if !self.system_ui_policy.custom_action_ids.contains(&action_id) {
                    return Err(NativeVnHostError::Input(format!(
                        "ASTRA_PLAYER_SYSTEM_ACTION_UNDECLARED: action {action_id} is not declared by profile {}",
                        self.ui_profile
                    )));
                }
                VnPlayerCommand::InvokeSystemAction { action_id }
            }
            VnUiAction::ReplayVoice { voice_id } => {
                VnPlayerCommand::ReplayVoice { voice: voice_id }
            }
            VnUiAction::RequestSave { slot_id } => {
                return self.queue_ui_save_request(slot_id, true)
            }
            VnUiAction::RequestSaveConfirmed { slot_id } => {
                return self.queue_ui_save_request(slot_id, true)
            }
            VnUiAction::RequestLoad { slot_id } => {
                return self.queue_ui_save_request(slot_id, false)
            }
            VnUiAction::RequestDeleteSave { slot_id } => {
                return self.queue_ui_delete_request(slot_id)
            }
            VnUiAction::StartReplay { replay_id } => VnPlayerCommand::StartReplay { replay_id },
            VnUiAction::PreviewGallery { item_id } => VnPlayerCommand::PreviewGallery { item_id },
            VnUiAction::RequestRouteJump { node_id } => VnPlayerCommand::JumpRoute { node_id },
            VnUiAction::RequestBacklogJump { command_id } => {
                VnPlayerCommand::JumpBacklog { command_id }
            }
            VnUiAction::SubmitText { input_id, value } => {
                VnPlayerCommand::SubmitText { input_id, value }
            }
        };
        let previous_locale = if let Some(locale) = requested_locale.as_deref() {
            let previous = self.localization.locale.clone();
            self.activate_locale(locale)?;
            Some(previous)
        } else {
            None
        };
        match self.command(command) {
            Ok(batch) => {
                if let Some(locale) = requested_locale {
                    tracing::info!(
                        event = "player.vn.locale.changed",
                        locale,
                        "activated a packaged locale through the typed config action"
                    );
                }
                Ok(batch)
            }
            Err(error) => {
                if let Some(previous) = previous_locale {
                    self.activate_locale(&previous)?;
                }
                Err(error)
            }
        }
    }
}
