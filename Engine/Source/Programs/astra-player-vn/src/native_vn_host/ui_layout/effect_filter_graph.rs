use super::*;

pub(in super::super) fn effect_filter_graph(
    effect: &astra_vn_package::ProductStageEffect,
) -> Result<FilterGraph, NativeVnHostError> {
    let (kind, params) = match effect.filter.as_str() {
        "soft_glow" | "astra.filter.bloom" => (
            "astra.filter.bloom",
            BTreeMap::from([("intensity".to_string(), FilterParam::Float(0.15))]),
        ),
        "astra.filter.fade" => (
            "astra.filter.fade",
            BTreeMap::from([("amount".to_string(), FilterParam::Float(0.15))]),
        ),
        other => {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_FILTER_UNSUPPORTED: {other}"
            )))
        }
    };
    Ok(FilterGraph {
        schema: "astra.filter_graph.v1".to_string(),
        nodes: vec![FilterNode {
            id: format!("vn.effect.{}", effect.target),
            kind: kind.to_string(),
            input: FilterTarget::Final,
            output: FilterTarget::Final,
            params,
            deterministic: true,
            allow_cpu_fallback: true,
        }],
    })
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn entity_destination(
    state: &ProductStageState,
    texture_width: u32,
    texture_height: u32,
    fit: StageFitMode,
    entity_x: i64,
    entity_y: i64,
    width: u32,
    height: u32,
) -> Result<RectI, NativeVnHostError> {
    let zoom = state.camera.zoom.millionths;
    if zoom <= 0 {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_CAMERA_ZOOM: camera zoom must remain positive".to_string(),
        ));
    }
    let (base_width, base_height) = match fit {
        StageFitMode::ContainHeight => {
            let height_fit = height.saturating_mul(9) / 10;
            let width_fit =
                (u64::from(height_fit) * u64::from(texture_width)) / u64::from(texture_height);
            if width_fit <= u64::from(width) {
                (width_fit, u64::from(height_fit))
            } else {
                (
                    u64::from(width),
                    u64::from(width) * u64::from(texture_height) / u64::from(texture_width),
                )
            }
        }
        StageFitMode::Native => {
            let viewport_width = u64::from(state.viewport.width);
            let viewport_height = u64::from(state.viewport.height);
            if viewport_width == 0 || viewport_height == 0 {
                return Err(stage_coordinate_error());
            }
            let scale_numerator =
                (u64::from(width) * viewport_height).min(u64::from(height) * viewport_width);
            (
                (u64::from(texture_width) * scale_numerator / viewport_width / viewport_height)
                    .max(1),
                (u64::from(texture_height) * scale_numerator / viewport_width / viewport_height)
                    .max(1),
            )
        }
    };
    let destination_width = scale_dimension(base_width, zoom)?;
    let destination_height = scale_dimension(base_height, zoom)?;
    let center_x = camera_coordinate(
        entity_x,
        state.camera.x.millionths + state.camera.shake_x.millionths,
        i64::from(state.viewport.width) * 500_000,
        i64::from(width) * 500_000,
        zoom,
    )?;
    let bottom_y = camera_coordinate(
        entity_y,
        state.camera.y.millionths + state.camera.shake_y.millionths,
        i64::from(state.viewport.height) * 500_000,
        i64::from(height) * 500_000,
        zoom,
    )?;
    let x = center_x - i64::from(destination_width) / 2;
    let y = bottom_y - i64::from(destination_height);
    Ok(RectI::new(
        i32::try_from(x).map_err(|_| stage_coordinate_error())?,
        i32::try_from(y).map_err(|_| stage_coordinate_error())?,
        destination_width,
        destination_height,
    ))
}

pub(in super::super) fn camera_coordinate(
    value: i64,
    camera: i64,
    stage_center: i64,
    output_center: i64,
    zoom: i64,
) -> Result<i64, NativeVnHostError> {
    let centered = i128::from(value) - i128::from(camera) - i128::from(stage_center);
    let scaled = centered
        .checked_mul(i128::from(zoom))
        .ok_or_else(stage_coordinate_error)?
        / 1_000_000;
    i64::try_from((scaled + i128::from(output_center)) / 1_000_000)
        .map_err(|_| stage_coordinate_error())
}

pub(in super::super) fn scale_dimension(value: u64, zoom: i64) -> Result<u32, NativeVnHostError> {
    let scaled = u128::from(value)
        .checked_mul(u128::try_from(zoom).map_err(|_| stage_coordinate_error())?)
        .ok_or_else(stage_coordinate_error)?
        / 1_000_000;
    u32::try_from(scaled.max(1)).map_err(|_| stage_coordinate_error())
}

pub(in super::super) fn safe_area_rect(
    width: u32,
    height: u32,
    ratio_width: u32,
    ratio_height: u32,
) -> Result<RectI, NativeVnHostError> {
    if ratio_width == 0 || ratio_height == 0 {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_SAFE_AREA: safe-area ratio is invalid".to_string(),
        ));
    }
    let candidate_height = u64::from(width) * u64::from(ratio_height) / u64::from(ratio_width);
    let (safe_width, safe_height) = if candidate_height <= u64::from(height) {
        (
            width,
            u32::try_from(candidate_height).map_err(|_| stage_coordinate_error())?,
        )
    } else {
        (
            u32::try_from(u64::from(height) * u64::from(ratio_width) / u64::from(ratio_height))
                .map_err(|_| stage_coordinate_error())?,
            height,
        )
    };
    Ok(RectI::new(
        ((width - safe_width) / 2) as i32,
        ((height - safe_height) / 2) as i32,
        safe_width,
        safe_height,
    ))
}

pub(in super::super) fn fixed_opacity(millionths: i64) -> Result<f32, NativeVnHostError> {
    if !(0..=1_000_000).contains(&millionths) {
        return Err(NativeVnHostError::Asset(
            "ASTRA_PLAYER_STAGE_OPACITY: stage opacity is outside zero and one".to_string(),
        ));
    }
    Ok(millionths as f32 / 1_000_000.0)
}

pub(in super::super) fn stage_blend(blend: StageBlendMode) -> Result<BlendMode, NativeVnHostError> {
    match blend {
        StageBlendMode::Normal => Ok(BlendMode::Alpha),
        StageBlendMode::Add => Ok(BlendMode::Add),
        StageBlendMode::Multiply => Ok(BlendMode::Multiply),
        StageBlendMode::Screen => Ok(BlendMode::Screen),
    }
}

pub(in super::super) fn stage_coordinate_error() -> NativeVnHostError {
    NativeVnHostError::Asset(
        "ASTRA_PLAYER_STAGE_COORDINATE: stage transform exceeds renderer coordinate limits"
            .to_string(),
    )
}

pub(in super::super) fn blueprint_view_localization_keys(
    bundle: &UiBlueprintBundle,
) -> BTreeMap<String, BTreeSet<String>> {
    bundle
        .views
        .iter()
        .map(|(id, view)| {
            let mut keys = BTreeSet::new();
            collect_node_localization_keys(&view.root, &mut keys);
            (id.clone(), keys)
        })
        .collect()
}

pub(in super::super) fn collect_node_localization_keys(
    node: &UiNodeBlueprint,
    keys: &mut BTreeSet<String>,
) {
    for expression in node.properties.values() {
        collect_expression_localization_keys(expression, keys);
    }
    for event in &node.events {
        for expression in event.arguments.values() {
            collect_expression_localization_keys(expression, keys);
        }
    }
    if let Some(repeat) = &node.repeat {
        collect_expression_localization_keys(&repeat.items, keys);
    }
    for child in &node.children {
        collect_node_localization_keys(child, keys);
    }
}

pub(in super::super) fn collect_expression_localization_keys(
    expression: &UiValueExpr,
    keys: &mut BTreeSet<String>,
) {
    match expression {
        UiValueExpr::LocalizationKey { key } => {
            keys.insert(key.clone());
        }
        UiValueExpr::Literal { .. }
        | UiValueExpr::Binding { .. }
        | UiValueExpr::AssetRef { .. }
        | UiValueExpr::ThemeToken { .. } => {}
    }
}

pub(in super::super) fn collect_value_strings(value: &UiValue, keys: &mut BTreeSet<String>) {
    match value {
        UiValue::String(value) => {
            keys.insert(value.clone());
        }
        UiValue::List(values) => {
            for value in values {
                collect_value_strings(value, keys);
            }
        }
        UiValue::Map(values) => {
            for value in values.values() {
                collect_value_strings(value, keys);
            }
        }
        UiValue::Null | UiValue::Bool(_) | UiValue::Integer(_) | UiValue::Number(_) => {}
    }
}

pub(in super::super) fn frame_localization_subset(
    dictionary: &BTreeMap<String, String>,
    view_keys: &BTreeMap<String, BTreeSet<String>>,
    view_id: &str,
    model: &UiValue,
    state: &UiValue,
    modals: &[UiBlueprintModalFrameModel],
) -> Result<BTreeMap<String, String>, NativeVnHostError> {
    let mut static_required = view_keys.get(view_id).cloned().ok_or_else(|| {
        NativeVnHostError::Localization(format!(
            "ASTRA_PLAYER_UI_LOCALIZATION_VIEW_MISSING: view {view_id} has no localization projection"
        ))
    })?;
    for modal in modals {
        static_required.extend(view_keys.get(&modal.view_id).cloned().ok_or_else(|| {
            NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_UI_LOCALIZATION_VIEW_MISSING: modal view {} has no localization projection",
                modal.view_id
            ))
        })?);
    }
    if let Some(missing) = static_required
        .iter()
        .find(|key| !dictionary.contains_key(*key))
    {
        return Err(NativeVnHostError::Localization(format!(
            "ASTRA_PLAYER_UI_LOCALIZATION_MISSING: projected key {missing} is absent"
        )));
    }
    let mut required = static_required;
    collect_value_strings(model, &mut required);
    collect_value_strings(state, &mut required);
    for modal in modals {
        collect_value_strings(&modal.model, &mut required);
        collect_value_strings(&modal.state, &mut required);
    }
    Ok(required
        .into_iter()
        .filter_map(|key| dictionary.get(&key).cloned().map(|value| (key, value)))
        .collect())
}

pub(in super::super) fn stage_director_error(
    error: astra_vn_package::VnError,
) -> NativeVnHostError {
    NativeVnHostError::Asset(format!("{}: {error}", error.code()))
}

pub(in super::super) fn missing_texture(asset_id: &str) -> NativeVnHostError {
    NativeVnHostError::Asset(format!(
        "ASTRA_PLAYER_ASSET_MISSING: cooked texture {asset_id} is not mounted"
    ))
}

pub(in super::super) fn scene_texture_ids(draw: &[SceneCommand]) -> BTreeSet<String> {
    draw.iter()
        .filter_map(|command| match command {
            SceneCommand::Sprite { texture_id, .. } => Some(texture_id.clone()),
            _ => None,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn append_text_value(
    measured: &MeasuredUiTextLayout,
    layout_ids: &mut BTreeSet<String>,
    pending: &mut Vec<PendingUiTextLayout>,
    layout_id: &str,
    text: &str,
    x: u32,
    y: u32,
    max_width: u32,
    max_height: u32,
    font_size: f32,
    max_lines: u32,
    direction: TextDirection,
    rgba: [u8; 4],
    horizontal_align: UiTextAlignment,
    vertical_align: UiTextAlignment,
    clip: Option<RectI>,
) -> Result<(), NativeVnHostError> {
    if measured.text != text
        || measured.font_size.to_bits() != font_size.to_bits()
        || measured.max_lines != max_lines
        || measured.direction != direction
    {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_UI_TEXT_MEASUREMENT_IDENTITY: measured layout does not match semantic node {layout_id}"
        )));
    }
    let layout = Arc::clone(&measured.layout);
    // Keep the existing scene-placement contract: Yakui's semantic bounds
    // describe the widget box, while text padding is applied by the scene
    // bridge. The authoritative measured layout may therefore be wider than
    // the inner box for compact fixed-size controls; alignment remains
    // clamped to that box exactly as before the measured-layout handoff.
    let layout_width = layout.width.ceil().clamp(0.0, max_width as f32) as u32;
    let layout_height = layout.height.ceil().clamp(0.0, max_height as f32) as u32;
    let aligned_x = x.saturating_add(horizontal_align.offset(max_width, layout_width));
    let aligned_y = y.saturating_add(vertical_align.offset(max_height, layout_height));
    let aligned_x = i32::try_from(aligned_x).map_err(|_| {
        NativeVnHostError::Asset("ASTRA_PLAYER_LAYOUT_COORDINATE: x exceeds i32".into())
    })?;
    let aligned_y = i32::try_from(aligned_y).map_err(|_| {
        NativeVnHostError::Asset("ASTRA_PLAYER_LAYOUT_COORDINATE: y exceeds i32".into())
    })?;
    if !layout_ids.insert(layout_id.to_string()) {
        return Err(NativeVnHostError::Asset(format!(
            "ASTRA_PLAYER_LAYOUT_DUPLICATE: layout id {layout_id} was emitted twice"
        )));
    }
    pending.push(PendingUiTextLayout {
        layout_id: layout_id.to_string(),
        layout,
        rgba,
        aligned_x,
        aligned_y,
        clip,
    });
    Ok(())
}

pub(in super::super) struct PendingUiTextLayout {
    pub(in super::super) layout_id: String,
    pub(in super::super) layout: Arc<TextLayoutResult>,
    pub(in super::super) rgba: [u8; 4],
    pub(in super::super) aligned_x: i32,
    pub(in super::super) aligned_y: i32,
    pub(in super::super) clip: Option<RectI>,
}
