use astra_core::{Diagnostic, Hash128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioReport {
    pub schema: String,
    pub stage: String,
    pub status: ScenarioStatus,
    pub hashes: ScenarioHashes,
    pub checks: Vec<ScenarioCheck>,
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
