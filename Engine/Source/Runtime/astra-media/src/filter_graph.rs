use std::collections::BTreeMap;

use astra_core::{Diagnostic, DiagnosticSeverity};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FilterGraph {
    pub schema: String,
    pub nodes: Vec<FilterNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FilterNode {
    pub id: String,
    pub kind: String,
    pub input: FilterTarget,
    pub output: FilterTarget,
    #[serde(default)]
    pub params: BTreeMap<String, FilterParam>,
    pub deterministic: bool,
    pub allow_cpu_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterTarget {
    Background,
    Character,
    Ui,
    Text,
    Video,
    Final,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilterParam {
    Float(f32),
    Int(i64),
    Bool(bool),
    Text(String),
}

impl From<f32> for FilterParam {
    fn from(value: f32) -> Self {
        Self::Float(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilterValidationReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl FilterValidationReport {
    pub fn blocking_diagnostics(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Blocking)
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct FilterValidator;

impl FilterValidator {
    pub fn validate(&self, graph: &FilterGraph) -> FilterValidationReport {
        let mut diagnostics = Vec::new();
        if graph.schema != "astra.filter_graph.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_FILTER_SCHEMA",
                "filter graph schema must be astra.filter_graph.v1",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for node in &graph.nodes {
            if !seen.insert(node.id.clone()) {
                diagnostics.push(Diagnostic::blocking(
                    "ASTRA_FILTER_DUPLICATE_NODE",
                    format!("duplicate filter node {}", node.id),
                ));
            }
            if !node.deterministic {
                diagnostics.push(Diagnostic::blocking(
                    "ASTRA_FILTER_NONDETERMINISTIC",
                    format!("filter node {} is not deterministic", node.id),
                ));
            }
            if node.kind == "astra.filter.bloom" {
                match node.params.get("intensity") {
                    Some(FilterParam::Float(_)) => {}
                    _ => diagnostics.push(Diagnostic::blocking(
                        "ASTRA_FILTER_PARAM_TYPE",
                        "bloom intensity must be a float",
                    )),
                }
            }
            if node.allow_cpu_fallback {
                diagnostics.push(Diagnostic::warning(
                    "ASTRA_FILTER_CPU_FALLBACK",
                    format!("filter node {} can use CPU fallback", node.id),
                ));
            }
        }
        FilterValidationReport { diagnostics }
    }
}
