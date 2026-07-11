use astra_player::{WebCdpInputHost, WEB_CDP_KEYBOARD, WEB_CDP_MOUSE};
use astra_player_core::{
    PlayerAudioMeterEvidence, PlayerAutomationScript, PlayerAutomationStatus, PlayerAutomationStep,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerInputTranscript, PlayerPlatform,
    PlayerRuntimeRouteEvidence, PlayerVisualComparisonEvidence, PlayerVisualRegionEvidence,
};

#[test]
fn web_cdp_transcript_produces_full_playable_report() {
    let script = script();
    let transcript = transcript(vec!["cdp.session", WEB_CDP_MOUSE, WEB_CDP_KEYBOARD]);

    let report = WebCdpInputHost.build_report(&script, &transcript);

    assert_eq!(report.status, PlayerAutomationStatus::Pass);
    assert!(report.full_playable_passed());
    assert!(report.checks.iter().any(|check| {
        check.id == "player.live_input_surface" && check.status == PlayerAutomationStatus::Pass
    }));
}

#[test]
fn web_blocks_dom_or_js_callback_transcripts() {
    let script = script();
    for source in ["dom.click", "js.callback", "vn_player_command"] {
        let transcript = transcript(vec![source]);
        let report = WebCdpInputHost.build_report(&script, &transcript);
        assert_eq!(report.status, PlayerAutomationStatus::Blocked);
        assert!(report.checks.iter().any(|check| {
            check.id == "player.live_input_surface"
                && check.status == PlayerAutomationStatus::Blocked
        }));
    }
}

fn script() -> PlayerAutomationScript {
    let mut script = PlayerAutomationScript::new(
        "tsuinosora-internal-game",
        "classic",
        PlayerPlatform::Web,
        "sha256:4444444444444444444444444444444444444444444444444444444444444444",
        "scenario.refs/internal-classic.yaml",
    );
    script.expected_routes = vec!["opening".to_string()];
    script.steps = vec![
        PlayerAutomationStep {
            id: "focus".to_string(),
            action: "focus_browser".to_string(),
            expected_route_id: None,
        },
        PlayerAutomationStep {
            id: "choose-opening".to_string(),
            action: "cdp_pointer_choose".to_string(),
            expected_route_id: Some("opening".to_string()),
        },
    ];
    script
}

fn transcript(sources: Vec<&str>) -> PlayerInputTranscript {
    PlayerInputTranscript {
        schema: "astra.player_input_transcript.v2".to_string(),
        target: "tsuinosora-internal-game".to_string(),
        profile: "classic".to_string(),
        platform: PlayerPlatform::Web,
        package_hash: "sha256:4444444444444444444444444444444444444444444444444444444444444444"
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
            .filter(|(_, source)| matches!(*source, WEB_CDP_MOUSE | WEB_CDP_KEYBOARD))
            .map(|(index, _)| PlayerInputConsumptionEvidence {
                input_sequence: (index + 1) as u64,
                player_sequence: (index + 1) as u64,
                source: "browser_host.trace".to_string(),
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
            before_hash: "sha256:5555555555555555555555555555555555555555555555555555555555555555"
                .to_string(),
            after_hash: "sha256:6666666666666666666666666666666666666666666666666666666666666666"
                .to_string(),
            width: 1280,
            height: 720,
        }],
        audio_meter: PlayerAudioMeterEvidence {
            provider: "webaudio".into(),
            callback_count: 4,
            host_report_hash: "sha256:host".into(),
            sample_count: 24_000,
            peak_dbfs: -9.0,
            rms_dbfs: -21.0,
        },
        visual_comparison: Some(PlayerVisualComparisonEvidence {
            report_hash: "sha256:8888888888888888888888888888888888888888888888888888888888888888"
                .to_string(),
            checkpoint_count: 2,
            status: PlayerAutomationStatus::Pass,
        }),
        runtime_routes: vec![PlayerRuntimeRouteEvidence {
            input_sequence: 2,
            player_sequence: 2,
            fixed_step: 1,
            coverage_reached: vec!["opening".to_string()],
            current_state_id: Some("opening".to_string()),
            pending_choice_ids: Vec::new(),
            terminal_route_ids: vec!["opening".to_string()],
            runtime_state_hash: "hash128:11111111111111111111111111111111".to_string(),
            runtime_event_hash: "hash128:22222222222222222222222222222222".to_string(),
            runtime_presentation_hash: "hash128:33333333333333333333333333333333".to_string(),
            trace_hash: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
        }],
        route_coverage: vec!["opening".to_string()],
    }
}
