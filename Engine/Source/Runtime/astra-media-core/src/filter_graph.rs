use std::collections::BTreeMap;

use astra_core::{Diagnostic, DiagnosticSeverity, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{frame_hash, CpuFrame, MediaError};

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
        tracing::debug!(
            event = "media.filter.validate.start",
            node_count = graph.nodes.len(),
            "filter graph validation started"
        );
        let mut diagnostics = Vec::new();
        if graph.schema != "astra.filter_graph.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_FILTER_SCHEMA",
                "filter graph schema must be astra.filter_graph.v1",
            ));
        }
        if graph.nodes.len() > 256 {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_FILTER_NODE_BUDGET",
                "filter graph exceeds the bounded node budget",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for node in &graph.nodes {
            if !safe_symbol(&node.id) || !safe_filter_kind(&node.kind) {
                diagnostics.push(Diagnostic::blocking(
                    "ASTRA_FILTER_NODE_IDENTITY",
                    "filter node id or kind is invalid",
                ));
            }
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
            if node.input != node.output {
                diagnostics.push(Diagnostic::blocking(
                    "ASTRA_FILTER_TARGET_FLOW",
                    "the current deterministic executor requires in-place target flow",
                ));
            }
            if let Err(diagnostic) = validate_node_params(node) {
                diagnostics.push(diagnostic);
            }
            if node.allow_cpu_fallback {
                diagnostics.push(Diagnostic::warning(
                    "ASTRA_FILTER_CPU_FALLBACK",
                    format!("filter node {} can use CPU fallback", node.id),
                ));
            }
        }
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Blocking)
        {
            tracing::error!(
                event = "media.filter.validate.blocked",
                diagnostic_count = diagnostics.len(),
                "filter graph validation blocked"
            );
        } else if !diagnostics.is_empty() {
            tracing::warn!(
                event = "media.filter.validate.warning",
                diagnostic_count = diagnostics.len(),
                "filter graph validation completed with warnings"
            );
        } else {
            tracing::trace!(
                event = "media.filter.validate.complete",
                node_count = graph.nodes.len(),
                "filter graph validation completed"
            );
        }
        FilterValidationReport { diagnostics }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilterExecutionReport {
    pub schema: String,
    pub input_hash: Hash256,
    pub output_hash: Hash256,
    pub executed_nodes: Vec<FilterExecutionNode>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilterExecutionNode {
    pub id: String,
    pub kind: String,
    pub fallback_used: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CpuFilterExecutor;

impl CpuFilterExecutor {
    pub fn execute(
        &self,
        graph: &FilterGraph,
        mut frame: CpuFrame,
    ) -> Result<(CpuFrame, FilterExecutionReport), MediaError> {
        tracing::trace!(
            event = "media.filter.execute.start",
            node_count = graph.nodes.len(),
            input_hash = %frame.hash,
            "CPU filter execution started"
        );
        let validation = FilterValidator.validate(graph);
        if !validation.blocking_diagnostics().is_empty() {
            return Err(MediaError::Diagnostics(validation.diagnostics));
        }

        validate_frame(&frame)?;
        let input_hash = frame.hash;
        let mut executed_nodes = Vec::with_capacity(graph.nodes.len());
        for node in &graph.nodes {
            match node.kind.as_str() {
                "astra.filter.bloom" => {
                    require_cpu_fallback(node)?;
                    let intensity = required_float_param(node, "intensity")?;
                    apply_bloom(&mut frame.bytes, intensity);
                    executed_nodes.push(FilterExecutionNode {
                        id: node.id.clone(),
                        kind: node.kind.clone(),
                        fallback_used: true,
                    });
                }
                "astra.filter.color_matrix" => {
                    require_cpu_fallback(node)?;
                    apply_color_matrix(&mut frame.bytes, node)?;
                    executed_nodes.push(FilterExecutionNode {
                        id: node.id.clone(),
                        kind: node.kind.clone(),
                        fallback_used: true,
                    });
                }
                "astra.filter.fade" => {
                    require_cpu_fallback(node)?;
                    let amount = required_float_param(node, "amount")?;
                    apply_fade(&mut frame.bytes, amount);
                    executed_nodes.push(FilterExecutionNode {
                        id: node.id.clone(),
                        kind: node.kind.clone(),
                        fallback_used: true,
                    });
                }
                _ => {
                    return Err(MediaError::Diagnostics(vec![Diagnostic::blocking(
                        "ASTRA_FILTER_UNSUPPORTED",
                        format!("filter node {} has no CPU executor", node.id),
                    )]));
                }
            }
        }

        frame.hash = frame_hash(frame.width, frame.height, frame.format, &frame.bytes);
        let report = FilterExecutionReport {
            schema: "astra.filter_execution_report.v1".to_string(),
            input_hash,
            output_hash: frame.hash,
            executed_nodes,
            diagnostics: validation.diagnostics,
        };
        tracing::info!(
            event = "media.filter.execute.complete",
            node_count = report.executed_nodes.len(),
            input_hash = %report.input_hash,
            output_hash = %report.output_hash,
            "CPU filter execution completed"
        );
        Ok((frame, report))
    }
}

fn apply_bloom(bytes: &mut [u8], intensity: f32) {
    let add = (255.0 * intensity.clamp(0.0, 1.0)) as u8;
    for pixel in bytes.chunks_exact_mut(4) {
        pixel[0] = pixel[0].saturating_add(add);
        pixel[1] = pixel[1].saturating_add(add);
        pixel[2] = pixel[2].saturating_add(add);
    }
}

fn apply_color_matrix(bytes: &mut [u8], node: &FilterNode) -> Result<(), MediaError> {
    let r = required_float_param(node, "r")?;
    let g = required_float_param(node, "g")?;
    let b = required_float_param(node, "b")?;
    let a = required_float_param(node, "a")?;
    for pixel in bytes.chunks_exact_mut(4) {
        pixel[0] = scaled_channel(pixel[0], r);
        pixel[1] = scaled_channel(pixel[1], g);
        pixel[2] = scaled_channel(pixel[2], b);
        pixel[3] = scaled_channel(pixel[3], a);
    }
    Ok(())
}

fn apply_fade(bytes: &mut [u8], amount: f32) {
    let scale = amount.clamp(0.0, 1.0);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel[0] = scaled_channel(pixel[0], scale);
        pixel[1] = scaled_channel(pixel[1], scale);
        pixel[2] = scaled_channel(pixel[2], scale);
    }
}

fn scaled_channel(channel: u8, scale: f32) -> u8 {
    ((channel as f32) * scale).clamp(0.0, 255.0) as u8
}

fn float_param(node: &FilterNode, name: &str) -> Option<f32> {
    match node.params.get(name) {
        Some(FilterParam::Float(value)) => Some(*value),
        _ => None,
    }
}

fn validate_node_params(node: &FilterNode) -> Result<(), Diagnostic> {
    let required: &[&str] = match node.kind.as_str() {
        "astra.filter.bloom" => &["intensity"],
        "astra.filter.fade" => &["amount"],
        "astra.filter.color_matrix" => &["r", "g", "b", "a"],
        _ => {
            return Err(Diagnostic::blocking(
                "ASTRA_FILTER_UNSUPPORTED",
                "filter node kind has no deterministic executor",
            ));
        }
    };
    if node.params.len() != required.len()
        || node
            .params
            .keys()
            .any(|name| !required.contains(&name.as_str()))
    {
        return Err(Diagnostic::blocking(
            "ASTRA_FILTER_PARAM_SET",
            "filter node parameter set does not match its contract",
        ));
    }
    for name in required {
        let Some(FilterParam::Float(value)) = node.params.get(*name) else {
            return Err(Diagnostic::blocking(
                "ASTRA_FILTER_PARAM_TYPE",
                "filter parameter must be an explicit float",
            ));
        };
        let max = if node.kind == "astra.filter.color_matrix" {
            4.0
        } else {
            1.0
        };
        if !value.is_finite() || !(0.0..=max).contains(value) {
            return Err(Diagnostic::blocking(
                "ASTRA_FILTER_PARAM_RANGE",
                "filter parameter is outside its finite supported range",
            ));
        }
    }
    Ok(())
}

fn required_float_param(node: &FilterNode, name: &str) -> Result<f32, MediaError> {
    float_param(node, name).ok_or_else(|| {
        MediaError::Diagnostics(vec![Diagnostic::blocking(
            "ASTRA_FILTER_PARAM_TYPE",
            "validated filter parameter is unavailable",
        )])
    })
}

fn require_cpu_fallback(node: &FilterNode) -> Result<(), MediaError> {
    if !node.allow_cpu_fallback {
        return Err(MediaError::Diagnostics(vec![Diagnostic::blocking(
            "ASTRA_FILTER_CPU_FALLBACK_FORBIDDEN",
            "CPU execution was not explicitly authorized for this filter node",
        )]));
    }
    Ok(())
}

fn validate_frame(frame: &CpuFrame) -> Result<(), MediaError> {
    let expected = frame
        .width
        .checked_mul(frame.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| MediaError::message("ASTRA_FILTER_FRAME_SIZE: frame size overflows"))?;
    if frame.width == 0
        || frame.height == 0
        || frame.bytes.len() != expected
        || frame_hash(frame.width, frame.height, frame.format, &frame.bytes) != frame.hash
    {
        return Err(MediaError::message(
            "ASTRA_FILTER_FRAME_INVALID: input frame dimensions, bytes, or hash are invalid",
        ));
    }
    Ok(())
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_filter_kind(value: &str) -> bool {
    value.starts_with("astra.filter.") && safe_symbol(value)
}
