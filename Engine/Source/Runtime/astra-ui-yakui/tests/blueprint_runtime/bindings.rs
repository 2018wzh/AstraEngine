use super::*;

pub(super) fn node(id: &str, widget: &str) -> UiNodeBlueprint {
    UiNodeBlueprint {
        source_id: format!("source.{id}"),
        local_id: id.into(),
        widget: widget.into(),
        properties: BTreeMap::new(),
        events: Vec::new(),
        children: Vec::new(),
        repeat: None,
    }
}

#[test]
pub(super) fn explicit_max_size_bounds_an_absolutely_positioned_window() {
    let mut window = node("window", "column");
    for (key, value) in [
        ("min_width", 336.0),
        ("max_width", 336.0),
        ("min_height", 242.0),
        ("max_height", 242.0),
        ("position_x", 232.0),
        ("position_y", 166.0),
    ] {
        window.properties.insert(
            key.into(),
            UiValueExpr::Literal {
                value: UiValue::Number(value),
            },
        );
    }
    window.properties.insert(
        "clip_children".into(),
        UiValueExpr::Literal {
            value: UiValue::Bool(true),
        },
    );
    let mut oversized = node("oversized", "panel");
    oversized.properties.insert(
        "min_height".into(),
        UiValueExpr::Literal {
            value: UiValue::Number(320.0),
        },
    );
    window.children.push(oversized);
    let mut root = node("root", "screen");
    root.children.push(window);
    let view = UiViewBlueprint {
        id: "ui.fixed_window".into(),
        source_id: "ui.fixed_window".into(),
        model_schema: "model.fixed_window.v1".into(),
        theme_id: "theme.classic".into(),
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
        id: "theme.classic".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        revision: 1,
    });
    let frame = UiBlueprintFrameModel {
        schema: "astra.ui_blueprint_frame_model.v1".into(),
        view_id: "ui.fixed_window".into(),
        model: UiValue::Map(BTreeMap::new()),
        state: UiValue::Map(BTreeMap::new()),
        modals: Vec::new(),
        focus_request: None,
        localization: BTreeMap::new(),
    };
    let request = UiFrameRequest {
        schema: "astra.ui_frame_request.v1".into(),
        session_id: "session.fixed_window".into(),
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
        model_schema: "model.fixed_window.v1".into(),
        model_revision: 1,
        model: frame,
    };
    let renderer = BlueprintYakuiRenderer::new(bundle).expect("renderer");
    let mut backend =
        AstraYakuiBackend::new(renderer, Hash256::from_sha256(b"test")).expect("backend");
    let output = backend.render_frame(request).expect("render");
    let window = output
        .semantics
        .nodes
        .iter()
        .find(|node| node.id == "root/window")
        .expect("window semantics");
    assert_eq!(
        window.bounds_points.max.x - window.bounds_points.min.x,
        336.0
    );
    assert_eq!(
        window.bounds_points.max.y - window.bounds_points.min.y,
        242.0
    );
    let child = output
        .semantics
        .nodes
        .iter()
        .find(|node| node.id == "root/window/oversized")
        .unwrap();
    for (key, value) in [
        ("x", "232"),
        ("y", "166"),
        ("width", "336"),
        ("height", "242"),
    ] {
        assert_eq!(
            child
                .properties
                .get(&format!("text.clip.{key}"))
                .map(String::as_str),
            Some(value)
        );
    }
}

#[test]
pub(super) fn modal_stack_is_rendered_as_bounded_dialog_semantics() {
    let mut base_root = node("root", "screen");
    base_root.children.push(node("content", "panel"));
    let base = UiViewBlueprint {
        id: "ui.base".into(),
        source_id: "ui.base".into(),
        model_schema: "model.base.v1".into(),
        theme_id: "theme.classic".into(),
        required_capabilities: Vec::new(),
        root: base_root,
    };
    let mut dialog_root = node("dialog", "modal");
    let mut message = node("message", "text");
    message.properties.insert(
        "text_align".into(),
        UiValueExpr::Literal {
            value: UiValue::String("center".into()),
        },
    );
    message.properties.insert(
        "vertical_align".into(),
        UiValueExpr::Literal {
            value: UiValue::String("end".into()),
        },
    );
    dialog_root.children.push(message);
    let dialog = UiViewBlueprint {
        id: "ui.dialog".into(),
        source_id: "ui.dialog".into(),
        model_schema: "model.dialog.v1".into(),
        theme_id: "theme.classic".into(),
        required_capabilities: Vec::new(),
        root: dialog_root,
    };
    let mut bundle = UiBlueprintBundle {
        schema: "astra.ui_blueprint_bundle.v2".into(),
        views: BTreeMap::from([(base.id.clone(), base), (dialog.id.clone(), dialog)]),
        hash: Hash256::from_sha256(&[]),
    };
    bundle.hash = bundle.compute_hash().expect("bundle hash");
    let theme = Arc::new(UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "theme.classic".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        revision: 1,
    });
    let frame = UiBlueprintFrameModel {
        schema: "astra.ui_blueprint_frame_model.v1".into(),
        view_id: "ui.base".into(),
        model: UiValue::Map(BTreeMap::new()),
        state: UiValue::Map(BTreeMap::new()),
        modals: vec![UiBlueprintModalFrameModel {
            view_id: "ui.dialog".into(),
            model_schema: "model.dialog.v1".into(),
            model: UiValue::Map(BTreeMap::new()),
            state: UiValue::Map(BTreeMap::new()),
        }],
        focus_request: None,
        localization: BTreeMap::new(),
    };
    let request = UiFrameRequest {
        schema: "astra.ui_frame_request.v1".into(),
        session_id: "session.modal".into(),
        generation: 1,
        viewport: UiViewport {
            physical_width: 1280,
            physical_height: 720,
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
        model_schema: "model.base.v1".into(),
        model_revision: 1,
        model: frame,
    };
    let renderer = BlueprintYakuiRenderer::new(bundle).expect("renderer");
    let mut backend =
        AstraYakuiBackend::new(renderer, Hash256::from_sha256(b"test")).expect("backend");
    let output = backend.render_frame(request).expect("render");
    assert!(output
        .semantics
        .nodes
        .iter()
        .any(|node| { node.id == "root/modal.0" && node.role == UiSemanticRole::Dialog }));
    assert!(output
        .semantics
        .nodes
        .iter()
        .any(|node| node.id == "root/modal.0/dialog/message"));
    let message = output
        .semantics
        .nodes
        .iter()
        .find(|node| node.id == "root/modal.0/dialog/message")
        .expect("message semantic node");
    assert_eq!(
        message.properties.get("text.align").map(String::as_str),
        Some("center")
    );
    assert_eq!(
        message
            .properties
            .get("text.vertical_align")
            .map(String::as_str),
        Some("end")
    );
    assert_eq!(output.semantics.root_id, "root");
}

#[test]
pub(super) fn ten_thousand_items_instantiate_only_visible_rows() {
    let mut item = node("entry", "button");
    item.properties.insert(
        "min_height".into(),
        UiValueExpr::Literal {
            value: UiValue::Number(48.0),
        },
    );
    let mut list = node("backlog", "virtual_list");
    list.repeat = Some(UiRepeatBinding {
        items: UiValueExpr::Binding {
            root: astra_ui_core::UiBindingRoot::Model,
            path: vec!["entries".into()],
        },
        item_key_path: vec!["id".into()],
        overscan: 8,
    });
    list.children.push(item);
    let mut root = node("root", "screen");
    root.children.push(list);
    let view = UiViewBlueprint {
        id: "ui.backlog".into(),
        source_id: "ui.backlog".into(),
        model_schema: "astra.vn.ui_model.backlog.v1".into(),
        theme_id: "theme.classic".into(),
        required_capabilities: vec![UiCapability::VirtualList],
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
        id: "theme.classic".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        revision: 1,
    });
    let entries = (0..10_000)
        .map(|index| {
            UiValue::Map(BTreeMap::from([(
                "id".into(),
                UiValue::String(format!("e{index}")),
            )]))
        })
        .collect();
    let model = UiBlueprintFrameModel {
        schema: "astra.ui_blueprint_frame_model.v1".into(),
        view_id: "ui.backlog".into(),
        model: UiValue::Map(BTreeMap::from([("entries".into(), UiValue::List(entries))])),
        state: UiValue::Map(BTreeMap::new()),
        modals: Vec::new(),
        focus_request: None,
        localization: BTreeMap::new(),
    };
    let request = UiFrameRequest {
        schema: "astra.ui_frame_request.v1".into(),
        session_id: "session.backlog".into(),
        generation: 1,
        viewport: UiViewport {
            physical_width: 1280,
            physical_height: 720,
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
        model_schema: "astra.vn.ui_model.backlog.v1".into(),
        model_revision: 1,
        model,
    };
    let renderer = BlueprintYakuiRenderer::new(bundle).expect("renderer");
    let mut backend =
        AstraYakuiBackend::new(renderer, Hash256::from_sha256(b"test")).expect("backend");
    let output = backend.render_frame(request).expect("render");
    assert!(output.performance.instantiated_nodes <= 32);
    assert!(output.semantics.nodes.len() <= 32);
    assert_eq!(
        output
            .semantics
            .nodes
            .iter()
            .map(|node| &node.id)
            .collect::<BTreeSet<_>>()
            .len(),
        output.semantics.nodes.len()
    );
}
