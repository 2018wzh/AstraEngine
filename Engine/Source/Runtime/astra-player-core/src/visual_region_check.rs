use super::*;

pub(super) fn visual_region_check(regions: &[PlayerVisualRegionEvidence]) -> PlayerAutomationCheck {
    let changed_regions = regions
        .iter()
        .filter(|region| {
            region.width > 0
                && region.height > 0
                && region.before_hash.starts_with("sha256:")
                && region.after_hash.starts_with("sha256:")
                && region.before_hash != region.after_hash
        })
        .count();
    if changed_regions > 0 {
        pass_check(
            "player.visual_region_hash",
            "visual region hash changed after live input",
            vec![evidence("changed_region_count", changed_regions)],
        )
    } else {
        blocked_check(
            "player.visual_region_hash",
            "visual region hash did not prove player-visible state change",
            "ASTRA_PLAYER_VISUAL_REGION_MISSING",
        )
    }
}

pub(super) fn audio_meter_check(
    platform: PlayerPlatform,
    meter: &PlayerAudioMeterEvidence,
) -> PlayerAutomationCheck {
    let provider_matches = match platform {
        PlayerPlatform::Windows => meter.provider == "wasapi",
        PlayerPlatform::Web => meter.provider == "webaudio",
        PlayerPlatform::Linux => matches!(meter.provider.as_str(), "alsa" | "pipewire"),
        PlayerPlatform::Macos => meter.provider == "coreaudio",
        PlayerPlatform::Android => {
            matches!(meter.provider.as_str(), "oboe_aaudio" | "oboe_opensl_es")
        }
    };
    if provider_matches
        && meter.callback_count > 0
        && meter.host_report_hash.starts_with("sha256:")
        && meter.sample_count > 0
        && meter.peak_dbfs > -80.0
        && meter.rms_dbfs.is_finite()
    {
        pass_check(
            "player.audio_meter",
            "audio meter recorded non-silent output",
            vec![
                evidence("sample_count", meter.sample_count),
                evidence("callback_count", meter.callback_count),
                evidence("provider", &meter.provider),
                evidence("host_report_hash", &meter.host_report_hash),
                evidence("peak_dbfs", format!("{:.2}", meter.peak_dbfs)),
            ],
        )
    } else {
        blocked_check(
            "player.audio_meter",
            "audio meter did not prove non-silent playback",
            "ASTRA_PLAYER_AUDIO_METER_MISSING",
        )
    }
}

pub(super) fn visual_comparison_check(
    comparison: Option<&PlayerVisualComparisonEvidence>,
) -> PlayerAutomationCheck {
    let Some(comparison) = comparison else {
        return blocked_check(
            "player.visual_comparison",
            "visual comparison evidence is missing",
            "ASTRA_PLAYER_VISUAL_COMPARISON_MISSING",
        );
    };
    if comparison.status == PlayerAutomationStatus::Pass
        && comparison.report_hash.starts_with("sha256:")
        && comparison.checkpoint_count > 0
    {
        pass_check(
            "player.visual_comparison",
            "visual comparison report passed required checkpoints",
            vec![
                evidence("visual_comparison_report_hash", &comparison.report_hash),
                evidence("checkpoint_count", comparison.checkpoint_count),
            ],
        )
    } else {
        blocked_check(
            "player.visual_comparison",
            "visual comparison evidence did not pass required checkpoints",
            "ASTRA_PLAYER_VISUAL_COMPARISON_BLOCKED",
        )
    }
}

pub(super) fn route_coverage_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    let expected = expected_routes(script);
    let covered = transcript
        .route_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let missing = expected
        .iter()
        .filter(|route| !covered.contains(*route))
        .cloned()
        .collect::<Vec<_>>();
    if !expected.is_empty() && missing.is_empty() {
        pass_check(
            "player.route_coverage",
            "transcript covered all expected route ids",
            vec![evidence("route_count", covered.len())],
        )
    } else {
        blocked_check_with_evidence(
            "player.route_coverage",
            "transcript did not cover all expected route ids",
            "ASTRA_PLAYER_ROUTE_COVERAGE_MISSING",
            vec![evidence("missing_route_count", missing.len())],
        )
    }
}

pub(super) fn expected_routes(script: &PlayerAutomationScript) -> BTreeSet<String> {
    let mut routes = script
        .expected_routes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for step in &script.steps {
        if let Some(route) = &step.expected_route_id {
            routes.insert(route.clone());
        }
    }
    routes
}

pub(super) fn is_live_input_source(platform: PlayerPlatform, source: &str) -> bool {
    match platform {
        PlayerPlatform::Windows => matches!(
            source,
            "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Linux => matches!(
            source,
            "uinput.keyboard" | "uinput.mouse" | "uinput.touch" | "uinput.gamepad"
        ),
        PlayerPlatform::Macos => matches!(
            source,
            "cgevent.keyboard" | "cgevent.mouse" | "cgevent.gamepad"
        ),
        PlayerPlatform::Web => matches!(source, "cdp.mouse" | "cdp.keyboard"),
        PlayerPlatform::Android => matches!(
            source,
            "android.touch" | "android.keyboard" | "android.gamepad" | "android.accessibility"
        ),
    }
}

pub(super) fn is_consumption_trace_source(platform: PlayerPlatform, source: &str) -> bool {
    match platform {
        PlayerPlatform::Windows => source == "player_host.trace",
        PlayerPlatform::Linux => source == "player_host.trace",
        PlayerPlatform::Macos => source == "player_host.trace",
        PlayerPlatform::Web => matches!(source, "player_host.trace" | "browser_host.trace"),
        PlayerPlatform::Android => {
            matches!(source, "player_host.trace" | "android_host.trace")
        }
    }
}

pub(super) fn is_forbidden_input_source(source: &str) -> bool {
    matches!(
        source,
        "route_scenario"
            | "--route-scenario"
            | "dom.click"
            | "dom_click"
            | "js.callback"
            | "js_callback"
            | "vn_player_command"
            | "direct.vn_command"
    )
}

pub(super) fn is_safe_relative_ref(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains("://")
        && !value.contains('\\')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(':'))
}

pub(super) fn pass_check(
    id: impl Into<String>,
    summary: impl Into<String>,
    evidence: Vec<PlayerAutomationEvidence>,
) -> PlayerAutomationCheck {
    PlayerAutomationCheck {
        id: id.into(),
        status: PlayerAutomationStatus::Pass,
        summary: summary.into(),
        diagnostic: None,
        evidence,
    }
}

pub(super) fn blocked_check(
    id: impl Into<String>,
    summary: impl Into<String>,
    code: impl Into<String>,
) -> PlayerAutomationCheck {
    blocked_check_with_evidence(id, summary, code, Vec::new())
}

pub(super) fn blocked_check_with_evidence(
    id: impl Into<String>,
    summary: impl Into<String>,
    code: impl Into<String>,
    evidence: Vec<PlayerAutomationEvidence>,
) -> PlayerAutomationCheck {
    PlayerAutomationCheck {
        id: id.into(),
        status: PlayerAutomationStatus::Blocked,
        summary: summary.into(),
        diagnostic: Some(Diagnostic::blocking(
            code,
            "player automation evidence blocked",
        )),
        evidence,
    }
}

pub(super) fn evidence(key: impl Into<String>, value: impl ToString) -> PlayerAutomationEvidence {
    PlayerAutomationEvidence {
        key: key.into(),
        value: value.to_string(),
    }
}
