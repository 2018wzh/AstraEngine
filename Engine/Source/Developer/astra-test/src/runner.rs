use std::{collections::BTreeMap, fs, path::Path, path::PathBuf, process::Command};

use astra_core::{Diagnostic, DiagnosticSeverity, StableId};
use astra_package::{PackageManifest, PackageReader};
use astra_plugin::{
    dylib_path, LoadedPlugin, PluginDescriptor, PluginError, PluginGate, PluginLoader,
    PluginRegistrar,
};
use astra_runtime::{
    ActionInvocation, ActorId, BlackboardValue, EventPayload, EventSource, GuardExpr,
    PackageHandle, PresentationCommand, RuntimeConfig, RuntimeWorld, SaveBlob, SaveRequest,
    StateDefinition, StateMachineDefinition, TickInput, TransitionDefinition,
};
use astra_target::{validate_manifest, TargetManifest, TargetValidationStatus};
use semver::Version;
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{
    EmitAction, Scenario, ScenarioAction, ScenarioCheck, ScenarioHashes, ScenarioReport,
    ScenarioStatus, ScenarioValue,
};

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("scenario io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("scenario yaml failed: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("runtime failed: {0}")]
    Runtime(#[from] astra_runtime::RuntimeError),
    #[error("plugin failed: {0}")]
    Plugin(#[from] PluginError),
    #[error("scenario failed: {0}")]
    Message(String),
}

pub struct ScenarioRunner;

#[derive(Debug, Clone, Default)]
pub struct ScenarioRunOptions {
    pub package: Option<PathBuf>,
    pub target: Option<String>,
    pub profile: Option<String>,
    pub headless: bool,
}

impl ScenarioRunner {
    pub fn run_file(path: impl AsRef<Path>) -> Result<ScenarioReport, ScenarioError> {
        Self::run_file_with_options(path, ScenarioRunOptions::default())
    }

    pub fn run_file_with_options(
        path: impl AsRef<Path>,
        options: ScenarioRunOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)?;
        let scenario: Scenario = serde_yaml::from_str(&text)?;
        info!(
            schema = %scenario.schema,
            action_count = scenario.actions.len(),
            assertion_count = scenario.assertions.len(),
            "scenario.load"
        );
        let root = scenario_root(path)?;
        Self::run_with_root_and_options(&scenario, root, Some(path), options)
    }

    pub fn run(scenario: &Scenario) -> Result<ScenarioReport, ScenarioError> {
        Self::run_with_options(scenario, ScenarioRunOptions::default())
    }

    pub fn run_with_options(
        scenario: &Scenario,
        options: ScenarioRunOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        info!(
            schema = %scenario.schema,
            action_count = scenario.actions.len(),
            assertion_count = scenario.assertions.len(),
            "scenario.run"
        );
        let root = std::env::current_dir()?;
        Self::run_with_root_and_options(scenario, root, None, options)
    }

    pub fn run_with_root(
        scenario: &Scenario,
        workspace_root: PathBuf,
    ) -> Result<ScenarioReport, ScenarioError> {
        Self::run_with_root_and_options(
            scenario,
            workspace_root,
            None,
            ScenarioRunOptions::default(),
        )
    }

    pub fn run_with_root_and_options(
        scenario: &Scenario,
        workspace_root: PathBuf,
        scenario_path: Option<&Path>,
        options: ScenarioRunOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        info!(
            schema = %scenario.schema,
            action_count = scenario.actions.len(),
            assertion_count = scenario.assertions.len(),
            "scenario.run"
        );
        let package_context =
            prepare_package_context(scenario, &workspace_root, scenario_path, &options);
        let mut context = RunContext::new(
            scenario.seed,
            workspace_root.clone(),
            package_context.handle.clone(),
        )?;
        context.diagnostics.extend(package_context.diagnostics);
        for key in scenario.unsupported.keys() {
            context.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_SCENARIO_FIELD_UNSUPPORTED",
                    "scenario contains an unsupported top-level field",
                )
                .with_field("field", key),
            );
        }
        let mut replayable = Vec::new();
        for action in &scenario.actions {
            context.apply(action)?;
            if action.is_replayable() {
                replayable.push(action.clone());
            }
        }
        let hashes = context.hashes();
        let replay_hashes = Self::run_replay(
            scenario.seed,
            workspace_root,
            package_context.handle.clone(),
            &replayable,
        )?;
        let replay_match = hashes == replay_hashes;
        let mut diagnostics = context.diagnostics.clone();
        let plugin_gate = run_plugin_descriptor_gate(&mut diagnostics);
        let mut checks = vec![
            ScenarioCheck {
                id: "runtime.determinism".to_string(),
                status: if replay_match {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            },
            ScenarioCheck {
                id: "save.load.replay".to_string(),
                status: if context.saved.is_some() && replay_match {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            },
            ScenarioCheck {
                id: "plugin.descriptor_gate".to_string(),
                status: plugin_gate,
            },
        ];
        if context.fixture_actions_registered {
            checks.push(ScenarioCheck {
                id: "plugin.ffi_action_provider".to_string(),
                status: if context.fixture_action_ran() {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            });
        }
        if !context.expected_delayed_events.is_empty() {
            checks.push(ScenarioCheck {
                id: "runtime.delayed_event".to_string(),
                status: if context.delayed_events_delivered() {
                    ScenarioStatus::Pass
                } else {
                    ScenarioStatus::Blocked
                },
            });
        }
        if package_context.package.is_some() {
            let package_blocked = diagnostics.iter().any(|diagnostic| {
                diagnostic.code.starts_with("ASTRA_SCENARIO_PACKAGE")
                    || diagnostic.code.starts_with("ASTRA_SCENARIO_TARGET")
                    || diagnostic.code.starts_with("ASTRA_SCENARIO_REF")
                    || diagnostic.code.starts_with("ASTRA_TARGET")
            });
            checks.push(ScenarioCheck {
                id: "package.target_refs".to_string(),
                status: if package_blocked {
                    ScenarioStatus::Blocked
                } else {
                    ScenarioStatus::Pass
                },
            });
        }
        if scenario.unsupported.is_empty() {
            checks.push(ScenarioCheck {
                id: "scenario.schema".to_string(),
                status: ScenarioStatus::Pass,
            });
        } else {
            checks.push(ScenarioCheck {
                id: "scenario.schema".to_string(),
                status: ScenarioStatus::Blocked,
            });
        }
        let unsupported_actions = context.unsupported_actions.clone();
        if !unsupported_actions.is_empty() {
            checks.push(ScenarioCheck {
                id: "action.unsupported_schema".to_string(),
                status: ScenarioStatus::Blocked,
            });
        }
        let mut unsupported_assertions = Vec::new();
        for assertion in &scenario.assertions {
            let keys: Vec<_> = assertion.unsupported_keys().collect();
            if !keys.is_empty() {
                for key in &keys {
                    diagnostics.push(
                        Diagnostic::blocking(
                            "ASTRA_SCENARIO_ASSERTION_UNSUPPORTED",
                            "scenario assertion is not implemented by this runner",
                        )
                        .with_field("assertion", key),
                    );
                }
                unsupported_assertions.extend(keys.into_iter().map(str::to_string));
            }
            if assertion.replay_hash_match == Some(true) && !replay_match {
                checks.push(ScenarioCheck {
                    id: "assert.replay_hash_match".to_string(),
                    status: ScenarioStatus::Blocked,
                });
            }
            let no_blocking = diagnostics
                .iter()
                .all(|diag| diag.severity != DiagnosticSeverity::Blocking);
            if assertion.no_blocking_diagnostics == Some(true) && !no_blocking {
                checks.push(ScenarioCheck {
                    id: "assert.no_blocking_diagnostics".to_string(),
                    status: ScenarioStatus::Blocked,
                });
            }
        }
        if !unsupported_assertions.is_empty() {
            checks.push(ScenarioCheck {
                id: "assert.unsupported_schema".to_string(),
                status: ScenarioStatus::Blocked,
            });
        }
        let status = if checks
            .iter()
            .all(|check| check.status == ScenarioStatus::Pass)
        {
            ScenarioStatus::Pass
        } else {
            ScenarioStatus::Blocked
        };
        info!(
            schema = "astra.scenario_report.v1",
            status = ?status,
            check_count = checks.len(),
            diagnostic_count = diagnostics.len(),
            "scenario.report"
        );
        Ok(ScenarioReport {
            schema: "astra.scenario_report.v1".to_string(),
            stage: scenario.stage.clone().unwrap_or_else(|| {
                if package_context.package.is_some() {
                    "stage2-media-package".to_string()
                } else {
                    "stage1-enginecore".to_string()
                }
            }),
            package: package_context.package,
            target: package_context.target,
            profile: package_context.profile,
            status,
            hashes,
            checks,
            unsupported_actions,
            unsupported_assertions,
            release_gate_checks: package_context.release_gate_checks,
            diagnostics,
        })
    }

    fn run_replay(
        seed: u64,
        workspace_root: PathBuf,
        package: PackageHandle,
        actions: &[ScenarioAction],
    ) -> Result<ScenarioHashes, ScenarioError> {
        info!(action_count = actions.len(), "scenario.replay.start");
        let mut context = RunContext::new(seed, workspace_root, package)?;
        for action in actions {
            context.apply(action)?;
        }
        let hashes = context.hashes();
        info!(
            state_hash = %hashes.state,
            event_hash = %hashes.event,
            presentation_hash = %hashes.presentation,
            "scenario.replay"
        );
        Ok(hashes)
    }
}

#[derive(Debug, Clone)]
struct PackageContext {
    handle: PackageHandle,
    package: Option<String>,
    target: Option<String>,
    profile: Option<String>,
    diagnostics: Vec<Diagnostic>,
    release_gate_checks: Vec<String>,
}

fn prepare_package_context(
    scenario: &Scenario,
    workspace_root: &Path,
    scenario_path: Option<&Path>,
    options: &ScenarioRunOptions,
) -> PackageContext {
    let package = options
        .package
        .as_ref()
        .map(|path| normalize_repo_path(workspace_root, path))
        .or_else(|| scenario.package.clone());
    let target = options.target.clone().or_else(|| scenario.target.clone());
    let profile = options.profile.clone().or_else(|| scenario.profile.clone());
    let mut context = PackageContext {
        handle: PackageHandle::default(),
        package: package.clone(),
        target: target.clone(),
        profile,
        diagnostics: Vec::new(),
        release_gate_checks: Vec::new(),
    };

    let Some(package_ref) = package else {
        if target.is_some() {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_PACKAGE_MISSING",
                "scenario target was supplied without a package",
            ));
        }
        return context;
    };
    if target.is_none() {
        context.diagnostics.push(Diagnostic::blocking(
            "ASTRA_SCENARIO_TARGET_MISSING",
            "scenario package runs must declare a target",
        ));
    }
    context.handle = PackageHandle {
        package_id: package_ref.clone(),
    };
    context
        .release_gate_checks
        .push("package.integrity".to_string());

    let package_path = options
        .package
        .clone()
        .unwrap_or_else(|| workspace_root.join(&package_ref));
    let bytes = match fs::read(&package_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            context.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_SCENARIO_PACKAGE_MISSING",
                    format!("scenario package could not be read: {err}"),
                )
                .with_field("package", &package_ref),
            );
            return context;
        }
    };
    let reader = match PackageReader::open(&bytes) {
        Ok(reader) => reader,
        Err(err) => {
            context.diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_SCENARIO_PACKAGE_INVALID",
                    format!("scenario package could not be opened: {err}"),
                )
                .with_field("package", &package_ref),
            );
            return context;
        }
    };
    if let Ok(manifest) = reader
        .container()
        .decode_postcard::<PackageManifest>("package.manifest")
    {
        context.handle = PackageHandle {
            package_id: manifest.package_id,
        };
    }
    validate_package_target(&reader, target.as_deref(), &mut context);
    validate_package_scenario_ref(&reader, workspace_root, scenario_path, &mut context);
    context
}

fn validate_package_target(
    reader: &PackageReader,
    target: Option<&str>,
    context: &mut PackageContext,
) {
    let bytes = match reader
        .container()
        .read_bounded("target.manifest", 256 * 1024)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_TARGET_MANIFEST",
                format!("package target manifest could not be read: {err}"),
            ));
            return;
        }
    };
    let manifest: TargetManifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_TARGET_MANIFEST_JSON",
                format!("package target manifest is not valid JSON: {err}"),
            ));
            return;
        }
    };
    let report = validate_manifest(&manifest, target);
    context
        .release_gate_checks
        .push("target.manifest".to_string());
    if report.status == TargetValidationStatus::Blocked {
        context.diagnostics.extend(report.diagnostics);
    }
}

fn validate_package_scenario_ref(
    reader: &PackageReader,
    workspace_root: &Path,
    scenario_path: Option<&Path>,
    context: &mut PackageContext,
) {
    let Some(scenario_path) = scenario_path else {
        return;
    };
    let bytes = match reader.container().read_bounded("scenario.refs", 256 * 1024) {
        Ok(bytes) => bytes,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_REFS",
                format!("package scenario refs could not be read: {err}"),
            ));
            return;
        }
    };
    let refs: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(refs) => refs,
        Err(err) => {
            context.diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_REFS_JSON",
                format!("package scenario refs are not valid JSON: {err}"),
            ));
            return;
        }
    };
    let Some(scenarios) = refs.get("scenarios").and_then(serde_json::Value::as_array) else {
        context.diagnostics.push(Diagnostic::blocking(
            "ASTRA_SCENARIO_REFS_EMPTY",
            "package scenario refs must contain a scenarios array",
        ));
        return;
    };
    let scenario_ref = normalize_repo_path(workspace_root, scenario_path);
    let listed = scenarios
        .iter()
        .filter_map(serde_json::Value::as_str)
        .any(|entry| entry == scenario_ref);
    context
        .release_gate_checks
        .push("scenario.refs".to_string());
    if !listed {
        context.diagnostics.push(
            Diagnostic::blocking(
                "ASTRA_SCENARIO_REF_MISSING",
                "scenario file is not listed in package scenario refs",
            )
            .with_field("scenario", scenario_ref),
        );
    }
}

fn normalize_repo_path(root: &Path, path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root) {
        return relative.to_string_lossy().replace('\\', "/");
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("external-package")
        .to_string()
}

struct RunContext {
    world: RuntimeWorld,
    workspace_root: PathBuf,
    system_actor: ActorId,
    loaded_plugins: Vec<LoadedPlugin>,
    step: u64,
    saved: Option<SaveBlob>,
    diagnostics: Vec<Diagnostic>,
    unsupported_actions: Vec<String>,
    fixture_actions_registered: bool,
    expected_delayed_events: Vec<String>,
}

impl RunContext {
    fn new(
        seed: u64,
        workspace_root: PathBuf,
        package: PackageHandle,
    ) -> Result<Self, ScenarioError> {
        let mut world = RuntimeWorld::create(
            RuntimeConfig {
                seed,
                required_slots: Vec::new(),
            },
            package,
        )?;
        let system_actor = world.create_actor("scenario.system", vec!["scenario".to_string()]);
        Ok(Self {
            world,
            workspace_root,
            system_actor,
            loaded_plugins: Vec::new(),
            step: 0,
            saved: None,
            diagnostics: Vec::new(),
            unsupported_actions: Vec::new(),
            fixture_actions_registered: false,
            expected_delayed_events: Vec::new(),
        })
    }

    fn apply(&mut self, action: &ScenarioAction) -> Result<(), ScenarioError> {
        debug!(action = scenario_action_kind(action), "scenario.action");
        let unsupported: Vec<_> = action.unsupported_keys().map(str::to_string).collect();
        if !unsupported.is_empty() {
            for key in unsupported {
                self.diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_SCENARIO_ACTION_UNSUPPORTED",
                        "scenario action is not implemented by this runner",
                    )
                    .with_field("action", &key),
                );
                self.unsupported_actions.push(key);
            }
            return Ok(());
        }
        if action.register_fixture_actions.is_some() {
            self.register_fixture_actions()?;
        }
        if let Some(add_state_machine) = &action.add_state_machine {
            self.add_state_machine(add_state_machine)?;
        }
        if let Some(schedule) = &action.schedule_delayed_event {
            self.schedule_delayed_event(schedule);
        }
        if action.launch.is_some() {
            self.advance(1)?;
        }
        if let Some(emit) = &action.emit {
            self.emit(emit);
        }
        if let Some(advance) = action.advance {
            for _ in 0..advance.ticks {
                self.advance(1)?;
            }
        }
        if let Some(choice) = &action.choose {
            let mut data = BTreeMap::new();
            data.insert(
                "choice".to_string(),
                BlackboardValue::String(scenario_choice(choice)),
            );
            self.world.emit_event(
                EventSource::Scenario,
                EventPayload {
                    kind: "choice.selected".to_string(),
                    data,
                },
            );
            self.advance(1)?;
        }
        if action.save.is_some() {
            self.saved = Some(self.world.save(SaveRequest::default())?);
        }
        if action.load.is_some() {
            let save = self
                .saved
                .clone()
                .ok_or_else(|| ScenarioError::Message("load requested before save".to_string()))?;
            self.world.load(save)?;
        }
        if action.replay_from_start.is_some() {
            self.advance(1)?;
        }
        Ok(())
    }

    fn register_fixture_actions(&mut self) -> Result<(), ScenarioError> {
        let dylib = dylib_path(&self.workspace_root, "headless_presentation_provider");
        if !dylib.exists() {
            info!("scenario.fixture.build");
            let status = Command::new("cargo")
                .args(["build", "-p", "headless-presentation-provider"])
                .current_dir(&self.workspace_root)
                .status()?;
            if !status.success() {
                warn!("scenario.fixture.build_failed");
                return Err(ScenarioError::Message(
                    "fixture action provider build failed".to_string(),
                ));
            }
        }

        let mut registrar = PluginRegistrar::default();
        let loader = PluginLoader::new(PluginGate {
            engine_version: Version::parse("0.1.0").expect("valid engine version"),
            rustc_fingerprint: "rustc-stable".to_string(),
            feature_fingerprint: "stage1-core".to_string(),
            required_capabilities: vec![
                "presentation.headless".to_string(),
                "action.fixture".to_string(),
            ],
            required_permissions: vec![
                "runtime.presentation".to_string(),
                "runtime.action".to_string(),
            ],
        });
        info!("scenario.fixture.load");
        let plugin = loader.load(dylib, &mut registrar)?;
        if let Some(provider) =
            registrar.selected_provider(&astra_plugin::EngineModuleSlot("presentation".to_string()))
        {
            self.world
                .mount_module(provider.slot.0.clone(), provider.provider_id.clone());
        }
        plugin.install_runtime_actions(&mut self.world)?;
        self.loaded_plugins.push(plugin);
        self.fixture_actions_registered = true;
        info!("scenario.fixture.ready");
        Ok(())
    }

    fn add_state_machine(
        &mut self,
        action: &crate::AddStateMachineAction,
    ) -> Result<(), ScenarioError> {
        let start = self.named_id(&format!("{}.start", action.id));
        let done = self.named_id(&format!("{}.done", action.id));
        self.world.add_state_machine(StateMachineDefinition {
            id: self.named_id(&action.id),
            owner: self.system_actor,
            states: vec![
                StateDefinition {
                    id: start,
                    name: "start".to_string(),
                    terminal: false,
                },
                StateDefinition {
                    id: done,
                    name: "done".to_string(),
                    terminal: true,
                },
            ],
            transitions: vec![TransitionDefinition {
                from: start,
                to: done,
                guard: GuardExpr::EventIs {
                    kind: action.trigger.clone(),
                },
                actions: action
                    .actions
                    .iter()
                    .map(|invocation| ActionInvocation {
                        action_id: invocation.action_id.clone(),
                        input: convert_map(&invocation.input),
                    })
                    .collect(),
                priority: 0,
                source_ref: None,
            }],
            initial_state: start,
        })?;
        Ok(())
    }

    fn schedule_delayed_event(&mut self, action: &crate::ScheduleDelayedEventAction) {
        self.world.schedule_event(
            action.due_tick,
            EventSource::Scenario,
            EventPayload {
                kind: action.kind.clone(),
                data: convert_map(&action.data),
            },
        );
        self.expected_delayed_events.push(action.kind.clone());
    }

    fn emit(&mut self, emit: &EmitAction) {
        self.world.emit_event(
            EventSource::Scenario,
            EventPayload {
                kind: emit.kind.clone(),
                data: convert_map(&emit.data),
            },
        );
        match emit.kind.as_str() {
            "dialogue" => self.world.emit_presentation(PresentationCommand::Dialogue {
                speaker: string_field(&emit.data, "speaker").unwrap_or_default(),
                text: string_field(&emit.data, "text").unwrap_or_default(),
            }),
            "choice" => self.world.emit_presentation(PresentationCommand::Choice {
                prompt: string_field(&emit.data, "prompt").unwrap_or_default(),
                options: list_field(&emit.data, "options"),
            }),
            "text_event" => self
                .world
                .emit_presentation(PresentationCommand::TextEvent {
                    key: string_field(&emit.data, "key").unwrap_or_default(),
                }),
            "marker" => self.world.emit_presentation(PresentationCommand::Marker {
                name: string_field(&emit.data, "name").unwrap_or_default(),
            }),
            other => self.world.emit_presentation(PresentationCommand::Custom {
                kind: other.to_string(),
                data: convert_map(&emit.data),
            }),
        }
    }

    fn advance(&mut self, ticks: u64) -> Result<(), ScenarioError> {
        for _ in 0..ticks {
            self.step += 1;
            debug!(step = self.step, "scenario.advance");
            let report = self.world.tick(TickInput {
                fixed_step: self.step,
                delta_ns: 16_666_667,
                seed: 0,
            })?;
            for diagnostic in &report.diagnostics {
                warn!(
                    step = report.step,
                    diagnostic_code = %diagnostic.code,
                    "scenario.diagnostic"
                );
            }
            self.diagnostics.extend(report.diagnostics);
        }
        Ok(())
    }

    fn hashes(&self) -> ScenarioHashes {
        ScenarioHashes {
            state: self.world.state_hash(),
            event: self.world.event_hash(),
            presentation: self.world.presentation_hash(),
        }
    }

    fn fixture_action_ran(&self) -> bool {
        self.world.snapshot().blackboard.get("fixture.action")
            == Some(&BlackboardValue::from("ran"))
    }

    fn delayed_events_delivered(&self) -> bool {
        let trace = self.world.debug_session().event_trace();
        self.expected_delayed_events.iter().all(|expected| {
            trace
                .iter()
                .any(|event| event.payload.kind.as_str() == expected)
        })
    }

    fn named_id(&self, name: &str) -> StableId {
        StableId::deterministic_v7(10, stable_hash(name), self.world.snapshot().config.seed)
    }
}

fn scenario_action_kind(action: &ScenarioAction) -> &'static str {
    if !action.unsupported.is_empty() {
        "unsupported"
    } else if action.register_fixture_actions.is_some() {
        "register_fixture_actions"
    } else if action.add_state_machine.is_some() {
        "add_state_machine"
    } else if action.schedule_delayed_event.is_some() {
        "schedule_delayed_event"
    } else if action.launch.is_some() {
        "launch"
    } else if action.emit.is_some() {
        "emit"
    } else if action.advance.is_some() {
        "advance"
    } else if action.choose.is_some() {
        "choose"
    } else if action.save.is_some() {
        "save"
    } else if action.load.is_some() {
        "load"
    } else if action.replay_from_start.is_some() {
        "replay_from_start"
    } else {
        "empty"
    }
}

fn scenario_root(path: &Path) -> Result<PathBuf, ScenarioError> {
    if path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("scenarios")
    {
        let parent = path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| ScenarioError::Message("invalid scenario path".to_string()))?;
        return Ok(parent.to_path_buf());
    }
    Ok(std::env::current_dir()?)
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn convert_map(map: &BTreeMap<String, ScenarioValue>) -> BTreeMap<String, BlackboardValue> {
    map.iter()
        .map(|(key, value)| (key.clone(), BlackboardValue::from(value.clone())))
        .collect()
}

fn scenario_choice(value: &ScenarioValue) -> String {
    match value {
        ScenarioValue::String(value) => value.clone(),
        ScenarioValue::I64(value) => value.to_string(),
        ScenarioValue::F64(value) => value.to_string(),
        ScenarioValue::Bool(value) => value.to_string(),
        ScenarioValue::Null => "null".to_string(),
        ScenarioValue::List(_) | ScenarioValue::Map(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| "complex_choice".to_string())
        }
    }
}

fn string_field(map: &BTreeMap<String, ScenarioValue>, key: &str) -> Option<String> {
    match map.get(key) {
        Some(ScenarioValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn list_field(map: &BTreeMap<String, ScenarioValue>, key: &str) -> Vec<String> {
    match map.get(key) {
        Some(ScenarioValue::List(values)) => values
            .iter()
            .filter_map(|value| match value {
                ScenarioValue::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn run_plugin_descriptor_gate(diagnostics: &mut Vec<Diagnostic>) -> ScenarioStatus {
    let descriptor = r#"
id: astra.fixture.headless_presentation
version: 0.1.0
engine_version: 0.1.0
rustc_fingerprint: rustc-stable
feature_fingerprint: stage1-core
abi_style: abi_stable_rust
capabilities:
  - presentation.headless
  - action.fixture
permissions:
  - runtime.presentation
  - runtime.action
packaged: true
"#;
    let gate = PluginGate {
        engine_version: Version::parse("0.1.0").expect("valid engine version"),
        rustc_fingerprint: "rustc-stable".to_string(),
        feature_fingerprint: "stage1-core".to_string(),
        required_capabilities: vec!["presentation.headless".to_string()],
        required_permissions: vec!["runtime.presentation".to_string()],
    };
    match PluginDescriptor::from_yaml(descriptor).and_then(|descriptor| descriptor.validate(&gate))
    {
        Ok(()) => ScenarioStatus::Pass,
        Err(PluginError::GateBlocked(mut plugin_diagnostics)) => {
            diagnostics.append(&mut plugin_diagnostics);
            ScenarioStatus::Blocked
        }
        Err(err) => {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_SCENARIO_PLUGIN_GATE",
                err.to_string(),
            ));
            ScenarioStatus::Blocked
        }
    }
}
