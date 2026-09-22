use astra_vn_presentation::{
    BackgroundRegionCommand, CharacterRegionCommand, FenceStatus, MovieLoopMode,
    PresentationCommandEnvelope, PresentationCoordinator, PresentationInterruptPolicy,
    PresentationRegionCommand, TextAdvanceDisposition, TextRegionCommand, VideoRegionCommand,
    VnMovieEndBehavior,
};

fn character(sequence: u64, layer: &str) -> PresentationCommandEnvelope {
    PresentationCommandEnvelope {
        fixed_step: 1,
        sequence,
        command_id: format!("character.{sequence}"),
        interrupt: PresentationInterruptPolicy::ReplaceFromCurrent,
        fence: Some(format!("fence.character.{sequence}")),
        payload: PresentationRegionCommand::Character(CharacterRegionCommand {
            character_id: "hero".into(),
            asset: "asset:/hero.png".into(),
            pose: Some("smile".into()),
            layer: layer.into(),
            visible: true,
            duration_ns: 500_000_000,
        }),
    }
}

fn background(sequence: u64, layer: &str) -> PresentationCommandEnvelope {
    PresentationCommandEnvelope {
        fixed_step: 1,
        sequence,
        command_id: format!("background.{sequence}"),
        interrupt: PresentationInterruptPolicy::SnapThenStart,
        fence: Some(format!("fence.background.{sequence}")),
        payload: PresentationRegionCommand::Background(BackgroundRegionCommand {
            layer: layer.into(),
            asset: Some("asset:/room.png".into()),
            duration_ns: 1_000_000_000,
        }),
    }
}

fn text(sequence: u64) -> PresentationCommandEnvelope {
    PresentationCommandEnvelope {
        fixed_step: 1,
        sequence,
        command_id: format!("text.{sequence}"),
        interrupt: PresentationInterruptPolicy::ReplaceFromCurrent,
        fence: Some(format!("fence.text.{sequence}")),
        payload: PresentationRegionCommand::Text(TextRegionCommand {
            text_key: "line.hello".into(),
            speaker: Some("hero".into()),
            window: Some("main".into()),
            grapheme_count: 20,
            graphemes_per_second: 20,
        }),
    }
}

fn video(sequence: u64, layer: &str) -> PresentationCommandEnvelope {
    PresentationCommandEnvelope {
        fixed_step: 1,
        sequence,
        command_id: format!("video.{sequence}"),
        interrupt: PresentationInterruptPolicy::Reject,
        fence: Some(format!("fence.video.{sequence}")),
        payload: PresentationRegionCommand::Video(VideoRegionCommand {
            session_id: "opening".into(),
            layer: layer.into(),
            asset: "asset:/opening.webm".into(),
            logical_start_ns: 125_000_000,
            loop_mode: MovieLoopMode::Once,
            end_behavior: VnMovieEndBehavior::Wait,
            fallback: Some("asset:/opening-fallback.png".into()),
        }),
    }
}

#[test]
fn serial_and_parallel_region_preparation_are_identical() {
    let commands = vec![
        character(1, "character"),
        background(2, "background"),
        text(3),
        video(4, "movie"),
    ];
    let serial = PresentationCoordinator::default()
        .prepare_batch(&commands, 1)
        .unwrap();
    let parallel = PresentationCoordinator::default()
        .prepare_batch(&commands, 4)
        .unwrap();
    assert_eq!(serial.0, parallel.0);
    assert_eq!(serial.1, parallel.1);
}

#[test]
fn cross_region_layer_conflict_fails_without_partial_commit() {
    let mut coordinator = PresentationCoordinator::default();
    let before = coordinator.clone();
    let error = coordinator
        .apply_batch(&[character(1, "foreground"), video(2, "foreground")], 4)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_VN_PRESENTATION_REGION_WRITE_CONFLICT"));
    assert_eq!(coordinator, before);
}

#[test]
fn text_click_completes_reveal_before_requesting_story_advance() {
    let mut coordinator = PresentationCoordinator::default();
    coordinator.apply_batch(&[text(1)], 1).unwrap();
    assert_eq!(
        coordinator.request_text_advance(),
        TextAdvanceDisposition::RevealCompleted
    );
    assert_eq!(
        coordinator.request_text_advance(),
        TextAdvanceDisposition::StoryAdvanceRequested
    );
    assert_eq!(
        coordinator.state().fences.get("fence.text.1"),
        Some(&FenceStatus::Completed)
    );
}

#[test]
fn mid_animation_snapshot_and_video_failure_preserve_logical_state() {
    let mut coordinator = PresentationCoordinator::default();
    coordinator
        .apply_batch(&[background(1, "background"), video(2, "movie")], 2)
        .unwrap();
    coordinator.tick(250_000_000).unwrap();
    let snapshot = coordinator.snapshot().unwrap();
    let mut restored = PresentationCoordinator::restore(&snapshot).unwrap();
    assert_eq!(restored, coordinator);
    assert_eq!(
        restored.fail_video("opening").unwrap().as_deref(),
        Some("asset:/opening-fallback.png")
    );
    assert_eq!(
        restored.state().fences.get("fence.video.2"),
        Some(&FenceStatus::Failed)
    );
}

#[test]
fn explicit_queue_and_reject_policies_are_enforced() {
    let mut coordinator = PresentationCoordinator::default();
    coordinator
        .apply_batch(&[character(1, "character")], 1)
        .unwrap();

    let mut queued = character(2, "character");
    queued.interrupt = PresentationInterruptPolicy::Queue;
    coordinator.apply_batch(&[queued], 1).unwrap();
    assert_eq!(coordinator.state().character.queued.len(), 1);

    let mut rejected = character(3, "character");
    rejected.interrupt = PresentationInterruptPolicy::Reject;
    let error = coordinator.apply_batch(&[rejected], 1).unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_VN_CHARACTER_INTERRUPT_REJECTED"));
}

#[path = "support/coordinator_tick.rs"]
mod tick_regressions;

#[path = "support/fence_groups.rs"]
mod fence_groups;

#[test]
fn text_sequence_race_preserves_partial_restore_and_click_boundaries() {
    for click_first in [false, true] {
        let mut c = PresentationCoordinator::default();
        c.apply_batch(&[text(1)], 1).unwrap();
        c.tick(250_000_000).unwrap();
        let mut c = PresentationCoordinator::restore(&c.snapshot().unwrap()).unwrap();
        assert_eq!(c.state().text.active.as_ref().unwrap().visible_graphemes, 5);
        assert!(c.acknowledge_story_advance().is_err());
        if click_first {
            assert_eq!(
                c.request_text_advance(),
                TextAdvanceDisposition::RevealCompleted
            );
            assert!(c.tick(750_000_000).unwrap().is_empty());
        } else {
            assert_eq!(c.tick(750_000_000).unwrap(), vec!["fence.text.1"]);
        }
        let mut c = PresentationCoordinator::restore(&c.snapshot().unwrap()).unwrap();
        assert_eq!(
            c.state().text.active.as_ref().unwrap().visible_graphemes,
            20
        );
        assert!(c.tick(1).unwrap().is_empty());
        assert_eq!(
            c.request_text_advance(),
            TextAdvanceDisposition::StoryAdvanceRequested
        );
        c.acknowledge_story_advance().unwrap();
        assert_eq!(
            c.request_text_advance(),
            TextAdvanceDisposition::NoActiveText
        );
        assert!(c.acknowledge_story_advance().is_err());
    }
}

#[test]
fn current_dsl_expresses_parallel_movie_wait_and_independent_dialogue() {
    astra_vn_script::compile_astra_project([astra_vn_script::AstraSource::story("tasks.astra", r#"
story tasks #@id story.tasks
state start #@id state.start
  scene demo #@id scene.demo
    stage viewport:1280x720 safe_area:16:9 #@id stage.configure
    layer id:movie.left kind:video z:10 blend:normal clip:stage #@id layer.left
    layer id:movie.right kind:video z:11 blend:normal clip:stage #@id layer.right
    movie layer:movie.left asset:asset:/movie/short end:continue fence:movies.done interrupt:reject #@id movie.left
    movie layer:movie.right asset:asset:/movie/long end:continue fence:movies.done interrupt:reject #@id movie.right
    wait fence:movies.done #@id wait.movies
    text key:tasks.reveal speaker:narrator #@id text.reveal
    text key:tasks.next speaker:narrator #@id text.next
"#)], Default::default()).unwrap();
}

#[test]
fn old_coordinator_header_is_rejected_before_decoding_changed_fields() {
    let old = postcard::to_allocvec("astra.vn.presentation_coordinator.v5").unwrap();
    assert!(PresentationCoordinator::restore(&old)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_VN_PRESENTATION_SNAPSHOT_SCHEMA"));
}
