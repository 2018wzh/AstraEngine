use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_ui_core::{
    UiBackend, UiBlueprintBundle, UiBlueprintFrameModel, UiBlueprintModalFrameModel, UiCapability,
    UiFrameRequest, UiInputFrame, UiInsets, UiNodeBlueprint, UiRepeatBinding, UiSemanticRole,
    UiThemeManifest, UiThemeValue, UiValue, UiValueExpr, UiViewBlueprint, UiViewport,
};
use astra_ui_yakui::{AstraYakuiBackend, BlueprintYakuiRenderer};

fn node(id: &str, widget: &str) -> UiNodeBlueprint {
    UiNodeBlueprint {
        source_id: format!("source.{id}"),
        local_id: id.into(),
        widget: widget.into(),
        properties: BTreeMap::new(),
        events: Vec::new(),
        children: Vec::new(),
        repeat: None,
        component_id: None,
    }
}

#[astra_headless_test::test]
fn modal_stack_is_rendered_as_bounded_dialog_semantics() {
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
    dialog_root.children.push(node("message", "text"));
    let dialog = UiViewBlueprint {
        id: "ui.dialog".into(),
        source_id: "ui.dialog".into(),
        model_schema: "model.dialog.v1".into(),
        theme_id: "theme.classic".into(),
        required_capabilities: Vec::new(),
        root: dialog_root,
    };
    let mut bundle = UiBlueprintBundle {
        schema: "astra.ui_blueprint_bundle.v1".into(),
        views: BTreeMap::from([(base.id.clone(), base), (dialog.id.clone(), dialog)]),
        hash: Hash256::from_sha256(&[]),
    };
    bundle.hash = bundle.compute_hash().expect("bundle hash");
    let mut theme = UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "theme.classic".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        content_hash: Hash256::from_sha256(&[]),
    };
    theme.content_hash = theme.compute_hash().expect("theme hash");
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
        model_payload: postcard::to_allocvec(&frame).expect("frame encode"),
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
    assert_eq!(output.semantics.root_id, "root");
}

#[astra_headless_test::test]
fn ten_thousand_items_instantiate_only_visible_rows() {
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
        schema: "astra.ui_blueprint_bundle.v1".into(),
        views: BTreeMap::from([(view.id.clone(), view)]),
        hash: Hash256::from_sha256(&[]),
    };
    bundle.hash = bundle.compute_hash().expect("bundle hash");

    let mut theme = UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "theme.classic".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        content_hash: Hash256::from_sha256(&[]),
    };
    theme.content_hash = theme.compute_hash().expect("theme hash");
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
        model_payload: postcard::to_allocvec(&model).expect("model encode"),
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
