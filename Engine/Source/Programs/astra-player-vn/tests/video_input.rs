mod support;

use astra_media_core::TextureFrame;
use astra_player_vn::NativeVnHostCommandSource;
use astra_ui_core::{UiButtonState, UiInputEventKind, UiPoint, UiPointerButton};

fn video_source(skippable: bool, wait: bool) -> NativeVnHostCommandSource {
    let ending = if wait {
        "end:wait fence:movie.end"
    } else {
        "end:continue"
    };
    support::source_for_video(&format!(
        "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    stage viewport:320x180 safe_area:16:9 #@id stage.main\n    layer id:video kind:video z:100 blend:normal clip:stage #@id layer.video\n    skip_allowed allowed:{skippable} #@id skip.policy\n    movie layer:video asset:asset:/video/intro loop:true {ending} fallback:asset:/video/intro-fallback interrupt:reject #@id movie.intro\n    text key:line.after #@id line.after\n\nstory system #@id story.system\nstate popup #@id state.system.popup\n  scene popup #@id scene.system.popup\n    system_page kind:quick_panel #@id page.popup\n"
    ))
}

fn press_key(source: &mut NativeVnHostCommandSource, key: &str) {
    for state in [UiButtonState::Pressed, UiButtonState::Released] {
        source
            .prepare_ui_input(UiInputEventKind::Keyboard {
                physical_key: key.into(),
                logical_key: key.into(),
                state,
                repeat: false,
                modifiers: 0,
            })
            .unwrap();
    }
}

#[test]
fn physical_advance_cancels_only_an_allowed_movie_wait_and_rejects_late_frames() {
    for (skippable, wait) in [(true, true), (false, true), (true, false)] {
        let mut source = video_source(skippable, wait);
        source.launch().unwrap();
        source
            .prepare_ui_input(UiInputEventKind::Focus { focused: true })
            .unwrap();
        let request = source.take_video_requests().remove(0);
        press_key(&mut source, "Enter");
        let should_skip = skippable && wait;
        assert_eq!(request.is_cancelled(), should_skip);
        let completions = source.take_stage_completions();
        if should_skip {
            assert_eq!(completions, ["movie.end"]);
            let error = source
                .bind_decoded_video_frame(
                    &request,
                    TextureFrame::from_vec(1, 1, vec![0, 0, 0, 255]).unwrap(),
                    true,
                )
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("ASTRA_PLAYER_VIDEO_REQUEST_STALE"));
            source.complete_wait(completions[0].clone()).unwrap();
            assert_ne!(
                source.pending_wait().map(|wait| wait.fence.as_str()),
                Some("movie.end")
            );
        } else {
            assert!(completions.is_empty());
        }
        source.release_resources().unwrap();
        source.shutdown().unwrap();
    }
}

#[test]
fn system_page_enter_returns_to_the_movie_without_skipping_it() {
    let mut source = video_source(true, true);
    source.launch().unwrap();
    source
        .prepare_ui_input(UiInputEventKind::Focus { focused: true })
        .unwrap();
    let request = source.take_video_requests().remove(0);
    source
        .prepare_ui_input(UiInputEventKind::PointerButton {
            button: UiPointerButton::Secondary,
            state: UiButtonState::Pressed,
            position: UiPoint { x: 12.0, y: 12.0 },
        })
        .unwrap();
    source
        .prepare_ui_input(UiInputEventKind::PointerButton {
            button: UiPointerButton::Secondary,
            state: UiButtonState::Released,
            position: UiPoint { x: 12.0, y: 12.0 },
        })
        .unwrap();
    press_key(&mut source, "Tab");
    assert_eq!(
        source
            .product_observation_evidence()
            .unwrap()
            .active_system_page,
        Some(astra_vn_core::SystemPageKind::QuickPanel)
    );
    assert_eq!(
        source
            .product_observation_evidence()
            .unwrap()
            .focused_semantic_id
            .as_deref(),
        Some("root/back")
    );
    press_key(&mut source, "Enter");
    assert_eq!(
        source
            .product_observation_evidence()
            .unwrap()
            .active_system_page,
        None
    );
    assert!(!request.is_cancelled());
    assert!(source.take_stage_completions().is_empty());
    assert_eq!(
        source.pending_wait().map(|wait| wait.fence.as_str()),
        Some("movie.end")
    );
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}
