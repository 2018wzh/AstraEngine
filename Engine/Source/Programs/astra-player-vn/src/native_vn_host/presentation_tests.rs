use super::*;
use crate::test_native_package as native_package;

#[test]
fn input_action_preserves_resized_glyph_uploads() {
    let bytes = native_package::product_package_with_request(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n    text key:line.two speaker:hero #@id line.two\n",
        |_| {},
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
    let mut resident = BTreeSet::new();
    let mut validate = |batch: PlayerHostCommandBatch| {
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
                                    "missing glyph upload after input action"
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
    let resize = source
        .next_ui_event(UiInputEventKind::Resize {
            viewport: UiViewport {
                physical_width: 640,
                physical_height: 360,
                scale_factor: 2.0,
                font_scale: 2.0,
                safe_area_points: UiInsets {
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
            },
        })
        .unwrap();
    let advance = source
        .next_ui_event(UiInputEventKind::Keyboard {
            physical_key: "Enter".into(),
            logical_key: "Enter".into(),
            state: UiButtonState::Pressed,
            repeat: false,
            modifiers: 0,
        })
        .unwrap();
    validate(source.dispatch_ui_events(vec![resize, advance]).unwrap());
    validate(source.release_resources().unwrap());
    source.shutdown().unwrap();
}

const STORY: &str = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    stage viewport:320x180 safe_area:16:9 #@id stage.main
    layer id:bg kind:background z:0 blend:normal clip:stage #@id layer.bg
    background asset:asset:/background/apartment-night layer:bg duration:1000 interrupt:replace_from_current #@id background.main
    text key:line.one speaker:hero #@id line.one
"#;

fn source() -> NativeVnHostCommandSource {
    source_for(STORY)
}

fn source_for(story: &str) -> NativeVnHostCommandSource {
    let bytes = native_package::product_package_with_request(story, |_| {});
    let package = astra_package::PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    source
}

#[test]
fn invalid_transition_asset_restore_keeps_live_world_and_work() {
    let mut source = source_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene first #@id scene.first
    stage viewport:320x180 safe_area:16:9 #@id stage.main
    layer id:bg kind:background z:0 blend:normal clip:stage #@id layer.bg
    background asset:asset:/background/apartment-night layer:bg duration:0 interrupt:replace_from_current #@id background.first
    text key:line.one speaker:hero #@id line.one
  scene second #@id scene.second
    transition preset:director_puppet_9 duration:250 descriptor:director.puppet.9 #@id transition.center
    background asset:asset:/background/apartment-night layer:bg duration:0 interrupt:replace_from_current #@id background.second
    text key:line.two speaker:hero #@id line.two
"#,
    );
    for _ in 0..2 {
        source.command(VnPlayerCommand::Advance).unwrap();
    }
    source.tick_presentation(16_666_667).unwrap();
    source
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    source
        .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 0)
        .unwrap();
    let saved = source.save("slot.01").unwrap();
    let mut envelope = decode_save_envelope(&saved).unwrap();
    let snapshot = envelope
        .payload
        .director_transition_snapshot
        .as_mut()
        .unwrap();
    snapshot
        .source_state
        .entities
        .values_mut()
        .next()
        .unwrap()
        .asset = "asset:/missing".into();
    let corrupted = postcard::to_allocvec(&envelope).unwrap();
    let world = source.host.save().unwrap().0;
    let scope = source.media_scope.clone();
    let step = source.fixed_step;
    assert!(source.restore(&corrupted).is_err());
    assert_eq!(source.host.save().unwrap().0, world);
    assert_eq!(source.fixed_step, step);
    assert_eq!(source.media_scope, scope);
    assert!(!scope.is_cancelled());
    assert!(!source.presentation_failed);
    source.tick_presentation(16_666_667).unwrap();
    source
        .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 0)
        .unwrap();
    assert!(source.save("slot.01").is_ok());
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
fn frame_failure_keeps_advanced_state_and_requires_successful_restore() {
    let mut source = source();
    source
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    source
        .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 3_723_000)
        .unwrap();
    let save = source.save("slot.01").unwrap();
    let frame = source.stage_director.state().frame_index;
    assert!(source
        .tick_presentation(0)
        .unwrap_err()
        .to_string()
        .contains("TICK_DELTA"));
    assert!(!source.presentation_failed);
    assert_eq!(source.stage_director.state().frame_index, frame);

    let budget = source.texture_cpu_budget_bytes;
    source.textures.clear();
    source.live_texture_ids.clear();
    source.texture_cpu_bytes = 0;
    source.texture_cpu_budget_bytes = 1;
    assert!(source
        .tick_presentation(16_666_667)
        .unwrap_err()
        .to_string()
        .contains("CPU_TEXTURE_ENTRY_BUDGET"));
    assert!(source.stage_director.state().frame_index > frame);
    assert!(source.presentation_failed);
    assert!(source
        .tick_presentation(16_666_667)
        .unwrap_err()
        .to_string()
        .contains("SESSION_FAILED"));
    assert!(source
        .save("slot.01")
        .unwrap_err()
        .to_string()
        .contains("SESSION_FAILED"));
    assert!(source
        .command(VnPlayerCommand::Advance)
        .unwrap_err()
        .to_string()
        .contains("SESSION_FAILED"));
    assert!(source.restore(b"invalid").is_err());
    assert!(source.presentation_failed);
    // A validated save still fails if rebuilding its presentation cannot fit.
    assert!(source.restore(&save).is_err());
    assert!(source.presentation_failed);
    source.texture_cpu_budget_bytes = budget;
    source.restore(&save).unwrap();
    assert!(!source.presentation_failed);
    source.tick_presentation(16_666_667).unwrap();
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
fn only_failure_of_the_awaited_fence_ends_the_player_session() {
    use astra_vn_core::{
        FixedScalar, PresentationInterruptPolicy, StageBlendMode, StageClipPolicy, StageLayerKind,
        VnMovieEndBehavior, VnWaitState,
    };
    let mut source = source();
    source
        .stage_director
        .apply(&StageCommand::DeclareLayer {
            id: "movie".into(),
            kind: StageLayerKind::Video,
            z: 200,
            blend: StageBlendMode::Normal,
            clip: Some(StageClipPolicy::Stage),
            input: None,
        })
        .unwrap();
    source
        .stage_director
        .apply(&StageCommand::Movie {
            layer: "movie".into(),
            asset: "asset:/test.webm".into(),
            alpha: FixedScalar {
                millionths: 1_000_000,
            },
            loop_mode: MovieLoopMode::Once,
            end: VnMovieEndBehavior::Wait,
            fence: Some("movie.done".into()),
            fallback: None,
            interrupt: PresentationInterruptPolicy::Reject,
        })
        .unwrap();
    source.stage_director.fail_video("movie").unwrap();
    source.ensure_presentation_active().unwrap();
    source.runtime_state.as_mut().unwrap().pending_wait = Some(VnWaitState::new(
        VnWaitKind::MovieEnd,
        "movie.done",
        "movie.command",
    ));
    assert!(source
        .tick_presentation(16_666_667)
        .unwrap_err()
        .to_string()
        .contains("FENCE_FAILED"));
    assert!(source.presentation_failed);
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
fn video_work_cannot_cross_replacement_restore_or_source_lifetime() {
    let mut source = source_for("story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n");
    source
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .unwrap();
    source
        .prepare_save_metadata("slot.01", "2000-01-01T00:00:00Z".into(), 0)
        .unwrap();
    let save = source.save("slot.01").unwrap();
    let make_request = |scope| NativeVnVideoRequest {
        scope,
        layer: "movie".into(),
        asset_id: "asset:/test.webm".into(),
        codec: "webm".into(),
        encoded_bytes: vec![1].into(),
        encoded_length: 1,
        alpha_millionths: 1_000_000,
        looping: false,
        fence: Some("movie.done".into()),
    };
    let old = make_request(source.replace_video_scope("movie"));
    let current = make_request(source.replace_video_scope("movie"));
    assert!(old.is_cancelled());
    assert!(source
        .prepare_video_decode(&old)
        .err()
        .unwrap()
        .to_string()
        .contains("REQUEST_STALE"));
    source.validate_video_request(&current).unwrap();
    let foreign = make_request(astra_runtime::TaskScope::new());
    assert!(source
        .validate_video_request(&foreign)
        .unwrap_err()
        .to_string()
        .contains("REQUEST_STALE"));
    source.pending_video.push(current.clone());
    source.pending_stage_completions.push("movie.done".into());
    source
        .pending_audio
        .push(NativeVnAudioOutput::Control(NativeVnAudioControlRequest {
            command_id: "stop.old".into(),
            action: "stop".into(),
            target: "bgm".into(),
            duration_ms: None,
            fence: None,
        }));
    source.pending_ui_host_request = Some(VnUiHostRequest::Load {
        slot_id: "old.slot".into(),
    });
    assert!(source.restore(b"invalid").is_err());
    assert!(!current.is_cancelled());
    assert_eq!(source.pending_video.len(), 1);
    assert_eq!(source.pending_stage_completions.len(), 1);
    let worker_request = current.clone();
    let (release, ready) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        ready.recv().unwrap();
        worker_request
    });
    source.restore(&save).unwrap();
    release.send(()).unwrap();
    let late = worker.join().unwrap();
    assert!(late.is_cancelled());
    let frame = TextureFrame::from_vec(1, 1, vec![0; 4]).unwrap();
    assert!(source
        .bind_decoded_video_frame(&late, frame, true)
        .unwrap_err()
        .to_string()
        .contains("REQUEST_STALE"));
    assert!(source
        .complete_video_fence(&late)
        .unwrap_err()
        .to_string()
        .contains("REQUEST_STALE"));
    assert!(source.take_video_requests().is_empty());
    assert!(source.take_stage_completions().is_empty());
    assert!(source.take_audio_requests().is_empty());
    assert!(source.take_ui_host_request().is_none());
    let fresh = make_request(source.replace_video_scope("movie"));
    source.validate_video_request(&fresh).unwrap();
    source.release_resources().unwrap();
    assert!(fresh.is_cancelled());
    source.shutdown().unwrap();
    let source = source_for("story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:line.one speaker:hero #@id line.one\n");
    let scope = source.media_scope.child();
    drop(source);
    assert!(scope.is_cancelled());
}
