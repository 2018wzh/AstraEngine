use super::*;

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
        schema: "astra.ui_blueprint_bundle.v2".to_string(),
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
        schema: "astra.vn.compiled_project.v5".to_string(),
        project_hash,
        story,
        ui_blueprints,
        ui_bindings,
        ui_source_map: source_map,
        controller_ids,
        controller_sources,
        theme_ids,
        themes,
        system_ui_profiles,
    })
}

pub(super) fn parse_system_ui_policy(line: &ParsedLine) -> Result<SystemUiProfilePolicy, VnError> {
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

pub(super) fn split_policy_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn validate_policy_id(
    line: &ParsedLine,
    field: &str,
    value: &str,
) -> Result<(), VnError> {
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

pub(super) fn parse_view(
    lines: &[ParsedLine],
    index: usize,
) -> Result<(UiViewBlueprint, usize), VnError> {
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

pub(super) fn validate_view_bindings(view: &UiViewBlueprint) -> Result<(), VnError> {
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

pub(super) fn is_known_model_schema(schema: &str) -> bool {
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
