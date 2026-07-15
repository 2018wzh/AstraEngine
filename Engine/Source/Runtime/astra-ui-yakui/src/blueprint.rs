use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_ui_core::{
    UiActionEnvelope, UiBindingRoot, UiBlueprintBundle, UiBlueprintFrameModel, UiEventBinding,
    UiFrameRequest, UiInputEventKind, UiPoint, UiRect, UiSemanticAction, UiSemanticNode,
    UiSemanticRole, UiSemanticSnapshot, UiThemeValue, UiValidationError, UiValue, UiValueExpr,
    ValidateUi,
};
use yakui_core::geometry::{Color, Vec2};
use yakui_core::WidgetId;
use yakui_widgets::widgets::{Checkbox, CountGrid, List, Slider, Stack};

use crate::{
    AstraNodeProps, AstraNodeWidget, VirtualGridState, VirtualListState, YakuiViewOutput,
    YakuiViewRenderer,
};

#[derive(Debug, Clone)]
struct PendingSemantic {
    id: String,
    parent_id: Option<String>,
    widget_id: WidgetId,
    role: UiSemanticRole,
    name: Option<String>,
    enabled: bool,
    focused: bool,
    actions: BTreeSet<UiSemanticAction>,
}

pub struct BlueprintYakuiRenderer {
    bundle: UiBlueprintBundle,
    pending: Vec<PendingSemantic>,
    virtual_lists: BTreeMap<String, VirtualListState>,
    virtual_grids: BTreeMap<String, VirtualGridState>,
    accessibility_actions: BTreeMap<String, Vec<(UiEventBinding, Option<UiValue>)>>,
}

impl BlueprintYakuiRenderer {
    pub fn new(bundle: UiBlueprintBundle) -> Result<Self, UiValidationError> {
        bundle.validate()?;
        Ok(Self {
            bundle,
            pending: Vec::new(),
            virtual_lists: BTreeMap::new(),
            virtual_grids: BTreeMap::new(),
            accessibility_actions: BTreeMap::new(),
        })
    }

    fn render_node(
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
        let min_width = property_number(node, "min_width", frame, item)?.unwrap_or_else(|| {
            if interactive {
                180.0
            } else {
                0.0
            }
        });
        let min_height = property_number(node, "min_height", frame, item)?.unwrap_or_else(|| {
            if interactive {
                48.0
            } else {
                0.0
            }
        });
        let fill =
            property_color(node, "background", frame, item, request).unwrap_or(if interactive {
                Color::rgba(38, 58, 84, 255)
            } else {
                Color::CLEAR
            });
        let fill_available = matches!(node.widget.as_str(), "screen");
        let name = property_text(node, frame, item)?;
        let accessible_events = node
            .events
            .iter()
            .filter(|event| event.event == "activate")
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
            fill,
            interactive,
            fill_available,
        };
        let children = node.children.clone();
        let parent = Some(semantic_id.as_str());
        let mut child_error = None;
        let mut changed_event = None;
        let response = match node.widget.as_str() {
            "row" => AstraNodeWidget::show(props, || {
                List::row().show(|| {
                    child_error = self
                        .render_children(&children, parent, frame, item, request, actions)
                        .err();
                });
            }),
            "column" | "scroll" => AstraNodeWidget::show(props, || {
                List::column().show(|| {
                    child_error = self
                        .render_children(&children, parent, frame, item, request, actions)
                        .err();
                });
            }),
            "virtual_list" => AstraNodeWidget::show(props, || {
                List::column().show(|| {
                    child_error = self
                        .render_virtual_list(node, parent, frame, request, actions)
                        .err();
                });
            }),
            "virtual_grid" => {
                let columns = property_number(node, "columns", frame, item)?
                    .unwrap_or(1.0)
                    .round()
                    .clamp(1.0, 256.0) as usize;
                AstraNodeWidget::show(props, || {
                    CountGrid::col(columns).show(|| {
                        child_error = self
                            .render_virtual_grid(node, parent, frame, request, actions)
                            .err();
                    });
                })
            }
            "stack" | "modal" | "screen" | "canvas" => AstraNodeWidget::show(props, || {
                Stack::new().show(|| {
                    child_error = self
                        .render_children(&children, parent, frame, item, request, actions)
                        .err();
                });
            }),
            "slider" => {
                let value = property_number(node, "value", frame, item)?.unwrap_or(0.0) as f64;
                let min = property_number(node, "min", frame, item)?.unwrap_or(0.0) as f64;
                let max = property_number(node, "max", frame, item)?.unwrap_or(1.0) as f64;
                if min >= max || value < min || value > max {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_SLIDER_RANGE",
                        "slider requires min < max and a value inside the range",
                    ));
                }
                let step = property_number(node, "step", frame, item)?.map(f64::from);
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
                AstraNodeWidget::show(props, || {
                    let next = Checkbox::new(checked).show().checked;
                    if next != checked {
                        changed_event = Some(UiValue::Map(BTreeMap::from([
                            ("checked".to_string(), UiValue::Bool(next)),
                            ("value".to_string(), UiValue::Bool(next)),
                        ])));
                    }
                })
            }
            _ => AstraNodeWidget::show(props, || {
                child_error = self
                    .render_children(&children, parent, frame, item, request, actions)
                    .err();
            }),
        };
        if let Some(error) = child_error {
            return Err(error);
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
        let role = semantic_role(&node.widget);
        let mut semantic_actions = BTreeSet::new();
        if interactive {
            semantic_actions.insert(UiSemanticAction::Focus);
            semantic_actions.insert(UiSemanticAction::Activate);
        }
        self.pending.push(PendingSemantic {
            id: semantic_id,
            parent_id: parent_id.map(str::to_owned),
            widget_id: response.id,
            role,
            name,
            enabled,
            focused: response.focused,
            actions: semantic_actions,
        });
        Ok(())
    }

    fn render_children(
        &mut self,
        children: &[astra_ui_core::UiNodeBlueprint],
        parent_id: Option<&str>,
        frame: &UiBlueprintFrameModel,
        item: Option<&UiValue>,
        request: &UiFrameRequest,
        actions: &mut Vec<UiActionEnvelope>,
    ) -> Result<(), UiValidationError> {
        for child in children {
            if child.repeat.is_some()
                && !matches!(child.widget.as_str(), "virtual_list" | "virtual_grid")
            {
                let Some(repeat) = child.repeat.as_ref() else {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_REPEAT_STATE",
                        "repeat node lost its validated binding",
                    ));
                };
                let values = evaluate(&repeat.items, frame, item, None)?;
                let UiValue::List(values) = values else {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_REPEAT_TYPE",
                        "repeat items binding must resolve to a list",
                    ));
                };
                for value in values {
                    self.render_node(child, parent_id, frame, Some(&value), request, actions)?;
                }
            } else {
                self.render_node(child, parent_id, frame, item, request, actions)?;
            }
        }
        Ok(())
    }

    fn render_virtual_list(
        &mut self,
        node: &astra_ui_core::UiNodeBlueprint,
        parent_id: Option<&str>,
        frame: &UiBlueprintFrameModel,
        request: &UiFrameRequest,
        actions: &mut Vec<UiActionEnvelope>,
    ) -> Result<(), UiValidationError> {
        let repeat = node.repeat.as_ref().ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_REPEAT_MISSING",
                "virtual list requires items and item_key",
            )
        })?;
        let UiValue::List(values) = evaluate(&repeat.items, frame, None, None)? else {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_REPEAT_TYPE",
                "virtual list items must resolve to a list",
            ));
        };
        let item_extent = property_number(node, "item_extent", frame, None)?.unwrap_or(56.0);
        let state =
            self.virtual_lists
                .entry(node.local_id.clone())
                .or_insert(VirtualListState::new(
                    values.len(),
                    item_extent,
                    viewport_height_points(request),
                    repeat.overscan as usize,
                )?);
        state.set_item_count(values.len())?;
        state.set_viewport_extent(viewport_height_points(request))?;
        let range = state.visible_range();
        let leading = state.visible_leading_extent(range);
        if leading > 0.0 {
            virtual_leading_space(node, leading);
        }
        for value in &values[range.start..range.end] {
            for child in &node.children {
                self.render_node(child, parent_id, frame, Some(value), request, actions)?;
            }
        }
        Ok(())
    }

    fn render_virtual_grid(
        &mut self,
        node: &astra_ui_core::UiNodeBlueprint,
        parent_id: Option<&str>,
        frame: &UiBlueprintFrameModel,
        request: &UiFrameRequest,
        actions: &mut Vec<UiActionEnvelope>,
    ) -> Result<(), UiValidationError> {
        let repeat = node.repeat.as_ref().ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_REPEAT_MISSING",
                "virtual grid requires items and item_key",
            )
        })?;
        let UiValue::List(values) = evaluate(&repeat.items, frame, None, None)? else {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_REPEAT_TYPE",
                "virtual grid items must resolve to a list",
            ));
        };
        let columns = property_number(node, "columns", frame, None)?
            .unwrap_or(1.0)
            .round()
            .clamp(1.0, 256.0) as usize;
        let row_extent = property_number(node, "item_extent", frame, None)?.unwrap_or(180.0);
        let state =
            self.virtual_grids
                .entry(node.local_id.clone())
                .or_insert(VirtualGridState::new(
                    values.len(),
                    columns,
                    row_extent,
                    viewport_height_points(request),
                    repeat.overscan as usize,
                )?);
        state.configure(values.len(), columns, viewport_height_points(request))?;
        let range = state.visible_items();
        let leading = state.visible_leading_extent(range);
        if leading > 0.0 {
            virtual_leading_space(node, leading);
        }
        for value in &values[range.start..range.end] {
            for child in &node.children {
                self.render_node(child, parent_id, frame, Some(value), request, actions)?;
            }
        }
        Ok(())
    }
}

fn virtual_leading_space(node: &astra_ui_core::UiNodeBlueprint, extent: f32) {
    let _ = AstraNodeWidget::show(
        AstraNodeProps {
            semantic_id: format!("{}.virtual-leading", node.local_id),
            min_size: Vec2::new(0.0, extent),
            fill: Color::CLEAR,
            interactive: false,
            fill_available: false,
        },
        || {},
    );
}

impl YakuiViewRenderer for BlueprintYakuiRenderer {
    fn build(
        &mut self,
        _yakui: &mut yakui_core::Yakui,
        request: &UiFrameRequest,
    ) -> Result<YakuiViewOutput, UiValidationError> {
        let frame: UiBlueprintFrameModel =
            postcard::from_bytes(&request.model_payload).map_err(|error| {
                UiValidationError::invalid("ASTRA_UI_BLUEPRINT_MODEL_DECODE", error.to_string())
            })?;
        frame.validate()?;
        let mut actions = Vec::new();
        let mut force_consumed_sequences = BTreeSet::new();
        for input in &request.input.events {
            if let UiInputEventKind::AccessibilityAction {
                semantic_id,
                action,
                value: _,
            } = &input.kind
            {
                if action != "activate" && action != "invoke" {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_ACCESSIBILITY_ACTION_UNSUPPORTED",
                        "accessibility action is not supported by the semantic target",
                    ));
                }
                let bindings = self.accessibility_actions.get(semantic_id).ok_or_else(|| {
                    UiValidationError::invalid(
                        "ASTRA_UI_ACCESSIBILITY_TARGET_MISSING",
                        "accessibility target does not exist in the live semantic generation",
                    )
                })?;
                if bindings.len() != 1 {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_ACCESSIBILITY_ACTION_AMBIGUOUS",
                        "accessibility target must map to exactly one activate action",
                    ));
                }
                let (binding, item) = &bindings[0];
                actions.push(action_from_event(
                    binding,
                    semantic_id,
                    request,
                    &frame,
                    item.as_ref(),
                    None,
                )?);
                force_consumed_sequences.insert(input.sequence);
            }
        }
        for event in &request.input.events {
            if let UiInputEventKind::Wheel { delta_points } = event.kind {
                for state in self.virtual_lists.values_mut() {
                    state.scroll_by(-delta_points.y)?;
                }
                for state in self.virtual_grids.values_mut() {
                    state.scroll_by(-delta_points.y)?;
                }
            }
        }
        let view = self
            .bundle
            .views
            .get(&frame.view_id)
            .cloned()
            .ok_or_else(|| {
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
        self.pending.clear();
        self.accessibility_actions.clear();
        let root_id = view.root.local_id.clone();
        let mut render_error = None;
        Stack::new().show(|| {
            if let Err(error) =
                self.render_node(&view.root, None, &frame, None, request, &mut actions)
            {
                render_error = Some(error);
                return;
            }
            for (index, modal) in frame.modals.iter().enumerate() {
                let modal_view = match self.bundle.views.get(&modal.view_id).cloned() {
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
                        fill: Color::rgba(0, 0, 0, 96),
                        interactive: true,
                        fill_available: true,
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
                    enabled: true,
                    focused: response.focused,
                    actions: BTreeSet::from([UiSemanticAction::Focus, UiSemanticAction::Dismiss]),
                });
            }
        });
        if let Some(error) = render_error {
            return Err(error);
        }
        if let Some(focus_request) = &frame.focus_request {
            let target = self
                .pending
                .iter()
                .find(|pending| &pending.id == focus_request)
                .ok_or_else(|| {
                    UiValidationError::invalid(
                        "ASTRA_UI_FOCUS_TARGET_MISSING",
                        "focus target does not exist in the rendered semantic generation",
                    )
                })?;
            if !target.enabled || !target.actions.contains(&UiSemanticAction::Focus) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_FOCUS_TARGET_INVALID",
                    "focus target is disabled or not focusable",
                ));
            }
            _yakui.request_focus(Some(target.widget_id));
        }
        Ok(YakuiViewOutput {
            actions,
            repaint_after_ns: None,
            diagnostics: Vec::new(),
            instantiated_nodes: self.pending.len() as u32,
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
                .collect(),
        })
    }

    fn semantics(
        &mut self,
        yakui: &yakui_core::Yakui,
        request: &UiFrameRequest,
    ) -> Result<UiSemanticSnapshot, UiValidationError> {
        let scale = request.viewport.scale_factor * request.viewport.font_scale;
        let mut nodes = Vec::with_capacity(self.pending.len());
        for pending in &self.pending {
            let layout = yakui.layout_dom().get(pending.widget_id).ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_YAKUI_SEMANTIC_LAYOUT",
                    "semantic node has no Yakui layout node",
                )
            })?;
            let rect = layout.rect;
            nodes.push(UiSemanticNode {
                id: pending.id.clone(),
                parent_id: pending.parent_id.clone(),
                role: pending.role,
                bounds_points: UiRect {
                    min: UiPoint {
                        x: rect.pos().x / scale,
                        y: rect.pos().y / scale,
                    },
                    max: UiPoint {
                        x: (rect.pos().x + rect.size().x) / scale,
                        y: (rect.pos().y + rect.size().y) / scale,
                    },
                },
                name: pending.name.clone(),
                description: None,
                value: None,
                enabled: pending.enabled,
                hidden: false,
                focused: pending.focused,
                selected: false,
                checked: None,
                actions: pending.actions.clone(),
                properties: BTreeMap::new(),
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
        let mut snapshot = UiSemanticSnapshot {
            schema: "astra.ui_semantic_snapshot.v1".into(),
            session_id: request.session_id.clone(),
            generation: request.generation,
            root_id,
            nodes,
            hash: Hash256::from_sha256(&[]),
        };
        snapshot.hash = snapshot.compute_hash()?;
        snapshot.validate()?;
        Ok(snapshot)
    }
}

fn semantic_role(widget: &str) -> UiSemanticRole {
    match widget {
        "screen" => UiSemanticRole::Application,
        "modal" => UiSemanticRole::Dialog,
        "text" => UiSemanticRole::Text,
        "image" | "nine_slice" => UiSemanticRole::Image,
        "button" | "semantic_region" => UiSemanticRole::Button,
        "toggle" => UiSemanticRole::Toggle,
        "slider" => UiSemanticRole::Slider,
        "select" => UiSemanticRole::Select,
        "virtual_list" | "scroll" => UiSemanticRole::List,
        "virtual_grid" => UiSemanticRole::Grid,
        "text_input" => UiSemanticRole::TextInput,
        "canvas" => UiSemanticRole::Canvas,
        _ => UiSemanticRole::Group,
    }
}

fn viewport_height_points(request: &UiFrameRequest) -> f32 {
    request.viewport.physical_height as f32
        / (request.viewport.scale_factor * request.viewport.font_scale)
}

fn property_bool(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<Option<bool>, UiValidationError> {
    node.properties
        .get(key)
        .map(|expr| match evaluate(expr, frame, item, None)? {
            UiValue::Bool(value) => Ok(value),
            _ => Err(UiValidationError::invalid(
                "ASTRA_UI_PROPERTY_TYPE",
                format!("property {key} must resolve to bool"),
            )),
        })
        .transpose()
}

fn property_number(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<Option<f32>, UiValidationError> {
    node.properties
        .get(key)
        .map(|expr| match evaluate(expr, frame, item, None)? {
            UiValue::Integer(value) => Ok(value as f32),
            UiValue::Number(value) if value.is_finite() => Ok(value as f32),
            _ => Err(UiValidationError::invalid(
                "ASTRA_UI_PROPERTY_TYPE",
                format!("property {key} must resolve to number"),
            )),
        })
        .transpose()
}

fn property_color(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    request: &UiFrameRequest,
) -> Option<Color> {
    let expr = node.properties.get(key)?;
    let token = match expr {
        UiValueExpr::ThemeToken { token } => token,
        _ => return None,
    };
    let UiThemeValue::Color([r, g, b, a]) = request.theme.tokens.get(token)? else {
        return None;
    };
    let _ = (frame, item);
    Some(Color::rgba(*r, *g, *b, *a))
}

fn property_text(
    node: &astra_ui_core::UiNodeBlueprint,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<Option<String>, UiValidationError> {
    for key in ["text", "value", "text_key", "label_key"] {
        if let Some(expr) = node.properties.get(key) {
            let value = evaluate(expr, frame, item, None)?;
            return match value {
                UiValue::String(value) => Ok(Some(value)),
                UiValue::Null => Ok(None),
                _ => Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXT_PROPERTY_TYPE",
                    "text property must resolve to string",
                )),
            };
        }
    }
    Ok(None)
}

fn repeat_key(node: &astra_ui_core::UiNodeBlueprint, item: &UiValue) -> Option<String> {
    let value = if let Some(repeat) = &node.repeat {
        lookup(item, &repeat.item_key_path)?
    } else {
        let UiValue::Map(values) = item else {
            return None;
        };
        [
            "id",
            "option_id",
            "slot_id",
            "command_id",
            "item_id",
            "node_id",
        ]
        .iter()
        .find_map(|key| values.get(*key))?
    };
    match value {
        UiValue::String(value) => Some(value.clone()),
        UiValue::Integer(value) => Some(value.to_string()),
        _ => None,
    }
}

fn next_select_value(
    node: &astra_ui_core::UiNodeBlueprint,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<UiValue, UiValidationError> {
    let items = node.properties.get("items").ok_or_else(|| {
        UiValidationError::invalid("ASTRA_UI_SELECT_ITEMS", "select requires an items list")
    })?;
    let UiValue::List(items) = evaluate(items, frame, item, None)? else {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_SELECT_ITEMS_TYPE",
            "select items must resolve to a list",
        ));
    };
    if items.is_empty() {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_SELECT_ITEMS_EMPTY",
            "select items must not be empty",
        ));
    }
    let current = node
        .properties
        .get("value")
        .map(|expr| evaluate(expr, frame, item, None))
        .transpose()?
        .unwrap_or(UiValue::Null);
    let values = items
        .iter()
        .map(|entry| match entry {
            UiValue::Map(map) => map.get("id").cloned().ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_SELECT_ITEM_ID",
                    "select object items require a stable id",
                )
            }),
            value => Ok(value.clone()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let current_index = values.iter().position(|value| value == &current);
    let next = values[(current_index.map_or(0, |index| index + 1)) % values.len()].clone();
    Ok(UiValue::Map(BTreeMap::from([("value".to_string(), next)])))
}

fn action_from_event(
    event: &UiEventBinding,
    target: &str,
    request: &UiFrameRequest,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    event_value: Option<&UiValue>,
) -> Result<UiActionEnvelope, UiValidationError> {
    let input_sequence = request
        .input
        .events
        .last()
        .map(|event| event.sequence)
        .unwrap_or(0);
    let mut arguments = BTreeMap::new();
    for (name, expr) in &event.arguments {
        arguments.insert(name.clone(), evaluate(expr, frame, item, event_value)?);
    }
    Ok(UiActionEnvelope {
        schema: "astra.ui_action_envelope.v1".into(),
        input_sequence,
        semantic_target_id: target.into(),
        action_id: event.action_id.clone(),
        arguments,
        semantic_snapshot_hash: Hash256::from_sha256(&[]),
    })
}

fn evaluate(
    expr: &UiValueExpr,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    event: Option<&UiValue>,
) -> Result<UiValue, UiValidationError> {
    match expr {
        UiValueExpr::Literal { value } => Ok(value.clone()),
        UiValueExpr::Binding { root, path } => {
            let root = match root {
                UiBindingRoot::Model => &frame.model,
                UiBindingRoot::State => &frame.state,
                UiBindingRoot::Item => item.ok_or_else(|| {
                    UiValidationError::invalid("ASTRA_UI_ITEM_SCOPE", "item binding outside repeat")
                })?,
                UiBindingRoot::Event => event.ok_or_else(|| {
                    UiValidationError::invalid("ASTRA_UI_EVENT_SCOPE", "event binding unavailable")
                })?,
            };
            lookup(root, path).cloned().ok_or_else(|| {
                UiValidationError::invalid("ASTRA_UI_BINDING_PATH", "binding path does not exist")
            })
        }
        UiValueExpr::LocalizationKey { key } => frame
            .localization
            .get(key)
            .cloned()
            .map(UiValue::String)
            .ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_LOCALIZATION_MISSING",
                    "localization key is not present in the frame dictionary",
                )
            }),
        UiValueExpr::AssetRef { asset_id } => Ok(UiValue::String(asset_id.clone())),
        UiValueExpr::ThemeToken { token } => Ok(UiValue::String(token.clone())),
    }
}

fn lookup<'a>(value: &'a UiValue, path: &[String]) -> Option<&'a UiValue> {
    path.iter()
        .try_fold(value, |current, segment| match current {
            UiValue::Map(values) => values.get(segment),
            _ => None,
        })
}
