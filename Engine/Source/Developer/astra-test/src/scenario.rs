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
    pub platform: Option<String>,
    #[serde(default)]
    pub generated_route_id: Option<String>,
    #[serde(default)]
    pub mount_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub mount_probes: Vec<MountProbe>,
    #[serde(default)]
    pub mount_assets: Vec<MountAsset>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MountProbe {
    pub alias: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MountAsset {
    pub alias: String,
    pub path: String,
    pub role: String,
    pub route_id: String,
    pub sha256: String,
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
    pub player_input: Option<PlayerInputAction>,
    #[serde(default)]
    pub open_system: Option<String>,
    #[serde(default)]
    pub replay_voice: Option<String>,
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
                || self.player_input.is_some()
                || self.open_system.is_some()
                || self.replay_voice.is_some()
                || self.save.is_some()
                || self.load.is_some()
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInputAction {
    pub kind: PlayerInputKind,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub ticks: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerInputKind {
    Advance,
    Choose,
    OpenSystem,
    SystemReturn,
    ReplayVoice,
    Save,
    Load,
    SetAuto,
    SetSkip,
    SetConfig,
    UnlockGallery,
    UnlockReplay,
    CompleteWait,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioAssertion {
    #[serde(default)]
    pub check: Option<String>,
    #[serde(default)]
    pub replay_hash_match: Option<bool>,
    #[serde(default)]
    pub no_blocking_diagnostics: Option<bool>,
    #[serde(default)]
    pub route_reached: Option<String>,
    #[serde(default)]
    pub backlog_has_key: Option<String>,
    #[serde(default)]
    pub read_state_has: Option<String>,
    #[serde(default)]
    pub voice_replay_available: Option<String>,
    #[serde(default)]
    pub coverage: Option<CoverageAssertion>,
    #[serde(default)]
    pub hash: Option<HashAssertion>,
    #[serde(default)]
    pub visual_reference: Option<VisualReferenceAssertion>,
    #[serde(default)]
    pub system_state: Option<SystemStateAssertion>,
    #[serde(default, flatten)]
    pub unsupported: BTreeMap<String, ScenarioValue>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CoverageAssertion {
    #[serde(default)]
    pub routes: Vec<String>,
    #[serde(default)]
    pub backlog_keys: Vec<String>,
    #[serde(default)]
    pub read_state: Vec<String>,
    #[serde(default)]
    pub voice_replay: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HashAssertion {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub presentation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VisualReferenceAssertion {
    pub id: String,
    pub hash: String,
    #[serde(default)]
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemStateAssertion {
    #[serde(default)]
    pub auto_enabled: Option<bool>,
    #[serde(default)]
    pub skip_mode: Option<String>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub gallery_unlocks: Vec<String>,
    #[serde(default)]
    pub replay_unlocks: Vec<String>,
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
