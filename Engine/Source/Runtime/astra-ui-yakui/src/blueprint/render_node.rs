use super::*;

impl BlueprintYakuiRenderer {
    pub(super) fn render_node(
        &mut self,
        node: &astra_ui_core::UiNodeBlueprint,
        parent_id: Option<&str>,
        frame: &UiBlueprintFrameModel,
        item: Option<&UiValue>,
        request: &UiFrameRequest,
        actions: &mut Vec<UiActionEnvelope>,
    ) -> Result<(), UiValidationError> {
        let visible = property_bool(node, "visible", frame, item)?.unwrap_or(true);
        if !visible {
            return Ok(());
        }
        let enabled = property_bool(node, "enabled", frame, item)?.unwrap_or(true);
        let interactive = enabled && !node.events.is_empty();
        let mut semantic_id = parent_id
            .map(|parent| format!("{parent}/{}", node.local_id))
            .unwrap_or_else(|| node.local_id.clone());
        if let Some(item) = item {
            semantic_id.push('/');
            semantic_id.push_str(&repeat_key(node, item).ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_REPEAT_STABLE_ID",
                    "repeated item has no stable semantic identity",
                )
            })?);
        }
        let mut name = property_text(node, frame, item)?;
        let mut min_width = property_number(node, "min_width", frame, item)?.unwrap_or({
            if interactive {
                180.0
            } else {
                0.0
            }
        });
        let mut min_height = property_number(node, "min_height", frame, item)?.unwrap_or({
            if interactive {
                48.0
            } else {
                0.0
            }
        });
        let max_width = property_number(node, "max_width", frame, item)?.unwrap_or(f32::INFINITY);
        let max_height = property_number(node, "max_height", frame, item)?.unwrap_or(f32::INFINITY);
        let default_fill = if matches!(
            node.widget.as_str(),
            "button" | "select" | "toggle" | "slider" | "text_input"
        ) {
            Color::rgba(38, 58, 84, 255)
        } else {
            Color::CLEAR
        };
        let selected = property_bool(node, "selected", frame, item)?.unwrap_or(false);
        let fill =
            property_color(node, "background", frame, item, request)?.unwrap_or(default_fill);
        let fill = if selected { fill.adjust(0.75) } else { fill };
        if let Some(text_key) = name.as_deref() {
            let localized_text = frame
                .localization
                .get(text_key)
                .map(String::as_str)
                .unwrap_or(text_key);
            let text =
                if let Some(visible) = property_number(node, "visible_graphemes", frame, item)? {
                    localized_text
                        .graphemes(true)
                        .take(visible.max(0.0) as usize)
                        .collect::<String>()
                } else {
                    localized_text.to_string()
                };
            let viewport_width = viewport_width_points(request).max(1.0);
            let font_size = property_number(node, "font_size", frame, item)?
                .unwrap_or_else(|| default_ui_font_size(request));
            let text_padding =
                non_negative_layout_property(node, "text_padding", frame, item)?.unwrap_or(8.0);
            let max_lines = bounded_u32_property(node, "max_lines", frame, item, 4, 1, 1_024)?;
            let direction = property_string(node, "direction", frame, item)?
                .unwrap_or_else(|| "auto".to_string());
            let measurer = self.text_measurer.as_ref().ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_TEXT_MEASURER_MISSING",
                    "text-bearing widgets require the AstraText measurement provider",
                )
            })?;
            // Text is measured against the space its node may actually occupy.  Measuring
            // against the viewport made a child in a bounded row advertise its natural
            // viewport-wide width, which in turn forced otherwise fixed dialogue/system
            // panels beyond their authored bounds.
            let content_width = max_width.min(viewport_width) - text_padding * 2.0;
            let measured = measurer.measure(&AstraTextMeasureRequest {
                semantic_id: semantic_id.clone(),
                text,
                max_width: content_width.max(1.0),
                font_size,
                max_lines,
                direction,
            })?;
            if !measured.width.is_finite()
                || !measured.height.is_finite()
                || measured.width < 0.0
                || measured.height <= 0.0
            {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXT_MEASURE_INVALID",
                    "AstraText returned invalid UI layout metrics",
                ));
            }
            // A finite authored maximum is a hard layout boundary, not a validation
            // error caused by shaped/wrapped text.  The text renderer owns clipping or
            // scrolling inside that boundary; the container must never grow past it.
            min_width = min_width.max((measured.width + text_padding * 2.0).min(max_width));
            min_height = min_height.max((measured.height + text_padding * 2.0).min(max_height));
        }
        if max_width < min_width || max_height < min_height {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_LAYOUT_SIZE_RANGE",
                "max_width/max_height must contain measured content and min_width/min_height",
            ));
        }
        let fill_layout = property_bool(node, "fill", frame, item)?.unwrap_or(false);
        let fill_width = fill_layout
            || property_bool(node, "fill_width", frame, item)?.unwrap_or(false)
            || node.widget == "screen";
        let fill_height = fill_layout
            || property_bool(node, "fill_height", frame, item)?.unwrap_or(false)
            || node.widget == "screen";
        let mut semantic_value = None;
        let mut semantic_properties = semantic_text_properties(node, frame, item)?;
        if let Some(visible) = property_number(node, "visible_graphemes", frame, item)? {
            semantic_properties.insert(
                "text.visible_graphemes".into(),
                (visible.max(0.0) as u32).to_string(),
            );
        }
        let text_color = property_color(node, "text_color", frame, item, request)?.or_else(|| {
            match request.theme.tokens.get("text.body") {
                Some(UiThemeValue::Color([r, g, b, a])) => Some(Color::rgba(*r, *g, *b, *a)),
                _ => None,
            }
        });
        if let Some(color) = text_color {
            semantic_properties.insert(
                "text.rgba".into(),
                format!("{},{},{},{}", color.r, color.g, color.b, color.a),
            );
        }
        let accessible_events = node
            .events
            .iter()
            .filter(|event| matches!(event.event.as_str(), "activate" | "change" | "submit"))
            .cloned()
            .map(|event| (event, item.cloned()))
            .collect::<Vec<_>>();
        if !accessible_events.is_empty() {
            self.accessibility_actions
                .insert(semantic_id.clone(), accessible_events);
        }
        let props = AstraNodeProps {
            semantic_id: semantic_id.clone(),
            min_size: Vec2::new(min_width, min_height),
            max_size: Vec2::new(max_width, max_height),
            fill,
            interactive,
            fill_width,
            fill_height,
            loose_children: matches!(
                node.widget.as_str(),
                "screen" | "stack" | "modal" | "canvas"
            ),
            clip_children: property_bool(node, "clip_children", frame, item)?.unwrap_or(false),
        };
        // Blueprint nodes are immutable for the lifetime of the packaged UI.
        // Borrow the validated child slice while Yakui builds this generation;
        // cloning the recursive subtree here made every text/action frame copy
        // the complete view hierarchy before layout.
        let children = &node.children;
        let parent = Some(semantic_id.as_str());
        let mut child_error = None;
        let mut changed_event = None;
        let mut submitted_event = None;
        let mut semantic_checked = None;
        if node.widget == "text_input" {
            let multiline = property_bool(node, "multiline", frame, item)?.unwrap_or(false);
            let max_graphemes =
                bounded_usize_property(node, "max_graphemes", frame, item, 1_024, 1, 65_536)?;
            let policy_name = property_string(node, "character_policy", frame, item)?;
            let policy = TextCharacterPolicy::parse(policy_name.as_deref(), multiline)?;
            let initial = name.clone().unwrap_or_default();
            let editor = self
                .text_inputs
                .entry(semantic_id.clone())
                .or_insert(TextInputState::new(initial, max_graphemes)?);
            if self.focused_text_input.as_deref() == Some(semantic_id.as_str()) {
                let update = editor.update(&request.input.events, multiline, max_graphemes, policy);
                self.text_input_consumed_sequences
                    .extend(update.consumed_sequences);
                let event_value = UiValue::Map(BTreeMap::from([(
                    "value".to_string(),
                    UiValue::String(editor.text().to_string()),
                )]));
                if update.changed {
                    changed_event = Some(event_value.clone());
                }
                if update.submitted {
                    submitted_event = Some(event_value);
                }
            }
            semantic_properties.insert("text.cursor_grapheme".into(), editor.cursor().to_string());
            semantic_properties.insert("text.multiline".into(), multiline.to_string());
            semantic_properties.insert("text.max_graphemes".into(), max_graphemes.to_string());
            semantic_properties.insert(
                "text.character_policy".into(),
                policy_name.unwrap_or_else(|| {
                    if multiline {
                        "any".to_string()
                    } else {
                        "single_line".to_string()
                    }
                }),
            );
            if let Some((start, end)) = editor.selection() {
                semantic_properties.insert("text.selection_start".into(), start.to_string());
                semantic_properties.insert("text.selection_end".into(), end.to_string());
            }
            if let Some(composition) = editor.composition() {
                semantic_properties.insert("text.composition".into(), composition.to_string());
            }
            semantic_value = Some(editor.text().to_string());
            name = semantic_value.clone();
        }
        let gap = non_negative_layout_property(node, "gap", frame, item)?.unwrap_or(0.0);
        let padding = non_negative_layout_property(node, "padding", frame, item)?.unwrap_or(0.0);
        let cross_axis_alignment = property_string(node, "cross_align", frame, item)?
            .map(|value| parse_cross_axis_alignment(&value))
            .transpose()?
            .unwrap_or(CrossAxisAlignment::Stretch);
        let build_widget = || -> Result<_, UiValidationError> {
            Ok(match node.widget.as_str() {
                "row" => AstraNodeWidget::show(props, || {
                    Pad::all(padding).show(|| {
                        List::row()
                            .item_spacing(gap)
                            .main_axis_size(MainAxisSize::Min)
                            .show(|| {
                                child_error = self
                                    .render_children(
                                        children, parent, frame, item, request, actions,
                                    )
                                    .err();
                            });
                    });
                }),
                "column" => AstraNodeWidget::show(props, || {
                    Pad::all(padding).show(|| {
                        List::column()
                            .item_spacing(gap)
                            .main_axis_size(MainAxisSize::Min)
                            .cross_axis_alignment(cross_axis_alignment)
                            .show(|| {
                                child_error = self
                                    .render_children(
                                        children, parent, frame, item, request, actions,
                                    )
                                    .err();
                            });
                    });
                }),
                "virtual_list" => AstraNodeWidget::show(props, || {
                    Pad::all(padding).show(|| {
                        List::column().item_spacing(gap).show(|| {
                            child_error = self
                                .render_virtual_list(node, parent, frame, request, actions)
                                .err();
                        });
                    });
                }),
                "scroll" => AstraNodeWidget::show(props, || {
                    Pad::all(padding).show(|| {
                        // `Scrollable` owns clipping and applies the pointer-wheel
                        // offset during layout.  Keeping it inside the Astra node
                        // preserves authored bounds and semantic ownership.
                        Scrollable::vertical().show(|| {
                            List::column()
                                .item_spacing(gap)
                                .main_axis_size(MainAxisSize::Min)
                                .cross_axis_alignment(cross_axis_alignment)
                                .show(|| {
                                    child_error = self
                                        .render_children(
                                            children, parent, frame, item, request, actions,
                                        )
                                        .err();
                                });
                        });
                    });
                }),
                "virtual_grid" => {
                    let columns = property_number(node, "columns", frame, item)?
                        .unwrap_or(1.0)
                        .round()
                        .clamp(1.0, 256.0) as usize;
                    AstraNodeWidget::show(props, || {
                        Pad::all(padding).show(|| {
                            CountGrid::col(columns)
                                .main_axis_size(MainAxisSize::Min)
                                .show(|| {
                                    child_error = self
                                        .render_virtual_grid(node, parent, frame, request, actions)
                                        .err();
                                });
                        });
                    })
                }
                "stack" | "modal" | "screen" | "canvas" => AstraNodeWidget::show(props, || {
                    Pad::all(padding).show(|| {
                        Stack::new().show(|| {
                            child_error = self
                                .render_children(children, parent, frame, item, request, actions)
                                .err();
                        });
                    });
                }),
                "image" => {
                    let asset = required_visual_asset(node, frame, item, request)?;
                    let texture = *self.managed_textures.get(&asset).ok_or_else(|| {
                        UiValidationError::invalid(
                            "ASTRA_UI_IMAGE_RESOURCE_UNBOUND",
                            format!(
                                "image asset {asset} was not registered for this UI generation"
                            ),
                        )
                    })?;
                    AstraNodeWidget::show(props, || {
                        Image::new(texture, Vec2::new(min_width.max(1.0), min_height.max(1.0)))
                            .show();
                    })
                }
                "nine_slice" => {
                    let (asset, border) = required_nine_slice(node, frame, item, request)?;
                    let texture = *self.managed_textures.get(&asset).ok_or_else(|| {
                        UiValidationError::invalid(
                            "ASTRA_UI_NINE_SLICE_RESOURCE_UNBOUND",
                            format!(
                            "nine-slice asset {asset} was not registered for this UI generation"
                        ),
                        )
                    })?;
                    AstraNodeWidget::show(props, || {
                        NineSlice::new(
                            texture,
                            Pad {
                                left: border[0],
                                top: border[1],
                                right: border[2],
                                bottom: border[3],
                            },
                            1.0,
                        )
                        .show(|| {
                            child_error = self
                                .render_children(children, parent, frame, item, request, actions)
                                .err();
                        });
                    })
                }
                "slider" => {
                    let value = property_number_f64(node, "value", frame, item)?.unwrap_or(0.0);
                    let min = property_number_f64(node, "min", frame, item)?.unwrap_or(0.0);
                    let max = property_number_f64(node, "max", frame, item)?.unwrap_or(1.0);
                    if min >= max || value < min || value > max {
                        return Err(UiValidationError::invalid(
                            "ASTRA_UI_SLIDER_RANGE",
                            "slider requires min < max and a value inside the range",
                        ));
                    }
                    let step = property_number_f64(node, "step", frame, item)?;
                    semantic_properties.insert("range.value".into(), value.to_string());
                    semantic_properties.insert("range.min".into(), min.to_string());
                    semantic_properties.insert("range.max".into(), max.to_string());
                    semantic_properties.insert(
                        "range.step".into(),
                        step.unwrap_or((max - min) / 100.0).to_string(),
                    );
                    AstraNodeWidget::show(props, || {
                        let slider = Slider {
                            value,
                            min,
                            max,
                            step,
                        }
                        .show();
                        if let Some(next) = slider.value {
                            if (next - value).abs() > f64::EPSILON {
                                changed_event = Some(UiValue::Map(BTreeMap::from([(
                                    "value".to_string(),
                                    UiValue::Number(next),
                                )])));
                            }
                        }
                    })
                }
                "toggle" => {
                    let checked = property_bool(node, "checked", frame, item)?.unwrap_or(false);
                    let indicator_color = if checked {
                        property_color(node, "indicator_color", frame, item, request)?
                            .unwrap_or(Color::rgba(196, 158, 67, 255))
                    } else {
                        property_color(node, "indicator_off_color", frame, item, request)?
                            .unwrap_or(Color::rgba(178, 184, 184, 255))
                    };
                    semantic_checked = Some(checked);
                    AstraNodeWidget::show(props, || {
                        Align::new(Alignment::CENTER_RIGHT).show(|| {
                            let _ = AstraNodeWidget::show(
                                AstraNodeProps {
                                    semantic_id: format!("{semantic_id}.indicator"),
                                    min_size: Vec2::new(30.0, 30.0),
                                    max_size: Vec2::INFINITY,
                                    fill: indicator_color,
                                    interactive: false,
                                    fill_width: false,
                                    fill_height: false,
                                    loose_children: false,
                                    clip_children: false,
                                },
                                || {},
                            );
                        });
                    })
                }
                _ => AstraNodeWidget::show(props, || {
                    child_error = self
                        .render_children(children, parent, frame, item, request, actions)
                        .err();
                }),
            })
        };
        let anchor = property_string(node, "anchor", frame, item)?;
        let position_x = non_negative_layout_property(node, "position_x", frame, item)?;
        let position_y = non_negative_layout_property(node, "position_y", frame, item)?;
        let position_right = non_negative_layout_property(node, "position_right", frame, item)?;
        let position_bottom = non_negative_layout_property(node, "position_bottom", frame, item)?;
        let has_absolute_position = position_x.is_some()
            || position_y.is_some()
            || position_right.is_some()
            || position_bottom.is_some();
        if anchor.is_some() && has_absolute_position {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_POSITION_CONFLICT",
                "absolute position and anchor cannot be specified on the same widget",
            ));
        }
        let response = if has_absolute_position {
            let mut response = None;
            Pad {
                left: position_x.unwrap_or(0.0),
                top: position_y.unwrap_or(0.0),
                right: position_right.unwrap_or(0.0),
                bottom: position_bottom.unwrap_or(0.0),
            }
            .show(|| response = Some(build_widget()));
            response.ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_POSITION_LAYOUT",
                    "Yakui did not build the absolutely positioned Astra widget",
                )
            })??
        } else if let Some(anchor) = anchor {
            let alignment = parse_alignment(&anchor)?;
            let mut response = None;
            Align::new(alignment).show(|| response = Some(build_widget()));
            response.ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_ALIGNMENT_LAYOUT",
                    "Yakui did not build the aligned Astra widget",
                )
            })??
        } else {
            build_widget()?
        };
        if let Some(error) = child_error {
            return Err(error);
        }
        if node.widget == "toggle" && response.clicked_semantic_id.is_some() {
            let next = !semantic_checked.unwrap_or(false);
            changed_event = Some(UiValue::Map(BTreeMap::from([
                ("checked".to_string(), UiValue::Bool(next)),
                ("value".to_string(), UiValue::Bool(next)),
            ])));
        }
        if let Some(clicked_id) = response.clicked_semantic_id.as_deref() {
            for event in node.events.iter().filter(|event| event.event == "activate") {
                actions.push(action_from_event(
                    event, clicked_id, request, frame, item, None,
                )?);
            }
            if node.widget == "select" {
                let value = next_select_value(node, frame, item)?;
                for event in node.events.iter().filter(|event| event.event == "change") {
                    actions.push(action_from_event(
                        event,
                        clicked_id,
                        request,
                        frame,
                        item,
                        Some(&value),
                    )?);
                }
            }
        }
        if let Some(event_value) = changed_event.as_ref() {
            if !self
                .accessibility_dispatched_events
                .contains(&(semantic_id.clone(), "change".into()))
            {
                for event in node.events.iter().filter(|event| event.event == "change") {
                    actions.push(action_from_event(
                        event,
                        &semantic_id,
                        request,
                        frame,
                        item,
                        Some(event_value),
                    )?);
                }
            }
        }
        if let Some(event_value) = submitted_event.as_ref() {
            for event in node.events.iter().filter(|event| event.event == "submit") {
                actions.push(action_from_event(
                    event,
                    &semantic_id,
                    request,
                    frame,
                    item,
                    Some(event_value),
                )?);
            }
        }
        let role = semantic_role(&node.widget);
        let mut semantic_actions = BTreeSet::new();
        if interactive {
            semantic_actions.insert(UiSemanticAction::Focus);
            match role {
                UiSemanticRole::Slider => {
                    semantic_actions.insert(UiSemanticAction::Increment);
                    semantic_actions.insert(UiSemanticAction::Decrement);
                    semantic_actions.insert(UiSemanticAction::SetValue);
                }
                UiSemanticRole::TextInput => {
                    semantic_actions.insert(UiSemanticAction::SetValue);
                    semantic_actions.insert(UiSemanticAction::Activate);
                }
                _ => {
                    semantic_actions.insert(UiSemanticAction::Activate);
                }
            }
        }
        self.pending.push(PendingSemantic {
            id: semantic_id,
            parent_id: parent_id.map(str::to_owned),
            widget_id: response.id,
            role,
            name,
            value: semantic_value,
            enabled,
            focused: response.focused,
            selected,
            checked: semantic_checked,
            actions: semantic_actions,
            properties: semantic_properties,
        });
        Ok(())
    }
}
