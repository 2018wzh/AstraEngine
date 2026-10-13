use super::*;

#[test]
pub(super) fn requested_focus_activates_a_button_without_prior_navigation() {
    let mut button = node("confirm", "button");
    button.properties.insert(
        "selected".into(),
        UiValueExpr::Literal {
            value: UiValue::Bool(true),
        },
    );
    button.events.push(UiEventBinding {
        event: "activate".into(),
        action_id: "vn.advance".into(),
        arguments: BTreeMap::new(),
    });
    let mut alternate = node("alternate", "button");
    alternate.events = button.events.clone();
    let mut root = node("root", "screen");
    root.children.push(button);
    root.children.push(alternate);
    let view = UiViewBlueprint {
        id: "ui.focus".into(),
        source_id: "ui.focus".into(),
        model_schema: "model.focus.v1".into(),
        theme_id: "theme.focus".into(),
        required_capabilities: Vec::new(),
        root,
    };
    let mut bundle = UiBlueprintBundle {
        schema: "astra.ui_blueprint_bundle.v2".into(),
        views: BTreeMap::from([(view.id.clone(), view)]),
        hash: Hash256::from_sha256(&[]),
    };
    bundle.hash = bundle.compute_hash().expect("bundle hash");
    let theme = Arc::new(UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "theme.focus".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        revision: 1,
    });
    let request = |focus_request, events| {
        let frame = UiBlueprintFrameModel {
            schema: "astra.ui_blueprint_frame_model.v1".into(),
            view_id: "ui.focus".into(),
            model: UiValue::Map(BTreeMap::new()),
            state: UiValue::Map(BTreeMap::new()),
            modals: Vec::new(),
            focus_request,
            localization: BTreeMap::new(),
        };
        UiFrameRequest {
            schema: "astra.ui_frame_request.v1".into(),
            session_id: "session.focus".into(),
            generation: 1,
            viewport: UiViewport {
                physical_width: 800,
                physical_height: 600,
                scale_factor: 1.0,
                font_scale: 1.0,
                safe_area_points: UiInsets {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                },
            },
            fixed_time_ns: 0,
            input: UiInputFrame {
                schema: "astra.ui_input_frame.v1".into(),
                events,
            },
            theme: theme.clone(),
            model_schema: "model.focus.v1".into(),
            model_revision: 1,
            model: frame.clone(),
        }
    };
    let renderer = BlueprintYakuiRenderer::new(bundle).expect("renderer");
    let mut backend =
        AstraYakuiBackend::new(renderer, Hash256::from_sha256(b"focus-test")).expect("backend");
    let focused = backend
        .render_frame(request(Some("root/confirm".into()), Vec::new()))
        .expect("focus frame");
    assert_eq!(
        focused
            .semantics
            .nodes
            .iter()
            .filter(|node| node.focused)
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        vec!["root/confirm"]
    );
    let retained = backend
        .renderer_mut()
        .resolve_retained_activation(focused.semantics.generation, "root/confirm", 1)
        .expect("retained activation");
    assert_eq!(retained.action_id, "vn.advance");
    assert_eq!(retained.semantic_target_id, "root/confirm");
    assert_eq!(retained.semantic_generation, focused.semantics.generation);
    let stale = backend
        .renderer_mut()
        .resolve_retained_activation(focused.semantics.generation + 1, "root/confirm", 1)
        .expect_err("stale retained activation");
    assert_eq!(stale.code(), "ASTRA_UI_RETAINED_SEMANTIC_STALE");
    let activated = backend
        .render_frame(request(
            None,
            vec![UiInputEvent {
                sequence: 1,
                kind: UiInputEventKind::Keyboard {
                    logical_key: "Enter".into(),
                    physical_key: "Enter".into(),
                    state: UiButtonState::Pressed,
                    repeat: false,
                    modifiers: 0,
                },
            }],
        ))
        .expect("activate frame");
    assert_eq!(activated.actions.len(), 1);
    assert_eq!(activated.actions[0].action_id, "vn.advance");
    assert_eq!(activated.actions[0].semantic_target_id, "root/confirm");

    backend
        .render_frame(request(
            None,
            vec![UiInputEvent {
                sequence: 2,
                kind: UiInputEventKind::Keyboard {
                    logical_key: "Enter".into(),
                    physical_key: "Enter".into(),
                    state: UiButtonState::Released,
                    repeat: false,
                    modifiers: 0,
                },
            }],
        ))
        .expect("release frame");
    let navigated = backend
        .render_frame(request(
            None,
            vec![UiInputEvent {
                sequence: 3,
                kind: UiInputEventKind::Keyboard {
                    logical_key: "ArrowDown".into(),
                    physical_key: "ArrowDown".into(),
                    state: UiButtonState::Pressed,
                    repeat: false,
                    modifiers: 0,
                },
            }],
        ))
        .expect("navigation frame");
    assert!(navigated
        .semantics
        .nodes
        .iter()
        .any(|node| node.id == "root/alternate" && node.focused));
    assert!(navigated
        .semantics
        .nodes
        .iter()
        .any(|node| node.id == "root/confirm" && node.selected && !node.focused));
    assert!(navigated
        .semantics
        .nodes
        .iter()
        .any(|node| node.id == "root/alternate" && !node.selected));
    backend
        .render_frame(request(
            None,
            vec![UiInputEvent {
                sequence: 4,
                kind: UiInputEventKind::Keyboard {
                    logical_key: "ArrowDown".into(),
                    physical_key: "ArrowDown".into(),
                    state: UiButtonState::Released,
                    repeat: false,
                    modifiers: 0,
                },
            }],
        ))
        .expect("navigation release frame");
    let alternate = backend
        .render_frame(request(
            None,
            vec![UiInputEvent {
                sequence: 5,
                kind: UiInputEventKind::Keyboard {
                    logical_key: "Enter".into(),
                    physical_key: "Enter".into(),
                    state: UiButtonState::Pressed,
                    repeat: false,
                    modifiers: 0,
                },
            }],
        ))
        .expect("alternate activate frame");
    assert_eq!(alternate.actions.len(), 1);
    assert_eq!(alternate.actions[0].semantic_target_id, "root/alternate");
}

#[test]
pub(super) fn missing_focus_target_reports_bounded_focusable_semantic_ids() {
    let mut confirm = node("confirm", "button");
    confirm.events.push(UiEventBinding {
        event: "activate".into(),
        action_id: "vn.advance".into(),
        arguments: BTreeMap::new(),
    });
    let mut root = node("root", "screen");
    root.children.push(confirm);
    let view = UiViewBlueprint {
        id: "ui.focus.missing".into(),
        source_id: "ui.focus.missing".into(),
        model_schema: "model.focus.v1".into(),
        theme_id: "theme.focus".into(),
        required_capabilities: Vec::new(),
        root,
    };
    let mut bundle = UiBlueprintBundle {
        schema: "astra.ui_blueprint_bundle.v2".into(),
        views: BTreeMap::from([(view.id.clone(), view)]),
        hash: Hash256::from_sha256(&[]),
    };
    bundle.hash = bundle.compute_hash().expect("bundle hash");
    let theme = Arc::new(UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "theme.focus".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        revision: 1,
    });
    let frame = UiBlueprintFrameModel {
        schema: "astra.ui_blueprint_frame_model.v1".into(),
        view_id: "ui.focus.missing".into(),
        model: UiValue::Map(BTreeMap::new()),
        state: UiValue::Map(BTreeMap::new()),
        modals: Vec::new(),
        focus_request: Some("root/missing".into()),
        localization: BTreeMap::new(),
    };
    let request = UiFrameRequest {
        schema: "astra.ui_frame_request.v1".into(),
        session_id: "session.focus.missing".into(),
        generation: 1,
        viewport: UiViewport {
            physical_width: 800,
            physical_height: 600,
            scale_factor: 1.0,
            font_scale: 1.0,
            safe_area_points: UiInsets {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            },
        },
        fixed_time_ns: 0,
        input: UiInputFrame {
            schema: "astra.ui_input_frame.v1".into(),
            events: Vec::new(),
        },
        theme,
        model_schema: "model.focus.v1".into(),
        model_revision: 1,
        model: frame,
    };
    let renderer = BlueprintYakuiRenderer::new(bundle).expect("renderer");
    let mut backend = AstraYakuiBackend::new(renderer, Hash256::from_sha256(b"missing-focus-test"))
        .expect("backend");
    let error = backend.render_frame(request).expect_err("missing target");
    let message = error.to_string();
    assert!(message.contains("requested=root/missing"));
    assert!(message.contains("available=root/confirm"));
}
