use astra_core::{Diagnostic, Hash128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioReport {
    pub schema: String,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub status: ScenarioStatus,
    pub hashes: ScenarioHashes,
    pub checks: Vec<ScenarioCheck>,
    #[serde(default)]
    pub unsupported_actions: Vec<String>,
    #[serde(default)]
    pub unsupported_assertions: Vec<String>,
    #[serde(default)]
    pub release_gate_checks: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioHashes {
    pub state: Hash128,
    pub event: Hash128,
    pub presentation: Hash128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioCheck {
    pub id: String,
    pub status: ScenarioStatus,
}

impl ScenarioReport {
    pub fn explain(&self) -> String {
        let mut out = format!(
            "{} {}: state={}, event={}, presentation={}",
            self.schema, self.stage, self.hashes.state, self.hashes.event, self.hashes.presentation
        );
        for check in &self.checks {
            out.push_str(&format!("\n{}: {:?}", check.id, check.status));
        }
        out
    }
}
