use astra_runtime::{
    PackageHandle, RuntimeConfig, RuntimeWorld, TickInput, TickIntegrityMode, TickRequest,
};

fn request(step: u64) -> TickRequest {
    TickRequest::live(
        TickInput {
            fixed_step: step,
            delta_ns: 16_666_667,
            seed: 7,
        },
        vec![],
    )
}

#[astra_headless_test::test]
fn shipping_mode_disables_aggregate_hashes_and_replay_recording() {
    let mut world = RuntimeWorld::create_with_integrity(
        RuntimeConfig {
            seed: 7,
            required_slots: vec![],
        },
        PackageHandle::default(),
        TickIntegrityMode::Shipping,
    )
    .unwrap();
    let report = world.tick(request(1)).unwrap();
    assert_eq!(report.integrity_mode, TickIntegrityMode::Shipping);
    assert_eq!(report.state_hash, report.event_hash);
    assert_eq!(report.event_hash, report.presentation_hash);
    let error = world.begin_replay_recording().unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_RUNTIME_REPLAY_RECORDING_DISABLED"));
}

#[astra_headless_test::test]
fn evidence_mode_records_and_replays_v3_transcript() {
    let config = RuntimeConfig {
        seed: 7,
        required_slots: vec![],
    };
    let package = PackageHandle::default();
    let mut world = RuntimeWorld::create_with_integrity(
        config.clone(),
        package.clone(),
        TickIntegrityMode::Evidence,
    )
    .unwrap();
    let mut recorder = world.begin_replay_recording().unwrap();
    let request = request(1);
    let report = world.tick(request.clone()).unwrap();
    recorder.record(request, &report).unwrap();
    let transcript = recorder.finish();
    assert_eq!(transcript.schema, "astra.runtime_replay_transcript.v3");

    let mut replay_world =
        RuntimeWorld::create_with_integrity(config, package, TickIntegrityMode::Evidence).unwrap();
    let replay_report = replay_world.replay(transcript).unwrap();
    assert_eq!(replay_report.state_hash, report.state_hash);
    assert_eq!(replay_report.event_hash, report.event_hash);
    assert_eq!(replay_report.presentation_hash, report.presentation_hash);
}
