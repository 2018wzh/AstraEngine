use super::*;

pub(super) fn validate_node_bindings(
    node: &UiNodeBlueprint,
    model_schema: &str,
    inherited_item_collection: Option<&str>,
) -> Result<(), VnError> {
    let item_collection = node
        .repeat
        .as_ref()
        .and_then(|repeat| match &repeat.items {
            UiValueExpr::Binding {
                root: UiBindingRoot::Model,
                path,
            } => path.first().map(String::as_str),
            _ => None,
        })
        .or(inherited_item_collection);
    for expr in node
        .properties
        .values()
        .chain(
            node.events
                .iter()
                .flat_map(|event| event.arguments.values()),
        )
        .chain(node.repeat.iter().map(|repeat| &repeat.items))
    {
        validate_bound_expr(expr, model_schema, item_collection)?;
    }
    for child in &node.children {
        validate_node_bindings(child, model_schema, item_collection)?;
    }
    Ok(())
}

pub(super) fn validate_bound_expr(
    expr: &UiValueExpr,
    model_schema: &str,
    item_collection: Option<&str>,
) -> Result<(), VnError> {
    let UiValueExpr::Binding { root, path } = expr else {
        return Ok(());
    };
    let valid = match root {
        UiBindingRoot::Model => model_path_allowed(model_schema, path),
        UiBindingRoot::Item => item_collection
            .is_some_and(|collection| item_path_allowed(model_schema, collection, path)),
        UiBindingRoot::Event => path.as_slice() == ["value"] || path.as_slice() == ["node_id"],
        UiBindingRoot::State => !path.is_empty(),
    };
    if valid {
        Ok(())
    } else {
        Err(VnError::diagnostic(
            "ASTRA_UI_TYPED_BINDING_PATH",
            format!(
                "binding path {:?} is not declared by model schema {model_schema}",
                path
            ),
        ))
    }
}

pub(super) fn model_path_allowed(schema: &str, path: &[String]) -> bool {
    let first = path.first().map(String::as_str);
    if is_system_page_model_schema(schema)
        && matches!(
            first,
            Some(
                "underlay_kind"
                    | "underlay_title"
                    | "underlay_message"
                    | "underlay_choice"
                    | "underlay_text_key"
                    | "underlay_speaker_key"
            )
        )
    {
        return true;
    }
    match schema {
        "astra.vn.ui_model.message.v2" => matches!(
            first,
            Some(
                "command_id"
                    | "text_key"
                    | "speaker_key"
                    | "voice_id"
                    | "auto_enabled"
                    | "skip_mode"
                    | "visible_graphemes"
                    | "text_graphemes"
                    | "reveal_complete"
            )
        ),
        "astra.vn.ui_model.choice.v1" => {
            matches!(first, Some("choice_id" | "prompt_key" | "options"))
        }
        "astra.vn.ui_model.title.v1" => first == Some("can_continue"),
        "astra.vn.ui_model.quick_panel.v1" => {
            matches!(first, Some("auto_enabled" | "skip_mode"))
        }
        "astra.vn.ui_model.config.v1" => matches!(
            first,
            Some(
                "master_volume"
                    | "reading_hidden"
                    | "reading_manual"
                    | "reading_fast_forward"
                    | "audio_enabled"
                    | "audio_disabled"
                    | "text_speed"
                    | "auto_delay_ms"
                    | "high_contrast"
                    | "locale"
                    | "available_locales"
                    | "player_name"
            )
        ),
        "astra.vn.ui_model.save.v1" | "astra.vn.ui_model.load.v1" => first == Some("slots"),
        "astra.vn.ui_model.backlog.v1" | "astra.vn.ui_model.voice_replay.v1" => {
            first == Some("entries")
        }
        "astra.vn.ui_model.gallery.v1" | "astra.vn.ui_model.replay.v1" => first == Some("items"),
        "astra.vn.ui_model.route_chart.v1" => first == Some("nodes"),
        "astra.vn.ui_model.localization_preview.v1" => {
            matches!(first, Some("locale" | "entries"))
        }
        "astra.vn.ui_model.text_input.v1" => first == Some("input"),
        "astra.vn.ui_model.system.v1" => true,
        _ => false,
    }
}

pub(super) fn is_system_page_model_schema(schema: &str) -> bool {
    matches!(
        schema,
        "astra.vn.ui_model.title.v1"
            | "astra.vn.ui_model.quick_panel.v1"
            | "astra.vn.ui_model.config.v1"
            | "astra.vn.ui_model.save.v1"
            | "astra.vn.ui_model.load.v1"
            | "astra.vn.ui_model.backlog.v1"
            | "astra.vn.ui_model.gallery.v1"
            | "astra.vn.ui_model.replay.v1"
            | "astra.vn.ui_model.voice_replay.v1"
            | "astra.vn.ui_model.route_chart.v1"
            | "astra.vn.ui_model.localization_preview.v1"
            | "astra.vn.ui_model.text_input.v1"
            | "astra.vn.ui_model.system.v1"
    )
}

pub(super) fn item_path_allowed(schema: &str, collection: &str, path: &[String]) -> bool {
    let first = path.first().map(String::as_str);
    match (schema, collection) {
        ("astra.vn.ui_model.choice.v1", "options") => {
            matches!(first, Some("option_id" | "text_key" | "enabled"))
        }
        ("astra.vn.ui_model.save.v1" | "astra.vn.ui_model.load.v1", "slots") => matches!(
            first,
            Some(
                "slot_id"
                    | "occupied"
                    | "thumbnail_asset"
                    | "has_thumbnail"
                    | "title_key"
                    | "timestamp_text"
                    | "playtime_text"
                    | "metadata_text"
                    | "can_write"
                    | "can_load"
                    | "migration_status"
            )
        ),
        ("astra.vn.ui_model.backlog.v1" | "astra.vn.ui_model.voice_replay.v1", "entries") => {
            matches!(
                first,
                Some(
                    "command_id"
                        | "text_key"
                        | "speaker_key"
                        | "voice_id"
                        | "has_voice"
                        | "can_jump"
                        | "read"
                )
            )
        }
        ("astra.vn.ui_model.gallery.v1" | "astra.vn.ui_model.replay.v1", "items") => matches!(
            first,
            Some("item_id" | "label_key" | "thumbnail_asset" | "has_thumbnail" | "unlocked")
        ),
        ("astra.vn.ui_model.localization_preview.v1", "entries") => {
            matches!(first, Some("entry_id" | "text_key"))
        }
        ("astra.vn.ui_model.route_chart.v1", "nodes") => matches!(
            first,
            Some("node_id" | "label_key" | "terminal" | "reached" | "x_milli" | "y_milli")
        ),
        _ => false,
    }
}

pub(super) fn parse_node(
    lines: &[ParsedLine],
    index: usize,
) -> Result<(UiNodeBlueprint, usize), VnError> {
    let line = &lines[index];
    if matches!(line.keyword.as_str(), "on" | "ui_view" | "ui_bind") {
        return Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_EXPECTED",
            "expected a widget declaration",
        ));
    }
    validate_widget(line)?;
    let local_id = required_attr(line, "id")?.to_string();
    let mut properties = BTreeMap::new();
    for (key, value) in &line.attrs {
        if key == "id"
            || (line.keyword != "select"
                && matches!(key.as_str(), "items" | "item_key" | "overscan"))
        {
            continue;
        }
        properties.insert(key.clone(), parse_expr(value, line)?);
    }
    let repeat = if line.keyword != "select" {
        line.attr("items")
            .map(|items| {
                let item_key = required_attr(line, "item_key")?;
                Ok::<UiRepeatBinding, VnError>(UiRepeatBinding {
                    items: parse_expr(items, line)?,
                    item_key_path: parse_path(item_key.trim_start_matches("$item."), line)?,
                    overscan: line.attr("overscan").unwrap_or("0").parse().map_err(|_| {
                        diagnostic(
                            line,
                            "ASTRA_UI_OVERSCAN",
                            "overscan must be an unsigned 16-bit integer",
                        )
                    })?,
                })
            })
            .transpose()?
    } else {
        None
    };
    let mut node = UiNodeBlueprint {
        source_id: line
            .source_id
            .clone()
            .unwrap_or_else(|| format!("{}/{}", line.source, local_id)),
        local_id,
        widget: line.keyword.clone(),
        properties,
        events: Vec::new(),
        children: Vec::new(),
        repeat,
    };
    let mut next = index + 1;
    while next < lines.len() && lines[next].indent > line.indent {
        let child = &lines[next];
        if child.indent != line.indent + 2 {
            return Err(diagnostic(
                child,
                "ASTRA_UI_WIDGET_INDENT",
                "widget children and events must increase indentation by exactly 2",
            ));
        }
        if child.keyword == "on" {
            validate_widget_event(&node.widget, child)?;
            node.events.push(parse_event(child)?);
            next += 1;
        } else {
            let (parsed, after) = parse_node(lines, next)?;
            node.children.push(parsed);
            next = after;
        }
    }
    Ok((node, next))
}

pub(super) fn parse_event(line: &ParsedLine) -> Result<UiEventBinding, VnError> {
    if line.args.len() != 1 || line.attr("target").is_none() {
        return Err(diagnostic(
            line,
            "ASTRA_UI_EVENT_SYNTAX",
            "event syntax is `on <event> -> <action>`",
        ));
    }
    let event = line.args[0].clone();
    let action_id = line.attr("target").unwrap_or_default().to_string();
    let action_attrs = line
        .attrs
        .iter()
        .filter(|(key, _)| key.as_str() != "target")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    validate_action(line, &action_id, &action_attrs)?;
    let arguments = line
        .attrs
        .iter()
        .filter(|(key, _)| key.as_str() != "target")
        .map(|(key, value)| Ok((key.clone(), parse_expr(value, line)?)))
        .collect::<Result<_, VnError>>()?;
    Ok(UiEventBinding {
        event,
        action_id,
        arguments,
    })
}

pub(super) fn parse_binding(line: &ParsedLine) -> Result<UiViewBinding, VnError> {
    Ok(UiViewBinding {
        view_id: required_attr(line, "view")?.to_string(),
        controller_id: required_attr(line, "controller")?.to_string(),
        policy_bundle_id: required_attr(line, "policy")?.to_string(),
        theme_id: required_attr(line, "theme")?.to_string(),
    })
}

pub(super) fn binding_key(
    line: &ParsedLine,
) -> Result<(&'static str, String, Option<String>), VnError> {
    let mut found = Vec::new();
    for kind in ["command", "system_page", "surface"] {
        if let Some(value) = line.attr(kind) {
            found.push((kind, value.to_string()));
        }
    }
    if found.is_empty() {
        if let Some(profile) = line.attr("profile") {
            return Ok(("profile", profile.to_string(), None));
        }
    }
    if found.len() != 1 {
        return Err(diagnostic(
            line,
            "ASTRA_UI_BINDING_KEY",
            "ui_bind requires exactly one of command/system_page/surface, or a profile fallback",
        ));
    }
    let (kind, key) = found.remove(0);
    Ok((kind, key, line.attr("profile").map(str::to_string)))
}

pub(super) fn parse_expr(value: &str, line: &ParsedLine) -> Result<UiValueExpr, VnError> {
    for (prefix, root) in [
        ("$model.", UiBindingRoot::Model),
        ("$item.", UiBindingRoot::Item),
        ("$event.", UiBindingRoot::Event),
        ("$state.", UiBindingRoot::State),
    ] {
        if let Some(path) = value.strip_prefix(prefix) {
            return Ok(UiValueExpr::Binding {
                root,
                path: parse_path(path, line)?,
            });
        }
    }
    if value.starts_with('$') {
        return Err(diagnostic(
            line,
            "ASTRA_UI_BINDING_ROOT",
            "binding root must be $model/$item/$event/$state",
        ));
    }
    if let Some(key) = value.strip_prefix("l10n:") {
        return Ok(UiValueExpr::LocalizationKey {
            key: key.to_string(),
        });
    }
    if let Some(asset_id) = value.strip_prefix("asset:") {
        return Ok(UiValueExpr::AssetRef {
            asset_id: asset_id.to_string(),
        });
    }
    if let Some(token) = value.strip_prefix("theme:") {
        return Ok(UiValueExpr::ThemeToken {
            token: token.to_string(),
        });
    }
    let literal = match value {
        "true" => UiValue::Bool(true),
        "false" => UiValue::Bool(false),
        "null" => UiValue::Null,
        _ if value.parse::<i64>().is_ok() => {
            UiValue::Integer(value.parse().expect("checked integer"))
        }
        _ if value.parse::<f64>().is_ok() => {
            UiValue::Number(value.parse().expect("checked number"))
        }
        _ => UiValue::String(value.to_string()),
    };
    Ok(UiValueExpr::Literal { value: literal })
}

pub(super) fn parse_path(value: &str, line: &ParsedLine) -> Result<Vec<String>, VnError> {
    let path: Vec<String> = value.split('.').map(str::to_string).collect();
    if path.is_empty() || path.iter().any(String::is_empty) {
        return Err(diagnostic(
            line,
            "ASTRA_UI_BINDING_PATH",
            "binding path contains an empty segment",
        ));
    }
    Ok(path)
}

pub(super) fn validate_widget(line: &ParsedLine) -> Result<(), VnError> {
    const WIDGETS: &[&str] = &[
        "screen",
        "row",
        "column",
        "stack",
        "panel",
        "image",
        "nine_slice",
        "button",
        "slider",
        "toggle",
        "select",
        "scroll",
        "virtual_list",
        "virtual_grid",
        "modal",
        "canvas",
        "semantic_region",
        "text",
        "text_input",
        "spacer",
    ];
    if !WIDGETS.contains(&line.keyword.as_str()) {
        return Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_UNKNOWN",
            "widget is not registered in the v1 UI schema",
        ));
    }
    validate_widget_properties(line)
}
