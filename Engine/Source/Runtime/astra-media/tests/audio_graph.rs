use astra_media::{AudioCommand, AudioGraph, AudioVoiceState};

#[astra_headless_test::test]
fn audio_graph_voice_fade_pause_seek_loop_and_hash_are_semantic() {
    let mut graph = AudioGraph::default();
    graph.apply(AudioCommand::set_bus_gain("bgm", 0.5)).unwrap();
    graph
        .apply(AudioCommand::play_voice(
            "voice.theme",
            "bgm",
            "asset:/audio/bgm/theme",
            1_000,
            true,
        ))
        .unwrap();
    graph
        .apply(AudioCommand::fade_bus("fade.bgm", "bgm", 1.0, 600))
        .unwrap();
    for _ in 0..6 {
        graph.tick(100).unwrap();
    }
    assert_eq!(graph.buses()["bgm"].gain, 1.0);
    assert!(graph
        .completed_fences()
        .iter()
        .any(|fence| fence.kind == "fade_completed"));

    graph
        .apply(AudioCommand::PauseVoice {
            voice_id: "voice.theme".into(),
        })
        .unwrap();
    let paused_at = graph.voices()["voice.theme"].position_ms;
    graph.tick(100).unwrap();
    assert_eq!(graph.voices()["voice.theme"].position_ms, paused_at);
    assert_eq!(graph.voices()["voice.theme"].state, AudioVoiceState::Paused);
    graph
        .apply(AudioCommand::SeekVoice {
            voice_id: "voice.theme".into(),
            position_ms: 950,
        })
        .unwrap();
    graph
        .apply(AudioCommand::ResumeVoice {
            voice_id: "voice.theme".into(),
        })
        .unwrap();
    graph.tick(100).unwrap();
    assert_eq!(graph.voices()["voice.theme"].position_ms, 50);
    assert_eq!(graph.voices()["voice.theme"].loop_count, 1);

    let first = graph.deterministic_hash().unwrap();
    let second = graph.deterministic_hash().unwrap();
    assert_eq!(first, second);
}

#[astra_headless_test::test]
fn audio_graph_rejects_invalid_and_conflicting_commands_without_partial_state() {
    let mut graph = AudioGraph::default();
    graph
        .apply(AudioCommand::set_bus_gain("voice", 0.8))
        .unwrap();
    let before = graph.deterministic_hash().unwrap();
    assert!(graph
        .apply(AudioCommand::play_voice(
            "bad voice",
            "voice",
            "asset:/voice/line",
            100,
            false,
        ))
        .is_err());
    assert_eq!(graph.deterministic_hash().unwrap(), before);

    graph
        .apply(AudioCommand::fade_bus("fade.1", "voice", 0.0, 100))
        .unwrap();
    let before_conflict = graph.deterministic_hash().unwrap();
    assert!(graph
        .apply(AudioCommand::fade_bus("fade.2", "voice", 1.0, 100))
        .is_err());
    assert_eq!(graph.deterministic_hash().unwrap(), before_conflict);
    assert!(graph.tick(0).is_err());
    assert_eq!(graph.deterministic_hash().unwrap(), before_conflict);
}

#[astra_headless_test::test]
fn non_looping_voice_completes_and_is_released() {
    let mut graph = AudioGraph::default();
    graph
        .apply(AudioCommand::play_voice(
            "voice.line",
            "voice",
            "asset:/voice/line",
            100,
            false,
        ))
        .unwrap();
    graph.tick(100).unwrap();
    assert!(!graph.voices().contains_key("voice.line"));
    assert!(graph
        .completed_fences()
        .iter()
        .any(|fence| { fence.kind == "voice_completed" && fence.resource_id == "voice.line" }));
}
