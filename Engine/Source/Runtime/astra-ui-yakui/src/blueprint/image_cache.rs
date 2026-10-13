use super::*;

pub(super) const MAX_MANAGED_IMAGE_TEXTURES: usize = 128;

pub trait UiImageResourceProvider: Send + Sync {
    fn load_image(&self, asset: &str) -> Result<TextureFrame, UiValidationError>;
}

#[derive(Debug, Clone)]
pub(super) struct PendingSemantic {
    pub(super) id: String,
    pub(super) parent_id: Option<String>,
    pub(super) widget_id: WidgetId,
    pub(super) role: UiSemanticRole,
    pub(super) name: Option<String>,
    pub(super) value: Option<String>,
    pub(super) enabled: bool,
    pub(super) focused: bool,
    pub(super) selected: bool,
    pub(super) checked: Option<bool>,
    pub(super) actions: BTreeSet<UiSemanticAction>,
    pub(super) properties: BTreeMap<String, String>,
}

pub struct BlueprintYakuiRenderer {
    pub(super) views: BTreeMap<String, Arc<UiViewBlueprint>>,
    pub(super) pending: Vec<PendingSemantic>,
    pub(super) virtual_lists: BTreeMap<String, VirtualListState>,
    pub(super) virtual_grids: BTreeMap<String, VirtualGridState>,
    pub(super) accessibility_actions: BTreeMap<String, Vec<(UiEventBinding, Option<UiValue>)>>,
    pub(super) retained_frame: Option<UiBlueprintFrameModel>,
    pub(super) retained_semantic_generation: Option<u64>,
    pub(super) text_inputs: BTreeMap<String, TextInputState>,
    pub(super) focused_text_input: Option<String>,
    pub(super) semantic_focus_override: Option<String>,
    pub(super) backend_focus_request: Option<String>,
    pub(super) text_input_consumed_sequences: BTreeSet<u64>,
    pub(super) accessibility_dispatched_events: BTreeSet<(String, String)>,
    pub(super) image_resources: BTreeMap<String, TextureFrame>,
    pub(super) image_resource_provider: Option<Arc<dyn UiImageResourceProvider>>,
    pub(super) managed_textures: BoundedLru<String, ManagedTextureId>,
    pub(super) pending_removed_managed_textures: Vec<ManagedTextureId>,
    pub(super) text_measurer: Option<Arc<dyn AstraTextMeasurer>>,
}

impl BlueprintYakuiRenderer {
    pub fn new(bundle: UiBlueprintBundle) -> Result<Self, UiValidationError> {
        bundle.validate()?;
        Ok(Self {
            views: bundle
                .views
                .into_iter()
                .map(|(id, view)| (id, Arc::new(view)))
                .collect(),
            pending: Vec::new(),
            virtual_lists: BTreeMap::new(),
            virtual_grids: BTreeMap::new(),
            accessibility_actions: BTreeMap::new(),
            retained_frame: None,
            retained_semantic_generation: None,
            text_inputs: BTreeMap::new(),
            focused_text_input: None,
            semantic_focus_override: None,
            backend_focus_request: None,
            text_input_consumed_sequences: BTreeSet::new(),
            accessibility_dispatched_events: BTreeSet::new(),
            image_resources: BTreeMap::new(),
            image_resource_provider: None,
            managed_textures: BoundedLru::new(MAX_MANAGED_IMAGE_TEXTURES, MAX_TEXTURE_BYTES)?,
            pending_removed_managed_textures: Vec::new(),
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

    pub fn with_image_resource_provider(
        mut self,
        provider: Arc<dyn UiImageResourceProvider>,
    ) -> Self {
        self.image_resource_provider = Some(provider);
        self
    }

    pub fn resolve_retained_activation(
        &self,
        semantic_generation: u64,
        semantic_target_id: &str,
        input_sequence: u64,
    ) -> Result<UiActionEnvelope, UiValidationError> {
        if self.retained_semantic_generation != Some(semantic_generation) {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_RETAINED_SEMANTIC_STALE",
                "retained activation does not match the live semantic generation",
            ));
        }
        let frame = self.retained_frame.as_ref().ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_RETAINED_FRAME_MISSING",
                "retained activation requires a validated frame model",
            )
        })?;
        let target = self
            .pending
            .iter()
            .find(|pending| pending.id == semantic_target_id)
            .ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_RETAINED_TARGET_MISSING",
                    "retained activation target is absent from the live semantic generation",
                )
            })?;
        if !target.enabled || !target.actions.contains(&UiSemanticAction::Activate) {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_RETAINED_TARGET_DISABLED",
                "retained activation target is disabled or not activatable",
            ));
        }
        let bindings = self
            .accessibility_actions
            .get(semantic_target_id)
            .ok_or_else(|| {
                UiValidationError::invalid(
                    "ASTRA_UI_RETAINED_BINDING_MISSING",
                    "retained activation target has no typed action binding",
                )
            })?;
        let matching = bindings
            .iter()
            .filter(|(binding, _)| binding.event == "activate")
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_RETAINED_BINDING_AMBIGUOUS",
                "retained activation must map to exactly one typed action binding",
            ));
        }
        let (binding, item) = matching[0];
        let action = action_from_event_sequence(
            binding,
            semantic_target_id,
            input_sequence,
            semantic_generation,
            frame,
            item.as_ref(),
            None,
        )?;
        action.validate()?;
        Ok(action)
    }

    pub fn upsert_image_resource(
        &mut self,
        asset: String,
        frame: TextureFrame,
    ) -> Result<(), UiValidationError> {
        validate_image_resource(&asset, &frame)?;
        self.image_resources.insert(asset.clone(), frame);
        if let Some(managed) = self.managed_textures.remove(&asset) {
            self.pending_removed_managed_textures.push(managed);
        }
        Ok(())
    }

    pub fn remove_image_resource(&mut self, asset: &str) -> bool {
        let removed = self.image_resources.remove(asset).is_some();
        if let Some(managed) = self.managed_textures.remove(&asset.to_string()) {
            self.pending_removed_managed_textures.push(managed);
        }
        removed
    }

    pub(super) fn collect_visual_assets(
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
            let state_key = virtual_state_key(frame, node);
            let viewport_extent = virtual_viewport_extent(node, frame, request)?;
            let range = if node.widget == "virtual_list" {
                let item_extent =
                    property_number(node, "item_extent", frame, item)?.unwrap_or(56.0);
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
                state.visible_range()
            } else {
                let columns = property_number(node, "columns", frame, item)?
                    .unwrap_or(1.0)
                    .round()
                    .clamp(1.0, 256.0) as usize;
                let row_extent =
                    property_number(node, "item_extent", frame, item)?.unwrap_or(180.0);
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
}
