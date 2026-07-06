use std::collections::BTreeMap;

use astra_runtime::BlackboardValue;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scenario {
    pub schema: String,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    pub seed: u64,
    #[serde(default)]
    pub actions: Vec<ScenarioAction>,
    #[serde(default)]
    pub assertions: Vec<ScenarioAssertion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioAction {
    #[serde(default)]
    pub launch: Option<BTreeMap<String, ScenarioValue>>,
    #[serde(default)]
    pub emit: Option<EmitAction>,
    #[serde(default)]
    pub advance: Option<AdvanceAction>,
    #[serde(default)]
    pub choose: Option<String>,
    #[serde(default)]
    pub save: Option<String>,
    #[serde(default)]
    pub load: Option<String>,
    #[serde(default)]
    pub replay_from_start: Option<BTreeMap<String, ScenarioValue>>,
}

impl ScenarioAction {
    pub fn is_replayable(&self) -> bool {
        self.launch.is_some()
            || self.emit.is_some()
            || self.advance.is_some()
            || self.choose.is_some()
            || self.replay_from_start.is_some()
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioAssertion {
    #[serde(default)]
    pub replay_hash_match: Option<bool>,
    #[serde(default)]
    pub no_blocking_diagnostics: Option<bool>,
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
