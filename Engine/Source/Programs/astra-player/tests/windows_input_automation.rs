use astra_player::{WindowsSendInputHost, WINDOWS_SENDINPUT_KEYBOARD, WINDOWS_SENDINPUT_MOUSE};
use astra_player_core::{
    PlayerAudioMeterEvidence, PlayerAutomationScript, PlayerAutomationStatus, PlayerAutomationStep,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerInputTranscript, PlayerPlatform,
    PlayerVisualComparisonEvidence, PlayerVisualRegionEvidence,
};

#[test]
fn windows_sendinput_transcript_produces_full_playable_report() {
    let script = script();
    let transcript = transcript(vec![WINDOWS_SENDINPUT_MOUSE, WINDOWS_SENDINPUT_KEYBOARD]);

    let report = WindowsSendInputHost.build_report(&script, &transcript);

    assert_eq!(report.status, PlayerAutomationStatus::Pass);
    assert!(report.full_playable_passed());
    for id in [
        "player.input_transcript",
        "player.visual_region_hash",
        "player.visual_comparison",
        "player.audio_meter",
        "player.route_coverage",
    ] {
        assert!(report
            .checks
            .iter()
            .any(|check| check.id == id && check.status == PlayerAutomationStatus::Pass));
    }
}

#[test]
fn windows_blocks_route_scenario_transcript() {
    let script = script();
    let transcript = transcript(vec!["route_scenario"]);

    let report = WindowsSendInputHost.build_report(&script, &transcript);

    assert_eq!(report.status, PlayerAutomationStatus::Blocked);
    assert!(report.checks.iter().any(|check| {
        check.id == "player.live_input_surface" && check.status == PlayerAutomationStatus::Blocked
    }));
}

fn script() -> PlayerAutomationScript {
    let mut script = PlayerAutomationScript::new(
        "tsuinosora-internal-game",
        "classic",
        PlayerPlatform::Windows,
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "scenario.refs/internal-classic.yaml",
    );
    script.expected_routes = vec!["opening".to_string()];
    script.steps = vec![
        PlayerAutomationStep {
            id: "focus".to_string(),
            action: "focus_window".to_string(),
            expected_route_id: None,
        },
        PlayerAutomationStep {
            id: "choose-opening".to_string(),
            action: "pointer_choose".to_string(),
            expected_route_id: Some("opening".to_string()),
        },
    ];
    script
}

fn transcript(sources: Vec<&str>) -> PlayerInputTranscript {
    PlayerInputTranscript {
        schema: "astra.player_input_transcript.v1".to_string(),
        target: "tsuinosora-internal-game".to_string(),
        profile: "classic".to_string(),
        platform: PlayerPlatform::Windows,
        package_hash: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        events: sources
            .iter()
            .enumerate()
            .map(|(index, source)| PlayerInputEvent {
                step_id: format!("step-{index}"),
                source: (*source).to_string(),
                kind: "input".to_string(),
                sequence: (index + 1) as u64,
                route_id: Some("opening".to_string()),
            })
            .collect(),
        input_consumption: sources
            .into_iter()
            .enumerate()
            .filter(|(_, source)| {
                matches!(
                    *source,
                    WINDOWS_SENDINPUT_MOUSE | WINDOWS_SENDINPUT_KEYBOARD
                )
            })
            .map(|(index, _)| PlayerInputConsumptionEvidence {
                input_sequence: (index + 1) as u64,
                player_sequence: (index + 1) as u64,
                source: "player_host.trace".to_string(),
                kind: "input".to_string(),
                trace_event: "astra.player.input.consumed".to_string(),
                trace_hash:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                route_id: Some("opening".to_string()),
            })
            .collect(),
        visual_regions: vec![PlayerVisualRegionEvidence {
            region_id: "stage".to_string(),
            before_hash: "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                .to_string(),
            after_hash: "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                .to_string(),
            width: 1280,
            height: 720,
        }],
        audio_meter: PlayerAudioMeterEvidence {
            provider: "wasapi".into(),
            callback_count: 4,
            host_report_hash: "sha256:host".into(),
            sample_count: 24_000,
            peak_dbfs: -10.0,
            rms_dbfs: -22.0,
        },
        visual_comparison: Some(PlayerVisualComparisonEvidence {
            report_hash: "sha256:7777777777777777777777777777777777777777777777777777777777777777"
                .to_string(),
            checkpoint_count: 2,
            status: PlayerAutomationStatus::Pass,
        }),
        route_coverage: vec!["opening".to_string()],
    }
}
