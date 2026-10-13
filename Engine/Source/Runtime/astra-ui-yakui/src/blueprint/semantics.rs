use super::*;

pub(super) fn validate_image_resource(
    asset: &str,
    frame: &TextureFrame,
) -> Result<(), UiValidationError> {
    if asset.is_empty() {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_IMAGE_RESOURCE_ID",
            "UI image resource id must not be empty",
        ));
    }
    let expected = (frame.width as usize)
        .checked_mul(frame.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_IMAGE_RESOURCE_SIZE",
                "UI image resource dimensions overflow",
            )
        })?;
    if frame.width == 0
        || frame.height == 0
        || expected > MAX_TEXTURE_BYTES
        || frame.rgba8.len() != expected
    {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_IMAGE_RESOURCE_SIZE",
            "UI image resource dimensions do not match its bounded RGBA payload",
        ));
    }
    Ok(())
}

pub(super) fn semantic_role(widget: &str) -> UiSemanticRole {
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

pub(super) fn semantic_number(
    semantic: &PendingSemantic,
    key: &str,
) -> Result<f64, UiValidationError> {
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

pub(super) fn viewport_height_points(request: &UiFrameRequest) -> f32 {
    request.viewport.physical_height as f32
        / (request.viewport.scale_factor * request.viewport.font_scale)
}

pub(super) fn viewport_width_points(request: &UiFrameRequest) -> f32 {
    request.viewport.physical_width as f32
        / (request.viewport.scale_factor * request.viewport.font_scale)
}

pub(super) fn default_ui_font_size(_request: &UiFrameRequest) -> f32 {
    20.0
}

pub(super) fn bounded_u32_property(
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

pub(super) fn parse_alignment(value: &str) -> Result<Alignment, UiValidationError> {
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

pub(super) fn parse_cross_axis_alignment(
    value: &str,
) -> Result<CrossAxisAlignment, UiValidationError> {
    match value {
        "start" => Ok(CrossAxisAlignment::Start),
        "center" => Ok(CrossAxisAlignment::Center),
        "end" => Ok(CrossAxisAlignment::End),
        "stretch" => Ok(CrossAxisAlignment::Stretch),
        _ => Err(UiValidationError::invalid(
            "ASTRA_UI_CROSS_AXIS_ALIGNMENT",
            "cross_align must be start, center, end, or stretch",
        )),
    }
}

pub(super) fn property_bool(
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

pub(super) fn property_number(
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

pub(super) fn property_number_f64(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<Option<f64>, UiValidationError> {
    node.properties
        .get(key)
        .map(|expr| match evaluate(expr, frame, item, None)? {
            UiValue::Integer(value) => Ok(value as f64),
            UiValue::Number(value) if value.is_finite() => Ok(value),
            _ => Err(UiValidationError::invalid(
                "ASTRA_UI_PROPERTY_TYPE",
                format!("property {key} must resolve to number"),
            )),
        })
        .transpose()
}

pub(super) fn property_color(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    request: &UiFrameRequest,
) -> Result<Option<Color>, UiValidationError> {
    let Some(expr) = node.properties.get(key) else {
        return Ok(None);
    };
    let token = match expr {
        UiValueExpr::ThemeToken { token } => token,
        _ => {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_COLOR_PROPERTY_TYPE",
                format!("property {key} must resolve from a theme color token"),
            ));
        }
    };
    let Some(UiThemeValue::Color([r, g, b, a])) = request.theme.tokens.get(token) else {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_COLOR_THEME_TOKEN",
            format!("property {key} references missing or non-color theme token {token}"),
        ));
    };
    let _ = (frame, item);
    Ok(Some(Color::rgba(*r, *g, *b, *a)))
}

pub(super) fn non_negative_layout_property(
    node: &astra_ui_core::UiNodeBlueprint,
    key: &str,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
) -> Result<Option<f32>, UiValidationError> {
    let value = property_number(node, key, frame, item)?;
    if value.is_some_and(|value| !(0.0..=4_096.0).contains(&value)) {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_LAYOUT_METRIC_RANGE",
            format!("property {key} must be within 0..=4096"),
        ));
    }
    Ok(value)
}

pub(super) fn visual_token(node: &astra_ui_core::UiNodeBlueprint) -> Option<&UiValueExpr> {
    node.properties
        .get("asset")
        .or_else(|| node.properties.get("texture"))
}

pub(super) fn required_visual_asset(
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

pub(super) fn required_nine_slice(
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

pub(super) fn property_text(
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

pub(super) fn property_string(
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
