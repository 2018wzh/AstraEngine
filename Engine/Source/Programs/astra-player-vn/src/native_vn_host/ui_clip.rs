use super::*;

pub(super) fn semantic_text_clip(
    node: &UiSemanticNode,
) -> Result<Option<RectI>, NativeVnHostError> {
    let values =
        ["x", "y", "width", "height"].map(|name| node.properties.get(&format!("text.clip.{name}")));
    if values.iter().all(|value| value.is_none()) {
        return Ok(None);
    }
    let invalid = || {
        NativeVnHostError::Input(
            "ASTRA_PLAYER_UI_TEXT_CLIP: layout clip is incomplete or invalid".into(),
        )
    };
    let [Some(x), Some(y), Some(width), Some(height)] = values else {
        return Err(invalid());
    };
    Ok(Some(RectI::new(
        x.parse().map_err(|_| invalid())?,
        y.parse().map_err(|_| invalid())?,
        width.parse().map_err(|_| invalid())?,
        height.parse().map_err(|_| invalid())?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_native_package;

    #[test]
    fn retained_text_uses_the_same_ancestor_clip_as_its_yakui_container() {
        let ui = test_native_package::TEST_UI.replace(
            "text id:body value:$model.text_key",
            "column id:clipper min_width:100 max_width:100 min_height:40 max_height:40 clip_children:true\n      text id:body value:$model.text_key",
        );
        let bytes = test_native_package::product_package_with_ui_and_request(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n",
            &ui, test_native_package::test_compile_options(), |_| {},
        );
        let package = astra_package::PackageReader::open(&bytes).unwrap();
        let mut source = NativeVnHostCommandSource::from_package(
            &package,
            VnRunConfig::classic("en"),
            320,
            180,
            PlayerHostResourceId(1),
        )
        .unwrap();
        let batch = source.launch().unwrap();
        let mut found = false;
        for command in &batch.commands {
            let PlayerHostCommand::PresentScene { commands, .. } = command else {
                continue;
            };
            let mut clips = Vec::new();
            for command in commands {
                match command {
                    SceneCommand::PushClip { rect } => clips.push(*rect),
                    SceneCommand::PopClip => {
                        clips.pop().unwrap();
                    }
                    SceneCommand::GlyphRun { id, .. } if id.contains("clipper/body") => {
                        assert!(clips
                            .iter()
                            .any(|rect| rect.width == 100 && rect.height == 40));
                        found = true;
                    }
                    _ => {}
                }
            }
            assert!(clips.is_empty());
        }
        assert!(found, "the real packaged message must render retained text");
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }
}
