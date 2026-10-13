use super::*;

pub(super) fn bounded_usize_property(
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

pub(super) fn semantic_text_properties(
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
    for (source, target, allowed) in [
        ("text_align", "text.align", ["start", "center", "end"]),
        (
            "vertical_align",
            "text.vertical_align",
            ["start", "center", "end"],
        ),
    ] {
        if let Some(expr) = node.properties.get(source) {
            let UiValue::String(value) = evaluate(expr, frame, item, None)? else {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXT_ALIGNMENT_TYPE",
                    format!("{source} must resolve to a string"),
                ));
            };
            if !allowed.contains(&value.as_str()) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_TEXT_ALIGNMENT",
                    format!("{source} must be start, center or end"),
                ));
            }
            properties.insert(target.into(), value);
        }
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

pub(super) fn repeat_key(node: &astra_ui_core::UiNodeBlueprint, item: &UiValue) -> Option<String> {
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

pub(super) fn next_select_value(
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

pub(super) fn action_from_event(
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
    action_from_event_sequence(
        event,
        target,
        input_sequence,
        request.generation,
        frame,
        item,
        event_value,
    )
}

pub(super) fn action_from_event_sequence(
    event: &UiEventBinding,
    target: &str,
    input_sequence: u64,
    semantic_generation: u64,
    frame: &UiBlueprintFrameModel,
    item: Option<&UiValue>,
    event_value: Option<&UiValue>,
) -> Result<UiActionEnvelope, UiValidationError> {
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
        semantic_generation,
    })
}

pub(super) fn evaluate(
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

pub(super) fn evaluate_collection<'a>(
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

pub(super) fn lookup<'a>(value: &'a UiValue, path: &[String]) -> Option<&'a UiValue> {
    path.iter()
        .try_fold(value, |current, segment| match current {
            UiValue::Map(values) => values.get(segment),
            _ => None,
        })
}
