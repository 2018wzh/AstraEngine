use astra_player_core::{
    PlayerAudioMeterEvidence, PlayerAutomationScript, PlayerAutomationStatus, PlayerAutomationStep,
    PlayerAutomationValidator, PlayerInputEvent, PlayerInputTranscript, PlayerPlatform,
    PlayerVisualRegionEvidence,
};

#[test]
fn player_automation_report_passes_for_live_windows_input() {
    let script = script(PlayerPlatform::Windows);
    let transcript = transcript(PlayerPlatform::Windows, "sendinput.mouse");

    let report = PlayerAutomationValidator.validate(&script, &transcript);

    assert_eq!(report.schema, "astra.player_automation_report.v1");
    assert_eq!(report.status, PlayerAutomationStatus::Pass);
    assert!(report.full_playable_passed());
    assert!(report.checks.iter().any(|check| {
        check.id == "player.live_input_surface" && check.status == PlayerAutomationStatus::Pass
    }));
}

#[test]
fn player_automation_report_blocks_direct_route_scenario_input() {
    let script = script(PlayerPlatform::Windows);
    let transcript = transcript(PlayerPlatform::Windows, "route_scenario");

    let report = PlayerAutomationValidator.validate(&script, &transcript);

    assert_eq!(report.status, PlayerAutomationStatus::Blocked);
    assert!(report.checks.iter().any(|check| {
        check.id == "player.live_input_surface"
            && check.status == PlayerAutomationStatus::Blocked
            && check
                .diagnostic
                .as_ref()
                .is_some_and(|diagnostic| diagnostic.code == "ASTRA_PLAYER_FORBIDDEN_INPUT_SURFACE")
    }));
    assert!(!report.full_playable_passed());
}

fn script(platform: PlayerPlatform) -> PlayerAutomationScript {
    let mut script = PlayerAutomationScript::new(
        "tsuinosora-internal-game",
        "classic",
        platform,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "scenario.refs/demo.route.yaml",
    );
    script.expected_routes = vec!["route.opening".to_string()];
    script.steps = vec![PlayerAutomationStep {
        id: "step.choose.opening".to_string(),
        action: "choose".to_string(),
        expected_route_id: Some("route.opening".to_string()),
    }];
    script
}

fn transcript(platform: PlayerPlatform, source: &str) -> PlayerInputTranscript {
    PlayerInputTranscript {
        schema: "astra.player_input_transcript.v1".to_string(),
        target: "tsuinosora-internal-game".to_string(),
        profile: "classic".to_string(),
        platform,
        package_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_string(),
        events: vec![PlayerInputEvent {
            step_id: "step.choose.opening".to_string(),
            source: source.to_string(),
            kind: "pointer_up".to_string(),
            sequence: 1,
            route_id: Some("route.opening".to_string()),
        }],
        visual_regions: vec![PlayerVisualRegionEvidence {
            region_id: "stage".to_string(),
            before_hash: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            after_hash: "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .to_string(),
            width: 1280,
            height: 720,
        }],
        audio_meter: PlayerAudioMeterEvidence {
            sample_count: 48_000,
            peak_dbfs: -12.0,
            rms_dbfs: -24.0,
        },
        route_coverage: vec!["route.opening".to_string()],
    }
}
