use super::*;

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
pub(super) enum ScenarioInputAction {
    Advance,
    Choose { option_id: String },
    OpenSystem { page: String },
    Back,
}

impl ScenarioInputAction {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Advance => "advance",
            Self::Choose { .. } => "choose",
            Self::OpenSystem { .. } => "open_system",
            Self::Back => "back",
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct PlayerScenarioDocument {
    pub(super) schema: String,
    #[serde(default)]
    pub(super) actions: Vec<PlayerScenarioAction>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct PlayerScenarioAction {
    #[serde(default)]
    pub(super) launch: Option<serde_yaml::Value>,
    #[serde(default)]
    pub(super) player_input: Option<PlayerScenarioInput>,
    #[serde(default, flatten)]
    pub(super) unsupported: std::collections::BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct PlayerScenarioInput {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) value: Option<String>,
    #[serde(default = "one_tick")]
    pub(super) ticks: u64,
}

pub(super) fn one_tick() -> u64 {
    1
}

pub(super) fn parse_scenario_input_plan(bytes: &[u8]) -> Result<Vec<ScenarioInputAction>, String> {
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
            schema: "astra.player_input_transcript.v3".to_string(),
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
            schema: "astra.player_input_transcript.v3".to_string(),
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
