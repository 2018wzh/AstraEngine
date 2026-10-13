use super::*;

impl BlueprintYakuiRenderer {
    pub(super) fn render_children(
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

    pub(super) fn render_virtual_list(
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
        let viewport_extent = virtual_viewport_extent(node, frame, request)?;
        let state_key = virtual_state_key(frame, node);
        let state = self
            .virtual_lists
            .entry(state_key)
            .or_insert(VirtualListState::new(
                values.len(),
                item_extent,
                viewport_extent,
                repeat.overscan as usize,
            )?);
        state.set_item_count(values.len())?;
        state.set_viewport_extent(viewport_extent)?;
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

    pub(super) fn render_virtual_grid(
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
        let viewport_extent = virtual_viewport_extent(node, frame, request)?;
        let state_key = virtual_state_key(frame, node);
        let state = self
            .virtual_grids
            .entry(state_key)
            .or_insert(VirtualGridState::new(
                values.len(),
                columns,
                row_extent,
                viewport_extent,
                repeat.overscan as usize,
            )?);
        state.configure(values.len(), columns, viewport_extent)?;
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

pub(super) fn virtual_leading_space(node: &astra_ui_core::UiNodeBlueprint, extent: f32) {
    let _ = AstraNodeWidget::show(
        AstraNodeProps {
            semantic_id: format!("{}.virtual-leading", node.local_id),
            min_size: Vec2::new(0.0, extent),
            max_size: Vec2::INFINITY,
            fill: Color::CLEAR,
            interactive: false,
            fill_width: false,
            fill_height: false,
            loose_children: false,
            clip_children: false,
        },
        || {},
    );
}

pub(super) fn virtual_state_key(
    frame: &UiBlueprintFrameModel,
    node: &astra_ui_core::UiNodeBlueprint,
) -> String {
    format!("{}#{}", frame.view_id, node.source_id)
}

pub(super) fn virtual_viewport_extent(
    node: &astra_ui_core::UiNodeBlueprint,
    frame: &UiBlueprintFrameModel,
    request: &UiFrameRequest,
) -> Result<f32, UiValidationError> {
    let available = viewport_height_points(request);
    let requested =
        non_negative_layout_property(node, "viewport_extent", frame, None)?.unwrap_or(available);
    if requested <= 0.0 || requested > available {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_VIRTUAL_VIEWPORT_EXTENT",
            "virtual viewport_extent must be positive and no larger than the UI viewport",
        ));
    }
    Ok(requested)
}
