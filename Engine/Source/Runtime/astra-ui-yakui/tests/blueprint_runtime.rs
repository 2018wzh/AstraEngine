use std::collections::{BTreeMap, BTreeSet};

use astra_core::Hash256;
use astra_media_core::TextureFrame;
use astra_ui_core::{
    UiBackend, UiBlueprintBundle, UiBlueprintFrameModel, UiBlueprintModalFrameModel, UiButtonState,
    UiCapability, UiEventBinding, UiFrameRequest, UiInputDispositionKind, UiInputEvent,
    UiInputEventKind, UiInputFrame, UiInsets, UiNodeBlueprint, UiRepeatBinding, UiSemanticAction,
    UiSemanticRole, UiThemeManifest, UiThemeValue, UiValue, UiValueExpr, UiViewBlueprint,
    UiViewport,
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

#[astra_headless_test::test]
fn thousand_gallery_images_are_virtualized_and_bounded_by_lru() {
    let mut image = node("thumbnail", "image");
    image.properties.insert(
        "asset".into(),
        UiValueExpr::Binding {
            root: astra_ui_core::UiBindingRoot::Item,
            path: vec!["thumbnail_asset".into()],
        },
    );
    for key in ["min_width", "min_height"] {
        image.properties.insert(
            key.into(),
            UiValueExpr::Literal {
                value: UiValue::Number(64.0),
            },
        );
    }
    let mut grid = node("gallery", "virtual_grid");
    grid.properties.insert(
        "columns".into(),
        UiValueExpr::Literal {
            value: UiValue::Integer(4),
        },
    );
    grid.properties.insert(
        "item_extent".into(),
        UiValueExpr::Literal {
            value: UiValue::Number(96.0),
        },
    );
    grid.repeat = Some(UiRepeatBinding {
        items: UiValueExpr::Binding {
            root: astra_ui_core::UiBindingRoot::Model,
            path: vec!["items".into()],
        },
        item_key_path: vec!["item_id".into()],
        overscan: 2,
    });
    grid.children.push(image);
    let mut root = node("root", "screen");
    root.children.push(grid);
    let view = UiViewBlueprint {
        id: "ui.gallery".into(),
        source_id: "ui.gallery".into(),
        model_schema: "astra.vn.ui_model.gallery.v1".into(),
        theme_id: "theme.classic".into(),
        required_capabilities: vec![UiCapability::VirtualGrid],
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
    let items = (0..1_000)
        .map(|index| {
            UiValue::Map(BTreeMap::from([
                (
                    "item_id".into(),
                    UiValue::String(format!("gallery.{index}")),
                ),
                (
                    "thumbnail_asset".into(),
                    UiValue::String(format!("fixture.thumbnail.{index}")),
                ),
            ]))
        })
        .collect();
    let frame = UiBlueprintFrameModel {
        schema: "astra.ui_blueprint_frame_model.v1".into(),
        view_id: "ui.gallery".into(),
        model: UiValue::Map(BTreeMap::from([("items".into(), UiValue::List(items))])),
        state: UiValue::Map(BTreeMap::new()),
        modals: Vec::new(),
        focus_request: None,
        localization: BTreeMap::new(),
    };
    let resources = (0..1_000)
        .map(|index| {
            let rgba8 = vec![index as u8, 64, 128, 255];
            (
                format!("fixture.thumbnail.{index}"),
                TextureFrame {
                    width: 1,
                    height: 1,
                    hash: Hash256::from_sha256(&rgba8),
                    rgba8,
                },
            )
        })
        .collect();
    let encoded = postcard::to_allocvec(&frame).expect("frame encode");
    let request = |sequence: u64, wheel: bool| UiFrameRequest {
        schema: "astra.ui_frame_request.v1".into(),
        session_id: "session.gallery".into(),
        generation: sequence + 1,
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
        fixed_time_ns: sequence * 16_666_667,
        input: UiInputFrame {
            schema: "astra.ui_input_frame.v1".into(),
            events: wheel
                .then_some(UiInputEvent {
                    sequence,
                    kind: UiInputEventKind::Wheel {
                        delta_points: astra_ui_core::UiPoint { x: 0.0, y: -720.0 },
                    },
                })
                .into_iter()
                .collect(),
        },
        theme: theme.clone(),
        model_schema: "astra.vn.ui_model.gallery.v1".into(),
        model_payload: encoded.clone(),
    };
    let renderer = BlueprintYakuiRenderer::new(bundle)
        .expect("renderer")
        .with_image_resources(resources);
    let mut backend =
        AstraYakuiBackend::new(renderer, Hash256::from_sha256(b"test")).expect("backend");
    let first = backend
        .render_frame(request(0, false))
        .expect("first frame");
    assert!(first.performance.instantiated_nodes <= 64);
    assert!(first.performance.active_texture_bytes <= 128 * 4);

    let mut observed_release = false;
    for sequence in 1..40 {
        let output = backend
            .render_frame(request(sequence, true))
            .expect("scrolled frame");
        assert!(output.performance.instantiated_nodes <= 64);
        assert!(output.performance.active_texture_bytes <= 128 * 4);
        observed_release |= !output.render.textures.releases.is_empty();
    }
    assert!(
        observed_release,
        "bounded gallery LRU must release old textures"
    );
}

#[astra_headless_test::test]
fn accessibility_range_action_uses_typed_change_binding_and_is_consumed() {
    let mut slider = node("volume", "slider");
    for (name, value) in [("value", 0.5), ("min", 0.0), ("max", 1.0), ("step", 0.1)] {
        slider.properties.insert(
            name.into(),
            UiValueExpr::Literal {
                value: UiValue::Number(value),
            },
        );
    }
    slider.events.push(UiEventBinding {
        event: "change".into(),
        action_id: "vn.set_config".into(),
        arguments: BTreeMap::from([(
            "value".into(),
            UiValueExpr::Binding {
                root: astra_ui_core::UiBindingRoot::Event,
                path: vec!["value".into()],
            },
        )]),
    });
    let mut root = node("root", "screen");
    root.children.push(slider);
    let view = UiViewBlueprint {
        id: "ui.config".into(),
        source_id: "ui.config".into(),
        model_schema: "model.config.v1".into(),
        theme_id: "theme.classic".into(),
        required_capabilities: Vec::new(),
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
    let frame = UiBlueprintFrameModel {
        schema: "astra.ui_blueprint_frame_model.v1".into(),
        view_id: "ui.config".into(),
        model: UiValue::Map(BTreeMap::new()),
        state: UiValue::Map(BTreeMap::new()),
        modals: Vec::new(),
        focus_request: None,
        localization: BTreeMap::new(),
    };
    let request = |events| UiFrameRequest {
        schema: "astra.ui_frame_request.v1".into(),
        session_id: "session.accessibility".into(),
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
            events,
        },
        theme: theme.clone(),
        model_schema: "model.config.v1".into(),
        model_payload: postcard::to_allocvec(&frame).expect("frame encode"),
    };
    let renderer = BlueprintYakuiRenderer::new(bundle).expect("renderer");
    let mut backend =
        AstraYakuiBackend::new(renderer, Hash256::from_sha256(b"test")).expect("backend");
    let first = backend
        .render_frame(request(Vec::new()))
        .expect("first frame");
    let slider = first
        .semantics
        .nodes
        .iter()
        .find(|node| node.id == "root/volume")
        .expect("slider semantics");
    assert!(slider.actions.contains(&UiSemanticAction::Increment));
    assert!(slider.actions.contains(&UiSemanticAction::SetValue));

    let second = backend
        .render_frame(request(vec![UiInputEvent {
            sequence: 1,
            kind: UiInputEventKind::AccessibilityAction {
                semantic_id: "root/volume".into(),
                action: "increment".into(),
                value: None,
            },
        }]))
        .expect("accessibility frame");
    assert_eq!(second.dispositions.len(), 1);
    assert_eq!(
        second.dispositions[0].disposition,
        UiInputDispositionKind::Consumed
    );
    assert_eq!(second.actions.len(), 1);
    assert_eq!(second.actions[0].action_id, "vn.set_config");
    let UiValue::Number(value) = second.actions[0]
        .arguments
        .get("value")
        .expect("range action value")
    else {
        panic!("range action value must be numeric");
    };
    assert!((value - 0.6).abs() < 1e-6);
}

#[astra_headless_test::test]
fn requested_focus_activates_a_button_without_prior_navigation() {
    let mut button = node("confirm", "button");
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
        schema: "astra.ui_blueprint_bundle.v1".into(),
        views: BTreeMap::from([(view.id.clone(), view)]),
        hash: Hash256::from_sha256(&[]),
    };
    bundle.hash = bundle.compute_hash().expect("bundle hash");
    let mut theme = UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "theme.focus".into(),
        parent: None,
        tokens: BTreeMap::from([("surface".into(), UiThemeValue::Color([0, 0, 0, 255]))]),
        high_contrast_tokens: BTreeMap::new(),
        content_hash: Hash256::from_sha256(&[]),
    };
    theme.content_hash = theme.compute_hash().expect("theme hash");
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
            model_payload: postcard::to_allocvec(&frame).expect("frame encode"),
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
