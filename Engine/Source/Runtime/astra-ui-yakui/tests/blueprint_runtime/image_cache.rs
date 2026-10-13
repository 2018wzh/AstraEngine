use super::*;

#[test]
pub(super) fn thousand_gallery_images_are_virtualized_and_bounded_by_lru() {
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
                    rgba8: rgba8.into(),
                },
            )
        })
        .collect();
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
        model_revision: 1,
        model: frame.clone(),
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

    let replacement_rgba8 = vec![255, 32, 16, 255];
    backend
        .renderer_mut()
        .upsert_image_resource(
            "fixture.thumbnail.0".into(),
            TextureFrame {
                width: 1,
                height: 1,
                rgba8: replacement_rgba8.into(),
            },
        )
        .expect("replace live texture resource");
    let replaced = backend
        .render_frame(request(1, false))
        .expect("render replaced texture");
    assert!(
        !replaced.render.textures.releases.is_empty(),
        "replacing a live resource must release the superseded Yakui texture"
    );
    assert!(
        !replaced.render.textures.uploads.is_empty(),
        "replacing a visible resource must upload the replacement texture"
    );

    let mut observed_release = true;
    for sequence in 2..40 {
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

#[test]
pub(super) fn accessibility_range_action_uses_typed_change_binding_and_is_consumed() {
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
        view_id: "ui.config".into(),
        model: UiValue::Map(BTreeMap::new()),
        state: UiValue::Map(BTreeMap::new()),
        modals: Vec::new(),
        focus_request: Some("root/volume".into()),
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
        model_revision: 1,
        model: frame.clone(),
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

    let keyboard = backend
        .render_frame(request(vec![UiInputEvent {
            sequence: 2,
            kind: UiInputEventKind::Keyboard {
                logical_key: "ArrowLeft".into(),
                physical_key: "ArrowLeft".into(),
                state: UiButtonState::Pressed,
                repeat: false,
                modifiers: 0,
            },
        }]))
        .expect("keyboard slider frame");
    assert_eq!(keyboard.actions.len(), 1);
    assert_eq!(keyboard.actions[0].action_id, "vn.set_config");
    assert_eq!(keyboard.actions[0].arguments["value"], UiValue::Number(0.4));
    let UiValue::Number(value) = second.actions[0]
        .arguments
        .get("value")
        .expect("range action value")
    else {
        panic!("range action value must be numeric");
    };
    assert!((value - 0.6).abs() < 1e-6);
}
