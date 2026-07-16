use astra_player::{MacosCgEventHost, MACOS_CGEVENT_MOUSE};
use astra_player_core::{
    PlayerAudioMeterEvidence, PlayerAutomationScript, PlayerAutomationStatus, PlayerAutomationStep,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerInputTranscript, PlayerPlatform,
    PlayerRuntimeRouteEvidence, PlayerVisualComparisonEvidence, PlayerVisualRegionEvidence,
};

#[test]
fn macos_cgevent_transcript_produces_full_playable_report() {
    let mut script = PlayerAutomationScript::new(
        "nativevn-game",
        "classic",
        PlayerPlatform::Macos,
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "scenario.refs/classic.yaml",
    );
    script.expected_routes = vec!["opening".into()];
    script.steps = vec![PlayerAutomationStep {
        id: "input".into(),
        action: "pointer_choose".into(),
        expected_route_id: Some("opening".into()),
    }];
    let transcript = PlayerInputTranscript {
        schema: "astra.player_input_transcript.v2".into(),
        target: script.target.clone(),
        profile: script.profile.clone(),
        platform: PlayerPlatform::Macos,
        package_hash: script.package_hash.clone(),
        events: vec![PlayerInputEvent {
            step_id: "input".into(),
            source: MACOS_CGEVENT_MOUSE.into(),
            kind: "pointer".into(),
            sequence: 1,
            route_id: Some("opening".into()),
        }],
        input_consumption: vec![PlayerInputConsumptionEvidence {
            input_sequence: 1,
            player_sequence: 1,
            source: "player_host.trace".into(),
            kind: "pointer".into(),
            trace_event: "astra.player.input.consumed".into(),
            trace_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into(),
            route_id: Some("opening".into()),
        }],
        visual_regions: vec![PlayerVisualRegionEvidence {
            region_id: "stage".into(),
            before_hash: "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                .into(),
            after_hash: "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                .into(),
            width: 1280,
            height: 720,
        }],
        audio_meter: PlayerAudioMeterEvidence {
            provider: "coreaudio".into(),
            callback_count: 4,
            host_report_hash: "sha256:host".into(),
            sample_count: 24_000,
            peak_dbfs: -10.0,
            rms_dbfs: -22.0,
        },
        visual_comparison: Some(PlayerVisualComparisonEvidence {
            report_hash: "sha256:4444444444444444444444444444444444444444444444444444444444444444"
                .into(),
            checkpoint_count: 1,
            status: PlayerAutomationStatus::Pass,
        }),
        runtime_routes: vec![PlayerRuntimeRouteEvidence {
            input_sequence: 1,
            player_sequence: 1,
            fixed_step: 1,
            coverage_reached: vec!["opening".into()],
            current_state_id: Some("opening".into()),
            terminal_route_ids: vec!["opening".into()],
            pending_choice_ids: Vec::new(),
            runtime_state_hash: "hash128:55555555555555555555555555555555".into(),
            runtime_event_hash: "hash128:66666666666666666666666666666666".into(),
            runtime_presentation_hash: "hash128:77777777777777777777777777777777".into(),
            trace_hash: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .into(),
        }],
        route_coverage: vec!["opening".into()],
    };
    let report = MacosCgEventHost.build_report(&script, &transcript);
    assert_eq!(report.status, PlayerAutomationStatus::Pass);
    assert!(report.full_playable_passed());
}
