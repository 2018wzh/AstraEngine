use super::*;

impl PlayerAutomationValidator {
    pub fn validate(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        self.validate_internal(script, transcript, None)
    }

    pub fn validate_with_platform_identity(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
        identity: &PlayerPlatformEvidenceIdentity,
    ) -> PlayerAutomationReport {
        self.validate_internal(script, transcript, Some(identity))
    }

    pub(super) fn validate_internal(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
        identity: Option<&PlayerPlatformEvidenceIdentity>,
    ) -> PlayerAutomationReport {
        tracing::info!(
            event = "player.automation.validate.start",
            platform = ?script.platform,
            expected_route_count = script.expected_routes.len(),
            input_event_count = transcript.events.len(),
            "player automation validation started"
        );
        let (transcript_hash, serialization_check) = match transcript.hash() {
            Ok(hash) => (
                hash.to_string(),
                pass_check(
                    "player.transcript_serialization",
                    "player transcript has a canonical JSON identity",
                    vec![evidence("transcript_hash", hash)],
                ),
            ),
            Err(_) => (
                String::new(),
                blocked_check(
                    "player.transcript_serialization",
                    "player transcript could not be serialized without data loss",
                    "ASTRA_PLAYER_TRANSCRIPT_SERIALIZATION",
                ),
            ),
        };
        let mut checks = vec![
            serialization_check,
            schema_check(script, transcript),
            identity_check(script, transcript),
            live_input_surface_check(script.platform, &transcript.events),
            input_consumption_trace_check(
                script.platform,
                &transcript.events,
                &transcript.input_consumption,
            ),
            transcript_coverage_check(script, transcript),
            visual_region_check(&transcript.visual_regions),
            visual_comparison_check(transcript.visual_comparison.as_ref()),
            audio_meter_check(transcript.platform, &transcript.audio_meter),
            runtime_route_evidence_check(script, transcript),
            route_coverage_check(script, transcript),
        ];
        if let Some(identity) = identity {
            checks.push(platform_identity_check(identity));
        }
        let full_playable = if checks
            .iter()
            .all(|check| check.status == PlayerAutomationStatus::Pass)
        {
            let mut full_evidence = vec![
                evidence("transcript_hash", &transcript_hash),
                evidence("route_count", transcript.route_coverage.len()),
            ];
            if let Some(identity) = identity {
                full_evidence.extend([
                    evidence("profile_hash", &identity.profile_hash),
                    evidence("build_fingerprint", &identity.build_fingerprint),
                    evidence("session_id", &identity.session_id),
                ]);
            }
            pass_check(
                "player.full_playable",
                "live player automation covered route, visual and audio evidence",
                full_evidence,
            )
        } else {
            blocked_check(
                "player.full_playable",
                "live player automation evidence is incomplete or unsafe",
                "ASTRA_PLAYER_FULL_PLAYABLE_BLOCKED",
            )
        };
        checks.push(full_playable);
        let status = if checks
            .iter()
            .any(|check| check.status == PlayerAutomationStatus::Blocked)
        {
            PlayerAutomationStatus::Blocked
        } else {
            PlayerAutomationStatus::Pass
        };
        let report = PlayerAutomationReport {
            schema: "astra.player_automation_report.v1".to_string(),
            status,
            target: script.target.clone(),
            profile: script.profile.clone(),
            platform: script.platform,
            package_hash: script.package_hash.clone(),
            transcript_hash,
            route_coverage: transcript.route_coverage.clone(),
            checks,
        };
        match report.status {
            PlayerAutomationStatus::Pass => tracing::info!(
                event = "player.automation.validate.complete",
                status = "pass",
                check_count = report.checks.len(),
                route_count = report.route_coverage.len(),
                "player automation validation completed"
            ),
            PlayerAutomationStatus::Blocked => tracing::error!(
                event = "player.automation.validate.complete",
                status = "blocked",
                check_count = report.checks.len(),
                route_count = report.route_coverage.len(),
                "player automation validation blocked"
            ),
        }
        report
    }
}

pub(super) fn platform_identity_check(
    identity: &PlayerPlatformEvidenceIdentity,
) -> PlayerAutomationCheck {
    if identity.profile_hash.starts_with("sha256:")
        && identity.build_fingerprint.starts_with("sha256:")
        && !identity.session_id.is_empty()
    {
        pass_check(
            "player.platform_identity",
            "player evidence is bound to the host session",
            vec![
                evidence("profile_hash", &identity.profile_hash),
                evidence("build_fingerprint", &identity.build_fingerprint),
                evidence("session_id", &identity.session_id),
            ],
        )
    } else {
        blocked_check(
            "player.platform_identity",
            "player platform identity is incomplete",
            "ASTRA_PLAYER_PLATFORM_IDENTITY",
        )
    }
}

pub(super) fn schema_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    if script.schema == "astra.player_automation_script.v1"
        && transcript.schema == "astra.player_input_transcript.v3"
        && is_safe_relative_ref(&script.scenario_ref)
    {
        pass_check(
            "player.automation_schema",
            "automation script and transcript schemas are valid",
            vec![evidence("scenario_ref", &script.scenario_ref)],
        )
    } else {
        blocked_check(
            "player.automation_schema",
            "automation script or transcript schema is invalid",
            "ASTRA_PLAYER_AUTOMATION_SCHEMA",
        )
    }
}

pub(super) fn identity_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    if script.target == transcript.target
        && script.profile == transcript.profile
        && script.platform == transcript.platform
        && script.package_hash == transcript.package_hash
        && script.package_hash.starts_with("sha256:")
    {
        pass_check(
            "player.package_identity",
            "transcript matches package, target, profile and platform",
            vec![
                evidence("target", &script.target),
                evidence("profile", &script.profile),
                evidence("package_hash", &script.package_hash),
            ],
        )
    } else {
        blocked_check(
            "player.package_identity",
            "transcript identity does not match script",
            "ASTRA_PLAYER_PACKAGE_IDENTITY",
        )
    }
}

pub(super) fn live_input_surface_check(
    platform: PlayerPlatform,
    events: &[PlayerInputEvent],
) -> PlayerAutomationCheck {
    let mut forbidden = Vec::new();
    for event in events {
        if is_forbidden_input_source(&event.source) {
            forbidden.push(event.source.clone());
        }
    }
    if !forbidden.is_empty() {
        forbidden.sort();
        forbidden.dedup();
        return blocked_check_with_evidence(
            "player.live_input_surface",
            "transcript used a forbidden non-live input surface",
            "ASTRA_PLAYER_FORBIDDEN_INPUT_SURFACE",
            vec![evidence("forbidden_source", forbidden.join(","))],
        );
    }

    let has_required = events.iter().any(|event| match platform {
        PlayerPlatform::Windows => matches!(
            event.source.as_str(),
            "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Linux => matches!(
            event.source.as_str(),
            "uinput.keyboard" | "uinput.mouse" | "uinput.touch" | "uinput.gamepad"
        ),
        PlayerPlatform::Macos => matches!(
            event.source.as_str(),
            "cgevent.keyboard" | "cgevent.mouse" | "cgevent.gamepad"
        ),
        PlayerPlatform::Web => matches!(event.source.as_str(), "cdp.mouse" | "cdp.keyboard"),
        PlayerPlatform::Android => matches!(
            event.source.as_str(),
            "android.touch" | "android.keyboard" | "android.gamepad" | "android.accessibility"
        ),
    });
    let all_allowed = events.iter().all(|event| match platform {
        PlayerPlatform::Windows => matches!(
            event.source.as_str(),
            "window.focus" | "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Linux => matches!(
            event.source.as_str(),
            "window.focus" | "uinput.keyboard" | "uinput.mouse" | "uinput.touch" | "uinput.gamepad"
        ),
        PlayerPlatform::Macos => matches!(
            event.source.as_str(),
            "window.focus" | "cgevent.keyboard" | "cgevent.mouse" | "cgevent.gamepad"
        ),
        PlayerPlatform::Web => matches!(
            event.source.as_str(),
            "browser.focus" | "cdp.session" | "cdp.mouse" | "cdp.keyboard"
        ),
        PlayerPlatform::Android => matches!(
            event.source.as_str(),
            "android.focus"
                | "android.touch"
                | "android.keyboard"
                | "android.gamepad"
                | "android.accessibility"
        ),
    });
    if !events.is_empty() && has_required && all_allowed {
        pass_check(
            "player.live_input_surface",
            "transcript uses the required live input surface",
            vec![evidence("event_count", events.len())],
        )
    } else {
        blocked_check(
            "player.live_input_surface",
            "transcript does not prove live player input",
            "ASTRA_PLAYER_LIVE_INPUT_MISSING",
        )
    }
}

pub(super) fn input_consumption_trace_check(
    platform: PlayerPlatform,
    events: &[PlayerInputEvent],
    consumption: &[PlayerInputConsumptionEvidence],
) -> PlayerAutomationCheck {
    let live_inputs = events
        .iter()
        .filter(|event| is_live_input_source(platform, &event.source))
        .collect::<Vec<_>>();
    let mut missing = 0usize;
    let mut invalid = 0usize;
    let mut consumed_sequences = BTreeSet::new();
    for evidence in consumption {
        if evidence.trace_event != "astra.player.input.consumed"
            || !is_consumption_trace_source(platform, &evidence.source)
            || !evidence.trace_hash.starts_with("sha256:")
            || evidence.input_sequence == 0
            || evidence.player_sequence == 0
            || evidence.kind.trim().is_empty()
        {
            invalid += 1;
            continue;
        }
        consumed_sequences.insert(evidence.input_sequence);
    }
    for event in &live_inputs {
        if !consumed_sequences.contains(&event.sequence) {
            missing += 1;
        }
    }

    if !live_inputs.is_empty() && missing == 0 && invalid == 0 {
        pass_check(
            "player.input_consumption_trace",
            "player host trace proves live input was consumed",
            vec![
                evidence("live_input_count", live_inputs.len()),
                evidence("consumed_trace_count", consumption.len()),
            ],
        )
    } else {
        blocked_check_with_evidence(
            "player.input_consumption_trace",
            "player host trace does not prove live input consumption",
            "ASTRA_PLAYER_INPUT_CONSUMPTION_TRACE_MISSING",
            vec![
                evidence("live_input_count", live_inputs.len()),
                evidence("missing_consumption_count", missing),
                evidence("invalid_consumption_count", invalid),
            ],
        )
    }
}

pub(super) fn runtime_route_evidence_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    if transcript.runtime_routes.is_empty() {
        return blocked_check(
            "player.runtime_route_evidence",
            "route coverage has no Runtime/provider evidence",
            "ASTRA_PLAYER_RUNTIME_ROUTE_EVIDENCE_MISSING",
        );
    }
    let consumed = transcript
        .input_consumption
        .iter()
        .map(|item| {
            (
                item.input_sequence,
                item.player_sequence,
                item.trace_hash.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let mut actual_routes = BTreeSet::new();
    let mut terminal_routes = BTreeSet::new();
    let mut invalid = 0usize;
    let mut previous_step = 0_u64;
    let session_id = transcript
        .runtime_routes
        .first()
        .map(|route| route.session_id.as_str());
    for route in &transcript.runtime_routes {
        let hashes_valid = route.trace_hash.starts_with("sha256:");
        let identity_valid = !route.session_id.is_empty()
            && Some(route.session_id.as_str()) == session_id
            && route.input_sequence > 0
            && route.player_sequence > 0
            && route.fixed_step > previous_step
            && consumed.contains(&(
                route.input_sequence,
                route.player_sequence,
                route.trace_hash.as_str(),
            ));
        if !hashes_valid || !identity_valid {
            invalid += 1;
            continue;
        }
        previous_step = route.fixed_step;
        actual_routes.extend(route.coverage_reached.iter().cloned());
        terminal_routes.extend(route.terminal_route_ids.iter().cloned());
    }
    let declared_routes = transcript
        .route_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected_routes = script
        .expected_routes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if invalid == 0
        && !actual_routes.is_empty()
        && !terminal_routes.is_empty()
        && terminal_routes.is_subset(&actual_routes)
        && actual_routes == declared_routes
        && expected_routes.is_subset(&actual_routes)
    {
        pass_check(
            "player.runtime_route_evidence",
            "Runtime/provider traces prove route coverage",
            vec![
                evidence("runtime_step_count", transcript.runtime_routes.len()),
                evidence("runtime_route_count", actual_routes.len()),
                evidence("terminal_route_count", terminal_routes.len()),
            ],
        )
    } else {
        blocked_check_with_evidence(
            "player.runtime_route_evidence",
            "Runtime/provider traces do not match declared and expected routes",
            "ASTRA_PLAYER_RUNTIME_ROUTE_EVIDENCE_INVALID",
            vec![
                evidence("invalid_runtime_step_count", invalid),
                evidence("runtime_route_count", actual_routes.len()),
                evidence("terminal_route_count", terminal_routes.len()),
                evidence("declared_route_count", declared_routes.len()),
                evidence("expected_route_count", expected_routes.len()),
            ],
        )
    }
}

pub(super) fn transcript_coverage_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    let expected_steps = script.steps.len().max(1);
    if transcript.events.len() >= expected_steps
        && transcript
            .events
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    {
        pass_check(
            "player.input_transcript",
            "input transcript contains ordered live input events",
            vec![evidence("event_count", transcript.events.len())],
        )
    } else {
        blocked_check(
            "player.input_transcript",
            "input transcript is empty, incomplete or unordered",
            "ASTRA_PLAYER_TRANSCRIPT_INCOMPLETE",
        )
    }
}
