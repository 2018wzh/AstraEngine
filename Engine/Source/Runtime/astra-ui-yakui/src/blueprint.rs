use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use astra_core::Hash256;
use astra_media_core::TextureFrame;
use astra_ui_core::{
    UiActionEnvelope, UiBindingRoot, UiBlueprintBundle, UiBlueprintFrameModel, UiEventBinding,
    UiFrameRequest, UiInputEventKind, UiPoint, UiRect, UiSemanticAction, UiSemanticNode,
    UiSemanticRole, UiSemanticSnapshot, UiThemeValue, UiValidationError, UiValue, UiValueExpr,
    ValidateUi, MAX_TEXTURE_BYTES,
};
use yakui_core::geometry::{Color, UVec2, Vec2};
use yakui_core::paint::{Texture, TextureFormat};
use yakui_core::{Alignment, CrossAxisAlignment, MainAxisSize, ManagedTextureId, WidgetId};
use yakui_widgets::widgets::{
    Align, Checkbox, CountGrid, Image, List, NineSlice, Pad, Slider, Stack,
};

use crate::{
    AstraNodeProps, AstraNodeWidget, AstraTextMeasureRequest, AstraTextMeasurer, BoundedLru,
    TextCharacterPolicy, TextInputState, VirtualGridState, VirtualListState, YakuiViewOutput,
    YakuiViewRenderer,
};

const MAX_MANAGED_IMAGE_TEXTURES: usize = 128;

#[derive(Debug, Clone)]
struct PendingSemantic {
    id: String,
    parent_id: Option<String>,
    widget_id: WidgetId,
    role: UiSemanticRole,
    name: Option<String>,
    value: Option<String>,
    enabled: bool,
    focused: bool,
    checked: Option<bool>,
    actions: BTreeSet<UiSemanticAction>,
    properties: BTreeMap<String, String>,
}

pub struct BlueprintYakuiRenderer {
    bundle: UiBlueprintBundle,
    pending: Vec<PendingSemantic>,
    virtual_lists: BTreeMap<String, VirtualListState>,
    virtual_grids: BTreeMap<String, VirtualGridState>,
    accessibility_actions: BTreeMap<String, Vec<(UiEventBinding, Option<UiValue>)>>,
    text_inputs: BTreeMap<String, TextInputState>,
    focused_text_input: Option<String>,
    text_input_consumed_sequences: BTreeSet<u64>,
    accessibility_dispatched_events: BTreeSet<(String, String)>,
    image_resources: BTreeMap<String, TextureFrame>,
    managed_textures: BoundedLru<String, ManagedTextureId>,
    text_measurer: Option<Arc<dyn AstraTextMeasurer>>,
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
            text_inputs: BTreeMap::new(),
            focused_text_input: None,
            text_input_consumed_sequences: BTreeSet::new(),
            accessibility_dispatched_events: BTreeSet::new(),
            image_resources: BTreeMap::new(),
            managed_textures: BoundedLru::new(MAX_MANAGED_IMAGE_TEXTURES, MAX_TEXTURE_BYTES)?,
            text_measurer: None,
        })
    }

    pub fn with_text_measurer(mut self, measurer: Arc<dyn AstraTextMeasurer>) -> Self {
        self.text_measurer = Some(measurer);
        self
    }

    pub fn with_image_resources(mut self, resources: BTreeMap<String, TextureFrame>) -> Self {
        self.image_resources = resources;
        self
    }

    fn collect_visual_assets(
        &mut self,
        node: &astra_ui_core::UiNodeBlueprint,
        frame: &UiBlueprintFrameModel,
        item: Option<&UiValue>,
        request: &UiFrameRequest,
        output: &mut BTreeSet<String>,
    ) -> Result<(), UiValidationError> {
        if !property_bool(node, "visible", frame, item)?.unwrap_or(true) {
            return Ok(());
        }
        match node.widget.as_str() {
            "image" => {
                output.insert(required_visual_asset(node, frame, item, request)?);
            }
            "nine_slice" => {
                output.insert(required_nine_slice(node, frame, item, request)?.0);
            }
            _ => {}
        }

        if matches!(node.widget.as_str(), "virtual_list" | "virtual_grid") {
            let repeat = node.repeat.as_ref().ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_VIRTUAL_REPEAT_MISSING",
                    "virtual collection requires items and item_key",
                )
            })?;
            let values = evaluate_collection(&repeat.items, frame, item)?;
            let range = if node.widget == "virtual_list" {
                let item_extent =
                    property_number(node, "item_extent", frame, item)?.unwrap_or(56.0);
                let state = self.virtual_lists.entry(node.local_id.clone()).or_insert(
                    VirtualListState::new(
                        values.len(),
                        item_extent,
                        viewport_height_points(request),
                        repeat.overscan as usize,
                    )?,
                );
                state.set_item_count(values.len())?;
                state.set_viewport_extent(viewport_height_points(request))?;
                state.visible_range()
            } else {
                let columns = property_number(node, "columns", frame, item)?
                    .unwrap_or(1.0)
                    .round()
                    .clamp(1.0, 256.0) as usize;
                let row_extent =
                    property_number(node, "item_extent", frame, item)?.unwrap_or(180.0);
                let state = self.virtual_grids.entry(node.local_id.clone()).or_insert(
                    VirtualGridState::new(
                        values.len(),
                        columns,
                        row_extent,
                        viewport_height_points(request),
                        repeat.overscan as usize,
                    )?,
                );
                state.configure(values.len(), columns, viewport_height_points(request))?;
                state.visible_items()
            };
            for value in &values[range.start..range.end] {
                for child in &node.children {
                    self.collect_visual_assets(child, frame, Some(value), request, output)?;
                }
            }
            return Ok(());
        }

        for child in &node.children {
            if child.repeat.is_some() {
                let repeat = child.repeat.as_ref().ok_or_else(|| {
                    UiValidationError::invalid(
                        "ASTRA_UI_REPEAT_STATE",
                        "repeat node lost its validated binding",
                    )
                })?;
                let values = evaluate_collection(&repeat.items, frame, item)?;
                for value in values {
                    self.collect_visual_assets(child, frame, Some(value), request, output)?;
                }
            } else {
                self.collect_visual_assets(child, frame, item, request, output)?;
            }
        }
        Ok(())
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
        let default_fill = if matches!(
            node.widget.as_str(),
            "button" | "select" | "toggle" | "slider" | "text_input"
        ) {
            Color::rgba(38, 58, 84, 255)
        } else {
            Color::CLEAR
        };
        let fill = property_color(node, "background", frame, item, request).unwrap_or(default_fill);
        if let Some(text_key) = name.as_deref() {
            let text = frame
                .localization
                .get(text_key)
                .map(String::as_str)
                .unwrap_or(text_key);
            let max_width = viewport_width_points(request).max(1.0);
            let font_size = property_number(node, "font_size", frame, item)?
                .unwrap_or_else(|| default_ui_font_size(request));
            let max_lines = bounded_u32_property(node, "max_lines", frame, item, 4, 1, 1_024)?;
            let direction = property_string(node, "direction", frame, item)?
                .unwrap_or_else(|| "auto".to_string());
            let measurer = self.text_measurer.as_ref().ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_TEXT_MEASURER_MISSING",
                    "text-bearing widgets require the AstraText measurement provider",
                )
            })?;
            let measured = measurer.measure(&AstraTextMeasureRequest {
                semantic_id: semantic_id.clone(),
                text: text.to_string(),
                max_width: (max_width - 16.0).max(1.0),
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
            min_width = min_width.max(measured.width + 16.0);
            min_height = min_height.max(measured.height + 16.0);
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
            fill,
            interactive,
            fill_width,
            fill_height,
            loose_children: matches!(
                node.widget.as_str(),
                "screen" | "stack" | "modal" | "canvas"
            ),
        };
        let children = node.children.clone();
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
        let build_widget = || -> Result<_, UiValidationError> {
            Ok(match node.widget.as_str() {
                "row" => AstraNodeWidget::show(props, || {
                    List::row().main_axis_size(MainAxisSize::Min).show(|| {
                        child_error = self
                            .render_children(&children, parent, frame, item, request, actions)
                            .err();
                    });
                }),
                "column" | "scroll" => AstraNodeWidget::show(props, || {
                    List::column()
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .show(|| {
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
                                .render_children(&children, parent, frame, item, request, actions)
                                .err();
                        });
                    })
                }
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
                    semantic_checked = Some(checked);
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
            })
        };
        let anchor = property_string(node, "anchor", frame, item)?;
        let response = if let Some(anchor) = anchor {
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
            checked: semantic_checked,
            actions: semantic_actions,
            properties: semantic_properties,
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
                let values = evaluate_collection(&repeat.items, frame, item)?;
                for value in values {
                    self.render_node(child, parent_id, frame, Some(value), request, actions)?;
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
        let values = evaluate_collection(&repeat.items, frame, None)?;
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
        let values = evaluate_collection(&repeat.items, frame, None)?;
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
            fill_width: false,
            fill_height: false,
            loose_children: false,
        },
        || {},
    );
}

impl YakuiViewRenderer for BlueprintYakuiRenderer {
    fn build(
        &mut self,
        yakui: &mut yakui_core::Yakui,
        request: &UiFrameRequest,
    ) -> Result<YakuiViewOutput, UiValidationError> {
        let frame: UiBlueprintFrameModel =
            postcard::from_bytes(&request.model_payload).map_err(|error| {
                UiValidationError::invalid("ASTRA_UI_BLUEPRINT_MODEL_DECODE", error.to_string())
            })?;
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
                    &frame,
                    item.as_ref(),
                    event_value.as_ref(),
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
        let mut required_assets = BTreeSet::new();
        self.collect_visual_assets(&view.root, &frame, None, request, &mut required_assets)?;
        for modal in &frame.modals {
            let modal_view = self
                .bundle
                .views
                .get(&modal.view_id)
                .cloned()
                .ok_or_else(|| {
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
            let frame = self.image_resources.get(&asset).ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_IMAGE_RESOURCE_MISSING",
                    format!("UI blueprint references unavailable packaged asset {asset}"),
                )
            })?;
            let texture = Texture::new(
                TextureFormat::Rgba8Srgb,
                UVec2::new(frame.width, frame.height),
                frame.rgba8.clone(),
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
                        fill_width: true,
                        fill_height: true,
                        loose_children: true,
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
        let focus_request = accessibility_focus_request
            .as_ref()
            .or(frame.focus_request.as_ref());
        let focus_widget = if let Some(focus_request) = focus_request {
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
            Some(target.widget_id)
        } else {
            None
        };
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
                focused: pending.focused,
                selected: false,
                checked: pending.checked,
                actions: pending.actions.clone(),
                properties: pending.properties.clone(),
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

fn semantic_number(semantic: &PendingSemantic, key: &str) -> Result<f64, UiValidationError> {
    semantic
        .properties
        .get(key)
        .ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_ACCESSIBILITY_RANGE_METADATA",
                format!("semantic target is missing {key}"),
            )
        })?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_ACCESSIBILITY_RANGE_METADATA",
                format!("semantic target has invalid {key}"),
            )
        })
}

fn viewport_height_points(request: &UiFrameRequest) -> f32 {
    request.viewport.physical_height as f32
        / (request.viewport.scale_factor * request.viewport.font_scale)
}

fn viewport_width_points(request: &UiFrameRequest) -> f32 {
    request.viewport.physical_width as f32
        / (request.viewport.scale_factor * request.viewport.font_scale)
}

fn default_ui_font_size(_request: &UiFrameRequest) -> f32 {
    20.0
}

fn bounded_u32_property(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, UiValidationError> {
    let Some(value) = property_number(node, key, frame, item)? else {
        return Ok(default);
    };
    if value.fract() != 0.0 || value < min as f32 || value > max as f32 {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_INTEGER_PROPERTY_RANGE",
            format!("property {key} must be an integer within {min}..={max}"),
        ));
    }
    Ok(value as u32)
}

fn parse_alignment(value: &str) -> Result<Alignment, UiValidationError> {
    match value {
        "top" | "top_left" => Ok(Alignment::TOP_LEFT),
        "top_center" => Ok(Alignment::TOP_CENTER),
        "top_right" => Ok(Alignment::TOP_RIGHT),
        "center_left" => Ok(Alignment::CENTER_LEFT),
        "center" => Ok(Alignment::CENTER),
        "center_right" => Ok(Alignment::CENTER_RIGHT),
        "bottom" | "bottom_left" => Ok(Alignment::BOTTOM_LEFT),
        "bottom_center" => Ok(Alignment::BOTTOM_CENTER),
        "bottom_right" => Ok(Alignment::BOTTOM_RIGHT),
        _ => Err(UiValidationError::invalid(
            "ASTRA_UI_ALIGNMENT",
            "anchor must be a registered Astra alignment token",
        )),
    }
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

fn visual_token(node: &astra_ui_core::UiNodeBlueprint) -> Option<&UiValueExpr> {
    node.properties
        .get("asset")
        .or_else(|| node.properties.get("texture"))
}

fn required_visual_asset(
    node: &astra_ui_core::UiNodeBlueprint,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    request: &UiFrameRequest,
) -> Result<String, UiValidationError> {
    let expr = visual_token(node).ok_or_else(|| {
        UiValidationError::invalid(
            "ASTRA_UI_IMAGE_ASSET_REQUIRED",
            "image widget requires an asset or texture property",
        )
    })?;
    match expr {
        UiValueExpr::AssetRef { asset_id } => Ok(asset_id.clone()),
        UiValueExpr::ThemeToken { token } => match request.theme.tokens.get(token) {
            Some(UiThemeValue::Asset(asset)) | Some(UiThemeValue::NineSlice { asset, .. }) => {
                Ok(asset.clone())
            }
            _ => Err(UiValidationError::invalid(
                "ASTRA_UI_IMAGE_THEME_TOKEN",
                "image theme token must resolve to Asset or NineSlice",
            )),
        },
        _ => match evaluate(expr, frame, item, None)? {
            UiValue::String(asset) if !asset.trim().is_empty() => Ok(asset),
            _ => Err(UiValidationError::invalid(
                "ASTRA_UI_IMAGE_ASSET_TYPE",
                "image asset must resolve to a non-empty AssetRef",
            )),
        },
    }
}

fn required_nine_slice(
    node: &astra_ui_core::UiNodeBlueprint,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    request: &UiFrameRequest,
) -> Result<(String, [f32; 4]), UiValidationError> {
    let expr = visual_token(node).ok_or_else(|| {
        UiValidationError::invalid(
            "ASTRA_UI_NINE_SLICE_ASSET_REQUIRED",
            "nine_slice widget requires a texture theme token",
        )
    })?;
    let (asset, border) = match expr {
        UiValueExpr::ThemeToken { token } => match request.theme.tokens.get(token) {
            Some(UiThemeValue::NineSlice { asset, border }) => (asset.clone(), *border),
            _ => {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_NINE_SLICE_THEME_TOKEN",
                    "nine_slice theme token must resolve to NineSlice",
                ));
            }
        },
        _ => match evaluate(expr, frame, item, None)? {
            UiValue::String(asset) if !asset.trim().is_empty() => (asset, [0.0; 4]),
            _ => {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_NINE_SLICE_ASSET_TYPE",
                    "nine_slice texture must resolve to a theme token or AssetRef",
                ));
            }
        },
    };
    if border
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_NINE_SLICE_BORDER",
            "nine-slice border must be finite and non-negative",
        ));
    }
    Ok((asset, border))
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
                _ if key == "value" && node.widget != "text" => continue,
                _ => Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXT_PROPERTY_TYPE",
                    "text property must resolve to string",
                )),
            };
        }
    }
    Ok(None)
}

fn property_string(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<Option<String>, UiValidationError> {
    node.properties
        .get(key)
        .map(|expr| match evaluate(expr, frame, item, None)? {
            UiValue::String(value) => Ok(value),
            _ => Err(UiValidationError::invalid(
                "ASTRA_UI_PROPERTY_TYPE",
                format!("property {key} must resolve to string"),
            )),
        })
        .transpose()
}

fn bounded_usize_property(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, UiValidationError> {
    let Some(value) = property_number(node, key, frame, item)? else {
        return Ok(default);
    };
    if value.fract() != 0.0 || value < min as f32 || value > max as f32 {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_INTEGER_PROPERTY_RANGE",
            format!("property {key} must be an integer within {min}..={max}"),
        ));
    }
    Ok(value as usize)
}

fn semantic_text_properties(
    node: &astra_ui_core::UiNodeBlueprint,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<BTreeMap<String, String>, UiValidationError> {
    let mut properties = BTreeMap::new();
    if let Some(expr) = node.properties.get("direction") {
        let UiValue::String(direction) = evaluate(expr, frame, item, None)? else {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_TEXT_DIRECTION_TYPE",
                "text direction must resolve to a string",
            ));
        };
        if !matches!(
            direction.as_str(),
            "auto"
                | "left_to_right"
                | "right_to_left"
                | "vertical_right_to_left"
                | "vertical_left_to_right"
        ) {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_TEXT_DIRECTION",
                "text direction is not supported by AstraText v2",
            ));
        }
        properties.insert("text.direction".into(), direction);
    }
    for (source, target, min, max) in [
        ("max_lines", "text.max_lines", 1.0_f32, 1_024.0_f32),
        ("font_size", "text.font_size", 6.0_f32, 256.0_f32),
    ] {
        if let Some(value) = property_number(node, source, frame, item)? {
            if value < min || value > max {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXT_METRIC_RANGE",
                    format!("{source} must be within {min}..={max}"),
                ));
            }
            properties.insert(target.into(), value.to_string());
        }
    }
    Ok(properties)
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

fn evaluate_collection<'a>(
    expr: &'a UiValueExpr,
    frame: &'a UiBlueprintFrameModel,
    item: Option<&'a UiValue>,
) -> Result<&'a [UiValue], UiValidationError> {
    let value = match expr {
        UiValueExpr::Literal { value } => value,
        UiValueExpr::Binding { root, path } => {
            let root = match root {
                UiBindingRoot::Model => &frame.model,
                UiBindingRoot::State => &frame.state,
                UiBindingRoot::Item => item.ok_or_else(|| {
                    UiValidationError::invalid(
                        "ASTRA_UI_ITEM_SCOPE",
                        "item collection binding outside repeat",
                    )
                })?,
                UiBindingRoot::Event => {
                    return Err(UiValidationError::invalid(
                        "ASTRA_UI_EVENT_SCOPE",
                        "event binding cannot provide a retained collection",
                    ));
                }
            };
            lookup(root, path).ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_BINDING_PATH",
                    "collection binding path does not exist",
                )
            })?
        }
        _ => {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_REPEAT_BINDING",
                "repeat items must use a literal or retained model/item/state binding",
            ));
        }
    };
    match value {
        UiValue::List(values) => Ok(values),
        _ => Err(UiValidationError::invalid(
            "ASTRA_UI_REPEAT_TYPE",
            "repeat items binding must resolve to a list",
        )),
    }
}

fn lookup<'a>(value: &'a UiValue, path: &[String]) -> Option<&'a UiValue> {
    path.iter()
        .try_fold(value, |current, segment| match current {
            UiValue::Map(values) => values.get(segment),
            _ => None,
        })
}
