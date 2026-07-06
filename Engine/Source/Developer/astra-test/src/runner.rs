use std::{collections::BTreeMap, fs, path::Path, path::PathBuf, process::Command};

use astra_core::{Diagnostic, DiagnosticSeverity, StableId};
use astra_plugin::{
    dylib_path, LoadedPlugin, PluginDescriptor, PluginError, PluginGate, PluginLoader,
    PluginRegistrar,
};
use astra_runtime::{
    ActionInvocation, ActorId, BlackboardValue, EventPayload, EventSource, GuardExpr,
    PackageHandle, PresentationCommand, RuntimeConfig, RuntimeWorld, SaveBlob, SaveRequest,
    StateDefinition, StateMachineDefinition, TickInput, TransitionDefinition,
};
use semver::Version;
use thiserror::Error;

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

impl ScenarioRunner {
    pub fn run_file(path: impl AsRef<Path>) -> Result<ScenarioReport, ScenarioError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)?;
        let scenario: Scenario = serde_yaml::from_str(&text)?;
        let root = scenario_root(path)?;
        Self::run_with_root(&scenario, root)
    }

    pub fn run(scenario: &Scenario) -> Result<ScenarioReport, ScenarioError> {
        let root = std::env::current_dir()?;
        Self::run_with_root(scenario, root)
    }

    pub fn run_with_root(
        scenario: &Scenario,
        workspace_root: PathBuf,
    ) -> Result<ScenarioReport, ScenarioError> {
        let mut context = RunContext::new(scenario.seed, workspace_root.clone())?;
        let mut replayable = Vec::new();
        for action in &scenario.actions {
            context.apply(action)?;
            if action.is_replayable() {
                replayable.push(action.clone());
            }
        }
        let hashes = context.hashes();
        let replay_hashes = Self::run_replay(scenario.seed, workspace_root, &replayable)?;
        let replay_match = hashes == replay_hashes;
        let mut diagnostics = context.diagnostics.clone();
        let plugin_gate = run_plugin_descriptor_gate(&mut diagnostics);
        let no_blocking = diagnostics
            .iter()
            .all(|diag| diag.severity != DiagnosticSeverity::Blocking);

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
        for assertion in &scenario.assertions {
            if assertion.replay_hash_match == Some(true) && !replay_match {
                checks.push(ScenarioCheck {
                    id: "assert.replay_hash_match".to_string(),
                    status: ScenarioStatus::Blocked,
                });
            }
            if assertion.no_blocking_diagnostics == Some(true) && !no_blocking {
                checks.push(ScenarioCheck {
                    id: "assert.no_blocking_diagnostics".to_string(),
                    status: ScenarioStatus::Blocked,
                });
            }
        }
        let status = if checks
            .iter()
            .all(|check| check.status == ScenarioStatus::Pass)
        {
            ScenarioStatus::Pass
        } else {
            ScenarioStatus::Blocked
        };
        Ok(ScenarioReport {
            schema: "astra.scenario_report.v1".to_string(),
            stage: scenario
                .stage
                .clone()
                .unwrap_or_else(|| "stage1-enginecore".to_string()),
            status,
            hashes,
            checks,
            diagnostics,
        })
    }

    fn run_replay(
        seed: u64,
        workspace_root: PathBuf,
        actions: &[ScenarioAction],
    ) -> Result<ScenarioHashes, ScenarioError> {
        let mut context = RunContext::new(seed, workspace_root)?;
        for action in actions {
            context.apply(action)?;
        }
        Ok(context.hashes())
    }
}

struct RunContext {
    world: RuntimeWorld,
    workspace_root: PathBuf,
    system_actor: ActorId,
    loaded_plugins: Vec<LoadedPlugin>,
    step: u64,
    saved: Option<SaveBlob>,
    diagnostics: Vec<Diagnostic>,
    fixture_actions_registered: bool,
    expected_delayed_events: Vec<String>,
}

impl RunContext {
    fn new(seed: u64, workspace_root: PathBuf) -> Result<Self, ScenarioError> {
        let mut world = RuntimeWorld::create(
            RuntimeConfig {
                seed,
                required_slots: Vec::new(),
            },
            PackageHandle::default(),
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
            fixture_actions_registered: false,
            expected_delayed_events: Vec::new(),
        })
    }

    fn apply(&mut self, action: &ScenarioAction) -> Result<(), ScenarioError> {
        if action.register_fixture_actions.is_some() {
            self.register_fixture_actions()?;
        }
        if let Some(add_state_machine) = &action.add_state_machine {
            self.add_state_machine(add_state_machine);
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
                BlackboardValue::String(choice.clone()),
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
            let status = Command::new("cargo")
                .args(["build", "-p", "headless-presentation-provider"])
                .current_dir(&self.workspace_root)
                .status()?;
            if !status.success() {
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
        Ok(())
    }

    fn add_state_machine(&mut self, action: &crate::AddStateMachineAction) {
        let start = self.named_id(&format!("{}.start", action.id));
        let done = self.named_id(&format!("{}.done", action.id));
        self.world.add_state_machine(StateMachineDefinition {
            id: self.named_id(&action.id),
            owner: self.system_actor,
            states: vec![
                StateDefinition {
                    id: start,
                    name: "start".to_string(),
                },
                StateDefinition {
                    id: done,
                    name: "done".to_string(),
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
                source_ref: None,
            }],
            initial_state: start,
        });
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
            let report = self.world.tick(TickInput {
                fixed_step: self.step,
                delta_ns: 16_666_667,
                seed: 0,
            })?;
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
