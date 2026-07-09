use astra_vn_presentation::{
    AudioBus, AudioCommand, AudioLoopMode, AudioSync, MovieEndBehavior, PresentationTimeline,
    StageModel, StandardPresentationCommand, TimelineJoinPolicy, TimelineKeyframe,
    TimelineTaskStatus, TimelineTrack, VideoLayerState, VideoLoopMode,
};

#[test]
fn presentation_tracks_video_audio_and_timeline_lifecycle() {
    let mut stage = StageModel::new(1280, 720);

    stage.apply(StandardPresentationCommand::SetVideo(VideoLayerState {
        layer_id: "video.opening".to_string(),
        movie: "native-assets/movie/op.webm".to_string(),
        alpha: 0.75,
        loop_mode: VideoLoopMode::Once,
        end_behavior: MovieEndBehavior::Wait,
        fallback_frame: Some("native-assets/movie/op_fallback.png".to_string()),
        z: 50,
    }));
    stage.apply(StandardPresentationCommand::PlayAudio(AudioCommand {
        id: "voice.hero.0001".to_string(),
        bus: AudioBus::Voice,
        asset: "native-assets/voice/hero0001.ogg".to_string(),
        loop_mode: AudioLoopMode::Once,
        fade_ms: 0,
        sync: AudioSync::Text,
    }));

    let timeline = timeline(
        "tl.hero_enter",
        TimelineJoinPolicy::BlockUntilComplete,
        "hero",
    );
    stage.apply(StandardPresentationCommand::RunTimeline(timeline.clone()));
    stage.apply(StandardPresentationCommand::CompleteTimeline {
        id: "tl.hero_enter".to_string(),
    });

    assert_eq!(stage.video_layers[0].layer_id, "video.opening");
    assert_eq!(stage.audio_commands[0].sync, AudioSync::Text);
    assert!(stage.timelines.is_empty());
    assert_eq!(
        stage.timeline_tasks[0].status,
        TimelineTaskStatus::Completed
    );
    assert_eq!(stage.presentation_hash().to_hex().len(), 32);
    assert_eq!(timeline.stable_hash().to_hex().len(), 32);
}

#[test]
fn replace_target_timeline_cancels_conflicting_running_task() {
    let mut stage = StageModel::new(1920, 1080);

    stage.apply(StandardPresentationCommand::RunTimeline(timeline(
        "tl.hero_enter",
        TimelineJoinPolicy::FireAndForget,
        "hero",
    )));
    stage.apply(StandardPresentationCommand::RunTimeline(timeline(
        "tl.hero_exit",
        TimelineJoinPolicy::ReplaceTarget,
        "hero",
    )));

    assert_eq!(stage.timelines.len(), 1);
    assert_eq!(stage.timelines[0].id, "tl.hero_exit");
    assert_eq!(stage.timeline_tasks[0].status, TimelineTaskStatus::Canceled);
    assert_eq!(
        stage.timeline_tasks[0].cancel_reason.as_deref(),
        Some("replace_target")
    );
    assert_eq!(stage.timeline_tasks[1].status, TimelineTaskStatus::Running);
}

fn timeline(id: &str, join_policy: TimelineJoinPolicy, target: &str) -> PresentationTimeline {
    PresentationTimeline {
        id: id.to_string(),
        join_policy,
        tracks: vec![TimelineTrack {
            target: target.to_string(),
            property: "opacity".to_string(),
            keyframes: vec![
                TimelineKeyframe {
                    time_ms: 0,
                    value: 0.0,
                },
                TimelineKeyframe {
                    time_ms: 400,
                    value: 1.0,
                },
            ],
        }],
    }
}
