use astra_media::{AudioCommand, AudioGraph, AudioMeterProvider};

#[test]
fn audio_graph_mixes_bus_fade_loop_and_headless_meter_hash() {
    let mut graph = AudioGraph::default();
    graph.apply(AudioCommand::set_bus_gain("bgm", 0.5)).unwrap();
    graph
        .apply(AudioCommand::play_bgm(
            "bgm",
            "asset:/audio/bgm/theme",
            120_000,
            true,
        ))
        .unwrap();
    graph.apply(AudioCommand::fade_bus("bgm", 1.0, 60)).unwrap();
    for _ in 0..60 {
        graph.tick();
    }
    assert!(graph
        .completed_fences()
        .iter()
        .any(|fence| fence.kind == "fade"));

    let meter = AudioMeterProvider;
    let first = meter.meter_hash(&graph);
    let second = meter.meter_hash(&graph);
    assert_eq!(first, second);
    assert!(graph.voices().iter().any(|voice| voice.looping));
}
