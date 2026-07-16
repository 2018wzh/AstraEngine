use astra_core::Hash256;
pub use astra_player_core::{
    PlatformCommandSink, PlayerAudioMeterEvidence, PlayerAutomationReport, PlayerAutomationScript,
    PlayerAutomationStatus, PlayerAutomationStep, PlayerAutomationValidator,
    PlayerHostCommandExecutor, PlayerHostCommandResult, PlayerHostResourceId,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerInputTranscript, PlayerPlatform,
    PlayerPlatformEvidenceIdentity, PlayerRuntimeRouteEvidence, PlayerVisualComparisonEvidence,
    PlayerVisualRegionEvidence,
};
use std::{fs, path::PathBuf};

mod web_cdp;
pub use web_cdp::*;

mod native_session;
pub use native_session::*;

pub use astra_player_vn::*;

pub const WINDOWS_SENDINPUT_MOUSE: &str = "sendinput.mouse";
pub const WINDOWS_SENDINPUT_KEYBOARD: &str = "sendinput.keyboard";
pub const LINUX_UINPUT_MOUSE: &str = "uinput.mouse";
pub const LINUX_UINPUT_KEYBOARD: &str = "uinput.keyboard";
pub const LINUX_UINPUT_TOUCH: &str = "uinput.touch";
pub const LINUX_UINPUT_GAMEPAD: &str = "uinput.gamepad";
pub const MACOS_CGEVENT_MOUSE: &str = "cgevent.mouse";
pub const MACOS_CGEVENT_KEYBOARD: &str = "cgevent.keyboard";
pub const WEB_CDP_MOUSE: &str = "cdp.mouse";
pub const WEB_CDP_KEYBOARD: &str = "cdp.keyboard";
pub const ANDROID_TOUCH: &str = "android.touch";
pub const ANDROID_KEYBOARD: &str = "android.keyboard";
pub const ANDROID_GAMEPAD: &str = "android.gamepad";
pub const ANDROID_ACCESSIBILITY: &str = "android.accessibility";

pub type PlayerAutomationError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScenarioInputAction {
    Advance,
    Choose { option_id: String },
    OpenSystem { page: String },
    Back,
}

impl ScenarioInputAction {
    fn kind(&self) -> &'static str {
        match self {
            Self::Advance => "advance",
            Self::Choose { .. } => "choose",
            Self::OpenSystem { .. } => "open_system",
            Self::Back => "back",
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct PlayerScenarioDocument {
    schema: String,
    #[serde(default)]
    actions: Vec<PlayerScenarioAction>,
}

#[derive(Debug, serde::Deserialize)]
struct PlayerScenarioAction {
    #[serde(default)]
    launch: Option<serde_yaml::Value>,
    #[serde(default)]
    player_input: Option<PlayerScenarioInput>,
    #[serde(default, flatten)]
    unsupported: std::collections::BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, serde::Deserialize)]
struct PlayerScenarioInput {
    kind: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default = "one_tick")]
    ticks: u64,
}

fn one_tick() -> u64 {
    1
}

fn parse_scenario_input_plan(bytes: &[u8]) -> Result<Vec<ScenarioInputAction>, String> {
    let scenario: PlayerScenarioDocument = serde_yaml::from_slice(bytes)
        .map_err(|error| format!("ASTRA_PLAYER_SCENARIO_INVALID: {error}"))?;
    if scenario.schema != "astra.scenario.v1" {
        return Err(format!(
            "ASTRA_PLAYER_SCENARIO_VERSION_UNSUPPORTED: {}",
            scenario.schema
        ));
    }
    let mut plan = Vec::new();
    for (index, action) in scenario.actions.into_iter().enumerate() {
        if !action.unsupported.is_empty() {
            return Err(format!(
                "ASTRA_PLAYER_SCENARIO_ACTION_UNSUPPORTED: action {index} contains {}",
                action
                    .unsupported
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if action.launch.is_some() {
            if action.player_input.is_some() {
                return Err(format!(
                    "ASTRA_PLAYER_SCENARIO_ACTION_AMBIGUOUS: action {index} declares launch and player_input"
                ));
            }
            continue;
        }
        let input = action.player_input.ok_or_else(|| {
            format!("ASTRA_PLAYER_SCENARIO_ACTION_EMPTY: action {index} has no supported action")
        })?;
        match input.kind.as_str() {
            "advance" => {
                if input.ticks == 0 {
                    return Err(format!(
                        "ASTRA_PLAYER_SCENARIO_TICKS_INVALID: action {index} has zero ticks"
                    ));
                }
                plan.extend((0..input.ticks).map(|_| ScenarioInputAction::Advance));
            }
            "choose" => plan.push(ScenarioInputAction::Choose {
                option_id: input.value.ok_or_else(|| {
                    format!("ASTRA_PLAYER_SCENARIO_VALUE_REQUIRED: choose action {index}")
                })?,
            }),
            "open_system" => plan.push(ScenarioInputAction::OpenSystem {
                page: input.value.ok_or_else(|| {
                    format!("ASTRA_PLAYER_SCENARIO_VALUE_REQUIRED: open_system action {index}")
                })?,
            }),
            "back" => plan.push(ScenarioInputAction::Back),
            "complete_wait" => {
                return Err(format!(
                    "ASTRA_PLAYER_AUTOMATION_MEDIA_COMPLETION_REQUIRED: action {index} must be completed by the live media provider"
                ));
            }
            other => {
                return Err(format!(
                    "ASTRA_PLAYER_SCENARIO_INPUT_UNSUPPORTED: action {index} uses {other}"
                ));
            }
        }
    }
    if plan.is_empty() {
        return Err("ASTRA_PLAYER_SCENARIO_INPUT_EMPTY: scenario has no host input actions".into());
    }
    Ok(plan)
}

#[derive(Debug, Clone)]
pub struct WindowsLiveAutomationRequest {
    pub bundle_dir: PathBuf,
    pub visual_comparison_report: PathBuf,
    pub host_conformance_report: PathBuf,
    pub timeout_ms: u64,
    pub trace_log: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WindowsLiveAutomationRun {
    pub script: PlayerAutomationScript,
    pub transcript: PlayerInputTranscript,
    pub report: PlayerAutomationReport,
}

#[derive(Debug, Clone, Default)]
pub struct WindowsSendInputHost;

impl WindowsSendInputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Windows
    }

    pub fn run_live_bundle(
        &self,
        request: WindowsLiveAutomationRequest,
    ) -> Result<WindowsLiveAutomationRun, PlayerAutomationError> {
        let bundle = BundleContext::read(request.bundle_dir)?;
        if bundle.platform != "windows" {
            return Err("windows live automation requires a windows bundle".into());
        }
        let package_hash = sha256_file(&bundle.package_path)?;
        if package_hash != bundle.package_hash {
            return Err("bundle package hash does not match bundle manifest".into());
        }

        let scenario = bundle.automation_scenario()?;
        let host_inputs = bundle.resolve_host_inputs(&scenario.inputs)?;
        let expected_routes = vec![scenario.route_id.clone()];
        let mut script = PlayerAutomationScript::new(
            &bundle.target,
            &bundle.profile,
            PlayerPlatform::Windows,
            package_hash.clone(),
            scenario.scenario_ref.clone(),
        );
        script.expected_routes = expected_routes.clone();
        script.steps = vec![PlayerAutomationStep {
            id: "focus.window".to_string(),
            action: "focus_window".to_string(),
            expected_route_id: None,
        }];
        script
            .steps
            .extend(scenario.inputs.iter().enumerate().map(|(index, input)| {
                PlayerAutomationStep {
                    id: format!("input.{}.{}", input.kind(), index + 1),
                    action: format!("sendinput.{}", input.kind()),
                    expected_route_id: (index + 1 == scenario.inputs.len())
                        .then(|| scenario.route_id.clone()),
                }
            }));

        let live = windows_live_input(
            &bundle,
            request.timeout_ms,
            request.trace_log.as_ref(),
            &host_inputs,
        )?;
        let (audio_meter, platform_identity) =
            host_audio_meter(&request.host_conformance_report, &package_hash)?;
        let comparison = visual_comparison_evidence(&request.visual_comparison_report)?;
        let transcript = PlayerInputTranscript {
            schema: "astra.player_input_transcript.v2".to_string(),
            target: bundle.target,
            profile: bundle.profile,
            platform: PlayerPlatform::Windows,
            package_hash,
            events: live.events,
            input_consumption: live.input_consumption,
            visual_regions: live.visual_regions,
            audio_meter,
            visual_comparison: Some(comparison),
            runtime_routes: live.runtime_routes,
            route_coverage: live.route_coverage,
        };
        let report = PlayerAutomationValidator.validate_with_platform_identity(
            &script,
            &transcript,
            &platform_identity,
        );
        Ok(WindowsLiveAutomationRun {
            script,
            transcript,
            report,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct LinuxUinputHost;

impl LinuxUinputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Linux
    }
}

#[derive(Debug, Clone, Default)]
pub struct MacosCgEventHost;

impl MacosCgEventHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Macos
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebCdpInputHost;

#[derive(Debug, Clone, Default)]
pub struct AndroidInputHost;

impl AndroidInputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Android
    }
}

#[derive(Debug, Clone)]
pub struct WebLiveAutomationRequest {
    pub bundle_dir: PathBuf,
    pub browser_executable: PathBuf,
    pub visual_comparison_report: PathBuf,
    pub host_conformance_report: PathBuf,
    pub headless: bool,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct WebLiveAutomationRun {
    pub script: PlayerAutomationScript,
    pub transcript: PlayerInputTranscript,
    pub report: PlayerAutomationReport,
}

impl WebCdpInputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Web
    }

    pub fn run_live_bundle(
        &self,
        request: WebLiveAutomationRequest,
    ) -> Result<WebLiveAutomationRun, PlayerAutomationError> {
        let bundle = BundleContext::read(request.bundle_dir)?;
        if bundle.platform != "web" {
            return Err("ASTRA_PLAYER_WEB_BUNDLE_REQUIRED".into());
        }
        let package_hash = sha256_file(&bundle.package_path)?;
        if package_hash != bundle.package_hash {
            return Err("ASTRA_PLAYER_BUNDLE_PACKAGE_HASH_MISMATCH".into());
        }
        let scenario = bundle.automation_scenario()?;
        let mut script = PlayerAutomationScript::new(
            &bundle.target,
            &bundle.profile,
            PlayerPlatform::Web,
            package_hash.clone(),
            scenario.scenario_ref.clone(),
        );
        script.expected_routes = vec![scenario.route_id.clone()];
        script.steps = scenario
            .inputs
            .iter()
            .enumerate()
            .map(|(index, input)| PlayerAutomationStep {
                id: format!("input.{}.{}", input.kind(), index + 1),
                action: format!("cdp.{}", input.kind()),
                expected_route_id: (index + 1 == scenario.inputs.len())
                    .then(|| scenario.route_id.clone()),
            })
            .collect();
        let timeout = std::time::Duration::from_millis(request.timeout_ms.max(1));
        let live = run_web_live_input(
            &bundle,
            &scenario.inputs,
            request.browser_executable,
            request.headless,
            timeout,
        )?;
        let (audio_meter, platform_identity) =
            host_audio_meter(&request.host_conformance_report, &package_hash)?;
        validate_web_same_run_audio(&live.final_evidence, &audio_meter, &platform_identity)?;
        let comparison = visual_comparison_evidence(&request.visual_comparison_report)?;
        let transcript = PlayerInputTranscript {
            schema: "astra.player_input_transcript.v2".to_string(),
            target: bundle.target,
            profile: bundle.profile,
            platform: PlayerPlatform::Web,
            package_hash,
            events: live.live.events,
            input_consumption: live.live.input_consumption,
            visual_regions: live.live.visual_regions,
            audio_meter,
            visual_comparison: Some(comparison),
            runtime_routes: live.live.runtime_routes,
            route_coverage: live.live.route_coverage,
        };
        let report = PlayerAutomationValidator.validate_with_platform_identity(
            &script,
            &transcript,
            &platform_identity,
        );
        Ok(WebLiveAutomationRun {
            script,
            transcript,
            report,
        })
    }
}

fn validate_web_same_run_audio(
    evidence: &WebCdpRuntimeEvidence,
    meter: &PlayerAudioMeterEvidence,
    identity: &PlayerPlatformEvidenceIdentity,
) -> Result<(), PlayerAutomationError> {
    if evidence.session_id.as_deref() != Some(identity.session_id.as_str()) {
        return Err("ASTRA_PLAYER_WEB_SESSION_IDENTITY_MISMATCH".into());
    }
    let value = evidence
        .audio_meter
        .as_ref()
        .ok_or("ASTRA_PLAYER_WEB_AUDIO_EVIDENCE_MISSING")?;
    let u64_field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| -> PlayerAutomationError {
                format!("ASTRA_PLAYER_WEB_AUDIO_FIELD_MISSING: {key}").into()
            })
    };
    let callback_count = u64_field("callback_count")?;
    let consumed_samples = u64_field("consumed_samples")?;
    let peak_bits = u64_field("peak_dbfs_bits")?;
    let rms_bits = u64_field("rms_dbfs_bits")?;
    let peak_bits = u32::try_from(peak_bits).map_err(|_| "ASTRA_PLAYER_WEB_AUDIO_PEAK_RANGE")?;
    let rms_bits = u32::try_from(rms_bits).map_err(|_| "ASTRA_PLAYER_WEB_AUDIO_RMS_RANGE")?;
    if callback_count != meter.callback_count
        || consumed_samples != meter.sample_count
        || f32::from_bits(peak_bits).to_bits() != meter.peak_dbfs.to_bits()
        || f32::from_bits(rms_bits).to_bits() != meter.rms_dbfs.to_bits()
    {
        return Err("ASTRA_PLAYER_WEB_AUDIO_REPORT_MISMATCH".into());
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct BundleContext {
    bundle_dir: PathBuf,
    #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
    entrypoint: String,
    target: String,
    profile: String,
    platform: String,
    package_hash: String,
    package_path: PathBuf,
    scenario_refs: Vec<String>,
}

#[derive(Debug, Clone)]
struct BundleAutomationScenario {
    scenario_ref: String,
    route_id: String,
    inputs: Vec<ScenarioInputAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScenarioHostInput {
    kind: &'static str,
    virtual_key: u16,
}

impl BundleContext {
    fn read(bundle_dir: PathBuf) -> Result<Self, PlayerAutomationError> {
        let manifest_path = bundle_dir.join("bundle_manifest.json");
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
        let entrypoint = required_string(&manifest, "entrypoint")?;
        let target = required_string(&manifest, "target")?;
        let profile = required_string(&manifest, "profile")?;
        let platform = required_string(&manifest, "platform")?;
        let package_rel = required_string(&manifest, "package")?;
        if !is_safe_relative_ref(&entrypoint) || !is_safe_relative_ref(&package_rel) {
            return Err("bundle manifest contains an unsafe relative path".into());
        }
        let scenario_refs = manifest
            .get("scenario_refs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter(|value| is_safe_relative_ref(value))
            .map(str::to_string)
            .collect::<Vec<_>>();
        Ok(Self {
            bundle_dir: bundle_dir.clone(),
            #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
            entrypoint,
            target,
            profile,
            platform,
            package_hash: required_string(&manifest, "package_hash")?,
            package_path: bundle_dir.join(package_rel),
            scenario_refs,
        })
    }

    fn automation_scenario(&self) -> Result<BundleAutomationScenario, PlayerAutomationError> {
        let refs = self
            .scenario_refs
            .iter()
            .filter(|scenario_ref| {
                let platform_marker = format!(".{}.", self.platform);
                scenario_ref.contains(&platform_marker) || self.scenario_refs.len() == 1
            })
            .collect::<Vec<_>>();
        if refs.len() != 1 {
            return Err(format!(
                "ASTRA_PLAYER_AUTOMATION_SCENARIO_COUNT: expected exactly one {} scenario, found {}",
                self.platform,
                refs.len()
            )
            .into());
        }
        let scenario_ref = refs[0];
        let bytes = fs::read(self.bundle_dir.join(scenario_ref))?;
        let value: serde_yaml::Value = serde_yaml::from_slice(&bytes)?;
        let route_id = value
            .get("generated_route_id")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("ASTRA_PLAYER_AUTOMATION_ROUTE_MISSING: scenario has no generated_route_id")?
            .to_string();
        Ok(BundleAutomationScenario {
            scenario_ref: scenario_ref.clone(),
            route_id,
            inputs: parse_scenario_input_plan(&bytes)
                .map_err(|error| -> PlayerAutomationError { error.into() })?,
        })
    }

    fn resolve_host_inputs(
        &self,
        inputs: &[ScenarioInputAction],
    ) -> Result<Vec<ScenarioHostInput>, PlayerAutomationError> {
        let package_bytes = fs::read(&self.package_path)?;
        let package = astra_package::PackageReader::open(&package_bytes)?;
        let compiled = astra_vn_package::decode_compiled_project(&package)?;
        let mut option_indexes = std::collections::BTreeMap::new();
        for state in compiled.states.values() {
            for scene in &state.scenes {
                for command in &scene.commands {
                    if let astra_vn_core::CompiledCommand::Choice { options, .. } = command {
                        for (index, option) in options.iter().enumerate() {
                            if option_indexes.insert(option.id.clone(), index).is_some() {
                                return Err(format!(
                                    "ASTRA_PLAYER_CHOICE_ID_DUPLICATE: {}",
                                    option.id
                                )
                                .into());
                            }
                        }
                    }
                }
            }
        }
        inputs
            .iter()
            .map(|input| {
                let virtual_key = match input {
                    ScenarioInputAction::Advance => 0x20,
                    ScenarioInputAction::Choose { option_id } => {
                        let index = option_indexes.get(option_id).ok_or_else(|| {
                            format!("ASTRA_PLAYER_CHOICE_ID_UNKNOWN: {option_id}")
                        })?;
                        if *index >= 9 {
                            return Err(format!(
                                "ASTRA_PLAYER_CHOICE_KEY_UNAVAILABLE: {option_id} uses index {index}"
                            )
                            .into());
                        }
                        0x31 + *index as u16
                    }
                    ScenarioInputAction::OpenSystem { page } if page == "backlog" => 0x42,
                    ScenarioInputAction::OpenSystem { page } => {
                        return Err(format!(
                            "ASTRA_PLAYER_SYSTEM_KEY_UNAVAILABLE: no physical binding for {page}"
                        )
                        .into());
                    }
                    ScenarioInputAction::Back => 0x1B,
                };
                Ok(ScenarioHostInput {
                    kind: input.kind(),
                    virtual_key,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct LiveInputRun {
    events: Vec<PlayerInputEvent>,
    input_consumption: Vec<PlayerInputConsumptionEvidence>,
    visual_regions: Vec<PlayerVisualRegionEvidence>,
    runtime_routes: Vec<PlayerRuntimeRouteEvidence>,
    route_coverage: Vec<String>,
}

fn windows_live_input(
    bundle: &BundleContext,
    timeout_ms: u64,
    trace_log: Option<&PathBuf>,
    inputs: &[ScenarioHostInput],
) -> Result<LiveInputRun, PlayerAutomationError> {
    #[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
    {
        windows_live_input_impl(bundle, timeout_ms, trace_log, inputs)
    }
    #[cfg(not(all(target_os = "windows", feature = "platform-test-driver")))]
    {
        let _ = (bundle, timeout_ms, trace_log, inputs);
        Err("windows live automation requires Windows".into())
    }
}

fn host_audio_meter(
    report_path: &PathBuf,
    package_hash: &str,
) -> Result<(PlayerAudioMeterEvidence, PlayerPlatformEvidenceIdentity), PlayerAutomationError> {
    let report: astra_platform::PlatformHostConformanceReport =
        serde_json::from_slice(&fs::read(report_path)?)?;
    if report.schema != astra_platform::PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA
        || report.status != astra_platform::ConformanceStatus::Pass
        || report.package_hash != package_hash
    {
        return Err("host conformance report does not match the bundle package".into());
    }
    let check = report
        .checks
        .iter()
        .find(|check| {
            check.id == "audio.output_meter"
                && check.status == astra_platform::ConformanceStatus::Pass
        })
        .ok_or("host conformance report is missing passing audio.output_meter evidence")?;
    let value = |key: &str| {
        check
            .evidence
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
            .ok_or_else(|| format!("audio.output_meter is missing {key} evidence"))
    };
    Ok((
        PlayerAudioMeterEvidence {
            provider: value("provider")?.to_string(),
            callback_count: value("callback_count")?.parse()?,
            host_report_hash: sha256_file(report_path)?,
            sample_count: value("sample_count")?.parse()?,
            peak_dbfs: value("peak_dbfs")?.parse()?,
            rms_dbfs: value("rms_dbfs")?.parse()?,
        },
        PlayerPlatformEvidenceIdentity {
            profile_hash: report.profile_hash,
            build_fingerprint: report.build_fingerprint,
            session_id: report.session_id,
        },
    ))
}

fn visual_comparison_evidence(
    path: &PathBuf,
) -> Result<PlayerVisualComparisonEvidence, PlayerAutomationError> {
    let bytes = fs::read(path)?;
    let report: serde_json::Value = serde_json::from_slice(&bytes)?;
    let checkpoint_count = report
        .get("checkpoints")
        .and_then(serde_json::Value::as_array)
        .map(|items| items.len() as u32)
        .unwrap_or_default();
    let status = if report.get("schema").and_then(serde_json::Value::as_str)
        == Some("tsuinosora.visual_comparison_report.v1")
        && report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && checkpoint_count > 0
    {
        PlayerAutomationStatus::Pass
    } else {
        PlayerAutomationStatus::Blocked
    };
    Ok(PlayerVisualComparisonEvidence {
        report_hash: Hash256::from_sha256(&bytes).to_string(),
        checkpoint_count,
        status,
    })
}

fn required_string(value: &serde_json::Value, key: &str) -> Result<String, PlayerAutomationError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("bundle manifest missing string field: {key}").into())
}

fn sha256_file(path: &PathBuf) -> Result<String, PlayerAutomationError> {
    Ok(Hash256::from_sha256(&fs::read(path)?).to_string())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Hash256::from_sha256(bytes).to_string()
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

#[cfg(any(test, all(target_os = "windows", feature = "platform-test-driver")))]
#[derive(Debug, Clone)]
struct ParsedPlayerHostTrace {
    player_sequence: u64,
    fixed_step: u64,
    kind: String,
    trace_hash: String,
    coverage_reached: Vec<String>,
    runtime_state_hash: String,
    runtime_event_hash: String,
    runtime_presentation_hash: String,
    current_state_id: Option<String>,
    pending_choice_ids: Vec<String>,
    terminal_route_ids: Vec<String>,
}

#[cfg(any(test, all(target_os = "windows", feature = "platform-test-driver")))]
fn parse_player_host_traces(stderr: &str) -> Vec<ParsedPlayerHostTrace> {
    let mut consumed = std::collections::BTreeMap::new();
    let mut runtime = std::collections::BTreeMap::new();
    for line in stderr.lines() {
        if line.contains("event=astra.player.input.consumed") {
            if let (Some(sequence), Some(kind)) = (
                trace_token(line, "player_sequence").and_then(|value| value.parse().ok()),
                trace_token(line, "kind"),
            ) {
                consumed.insert(sequence, (kind.to_string(), line));
            }
        } else if line.contains("event=astra.player.vn.step") {
            let Some(sequence) =
                trace_token(line, "player_sequence").and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            let coverage = trace_token(line, "coverage")
                .filter(|value| *value != "-")
                .map(|value| {
                    value
                        .split(',')
                        .filter(|item| !item.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let Some(fixed_step) =
                trace_token(line, "fixed_step").and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            let Some(state_hash) = trace_token(line, "runtime_state_hash") else {
                continue;
            };
            let Some(event_hash) = trace_token(line, "runtime_event_hash") else {
                continue;
            };
            let Some(presentation_hash) = trace_token(line, "runtime_presentation_hash") else {
                continue;
            };
            runtime.insert(
                sequence,
                (
                    coverage,
                    fixed_step,
                    state_hash.to_string(),
                    event_hash.to_string(),
                    presentation_hash.to_string(),
                    trace_token(line, "current_state_id")
                        .filter(|value| *value != "-")
                        .map(str::to_string),
                    trace_token(line, "pending_choice_ids")
                        .filter(|value| *value != "-")
                        .map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
                        .unwrap_or_default(),
                    trace_token(line, "terminal_route_ids")
                        .filter(|value| *value != "-")
                        .map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
                        .unwrap_or_default(),
                    line,
                ),
            );
        }
    }
    consumed
        .into_iter()
        .filter_map(|(player_sequence, (kind, consumed_line))| {
            let (
                coverage_reached,
                fixed_step,
                runtime_state_hash,
                runtime_event_hash,
                runtime_presentation_hash,
                current_state_id,
                pending_choice_ids,
                terminal_route_ids,
                runtime_line,
            ) = runtime.remove(&player_sequence)?;
            Some(ParsedPlayerHostTrace {
                player_sequence,
                fixed_step,
                kind,
                trace_hash: Hash256::from_sha256(
                    format!("{consumed_line}\n{runtime_line}").as_bytes(),
                )
                .to_string(),
                coverage_reached,
                runtime_state_hash,
                runtime_event_hash,
                runtime_presentation_hash,
                current_state_id,
                pending_choice_ids,
                terminal_route_ids,
            })
        })
        .collect()
}

#[cfg(any(test, all(target_os = "windows", feature = "platform-test-driver")))]
fn trace_token<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&prefix))
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
fn windows_live_input_impl(
    bundle: &BundleContext,
    timeout_ms: u64,
    trace_log: Option<&PathBuf>,
    inputs: &[ScenarioHostInput],
) -> Result<LiveInputRun, PlayerAutomationError> {
    windows_live::run(bundle, timeout_ms, trace_log, inputs)
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
mod windows_live {
    use super::{
        parse_player_host_traces, sha256_bytes, BundleContext, LiveInputRun, ParsedPlayerHostTrace,
        PlayerAutomationError, PlayerInputConsumptionEvidence, PlayerInputEvent,
        PlayerRuntimeRouteEvidence, PlayerVisualRegionEvidence, ScenarioHostInput,
        WINDOWS_SENDINPUT_KEYBOARD, WINDOWS_SENDINPUT_MOUSE,
    };
    use astra_platform_windows::WindowsTestDriver;
    use std::{
        fs,
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::Duration,
    };

    pub fn run(
        bundle: &BundleContext,
        timeout_ms: u64,
        trace_log: Option<&PathBuf>,
        inputs: &[ScenarioHostInput],
    ) -> Result<LiveInputRun, PlayerAutomationError> {
        let mut trace_lines = Vec::new();
        trace_line(
            &mut trace_lines,
            "level=TRACE event=astra.player.automation.start platform=windows".to_string(),
        );
        let entrypoint = bundle.bundle_dir.join(&bundle.entrypoint);
        let mut child = Command::new(entrypoint)
            .current_dir(&bundle.bundle_dir)
            .env("ASTRA_PLAYER_TRACE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        trace_line(
            &mut trace_lines,
            "level=TRACE event=astra.player.bundle.launch role=bundle_entrypoint".to_string(),
        );
        let result = (|| {
            let window = WindowsTestDriver::wait_for_process_window(
                child.id(),
                Duration::from_millis(timeout_ms.max(1)),
            )?;
            trace_line(
                &mut trace_lines,
                "level=TRACE event=astra.player.window.found title=astra_player".to_string(),
            );
            window.focus()?;
            trace_line(
                &mut trace_lines,
                "level=TRACE event=astra.player.window.focus requested=true".to_string(),
            );
            thread::sleep(Duration::from_millis(150));
            let before = window.capture_rgba()?;
            let before_hash = sha256_bytes(&before.rgba8);
            trace_line(
                &mut trace_lines,
                format!(
                    "level=TRACE event=astra.player.capture.before width={} height={} hash={}",
                    before.width, before.height, before_hash
                ),
            );
            let mut events = vec![PlayerInputEvent {
                step_id: "focus.window".to_string(),
                source: "window.focus".to_string(),
                kind: "focus".to_string(),
                sequence: 1,
                route_id: None,
            }];
            let mut visual_regions = Vec::new();
            let mut previous = before;
            for (index, input) in inputs.iter().enumerate() {
                let input_sequence = (index + 2) as u64;
                trace_line(
                    &mut trace_lines,
                    format!(
                        "level=TRACE event=astra.player.input.sent source=sendinput.keyboard kind=key input_sequence={input_sequence}"
                    ),
                );
                window.send_key(input.virtual_key)?;
                thread::sleep(Duration::from_millis(160));
                let after = window.capture_rgba()?;
                let previous_hash = sha256_bytes(&previous.rgba8);
                let after_hash = sha256_bytes(&after.rgba8);
                let changed = previous_hash != after_hash;
                trace_line(
                    &mut trace_lines,
                    format!(
                        "level=TRACE event=astra.player.capture.after input_sequence={input_sequence} width={} height={} hash={} changed={}",
                        after.width, after.height, after_hash, changed
                    ),
                );
                events.push(PlayerInputEvent {
                    step_id: format!("input.{}.{}", input.kind, index + 1),
                    source: WINDOWS_SENDINPUT_KEYBOARD.to_string(),
                    kind: "key".to_string(),
                    sequence: input_sequence,
                    route_id: None,
                });
                visual_regions.push(PlayerVisualRegionEvidence {
                    region_id: format!("client_full_frame.input.{}", index + 1),
                    before_hash: previous_hash,
                    after_hash,
                    width: previous.width.min(after.width),
                    height: previous.height.min(after.height),
                });
                previous = after;
            }
            Ok(LiveInputRun {
                events,
                input_consumption: Vec::new(),
                visual_regions,
                runtime_routes: Vec::new(),
                route_coverage: Vec::new(),
            })
        })();
        let _ = child.kill();
        let output = child.wait_with_output()?;
        let host_stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let host_traces = parse_player_host_traces(&host_stderr);
        trace_line(
            &mut trace_lines,
            format!(
                "level=TRACE event=astra.player.host.trace_captured consumed_count={} trace_hash={}",
                host_traces.len(),
                sha256_bytes(host_stderr.as_bytes())
            ),
        );
        let mut result = result;
        if let Ok(run) = result.as_mut() {
            correlate_host_traces(run, &host_traces);
            trace_line(
                &mut trace_lines,
                format!(
                    "level=TRACE event=astra.player.input.correlation consumed_input_count={} route_coverage_count={}",
                    run.input_consumption.len(),
                    run.route_coverage.len()
                ),
            );
        }
        if let Some(path) = trace_log {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut combined = trace_lines.join("\n");
            if !host_stderr.trim().is_empty() {
                combined.push('\n');
                combined.push_str(host_stderr.trim_end());
            }
            combined.push('\n');
            fs::write(path, combined)?;
        }
        result
    }

    fn trace_line(lines: &mut Vec<String>, line: String) {
        eprintln!("{line}");
        lines.push(line);
    }

    fn correlate_host_traces(run: &mut LiveInputRun, traces: &[ParsedPlayerHostTrace]) {
        let input_events = run
            .events
            .iter_mut()
            .filter(|event| {
                matches!(
                    event.source.as_str(),
                    WINDOWS_SENDINPUT_KEYBOARD | WINDOWS_SENDINPUT_MOUSE
                )
            })
            .zip(traces.iter());
        let mut route_coverage = std::collections::BTreeSet::new();
        let mut consumption = Vec::new();
        let mut runtime_routes = Vec::new();
        for (event, trace) in input_events {
            if !trace.runtime_state_hash.starts_with("hash128:")
                || !trace.runtime_event_hash.starts_with("hash128:")
                || !trace.runtime_presentation_hash.starts_with("hash128:")
            {
                continue;
            }
            let route_id = trace
                .current_state_id
                .clone()
                .or_else(|| trace.coverage_reached.last().cloned());
            event.route_id = route_id.clone();
            route_coverage.extend(trace.coverage_reached.iter().cloned());
            consumption.push(PlayerInputConsumptionEvidence {
                input_sequence: event.sequence,
                player_sequence: trace.player_sequence,
                source: "player_host.trace".to_string(),
                kind: trace.kind.clone(),
                trace_event: "astra.player.input.consumed".to_string(),
                trace_hash: trace.trace_hash.clone(),
                route_id,
            });
            runtime_routes.push(PlayerRuntimeRouteEvidence {
                input_sequence: event.sequence,
                player_sequence: trace.player_sequence,
                fixed_step: trace.fixed_step,
                coverage_reached: trace.coverage_reached.clone(),
                current_state_id: trace.current_state_id.clone(),
                pending_choice_ids: trace.pending_choice_ids.clone(),
                terminal_route_ids: trace.terminal_route_ids.clone(),
                runtime_state_hash: trace.runtime_state_hash.clone(),
                runtime_event_hash: trace.runtime_event_hash.clone(),
                runtime_presentation_hash: trace.runtime_presentation_hash.clone(),
                trace_hash: trace.trace_hash.clone(),
            });
        }
        run.input_consumption = consumption;
        run.runtime_routes = runtime_routes;
        run.route_coverage = route_coverage.into_iter().collect();
    }
}

#[cfg(test)]
mod host_trace_tests {
    use super::{parse_player_host_traces, parse_scenario_input_plan, ScenarioInputAction};

    #[astra_headless_test::test]
    fn scenario_input_plan_uses_declared_actions_instead_of_expected_route_count() {
        let yaml = br#"
schema: astra.scenario.v1
generated_route_id: route.library
actions:
  - launch: {}
  - player_input: { kind: advance, ticks: 2 }
  - player_input: { kind: choose, value: choice.library }
  - player_input: { kind: open_system, value: backlog }
"#;

        let plan = parse_scenario_input_plan(yaml).unwrap();

        assert_eq!(
            plan,
            vec![
                ScenarioInputAction::Advance,
                ScenarioInputAction::Advance,
                ScenarioInputAction::Choose {
                    option_id: "choice.library".to_string()
                },
                ScenarioInputAction::OpenSystem {
                    page: "backlog".to_string()
                },
            ]
        );
    }

    #[astra_headless_test::test]
    fn scenario_input_plan_blocks_media_completion_bypass() {
        let yaml = br#"
schema: astra.scenario.v1
actions:
  - player_input: { kind: complete_wait, value: voice.end }
"#;

        let error = parse_scenario_input_plan(yaml).unwrap_err();

        assert!(error.contains("ASTRA_PLAYER_AUTOMATION_MEDIA_COMPLETION_REQUIRED"));
    }

    #[astra_headless_test::test]
    fn route_coverage_is_parsed_from_runtime_evidence_not_expected_labels() {
        let stderr = concat!(
            "event=astra.player.input.consumed player_sequence=17 kind=keyboard\n",
            "event=astra.player.vn.step player_sequence=17 fixed_step=3 coverage=state.library,state.good ",
            "runtime_state_hash=hash128:1111 runtime_event_hash=hash128:2222 ",
            "runtime_presentation_hash=hash128:3333 current_state_id=state.library ",
            "pending_choice_ids=choice.left,choice.right terminal_route_ids=ending.good\n",
        );

        let traces = parse_player_host_traces(stderr);

        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].player_sequence, 17);
        assert_eq!(traces[0].fixed_step, 3);
        assert_eq!(traces[0].kind, "keyboard");
        assert_eq!(
            traces[0].coverage_reached,
            vec!["state.library", "state.good"]
        );
        assert_eq!(traces[0].current_state_id.as_deref(), Some("state.library"));
        assert_eq!(traces[0].runtime_state_hash, "hash128:1111");
        assert_eq!(traces[0].runtime_event_hash, "hash128:2222");
        assert_eq!(traces[0].runtime_presentation_hash, "hash128:3333");
        assert_eq!(
            traces[0].pending_choice_ids,
            vec!["choice.left", "choice.right"]
        );
        assert_eq!(traces[0].terminal_route_ids, vec!["ending.good"]);
        assert!(traces[0].trace_hash.starts_with("sha256:"));
    }

    #[astra_headless_test::test]
    fn visual_change_without_runtime_route_trace_produces_no_coverage() {
        let stderr = "event=astra.player.input.consumed player_sequence=2 kind=keyboard\n";

        let traces = parse_player_host_traces(stderr);

        assert!(traces.is_empty());
    }
}
