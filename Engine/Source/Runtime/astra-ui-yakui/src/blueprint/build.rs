use super::*;

impl YakuiViewRenderer for BlueprintYakuiRenderer {
    fn request_focus(&mut self, semantic_id: String) {
        self.backend_focus_request = Some(semantic_id);
    }

    fn build(
        &mut self,
        yakui: &mut yakui_core::Yakui,
        request: &UiFrameRequest,
    ) -> Result<YakuiViewOutput, UiValidationError> {
        for managed in self.pending_removed_managed_textures.drain(..) {
            yakui.paint_dom().textures_mut().remove(managed);
        }
        let frame = &request.model;
        frame.validate()?;
        self.text_input_consumed_sequences.clear();
        self.accessibility_dispatched_events.clear();
        let mut actions = Vec::new();
        let mut force_consumed_sequences = BTreeSet::new();
        let mut accessibility_focus_request = None;
        for input in &request.input.events {
            if let UiInputEventKind::AccessibilityAction {
                semantic_id,
                action,
                value,
            } = &input.kind
            {
                let bindings = self.accessibility_actions.get(semantic_id).ok_or_else(|| {
                    UiValidationError::invalid(
                        "ASTRA_UI_ACCESSIBILITY_TARGET_MISSING",
                        "accessibility target does not exist in the live semantic generation",
                    )
                })?;
                let previous = self
                    .pending
                    .iter()
                    .find(|pending| &pending.id == semantic_id)
                    .ok_or_else(|| {
                        UiValidationError::invalid(
                            "ASTRA_UI_ACCESSIBILITY_TARGET_STALE",
                            "accessibility target is absent from the live semantic generation",
                        )
                    })?;
                let (event_name, event_value) = match action.as_str() {
                    "focus" => {
                        if !previous.actions.contains(&UiSemanticAction::Focus) {
                            return Err(UiValidationError::invalid(
                                "ASTRA_UI_ACCESSIBILITY_FOCUS_UNSUPPORTED",
                                "semantic target does not support focus",
                            ));
                        }
                        accessibility_focus_request = Some(semantic_id.clone());
                        force_consumed_sequences.insert(input.sequence);
                        continue;
                    }
                    "activate" | "invoke" if previous.role == UiSemanticRole::Toggle => (
                        "change",
                        Some(UiValue::Map(BTreeMap::from([
                            (
                                "checked".into(),
                                UiValue::Bool(!previous.checked.unwrap_or(false)),
                            ),
                            (
                                "value".into(),
                                UiValue::Bool(!previous.checked.unwrap_or(false)),
                            ),
                        ]))),
                    ),
                    "activate" | "invoke" => ("activate", None),
                    "increment" | "decrement" if previous.role == UiSemanticRole::Slider => {
                        let current = semantic_number(previous, "range.value")?;
                        let min = semantic_number(previous, "range.min")?;
                        let max = semantic_number(previous, "range.max")?;
                        let step = semantic_number(previous, "range.step")?;
                        let next = if action == "increment" {
                            (current + step).min(max)
                        } else {
                            (current - step).max(min)
                        };
                        (
                            "change",
                            Some(UiValue::Map(BTreeMap::from([(
                                "value".into(),
                                UiValue::Number(next),
                            )]))),
                        )
                    }
                    "set_value" if previous.role == UiSemanticRole::Slider => {
                        let next = value
                            .as_deref()
                            .ok_or_else(|| {
                                UiValidationError::invalid(
                                    "ASTRA_UI_ACCESSIBILITY_VALUE_MISSING",
                                    "slider set_value requires a numeric value",
                                )
                            })?
                            .parse::<f64>()
                            .map_err(|_| {
                                UiValidationError::invalid(
                                    "ASTRA_UI_ACCESSIBILITY_VALUE_INVALID",
                                    "slider accessibility value is not numeric",
                                )
                            })?;
                        let min = semantic_number(previous, "range.min")?;
                        let max = semantic_number(previous, "range.max")?;
                        if !next.is_finite() || next < min || next > max {
                            return Err(UiValidationError::invalid(
                                "ASTRA_UI_ACCESSIBILITY_VALUE_RANGE",
                                "slider accessibility value is outside its range",
                            ));
                        }
                        (
                            "change",
                            Some(UiValue::Map(BTreeMap::from([(
                                "value".into(),
                                UiValue::Number(next),
                            )]))),
                        )
                    }
                    "set_value" if previous.role == UiSemanticRole::TextInput => {
                        let next = value.as_deref().ok_or_else(|| {
                            UiValidationError::invalid(
                                "ASTRA_UI_ACCESSIBILITY_VALUE_MISSING",
                                "text input set_value requires a string value",
                            )
                        })?;
                        let editor = self.text_inputs.get_mut(semantic_id).ok_or_else(|| {
                            UiValidationError::invalid(
                                "ASTRA_UI_TEXT_INPUT_STATE_MISSING",
                                "text input accessibility state is unavailable",
                            )
                        })?;
                        let max_graphemes = previous
                            .properties
                            .get("text.max_graphemes")
                            .and_then(|value| value.parse::<usize>().ok())
                            .ok_or_else(|| {
                                UiValidationError::invalid(
                                    "ASTRA_UI_TEXT_INPUT_ACCESSIBILITY_METADATA",
                                    "text input max_graphemes metadata is invalid",
                                )
                            })?;
                        let multiline = previous
                            .properties
                            .get("text.multiline")
                            .and_then(|value| value.parse::<bool>().ok())
                            .ok_or_else(|| {
                                UiValidationError::invalid(
                                    "ASTRA_UI_TEXT_INPUT_ACCESSIBILITY_METADATA",
                                    "text input multiline metadata is invalid",
                                )
                            })?;
                        let policy = TextCharacterPolicy::parse(
                            previous
                                .properties
                                .get("text.character_policy")
                                .map(String::as_str),
                            multiline,
                        )?;
                        editor.replace_value(next, max_graphemes, policy)?;
                        (
                            "change",
                            Some(UiValue::Map(BTreeMap::from([(
                                "value".into(),
                                UiValue::String(next.to_string()),
                            )]))),
                        )
                    }
                    _ => {
                        return Err(UiValidationError::invalid(
                            "ASTRA_UI_ACCESSIBILITY_ACTION_UNSUPPORTED",
                            "accessibility action is not supported by the semantic target",
                        ));
                    }
                };
                let matching = bindings
                    .iter()
                    .filter(|(binding, _)| binding.event == event_name)
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_ACCESSIBILITY_ACTION_AMBIGUOUS",
                        "accessibility action must map to exactly one typed event binding",
                    ));
                }
                let (binding, item) = matching[0];
                self.accessibility_dispatched_events
                    .insert((semantic_id.clone(), event_name.into()));
                actions.push(action_from_event(
                    binding,
                    semantic_id,
                    request,
                    frame,
                    item.as_ref(),
                    event_value.as_ref(),
                )?);
                force_consumed_sequences.insert(input.sequence);
            }
        }
        let mut active_virtual_view_prefixes = BTreeSet::from([format!("{}#", frame.view_id)]);
        active_virtual_view_prefixes.extend(
            frame
                .modals
                .iter()
                .map(|modal| format!("{}#", modal.view_id)),
        );
        for event in &request.input.events {
            if let UiInputEventKind::Wheel { delta_points } = event.kind {
                for (key, state) in &mut self.virtual_lists {
                    if !active_virtual_view_prefixes
                        .iter()
                        .any(|prefix| key.starts_with(prefix))
                    {
                        continue;
                    }
                    state.scroll_by(-delta_points.y)?;
                }
                for (key, state) in &mut self.virtual_grids {
                    if !active_virtual_view_prefixes
                        .iter()
                        .any(|prefix| key.starts_with(prefix))
                    {
                        continue;
                    }
                    state.scroll_by(-delta_points.y)?;
                }
            }
        }
        let view = self.views.get(&frame.view_id).cloned().ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_VIEW_MISSING",
                "requested view is absent",
            )
        })?;
        if view.model_schema != request.model_schema {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BLUEPRINT_MODEL_SCHEMA",
                "frame model schema does not match the selected view",
            ));
        }
        let mut required_assets = BTreeSet::new();
        self.collect_visual_assets(&view.root, frame, None, request, &mut required_assets)?;
        for modal in &frame.modals {
            let modal_view = self.views.get(&modal.view_id).cloned().ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_MODAL_VIEW_MISSING",
                    "modal view is absent from the packaged blueprint bundle",
                )
            })?;
            let modal_frame = UiBlueprintFrameModel {
                schema: frame.schema.clone(),
                view_id: modal.view_id.clone(),
                model: modal.model.clone(),
                state: modal.state.clone(),
                modals: Vec::new(),
                focus_request: None,
                localization: frame.localization.clone(),
            };
            self.collect_visual_assets(
                &modal_view.root,
                &modal_frame,
                None,
                request,
                &mut required_assets,
            )?;
        }
        if required_assets.len() > MAX_MANAGED_IMAGE_TEXTURES {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_IMAGE_VISIBLE_BUDGET",
                "visible UI images exceed the bounded live texture cache",
            ));
        }
        for asset in required_assets {
            if self.managed_textures.get(&asset).is_some() {
                continue;
            }
            let frame = match self.image_resources.get(&asset) {
                Some(frame) => frame.clone(),
                None => self
                    .image_resource_provider
                    .as_ref()
                    .ok_or_else(|| {
                        UiValidationError::invalid(
                            "ASTRA_UI_IMAGE_RESOURCE_MISSING",
                            format!("UI blueprint references unavailable packaged asset {asset}"),
                        )
                    })?
                    .load_image(&asset)?,
            };
            let texture = Texture::new(
                TextureFormat::Rgba8Srgb,
                UVec2::new(frame.width, frame.height),
                frame.rgba8.as_ref().to_vec(),
            );
            let byte_len = frame.rgba8.len();
            let managed = yakui.add_texture(texture);
            for (_, evicted) in self.managed_textures.insert(asset, managed, byte_len)? {
                yakui.paint_dom().textures_mut().remove(evicted);
            }
        }
        self.pending.clear();
        self.accessibility_actions.clear();
        let root_id = view.root.local_id.clone();
        let mut render_error = None;
        Stack::new().show(|| {
            if let Err(error) =
                self.render_node(&view.root, None, frame, None, request, &mut actions)
            {
                render_error = Some(error);
                return;
            }
            for (index, modal) in frame.modals.iter().enumerate() {
                let modal_view = match self.views.get(&modal.view_id).cloned() {
                    Some(view) => view,
                    None => {
                        render_error = Some(UiValidationError::invalid(
                            "ASTRA_UI_MODAL_VIEW_MISSING",
                            "modal view is absent from the packaged blueprint bundle",
                        ));
                        return;
                    }
                };
                if modal_view.model_schema != modal.model_schema {
                    render_error = Some(UiValidationError::invalid(
                        "ASTRA_UI_MODAL_MODEL_SCHEMA",
                        "modal model schema does not match its packaged view",
                    ));
                    return;
                }
                let modal_frame = UiBlueprintFrameModel {
                    schema: frame.schema.clone(),
                    view_id: modal.view_id.clone(),
                    model: modal.model.clone(),
                    state: modal.state.clone(),
                    modals: Vec::new(),
                    focus_request: None,
                    localization: frame.localization.clone(),
                };
                let semantic_id = format!("{root_id}/modal.{index}");
                let mut child_error = None;
                let response = AstraNodeWidget::show(
                    AstraNodeProps {
                        semantic_id: semantic_id.clone(),
                        min_size: Vec2::ZERO,
                        max_size: Vec2::INFINITY,
                        fill: Color::rgba(0, 0, 0, 96),
                        interactive: true,
                        fill_width: true,
                        fill_height: true,
                        loose_children: true,
                        clip_children: false,
                    },
                    || {
                        child_error = self
                            .render_node(
                                &modal_view.root,
                                Some(&semantic_id),
                                &modal_frame,
                                None,
                                request,
                                &mut actions,
                            )
                            .err();
                    },
                );
                if let Some(error) = child_error {
                    render_error = Some(error);
                    return;
                }
                self.pending.push(PendingSemantic {
                    id: semantic_id,
                    parent_id: Some(root_id.clone()),
                    widget_id: response.id,
                    role: UiSemanticRole::Dialog,
                    name: None,
                    value: None,
                    enabled: true,
                    focused: response.focused,
                    selected: false,
                    checked: None,
                    actions: BTreeSet::from([UiSemanticAction::Focus, UiSemanticAction::Dismiss]),
                    properties: BTreeMap::new(),
                });
            }
        });
        if let Some(error) = render_error {
            return Err(error);
        }
        self.focused_text_input = self
            .pending
            .iter()
            .find(|pending| pending.role == UiSemanticRole::TextInput && pending.focused)
            .map(|pending| pending.id.clone());
        if !frame.modals.is_empty() {
            force_consumed_sequences.extend(request.input.events.iter().filter_map(|event| {
                (!matches!(
                    event.kind,
                    UiInputEventKind::FixedTime { .. }
                        | UiInputEventKind::Focus { .. }
                        | UiInputEventKind::Resize { .. }
                ))
                .then_some(event.sequence)
            }));
        }
        let backend_focus_request = self.backend_focus_request.take();
        let focus_request = accessibility_focus_request
            .as_ref()
            .or(backend_focus_request.as_ref())
            .or(frame.focus_request.as_ref());
        let focus_widget = if let Some(focus_request) = focus_request {
            self.semantic_focus_override = Some(focus_request.clone());
            let target = self
                .pending
                .iter()
                .find(|pending| &pending.id == focus_request)
                .ok_or_else(|| {
                    let available = self
                        .pending
                        .iter()
                        .filter(|pending| {
                            pending.enabled
                                && pending.actions.contains(&UiSemanticAction::Focus)
                        })
                        .map(|pending| pending.id.as_str())
                        .take(32)
                        .collect::<Vec<_>>()
                        .join(",");
                    UiValidationError::invalid(
                        "ASTRA_UI_FOCUS_TARGET_MISSING",
                        format!(
                            "focus target does not exist in the rendered semantic generation; requested={focus_request}; available={available}"
                        ),
                    )
                })?;
            if !target.enabled || !target.actions.contains(&UiSemanticAction::Focus) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_FOCUS_TARGET_INVALID",
                    "focus target is disabled or not focusable",
                ));
            }
            Some(target.widget_id)
        } else {
            None
        };
        self.retained_frame = Some(frame.clone());
        Ok(YakuiViewOutput {
            actions,
            repaint_after_ns: None,
            diagnostics: Vec::new(),
            instantiated_nodes: self.pending.len() as u32,
            active_texture_bytes: self.managed_textures.current_bytes() as u64,
            force_consumed_sequences: request
                .input
                .events
                .iter()
                .filter(|event| {
                    matches!(event.kind, UiInputEventKind::Wheel { .. })
                        && (!self.virtual_lists.is_empty() || !self.virtual_grids.is_empty())
                })
                .map(|event| event.sequence)
                .chain(force_consumed_sequences)
                .chain(self.text_input_consumed_sequences.iter().copied())
                .collect(),
            focus_widget,
        })
    }

    fn semantics(
        &mut self,
        yakui: &yakui_core::Yakui,
        request: &UiFrameRequest,
    ) -> Result<UiSemanticSnapshot, UiValidationError> {
        let focus_override = self.semantic_focus_override.clone();
        let raw_focused = self
            .pending
            .iter()
            .filter(|pending| pending.focused)
            .map(|pending| pending.id.as_str())
            .collect::<Vec<_>>();
        let focus_settled = focus_override
            .as_ref()
            .is_some_and(|expected| raw_focused.as_slice() == [expected.as_str()]);
        let mut nodes = Vec::with_capacity(self.pending.len());
        for pending in &self.pending {
            let layout = yakui.layout_dom().get(pending.widget_id).ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_YAKUI_SEMANTIC_LAYOUT",
                    "semantic node has no Yakui layout node",
                )
            })?;
            let rect = layout.rect;
            let mut properties = pending.properties.clone();
            crate::semantic_clip::append_layout_clip(yakui.layout_dom(), layout, &mut properties)?;
            nodes.push(UiSemanticNode {
                id: pending.id.clone(),
                parent_id: pending.parent_id.clone(),
                role: pending.role,
                bounds_points: UiRect {
                    min: UiPoint {
                        x: rect.pos().x,
                        y: rect.pos().y,
                    },
                    max: UiPoint {
                        x: rect.pos().x + rect.size().x,
                        y: rect.pos().y + rect.size().y,
                    },
                },
                name: pending.name.clone(),
                description: None,
                value: pending.value.clone(),
                enabled: pending.enabled,
                hidden: false,
                focused: focus_override.as_ref().map_or(
                    pending.focused && pending.actions.contains(&UiSemanticAction::Focus),
                    |expected| expected == &pending.id,
                ),
                selected: pending.selected,
                checked: pending.checked,
                actions: pending.actions.clone(),
                properties,
            });
        }
        let root_id = self
            .pending
            .iter()
            .find(|pending| pending.parent_id.is_none())
            .map(|pending| pending.id.clone())
            .ok_or_else(|| {
                UiValidationError::invalid("ASTRA_UI_SEMANTIC_EMPTY", "view is empty")
            })?;
        let snapshot = UiSemanticSnapshot {
            schema: "astra.ui_semantic_snapshot.v1".into(),
            session_id: request.session_id.clone(),
            generation: request.generation,
            root_id,
            nodes,
        };
        self.retained_semantic_generation = Some(snapshot.generation);
        if focus_settled {
            self.semantic_focus_override = None;
        }
        Ok(snapshot)
    }
}
