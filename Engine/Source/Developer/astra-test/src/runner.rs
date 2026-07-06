use std::{collections::BTreeMap, fs, path::Path};

use astra_core::{Diagnostic, DiagnosticSeverity};
use astra_plugin::{PluginDescriptor, PluginError, PluginGate};
use astra_runtime::{
    BlackboardValue, EventPayload, EventSource, PackageHandle, PresentationCommand, RuntimeConfig,
    RuntimeWorld, SaveBlob, SaveRequest, TickInput,
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
    #[error("scenario failed: {0}")]
    Message(String),
}

pub struct ScenarioRunner;

impl ScenarioRunner {
    pub fn run_file(path: impl AsRef<Path>) -> Result<ScenarioReport, ScenarioError> {
        let text = fs::read_to_string(path)?;
        let scenario: Scenario = serde_yaml::from_str(&text)?;
        Self::run(&scenario)
    }

    pub fn run(scenario: &Scenario) -> Result<ScenarioReport, ScenarioError> {
        let mut context = RunContext::new(scenario.seed)?;
        let mut replayable = Vec::new();
        for action in &scenario.actions {
            context.apply(action)?;
            if action.is_replayable() {
                replayable.push(action.clone());
            }
        }
        let hashes = context.hashes();
        let replay_hashes = Self::run_replay(scenario.seed, &replayable)?;
        let replay_match = hashes == replay_hashes;
        let mut diagnostics = context.diagnostics;
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

    fn run_replay(seed: u64, actions: &[ScenarioAction]) -> Result<ScenarioHashes, ScenarioError> {
        let mut context = RunContext::new(seed)?;
        for action in actions {
            context.apply(action)?;
        }
        Ok(context.hashes())
    }
}

struct RunContext {
    world: RuntimeWorld,
    step: u64,
    saved: Option<SaveBlob>,
    diagnostics: Vec<Diagnostic>,
}

impl RunContext {
    fn new(seed: u64) -> Result<Self, ScenarioError> {
        Ok(Self {
            world: RuntimeWorld::create(
                RuntimeConfig {
                    seed,
                    required_slots: Vec::new(),
                },
                PackageHandle::default(),
            )?,
            step: 0,
            saved: None,
            diagnostics: Vec::new(),
        })
    }

    fn apply(&mut self, action: &ScenarioAction) -> Result<(), ScenarioError> {
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
permissions:
  - runtime.presentation
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
