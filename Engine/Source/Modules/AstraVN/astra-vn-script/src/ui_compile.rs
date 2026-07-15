use std::collections::{BTreeMap, BTreeSet};

use astra_core::{DiagnosticSeverity, Hash256, SourceRef};
use astra_ui_core::{
    UiBindingManifest, UiBindingRoot, UiBlueprintBundle, UiCapability, UiEventBinding,
    UiNodeBlueprint, UiRepeatBinding, UiValue, UiValueExpr, UiViewBinding, UiViewBlueprint,
    ValidateUi,
};

use crate::lower::{lower_sources_from_cst, ParsedLine};
use crate::{AstraSource, CompiledStory, CompiledVnProject, VnError};

pub(crate) fn compile_project_ui(
    story: CompiledStory,
    sources: &[AstraSource],
) -> Result<CompiledVnProject, VnError> {
    for source in sources {
        let parsed = crate::parse_astra_source(source.path.clone(), &source.text);
        if let Some(diagnostic) = parsed
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code != "ASTRA_VN_UNKNOWN_COMMAND")
        {
            let mut diagnostic = diagnostic.clone();
            diagnostic.severity = DiagnosticSeverity::Blocking;
            return Err(VnError::Diagnostic(diagnostic));
        }
    }
    let lines = lower_sources_from_cst(sources);
    let mut views = BTreeMap::new();
    let mut command_bindings = BTreeMap::new();
    let mut system_page_bindings = BTreeMap::new();
    let mut surface_bindings = BTreeMap::new();
    let mut profile_bindings = BTreeMap::new();
    let mut source_map = BTreeMap::new();
    let mut controller_ids = BTreeSet::new();
    let mut theme_ids = BTreeSet::new();
    let mut component_ids = BTreeSet::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = &lines[index];
        if line.indent != 0 {
            return Err(diagnostic(
                line,
                "ASTRA_UI_TOP_LEVEL_INDENT",
                "UI declarations must start at column one",
            ));
        }
        match line.keyword.as_str() {
            "ui_view" => {
                let (view, next) = parse_view(&lines, index)?;
                if views.insert(view.id.clone(), view).is_some() {
                    return Err(diagnostic(
                        line,
                        "ASTRA_UI_VIEW_DUPLICATE",
                        "UI view id is already declared",
                    ));
                }
                source_map.insert(line.stable_id(), line.source_ref());
                index = next;
            }
            "ui_bind" => {
                let binding = parse_binding(line)?;
                controller_ids.insert(binding.controller_id.clone());
                theme_ids.insert(binding.theme_id.clone());
                let (kind, key) = binding_key(line)?;
                let duplicate = match kind {
                    "command" => command_bindings.insert(key, binding).is_some(),
                    "system_page" => system_page_bindings.insert(key, binding).is_some(),
                    "surface" => surface_bindings.insert(key, binding).is_some(),
                    "profile" => profile_bindings.insert(key, binding).is_some(),
                    _ => unreachable!(),
                };
                if duplicate {
                    return Err(diagnostic(
                        line,
                        "ASTRA_UI_BINDING_CONFLICT",
                        "binding precedence level has multiple entries for the same key",
                    ));
                }
                source_map.insert(line.stable_id(), line.source_ref());
                index += 1;
            }
            "ui_component" => {
                let id = required_argument(line, 0, "component id")?;
                if !component_ids.insert(id.to_string()) {
                    return Err(diagnostic(
                        line,
                        "ASTRA_UI_COMPONENT_DUPLICATE",
                        "UI component id is already declared",
                    ));
                }
                source_map.insert(line.stable_id(), line.source_ref());
                index += 1;
            }
            _ => {
                return Err(diagnostic(
                    line,
                    "ASTRA_UI_TOP_LEVEL_UNKNOWN",
                    "unknown top-level UI declaration",
                ))
            }
        }
    }

    for view in views.values() {
        theme_ids.insert(view.theme_id.clone());
        collect_component_ids(&view.root, &mut component_ids);
    }
    for binding in command_bindings
        .values()
        .chain(system_page_bindings.values())
        .chain(surface_bindings.values())
        .chain(profile_bindings.values())
    {
        if !views.contains_key(&binding.view_id) {
            return Err(VnError::diagnostic(
                "ASTRA_UI_BINDING_VIEW_MISSING",
                format!("binding references unknown view {}", binding.view_id),
            ));
        }
    }

    let mut ui_blueprints = UiBlueprintBundle {
        schema: "astra.ui_blueprint_bundle.v1".to_string(),
        views,
        hash: Hash256::from_sha256(&[]),
    };
    ui_blueprints.hash = hash_blueprints(&ui_blueprints)?;
    if !ui_blueprints.views.is_empty() {
        ui_blueprints.validate().map_err(ui_error)?;
    }
    let mut ui_bindings = UiBindingManifest {
        schema: "astra.ui_binding_manifest.v1".to_string(),
        command_bindings,
        system_page_bindings,
        surface_bindings,
        profile_bindings,
        hash: Hash256::from_sha256(&[]),
    };
    ui_bindings.hash = hash_bindings(&ui_bindings)?;
    ui_bindings.validate().map_err(ui_error)?;
    let project_hash = hash_project(&story, &ui_blueprints, &ui_bindings)?;
    Ok(CompiledVnProject {
        schema: "astra.vn.compiled_project.v1".to_string(),
        project_hash,
        story,
        ui_blueprints,
        ui_bindings,
        ui_source_map: source_map,
        controller_ids,
        theme_ids,
        component_ids,
    })
}

fn parse_view(lines: &[ParsedLine], index: usize) -> Result<(UiViewBlueprint, usize), VnError> {
    let line = &lines[index];
    let id = required_argument(line, 0, "view id")?.to_string();
    let model_schema = required_attr(line, "model")?.to_string();
    let theme_id = required_attr(line, "theme")?.to_string();
    let source_id = line.source_id.clone().ok_or_else(|| {
        diagnostic(
            line,
            "ASTRA_UI_VIEW_SOURCE_ID",
            "ui_view requires a stable #@id",
        )
    })?;
    let root_index = index + 1;
    if root_index >= lines.len() || lines[root_index].indent != 2 {
        return Err(diagnostic(
            line,
            "ASTRA_UI_VIEW_ROOT",
            "ui_view requires exactly one root widget at indent 2",
        ));
    }
    let (root, next) = parse_node(lines, root_index)?;
    if next < lines.len() && lines[next].indent != 0 {
        return Err(diagnostic(
            &lines[next],
            "ASTRA_UI_VIEW_STRUCTURE",
            "invalid widget indentation",
        ));
    }
    let mut required_capabilities = BTreeSet::new();
    infer_capabilities(&root, &mut required_capabilities);
    let view = UiViewBlueprint {
        id,
        source_id,
        model_schema,
        theme_id,
        required_capabilities: required_capabilities.into_iter().collect(),
        root,
    };
    view.validate().map_err(ui_error)?;
    Ok((view, next))
}

fn parse_node(lines: &[ParsedLine], index: usize) -> Result<(UiNodeBlueprint, usize), VnError> {
    let line = &lines[index];
    if matches!(
        line.keyword.as_str(),
        "on" | "ui_view" | "ui_bind" | "ui_component"
    ) {
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
        if matches!(
            key.as_str(),
            "id" | "items" | "item_key" | "overscan" | "component"
        ) {
            continue;
        }
        properties.insert(key.clone(), parse_expr(value, line)?);
    }
    let repeat = if let Some(items) = line.attr("items") {
        let item_key = required_attr(line, "item_key")?;
        Some(UiRepeatBinding {
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
        component_id: line.attr("component").map(str::to_string),
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

fn parse_event(line: &ParsedLine) -> Result<UiEventBinding, VnError> {
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

fn parse_binding(line: &ParsedLine) -> Result<UiViewBinding, VnError> {
    Ok(UiViewBinding {
        view_id: required_attr(line, "view")?.to_string(),
        controller_id: required_attr(line, "controller")?.to_string(),
        policy_bundle_id: required_attr(line, "policy")?.to_string(),
        theme_id: required_attr(line, "theme")?.to_string(),
    })
}

fn binding_key(line: &ParsedLine) -> Result<(&'static str, String), VnError> {
    let mut found = Vec::new();
    for kind in ["command", "system_page", "surface", "profile"] {
        if let Some(value) = line.attr(kind) {
            found.push((kind, value.to_string()));
        }
    }
    if found.len() != 1 {
        return Err(diagnostic(
            line,
            "ASTRA_UI_BINDING_KEY",
            "ui_bind requires exactly one of command/system_page/surface/profile",
        ));
    }
    Ok(found.remove(0))
}

fn parse_expr(value: &str, line: &ParsedLine) -> Result<UiValueExpr, VnError> {
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

fn parse_path(value: &str, line: &ParsedLine) -> Result<Vec<String>, VnError> {
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

fn validate_widget(line: &ParsedLine) -> Result<(), VnError> {
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
        "component_slot",
    ];
    if WIDGETS.contains(&line.keyword.as_str()) {
        Ok(())
    } else {
        Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_UNKNOWN",
            "widget is not registered in the v1 UI schema",
        ))
    }
}

fn validate_action(
    line: &ParsedLine,
    action: &str,
    arguments: &BTreeMap<String, String>,
) -> Result<(), VnError> {
    let required: &[&str] = match action {
        "vn.advance" | "vn.return_system" | "ui.close_modal" => &[],
        "vn.choose" => &["option_id"],
        "vn.open_system" => &["page"],
        "vn.request_save" | "vn.request_load" => &["slot_id"],
        "vn.set_config" => &["key", "value"],
        "vn.replay_voice" => &["voice_id"],
        "vn.start_replay" => &["replay_id"],
        "vn.request_route_jump" => &["node_id"],
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
    Ok(())
}

fn infer_capabilities(node: &UiNodeBlueprint, out: &mut BTreeSet<UiCapability>) {
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
        "component_slot" => {
            out.insert(UiCapability::ComponentSlots);
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

fn collect_component_ids(node: &UiNodeBlueprint, ids: &mut BTreeSet<String>) {
    if let Some(id) = &node.component_id {
        ids.insert(id.clone());
    }
    for child in &node.children {
        collect_component_ids(child, ids);
    }
}

fn hash_blueprints(bundle: &UiBlueprintBundle) -> Result<Hash256, VnError> {
    bundle.compute_hash().map_err(ui_error)
}

fn hash_bindings(bindings: &UiBindingManifest) -> Result<Hash256, VnError> {
    bindings.compute_hash().map_err(ui_error)
}

fn hash_project(
    story: &CompiledStory,
    blueprints: &UiBlueprintBundle,
    bindings: &UiBindingManifest,
) -> Result<Hash256, VnError> {
    Ok(Hash256::from_sha256(&postcard::to_allocvec(&(
        story.story_hash,
        blueprints.hash,
        bindings.hash,
    ))?))
}

fn required_argument<'a>(
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

fn required_attr<'a>(line: &'a ParsedLine, key: &str) -> Result<&'a str, VnError> {
    line.attr(key).ok_or_else(|| {
        diagnostic(
            line,
            "ASTRA_UI_ATTRIBUTE_MISSING",
            &format!("missing required attribute {key}"),
        )
    })
}

fn diagnostic(line: &ParsedLine, code: &str, message: &str) -> VnError {
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

fn ui_error(error: astra_ui_core::UiValidationError) -> VnError {
    VnError::diagnostic(error.code(), error.to_string())
}
