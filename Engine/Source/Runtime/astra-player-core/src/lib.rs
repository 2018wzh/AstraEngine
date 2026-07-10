use std::collections::BTreeSet;

use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationScript {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    pub scenario_ref: String,
    #[serde(default)]
    pub expected_routes: Vec<String>,
    #[serde(default)]
    pub steps: Vec<PlayerAutomationStep>,
}

impl PlayerAutomationScript {
    pub fn new(
        target: impl Into<String>,
        profile: impl Into<String>,
        platform: PlayerPlatform,
        package_hash: impl Into<String>,
        scenario_ref: impl Into<String>,
    ) -> Self {
        Self {
            schema: "astra.player_automation_script.v1".to_string(),
            target: target.into(),
            profile: profile.into(),
            platform,
            package_hash: package_hash.into(),
            scenario_ref: scenario_ref.into(),
            expected_routes: Vec::new(),
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationStep {
    pub id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_route_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerPlatform {
    Windows,
    Web,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInputTranscript {
    pub schema: String,
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    #[serde(default)]
    pub events: Vec<PlayerInputEvent>,
    #[serde(default)]
    pub input_consumption: Vec<PlayerInputConsumptionEvidence>,
    #[serde(default)]
    pub visual_regions: Vec<PlayerVisualRegionEvidence>,
    pub audio_meter: PlayerAudioMeterEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_comparison: Option<PlayerVisualComparisonEvidence>,
    #[serde(default)]
    pub route_coverage: Vec<String>,
}

impl PlayerInputTranscript {
    pub fn hash(&self) -> Hash256 {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        Hash256::from_sha256(&bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInputEvent {
    pub step_id: String,
    pub source: String,
    pub kind: String,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInputConsumptionEvidence {
    pub input_sequence: u64,
    pub player_sequence: u64,
    pub source: String,
    pub kind: String,
    pub trace_event: String,
    pub trace_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerVisualRegionEvidence {
    pub region_id: String,
    pub before_hash: String,
    pub after_hash: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAudioMeterEvidence {
    pub sample_count: u64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerVisualComparisonEvidence {
    pub report_hash: String,
    pub checkpoint_count: u32,
    pub status: PlayerAutomationStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationReport {
    pub schema: String,
    pub status: PlayerAutomationStatus,
    pub target: String,
    pub profile: String,
    pub platform: PlayerPlatform,
    pub package_hash: String,
    pub transcript_hash: String,
    #[serde(default)]
    pub route_coverage: Vec<String>,
    #[serde(default)]
    pub checks: Vec<PlayerAutomationCheck>,
}

impl PlayerAutomationReport {
    pub fn full_playable_passed(&self) -> bool {
        self.status == PlayerAutomationStatus::Pass
            && self.checks.iter().any(|check| {
                check.id == "player.full_playable" && check.status == PlayerAutomationStatus::Pass
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAutomationStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationCheck {
    pub id: String,
    pub status: PlayerAutomationStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Diagnostic>,
    #[serde(default)]
    pub evidence: Vec<PlayerAutomationEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerAutomationEvidence {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerPlatformEvidenceIdentity {
    pub profile_hash: String,
    pub build_fingerprint: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerAutomationValidator;

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

    fn validate_internal(
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
        let transcript_hash = transcript.hash().to_string();
        let mut checks = vec![
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
            audio_meter_check(&transcript.audio_meter),
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

fn platform_identity_check(identity: &PlayerPlatformEvidenceIdentity) -> PlayerAutomationCheck {
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

fn schema_check(
    script: &PlayerAutomationScript,
    transcript: &PlayerInputTranscript,
) -> PlayerAutomationCheck {
    if script.schema == "astra.player_automation_script.v1"
        && transcript.schema == "astra.player_input_transcript.v1"
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

fn identity_check(
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

fn live_input_surface_check(
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
        PlayerPlatform::Web => matches!(event.source.as_str(), "cdp.mouse" | "cdp.keyboard"),
    });
    let all_allowed = events.iter().all(|event| match platform {
        PlayerPlatform::Windows => matches!(
            event.source.as_str(),
            "window.focus" | "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Web => matches!(
            event.source.as_str(),
            "browser.focus" | "cdp.session" | "cdp.mouse" | "cdp.keyboard"
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

fn input_consumption_trace_check(
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

fn transcript_coverage_check(
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

fn visual_region_check(regions: &[PlayerVisualRegionEvidence]) -> PlayerAutomationCheck {
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

fn audio_meter_check(meter: &PlayerAudioMeterEvidence) -> PlayerAutomationCheck {
    if meter.sample_count > 0 && meter.peak_dbfs > -80.0 && meter.rms_dbfs.is_finite() {
        pass_check(
            "player.audio_meter",
            "audio meter recorded non-silent output",
            vec![
                evidence("sample_count", meter.sample_count),
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

fn visual_comparison_check(
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

fn route_coverage_check(
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

fn expected_routes(script: &PlayerAutomationScript) -> BTreeSet<String> {
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

fn is_live_input_source(platform: PlayerPlatform, source: &str) -> bool {
    match platform {
        PlayerPlatform::Windows => matches!(
            source,
            "sendinput.mouse" | "sendinput.keyboard" | "sendinput.touch"
        ),
        PlayerPlatform::Web => matches!(source, "cdp.mouse" | "cdp.keyboard"),
    }
}

fn is_consumption_trace_source(platform: PlayerPlatform, source: &str) -> bool {
    match platform {
        PlayerPlatform::Windows => source == "player_host.trace",
        PlayerPlatform::Web => matches!(source, "player_host.trace" | "browser_host.trace"),
    }
}

fn is_forbidden_input_source(source: &str) -> bool {
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

fn is_safe_relative_ref(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains("://")
        && !value.contains('\\')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(':'))
}

fn pass_check(
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

fn blocked_check(
    id: impl Into<String>,
    summary: impl Into<String>,
    code: impl Into<String>,
) -> PlayerAutomationCheck {
    blocked_check_with_evidence(id, summary, code, Vec::new())
}

fn blocked_check_with_evidence(
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

fn evidence(key: impl Into<String>, value: impl ToString) -> PlayerAutomationEvidence {
    PlayerAutomationEvidence {
        key: key.into(),
        value: value.to_string(),
    }
}
