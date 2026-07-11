use astra_player_core::{
    PlayerAudioMeterEvidence, PlayerAutomationScript, PlayerAutomationStatus, PlayerAutomationStep,
    PlayerAutomationValidator, PlayerInputConsumptionEvidence, PlayerInputEvent,
    PlayerInputTranscript, PlayerPlatform, PlayerPlatformEvidenceIdentity,
    PlayerRuntimeRouteEvidence, PlayerVisualComparisonEvidence, PlayerVisualRegionEvidence,
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
    assert!(report.checks.iter().any(|check| {
        check.id == "player.visual_comparison" && check.status == PlayerAutomationStatus::Pass
    }));
}

#[test]
fn player_automation_report_binds_platform_identity_to_full_playable() {
    let report = PlayerAutomationValidator.validate_with_platform_identity(
        &script(PlayerPlatform::Windows),
        &transcript(PlayerPlatform::Windows, "sendinput.mouse"),
        &PlayerPlatformEvidenceIdentity {
            profile_hash: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                .to_string(),
            build_fingerprint:
                "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                    .to_string(),
            session_id: "session.windows.1".to_string(),
        },
    );
    let full = report
        .checks
        .iter()
        .find(|check| check.id == "player.full_playable")
        .unwrap();
    assert!(full
        .evidence
        .iter()
        .any(|entry| entry.key == "session_id" && entry.value == "session.windows.1"));
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

#[test]
fn player_automation_report_blocks_missing_visual_comparison() {
    let script = script(PlayerPlatform::Windows);
    let mut transcript = transcript(PlayerPlatform::Windows, "sendinput.mouse");
    transcript.visual_comparison = None;

    let report = PlayerAutomationValidator.validate(&script, &transcript);

    assert_eq!(report.status, PlayerAutomationStatus::Blocked);
    assert!(report.checks.iter().any(|check| {
        check.id == "player.visual_comparison"
            && check.status == PlayerAutomationStatus::Blocked
            && check.diagnostic.as_ref().is_some_and(|diagnostic| {
                diagnostic.code == "ASTRA_PLAYER_VISUAL_COMPARISON_MISSING"
            })
    }));
    assert!(!report.full_playable_passed());
}

#[test]
fn player_automation_report_blocks_missing_consumed_trace() {
    let script = script(PlayerPlatform::Windows);
    let mut transcript = transcript(PlayerPlatform::Windows, "sendinput.mouse");
    transcript.input_consumption.clear();

    let report = PlayerAutomationValidator.validate(&script, &transcript);

    assert_eq!(report.status, PlayerAutomationStatus::Blocked);
    assert!(report.checks.iter().any(|check| {
        check.id == "player.input_consumption_trace"
            && check.status == PlayerAutomationStatus::Blocked
            && check.diagnostic.as_ref().is_some_and(|diagnostic| {
                diagnostic.code == "ASTRA_PLAYER_INPUT_CONSUMPTION_TRACE_MISSING"
            })
    }));
    assert!(!report.full_playable_passed());
}

#[test]
fn player_automation_report_blocks_route_coverage_without_runtime_evidence() {
    let script = script(PlayerPlatform::Windows);
    let mut transcript = transcript(PlayerPlatform::Windows, "sendinput.mouse");
    transcript.runtime_routes.clear();

    let report = PlayerAutomationValidator.validate(&script, &transcript);

    assert_eq!(report.status, PlayerAutomationStatus::Blocked);
    assert!(report.checks.iter().any(|check| {
        check.id == "player.runtime_route_evidence"
            && check.status == PlayerAutomationStatus::Blocked
            && check.diagnostic.as_ref().is_some_and(|diagnostic| {
                diagnostic.code == "ASTRA_PLAYER_RUNTIME_ROUTE_EVIDENCE_MISSING"
            })
    }));
}

#[test]
fn player_automation_report_blocks_route_without_terminal_signature() {
    let script = script(PlayerPlatform::Windows);
    let mut transcript = transcript(PlayerPlatform::Windows, "sendinput.mouse");
    transcript.runtime_routes[0].terminal_route_ids.clear();

    let report = PlayerAutomationValidator.validate(&script, &transcript);

    assert!(report.checks.iter().any(|check| {
        check.id == "player.runtime_route_evidence"
            && check.status == PlayerAutomationStatus::Blocked
            && check.diagnostic.as_ref().is_some_and(|diagnostic| {
                diagnostic.code == "ASTRA_PLAYER_RUNTIME_ROUTE_EVIDENCE_INVALID"
            })
    }));
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
        schema: "astra.player_input_transcript.v2".to_string(),
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
        input_consumption: vec![PlayerInputConsumptionEvidence {
            input_sequence: 1,
            player_sequence: 1,
            source: "player_host.trace".to_string(),
            kind: "pointer_up".to_string(),
            trace_event: "astra.player.input.consumed".to_string(),
            trace_hash: "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                .to_string(),
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
            provider: "wasapi".into(),
            callback_count: 4,
            host_report_hash: "sha256:host".into(),
            sample_count: 48_000,
            peak_dbfs: -12.0,
            rms_dbfs: -24.0,
        },
        visual_comparison: Some(PlayerVisualComparisonEvidence {
            report_hash: "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                .to_string(),
            checkpoint_count: 1,
            status: PlayerAutomationStatus::Pass,
        }),
        runtime_routes: vec![PlayerRuntimeRouteEvidence {
            input_sequence: 1,
            player_sequence: 1,
            fixed_step: 1,
            coverage_reached: vec!["route.opening".to_string()],
            current_state_id: Some("route.opening".to_string()),
            pending_choice_ids: Vec::new(),
            terminal_route_ids: vec!["route.opening".to_string()],
            runtime_state_hash: "hash128:11111111111111111111111111111111".to_string(),
            runtime_event_hash: "hash128:22222222222222222222222222222222".to_string(),
            runtime_presentation_hash: "hash128:33333333333333333333333333333333".to_string(),
            trace_hash: "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                .to_string(),
        }],
        route_coverage: vec!["route.opening".to_string()],
    }
}
