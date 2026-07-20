use std::collections::{BTreeMap, BTreeSet};

use astra_core::{DiagnosticSeverity, Hash256, SourceRef};
use astra_ui_core::{
    UiBindingManifest, UiBindingRoot, UiBlueprintBundle, UiCapability, UiEventBinding,
    UiNodeBlueprint, UiProfileScopedBindings, UiRepeatBinding, UiThemeManifest, UiValue,
    UiValueExpr, UiViewBinding, UiViewBlueprint, ValidateUi,
};

use crate::lower::{lower_sources_from_cst, ParsedLine};
use crate::{
    AstraSource, CompiledStory, CompiledVnProject, ReadingMode, SaveCompletionPolicy,
    SystemPageKind, SystemUiProfilePolicy, VnError,
};

pub(crate) fn compile_project_ui(
    story: CompiledStory,
    sources: &[AstraSource],
    ui_themes: Vec<UiThemeManifest>,
    controller_sources: BTreeMap<String, String>,
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
    let mut profile_scoped_bindings = BTreeMap::<String, UiProfileScopedBindings>::new();
    let mut system_ui_profiles = BTreeMap::<String, SystemUiProfilePolicy>::new();
    let mut source_map = BTreeMap::new();
    let mut controller_ids = BTreeSet::new();
    let mut theme_ids = BTreeSet::new();
    let mut component_ids = BTreeSet::new();
    let mut themes = BTreeMap::new();
    for theme in ui_themes {
        theme.validate().map_err(ui_error)?;
        if themes.insert(theme.id.clone(), theme).is_some() {
            return Err(VnError::message(
                "ASTRA_UI_THEME_DUPLICATE: theme id is already registered",
            ));
        }
    }
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
                let (kind, key, profile) = binding_key(line)?;
                let duplicate = match (profile, kind) {
                    (Some(profile), "command") => profile_scoped_bindings
                        .entry(profile)
                        .or_default()
                        .command_bindings
                        .insert(key, binding)
                        .is_some(),
                    (Some(profile), "system_page") => profile_scoped_bindings
                        .entry(profile)
                        .or_default()
                        .system_page_bindings
                        .insert(key, binding)
                        .is_some(),
                    (Some(profile), "surface") => profile_scoped_bindings
                        .entry(profile)
                        .or_default()
                        .surface_bindings
                        .insert(key, binding)
                        .is_some(),
                    (None, "command") => command_bindings.insert(key, binding).is_some(),
                    (None, "system_page") => system_page_bindings.insert(key, binding).is_some(),
                    (None, "surface") => surface_bindings.insert(key, binding).is_some(),
                    (None, "profile") => profile_bindings.insert(key, binding).is_some(),
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
            "ui_policy" => {
                let policy = parse_system_ui_policy(line)?;
                if system_ui_profiles
                    .insert(policy.profile_id.clone(), policy)
                    .is_some()
                {
                    return Err(diagnostic(
                        line,
                        "ASTRA_UI_SYSTEM_POLICY_DUPLICATE",
                        "UI profile declares more than one system policy",
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

    let bound_profiles = profile_scoped_bindings
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let policy_profiles = system_ui_profiles.keys().cloned().collect::<BTreeSet<_>>();
    if !policy_profiles.is_empty() && bound_profiles != policy_profiles {
        return Err(VnError::message(
            "ASTRA_UI_SYSTEM_POLICY_PROFILE_SET: every profile-scoped UI binding must have exactly one ui_policy",
        ));
    }

    for view in views.values() {
        theme_ids.insert(view.theme_id.clone());
        collect_component_ids(&view.root, &mut component_ids);
    }
    if !views.is_empty() {
        for theme_id in &theme_ids {
            if !themes.contains_key(theme_id) {
                return Err(VnError::diagnostic(
                    "ASTRA_UI_THEME_MISSING",
                    format!("UI references unregistered theme {theme_id}"),
                ));
            }
        }
    }
    if controller_ids != controller_sources.keys().cloned().collect() {
        return Err(VnError::message(
            "ASTRA_UI_CONTROLLER_SOURCE_SET: every bound controller requires exactly one validated source",
        ));
    }
    for source in controller_sources.values() {
        astra_ui_core::validate_ui_source_text(source).map_err(ui_error)?;
    }
    for binding in command_bindings
        .values()
        .chain(system_page_bindings.values())
        .chain(surface_bindings.values())
        .chain(profile_bindings.values())
        .chain(profile_scoped_bindings.values().flat_map(|scoped| {
            scoped
                .command_bindings
                .values()
                .chain(scoped.system_page_bindings.values())
                .chain(scoped.surface_bindings.values())
        }))
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
        profile_scoped_bindings,
        hash: Hash256::from_sha256(&[]),
    };
    ui_bindings.hash = hash_bindings(&ui_bindings)?;
    ui_bindings.validate().map_err(ui_error)?;
    let project_hash = hash_project(
        &story,
        &ui_blueprints,
        &ui_bindings,
        &themes,
        &controller_sources,
        &system_ui_profiles,
    )?;
    Ok(CompiledVnProject {
        schema: "astra.vn.compiled_project.v3".to_string(),
        project_hash,
        story,
        ui_blueprints,
        ui_bindings,
        ui_source_map: source_map,
        controller_ids,
        controller_sources,
        theme_ids,
        themes,
        component_ids,
        system_ui_profiles,
    })
}

fn parse_system_ui_policy(line: &ParsedLine) -> Result<SystemUiProfilePolicy, VnError> {
    let profile_id = required_attr(line, "profile")?.to_string();
    validate_policy_id(line, "profile", &profile_id)?;
    let save_slot_ids = split_policy_list(required_attr(line, "save_slots")?);
    if save_slot_ids.is_empty() {
        return Err(diagnostic(
            line,
            "ASTRA_UI_SYSTEM_SAVE_SLOTS",
            "save_slots must not be empty",
        ));
    }
    let mut unique_slots = BTreeSet::new();
    for slot in &save_slot_ids {
        validate_policy_id(line, "save_slots", slot)?;
        if !unique_slots.insert(slot.clone()) {
            return Err(diagnostic(
                line,
                "ASTRA_UI_SYSTEM_SAVE_SLOT_DUPLICATE",
                "save slot ids must be unique",
            ));
        }
    }
    let quick_slot_id = line
        .attr("quick_slot")
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(slot) = &quick_slot_id {
        validate_policy_id(line, "quick_slot", slot)?;
        if !unique_slots.contains(slot) {
            return Err(diagnostic(
                line,
                "ASTRA_UI_SYSTEM_QUICK_SLOT",
                "quick_slot must also be declared in save_slots",
            ));
        }
    }
    let allowed_pages = split_policy_list(required_attr(line, "allowed_pages")?)
        .into_iter()
        .map(|value| {
            let page = SystemPageKind::parse(&value);
            if page == SystemPageKind::Unknown {
                Err(diagnostic(
                    line,
                    "ASTRA_UI_SYSTEM_PAGE_UNKNOWN",
                    "allowed_pages contains an unknown page",
                ))
            } else {
                Ok(page)
            }
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let reading_modes = split_policy_list(required_attr(line, "reading_modes")?)
        .into_iter()
        .map(|value| match value.as_str() {
            "hidden" => Ok(ReadingMode::Hidden),
            "manual" => Ok(ReadingMode::Manual),
            "fast_forward" => Ok(ReadingMode::FastForward),
            _ => Err(diagnostic(
                line,
                "ASTRA_UI_READING_MODE_UNKNOWN",
                "reading_modes contains an unknown mode",
            )),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if !reading_modes.contains(&ReadingMode::Manual) {
        return Err(diagnostic(
            line,
            "ASTRA_UI_READING_MODE_MANUAL",
            "reading_modes must include manual",
        ));
    }
    let audio_toggle = match required_attr(line, "audio_toggle")? {
        "true" => true,
        "false" => false,
        _ => {
            return Err(diagnostic(
                line,
                "ASTRA_UI_AUDIO_TOGGLE",
                "audio_toggle must be true or false",
            ))
        }
    };
    let save_completion = match required_attr(line, "save_completion")? {
        "stay" => SaveCompletionPolicy::Stay,
        "return_system" => SaveCompletionPolicy::ReturnSystem,
        _ => {
            return Err(diagnostic(
                line,
                "ASTRA_UI_SAVE_COMPLETION",
                "save_completion must be stay or return_system",
            ))
        }
    };
    let custom_action_ids = line
        .attr("custom_actions")
        .map(split_policy_list)
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            validate_policy_id(line, "custom_actions", &id)?;
            Ok(id)
        })
        .collect::<Result<BTreeSet<_>, VnError>>()?;
    Ok(SystemUiProfilePolicy {
        profile_id,
        save_slot_ids,
        quick_slot_id,
        allowed_pages,
        reading_modes,
        audio_toggle,
        save_completion,
        custom_action_ids,
    })
}

fn split_policy_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn validate_policy_id(line: &ParsedLine, field: &str, value: &str) -> Result<(), VnError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(diagnostic(
            line,
            "ASTRA_UI_SYSTEM_POLICY_ID",
            &format!("{field} must be a bounded safe identifier"),
        ));
    }
    Ok(())
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
    validate_view_bindings(&view)?;
    view.validate().map_err(ui_error)?;
    Ok((view, next))
}

fn validate_view_bindings(view: &UiViewBlueprint) -> Result<(), VnError> {
    if !is_known_model_schema(&view.model_schema) {
        return Err(VnError::diagnostic(
            "ASTRA_UI_MODEL_SCHEMA_UNKNOWN",
            format!(
                "UI view references unknown model schema {}",
                view.model_schema
            ),
        ));
    }
    validate_node_bindings(&view.root, &view.model_schema, None)
}

fn is_known_model_schema(schema: &str) -> bool {
    matches!(
        schema,
        "astra.vn.ui_model.message.v2"
            | "astra.vn.ui_model.choice.v1"
            | "astra.vn.ui_model.title.v1"
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

fn validate_node_bindings(
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

fn validate_bound_expr(
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

fn model_path_allowed(schema: &str, path: &[String]) -> bool {
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

fn is_system_page_model_schema(schema: &str) -> bool {
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

fn item_path_allowed(schema: &str, collection: &str, path: &[String]) -> bool {
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
        if key == "id"
            || key == "component"
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

fn binding_key(line: &ParsedLine) -> Result<(&'static str, String, Option<String>), VnError> {
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
    if !WIDGETS.contains(&line.keyword.as_str()) {
        return Err(diagnostic(
            line,
            "ASTRA_UI_WIDGET_UNKNOWN",
            "widget is not registered in the v1 UI schema",
        ));
    }
    validate_widget_properties(line)
}

fn validate_widget_properties(line: &ParsedLine) -> Result<(), VnError> {
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
        "component_slot" => &["component"],
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

fn validate_widget_event(widget: &str, line: &ParsedLine) -> Result<(), VnError> {
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

fn validate_action(
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
