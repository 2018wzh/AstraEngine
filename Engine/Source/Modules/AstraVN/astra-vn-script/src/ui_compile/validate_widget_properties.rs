use super::*;

pub(super) fn validate_widget_properties(line: &ParsedLine) -> Result<(), VnError> {
    const COMMON: &[&str] = &[
        "id",
        "visible",
        "enabled",
        "min_width",
        "min_height",
        "max_width",
        "max_height",
        "fill",
        "fill_width",
        "fill_height",
        "clip_children",
        "background",
        "text_color",
        "text_padding",
        "grow",
        "anchor",
        "position_x",
        "position_y",
        "position_right",
        "position_bottom",
        "align",
        "cross_align",
        "gap",
        "padding",
        "style",
    ];
    const REPEAT: &[&str] = &[
        "items",
        "item_key",
        "overscan",
        "item_extent",
        "viewport_extent",
    ];
    let widget_properties: &[&str] = match line.keyword.as_str() {
        "text" => &[
            "text",
            "value",
            "text_key",
            "direction",
            "text_align",
            "vertical_align",
            "max_lines",
            "font_size",
            "visible_graphemes",
        ],
        "button" => &[
            "text",
            "value",
            "text_key",
            "label_key",
            "selected",
            "direction",
            "max_lines",
            "font_size",
        ],
        "slider" => &["value", "min", "max", "step", "label_key"],
        "toggle" => &[
            "checked",
            "label_key",
            "value",
            "indicator_color",
            "indicator_off_color",
        ],
        "select" => &["value", "items", "label_key", "item_key"],
        "text_input" => &[
            "value",
            "multiline",
            "max_graphemes",
            "character_policy",
            "direction",
            "max_lines",
            "font_size",
        ],
        "image" => &["asset", "texture", "fit", "opacity"],
        "nine_slice" => &["asset", "texture", "opacity"],
        "virtual_list" => REPEAT,
        "virtual_grid" => &[
            "items",
            "item_key",
            "overscan",
            "item_extent",
            "viewport_extent",
            "columns",
        ],
        "canvas" => &["nodes", "edges"],
        "screen" | "row" | "column" | "stack" | "panel" | "scroll" | "modal"
        | "semantic_region" | "spacer" => &[],
        _ => &[],
    };
    if let Some((name, _)) = line.attrs.iter().find(|(name, _)| {
        !COMMON.contains(&name.as_str()) && !widget_properties.contains(&name.as_str())
    }) {
        return Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_PROPERTY_UNKNOWN",
            &format!("widget {} does not declare property {name}", line.keyword),
        ));
    }
    if matches!(line.keyword.as_str(), "image" | "nine_slice")
        && line.attr("asset").is_none()
        && line.attr("texture").is_none()
    {
        return Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_ASSET_REQUIRED",
            &format!("widget {} requires asset or texture", line.keyword),
        ));
    }
    if matches!(line.keyword.as_str(), "virtual_list" | "virtual_grid")
        && (line.attr("items").is_none() || line.attr("item_key").is_none())
    {
        return Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_REPEAT_REQUIRED",
            &format!("widget {} requires items and item_key", line.keyword),
        ));
    }
    Ok(())
}

pub(super) fn validate_widget_event(widget: &str, line: &ParsedLine) -> Result<(), VnError> {
    let event = line.args.first().map(String::as_str).unwrap_or_default();
    let allowed: &[&str] = match widget {
        "button" | "panel" | "semantic_region" | "image" | "nine_slice" => &["activate"],
        "slider" | "toggle" | "select" => &["change", "activate"],
        "text_input" => &["change", "submit", "activate"],
        "modal" => &["dismiss"],
        "canvas" => &["activate", "node_activate", "node_hover"],
        _ => &[],
    };
    if allowed.contains(&event) {
        Ok(())
    } else {
        Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_EVENT_UNSUPPORTED",
            &format!("widget {widget} does not expose event {event}"),
        ))
    }
}

pub(super) fn validate_action(
    line: &ParsedLine,
    action: &str,
    arguments: &BTreeMap<String, String>,
) -> Result<(), VnError> {
    let required: &[&str] = match action {
        "vn.advance" | "vn.return_system" | "vn.request_exit" | "ui.close_modal" => &[],
        "vn.choose" => &["option_id"],
        "vn.open_system" | "vn.switch_system" => &["page"],
        "vn.request_save"
        | "vn.request_save_confirmed"
        | "vn.request_load"
        | "vn.request_delete_save" => &["slot_id"],
        "vn.set_config" => &["key", "value"],
        "vn.set_auto" => &["enabled"],
        "vn.set_skip" => &["mode"],
        "vn.set_reading_mode" => &["mode"],
        "vn.set_audio_enabled" => &["enabled"],
        "vn.invoke_system_action" => &["action_id"],
        "vn.replay_voice" => &["voice_id"],
        "vn.start_replay" => &["replay_id"],
        "vn.preview_gallery" => &["item_id"],
        "vn.request_route_jump" => &["node_id"],
        "vn.request_backlog_jump" => &["command_id"],
        "vn.submit_text" => &["input_id", "value"],
        "ui.open_modal" => &["view_id"],
        "ui.set_state" => &["key", "value"],
        _ => {
            return Err(diagnostic(
                line,
                "ASTRA_UI_ACTION_UNKNOWN",
                "action is not registered",
            ))
        }
    };
    if let Some(missing) = required.iter().find(|name| !arguments.contains_key(**name)) {
        return Err(diagnostic(
            line,
            "ASTRA_UI_ACTION_ARGUMENT_MISSING",
            &format!("action {action} requires argument {missing}"),
        ));
    }
    let optional: &[&str] = match action {
        "ui.open_modal" => &["model"],
        _ => &[],
    };
    if let Some(unexpected) = arguments
        .keys()
        .find(|name| !required.contains(&name.as_str()) && !optional.contains(&name.as_str()))
    {
        return Err(diagnostic(
            line,
            "ASTRA_UI_ACTION_ARGUMENT_UNKNOWN",
            &format!("action {action} does not declare argument {unexpected}"),
        ));
    }
    Ok(())
}

pub(super) fn infer_capabilities(node: &UiNodeBlueprint, out: &mut BTreeSet<UiCapability>) {
    match node.widget.as_str() {
        "virtual_list" => {
            out.insert(UiCapability::VirtualList);
        }
        "virtual_grid" => {
            out.insert(UiCapability::VirtualGrid);
        }
        "nine_slice" => {
            out.insert(UiCapability::NineSlice);
        }
        "canvas" => {
            out.insert(UiCapability::Canvas);
        }
        "text_input" => {
            out.insert(UiCapability::TextInput);
            out.insert(UiCapability::Ime);
        }
        _ => {}
    }
    if !node.events.is_empty() {
        out.insert(UiCapability::Pointer);
        out.insert(UiCapability::Keyboard);
        out.insert(UiCapability::GamepadNavigation);
    }
    for child in &node.children {
        infer_capabilities(child, out);
    }
}

pub(super) fn hash_blueprints(bundle: &UiBlueprintBundle) -> Result<Hash256, VnError> {
    bundle.compute_hash().map_err(ui_error)
}

pub(super) fn hash_bindings(bindings: &UiBindingManifest) -> Result<Hash256, VnError> {
    bindings.compute_hash().map_err(ui_error)
}

pub(super) fn hash_project(
    story: &CompiledStory,
    blueprints: &UiBlueprintBundle,
    bindings: &UiBindingManifest,
    themes: &BTreeMap<String, UiThemeManifest>,
    controller_sources: &BTreeMap<String, String>,
    system_ui_profiles: &BTreeMap<String, SystemUiProfilePolicy>,
) -> Result<Hash256, VnError> {
    Ok(Hash256::from_sha256(&postcard::to_allocvec(&(
        story.story_hash,
        blueprints.hash,
        bindings.hash,
        themes,
        controller_sources,
        system_ui_profiles,
    ))?))
}

pub(super) fn required_argument<'a>(
    line: &'a ParsedLine,
    index: usize,
    field: &str,
) -> Result<&'a str, VnError> {
    line.args.get(index).map(String::as_str).ok_or_else(|| {
        diagnostic(
            line,
            "ASTRA_UI_ARGUMENT_MISSING",
            &format!("missing {field}"),
        )
    })
}

pub(super) fn required_attr<'a>(line: &'a ParsedLine, key: &str) -> Result<&'a str, VnError> {
    line.attr(key).ok_or_else(|| {
        diagnostic(
            line,
            "ASTRA_UI_ATTRIBUTE_MISSING",
            &format!("missing required attribute {key}"),
        )
    })
}

pub(super) fn diagnostic(line: &ParsedLine, code: &str, message: &str) -> VnError {
    VnError::Diagnostic(
        astra_core::Diagnostic::blocking(code, message)
            .with_source(SourceRef {
                source: line.source.clone(),
                line: line.line as u32,
                column: line.column as u32,
                length: line.keyword.len() as u32,
            })
            .with_field("keyword", &line.keyword),
    )
}

pub(super) fn ui_error(error: astra_ui_core::UiValidationError) -> VnError {
    VnError::diagnostic(error.code(), error.to_string())
}
