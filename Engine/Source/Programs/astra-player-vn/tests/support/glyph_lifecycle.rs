use super::*;

#[test]
fn dialogue_wait_precedes_text_reveal_and_first_click_only_reveals() {
    let ui = TEST_UI.replace(
        "value:$model.text_key",
        "value:$model.text_key visible_graphemes:$model.visible_graphemes",
    );
    let bytes = product_package_with_ui_and_request(STORY, &ui, test_compile_options(), |_| {});
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    assert!(
        !source
            .product_observation_evidence()
            .unwrap()
            .text_reveal_complete
    );
    assert_eq!(source.pending_wait().unwrap().command_id, "line.one");
    advance(&mut source);
    assert!(
        source
            .product_observation_evidence()
            .unwrap()
            .text_reveal_complete
    );
    assert_eq!(source.pending_wait().unwrap().command_id, "line.one");
    source
        .dispatch_ui_event(UiInputEventKind::Keyboard {
            logical_key: "Enter".into(),
            physical_key: "Enter".into(),
            state: UiButtonState::Released,
            repeat: false,
            modifiers: 0,
        })
        .unwrap();
    advance(&mut source);
    assert_eq!(source.pending_wait().unwrap().command_id, "line.two");
    assert!(
        !source
            .product_observation_evidence()
            .unwrap()
            .text_reveal_complete
    );
    source.tick_presentation(1_000_000_000).unwrap();
    assert!(
        source
            .product_observation_evidence()
            .unwrap()
            .text_reveal_complete
    );
    assert_eq!(source.pending_wait().unwrap().command_id, "line.two");
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
fn repeated_physical_advances_keep_glyph_uploads_ordered() {
    let mut story = String::from(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n",
    );
    for index in 0..150 {
        let window = if index % 3 == 0 {
            " window:alternate"
        } else {
            ""
        };
        story.push_str(&format!(
            "    text key:line.{index} speaker:hero{window} #@id line.{index}\n"
        ));
    }
    let mut ui = TEST_UI.replace(
        "value:$model.text_key",
        "value:$model.text_key visible_graphemes:$model.visible_graphemes",
    );
    ui.push_str(r#"
ui_bind profile:classic surface:alternate view:ui.test.alternate controller:test.alternate policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.alternate
ui_view ui.test.alternate model:astra.vn.ui_model.message.v2 theme:astra.vn.theme.classic #@id ui.test.alternate
  screen id:root
    panel id:advance fill:true
      on activate -> vn.advance
    text id:body value:$model.text_key visible_graphemes:$model.visible_graphemes font_size:25
"#);
    let options = test_compile_options().with_ui_controller_source(
        "test.alternate",
        r#"
astra.ui.controller.register("test.alternate", {
  schema = "astra.vn.ui_controller.v1", view = "ui.test.alternate",
  model_schema = "astra.vn.ui_model.message.v2", snapshot = "none",
}, {
  on_open = function() return { astra.ui.effect.focus("root/advance") } end,
  on_action = function(_, _, action) return { astra.ui.effect.forward(action) } end,
})
"#
        .to_string(),
    );
    let bytes = product_package_with_ui_and_request(&story, &ui, options, |request| {
        let section = request
            .cooked_assets
            .iter_mut()
            .find(|section| section.id == "vn.localization.en")
            .unwrap();
        let mut table: serde_json::Value = serde_json::from_slice(&section.payload).unwrap();
        for index in 0..150 {
            table["strings"][format!("line.{index}")] = format!(
                "Line {index}: alphabetical variation {}.",
                char::from(b'A' + (index % 26) as u8)
            )
            .into();
        }
        section.payload = serde_json::to_vec(&table).unwrap();
    });
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package_with_options(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
        astra_player_vn::NativeVnHostOpenOptions {
            max_asset_cache_bytes: 16 * 1024 * 1024,
            max_glyph_cache_bytes: 1,
            runtime_execution: NativeVnRuntimeExecution::shipping_serial(),
        },
    )
    .unwrap();
    let mut resident = std::collections::BTreeSet::new();
    let mut validate = |batch: astra_player_core::PlayerHostCommandBatch| {
        for command in batch.commands {
            if let PlayerHostCommand::PresentScene { commands, .. } = command {
                for draw in commands {
                    match draw {
                        SceneCommand::UploadGlyph { resource_id, .. }
                        | SceneCommand::UploadTexture { resource_id, .. } => {
                            assert!(resident.insert(resource_id));
                        }
                        SceneCommand::ReleaseResource { resource_id } => {
                            assert!(resident.remove(&resource_id));
                        }
                        SceneCommand::GlyphRun { glyphs, .. } => {
                            for glyph in glyphs.iter() {
                                assert!(
                                    resident.contains(&glyph.resource_id),
                                    "glyph must be uploaded before use"
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    };
    validate(source.launch().unwrap());
    for index in 0..140 {
        if let Some(batch) = source.tick_presentation(16_666_667).unwrap() {
            validate(batch);
        }
        validate(
            source
                .dispatch_ui_event(UiInputEventKind::FixedTime {
                    time_ns: index * 16_666_667,
                })
                .unwrap(),
        );
        validate(advance(&mut source));
        if let Some(batch) = source.tick_presentation(16_666_667).unwrap() {
            validate(batch);
        }
        validate(
            source
                .dispatch_ui_event(UiInputEventKind::Keyboard {
                    logical_key: "Enter".into(),
                    physical_key: "Enter".into(),
                    state: UiButtonState::Released,
                    repeat: false,
                    modifiers: 0,
                })
                .unwrap(),
        );
    }
    validate(source.release_resources().unwrap());
    source.shutdown().unwrap();
}

#[test]
fn presentation_reveal_invalidates_cached_scroll_text_without_runtime_step() {
    let ui = TEST_UI.replace("    text id:body value:$model.text_key", r#"    row id:dialogue position_x:56 position_y:484 min_width:688 max_width:688 max_height:80 gap:18 clip_children:true
      text id:speaker_text value:$model.speaker_key min_width:92 max_width:92 max_height:72 font_size:20 max_lines:2
      scroll id:body_scroll min_width:578 max_width:578 min_height:72 max_height:72 clip_children:true
        text id:body value:$model.text_key visible_graphemes:$model.visible_graphemes max_width:562 font_size:22 max_lines:64 text_padding:0"#);
    let bytes = product_package_with_ui_and_request(STORY, &ui, test_compile_options(), |_| {});
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        800,
        600,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    let batch = source.tick_presentation(1_000_000_000).unwrap().unwrap();
    let semantics = source.ui_semantics().unwrap();
    let body = semantics
        .nodes
        .iter()
        .find(|node| node.id == "root/dialogue/body_scroll/body")
        .unwrap();
    assert!(
        body.properties["text.visible_graphemes"]
            .parse::<u32>()
            .unwrap()
            > 0
    );
    assert!(batch.commands.iter().any(|command| {
        if let PlayerHostCommand::PresentScene {commands,..} = command {
            commands.iter().any(|draw| matches!(draw, SceneCommand::GlyphRun {id,glyphs,..} if id.contains("body_scroll/body") && !glyphs.is_empty()))
        } else {false}
    }), "revealed scroll text must emit glyphs");
    let viewport = semantics
        .nodes
        .iter()
        .find(|node| node.id == "root/dialogue/body_scroll")
        .unwrap();
    assert!(
        body.bounds_points.min.x >= viewport.bounds_points.min.x
            && body.bounds_points.min.x < viewport.bounds_points.max.x,
        "body {:?}, viewport {:?}",
        body.bounds_points,
        viewport.bounds_points
    );
    assert!(
        body.bounds_points.min.y >= viewport.bounds_points.min.y
            && body.bounds_points.min.y < viewport.bounds_points.max.y,
        "body {:?}, viewport {:?}",
        body.bounds_points,
        viewport.bounds_points
    );
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}
