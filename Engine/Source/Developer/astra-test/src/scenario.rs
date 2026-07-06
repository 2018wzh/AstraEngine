use std::collections::BTreeMap;

use astra_runtime::BlackboardValue;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scenario {
    pub schema: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    pub seed: u64,
    #[serde(default)]
    pub actions: Vec<ScenarioAction>,
    #[serde(default)]
    pub assertions: Vec<ScenarioAssertion>,
    #[serde(default, flatten)]
    pub unsupported: BTreeMap<String, ScenarioValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioAction {
    #[serde(default)]
    pub launch: Option<BTreeMap<String, ScenarioValue>>,
    #[serde(default)]
    pub register_fixture_actions: Option<BTreeMap<String, ScenarioValue>>,
    #[serde(default)]
    pub add_state_machine: Option<AddStateMachineAction>,
    #[serde(default)]
    pub schedule_delayed_event: Option<ScheduleDelayedEventAction>,
    #[serde(default)]
    pub emit: Option<EmitAction>,
    #[serde(default)]
    pub advance: Option<AdvanceAction>,
    #[serde(default)]
    pub choose: Option<ScenarioValue>,
    #[serde(default)]
    pub save: Option<String>,
    #[serde(default)]
    pub load: Option<String>,
    #[serde(default)]
    pub replay_from_start: Option<BTreeMap<String, ScenarioValue>>,
    #[serde(default, flatten)]
    pub unsupported: BTreeMap<String, ScenarioValue>,
}

impl ScenarioAction {
    pub fn is_replayable(&self) -> bool {
        self.unsupported.is_empty()
            && (self.launch.is_some()
                || self.register_fixture_actions.is_some()
                || self.add_state_machine.is_some()
                || self.schedule_delayed_event.is_some()
                || self.emit.is_some()
                || self.advance.is_some()
                || self.choose.is_some()
                || self.replay_from_start.is_some())
    }

    pub fn unsupported_keys(&self) -> impl Iterator<Item = &str> {
        self.unsupported.keys().map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AddStateMachineAction {
    pub id: String,
    pub trigger: String,
    #[serde(default)]
    pub actions: Vec<ScenarioActionInvocation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioActionInvocation {
    pub action_id: String,
    #[serde(default)]
    pub input: BTreeMap<String, ScenarioValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleDelayedEventAction {
    pub due_tick: u64,
    pub kind: String,
    #[serde(default)]
    pub data: BTreeMap<String, ScenarioValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EmitAction {
    pub kind: String,
    #[serde(default)]
    pub data: BTreeMap<String, ScenarioValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AdvanceAction {
    #[serde(default = "default_ticks")]
    pub ticks: u64,
}

fn default_ticks() -> u64 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioAssertion {
    #[serde(default)]
    pub replay_hash_match: Option<bool>,
    #[serde(default)]
    pub no_blocking_diagnostics: Option<bool>,
    #[serde(default, flatten)]
    pub unsupported: BTreeMap<String, ScenarioValue>,
}

impl ScenarioAssertion {
    pub fn unsupported_keys(&self) -> impl Iterator<Item = &str> {
        self.unsupported.keys().map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ScenarioValue {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    List(Vec<ScenarioValue>),
    Map(BTreeMap<String, ScenarioValue>),
}

impl From<ScenarioValue> for BlackboardValue {
    fn from(value: ScenarioValue) -> Self {
        match value {
            ScenarioValue::Null => BlackboardValue::Null,
            ScenarioValue::Bool(value) => BlackboardValue::Bool(value),
            ScenarioValue::I64(value) => BlackboardValue::I64(value),
            ScenarioValue::F64(value) => BlackboardValue::F64(value),
            ScenarioValue::String(value) => BlackboardValue::String(value),
            ScenarioValue::List(values) => {
                BlackboardValue::List(values.into_iter().map(BlackboardValue::from).collect())
            }
            ScenarioValue::Map(values) => BlackboardValue::Map(
                values
                    .into_iter()
                    .map(|(key, value)| (key, BlackboardValue::from(value)))
                    .collect(),
            ),
        }
    }
}
