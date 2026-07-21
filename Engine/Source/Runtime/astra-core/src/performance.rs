use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Diagnostic, DiagnosticSeverity, Hash256};

pub const PERFORMANCE_BUDGET_SCHEMA: &str = "astra.performance_budget.v1";
pub const PERFORMANCE_REPORT_SCHEMA: &str = "astra.performance_report.v1";
pub const PERFORMANCE_TRACE_MANIFEST_SCHEMA: &str = "astra.performance_trace_manifest.v1";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PerformanceError {
    #[error("ASTRA_PERFORMANCE_BUDGET: {0}")]
    InvalidBudget(String),
    #[error("ASTRA_PERFORMANCE_IDENTITY: {0}")]
    InvalidIdentity(String),
    #[error("ASTRA_PERFORMANCE_METRIC: {0}")]
    InvalidMetric(String),
    #[error("ASTRA_PERFORMANCE_SAMPLE_BUDGET: {0}")]
    SampleBudget(String),
    #[error("ASTRA_PERFORMANCE_REPORT: {0}")]
    InvalidReport(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceUnit {
    Nanoseconds,
    Microseconds,
    Bytes,
    Count,
    ItemsPerSecond,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerformanceTraceManifest {
    pub schema: String,
    pub identity: PerformanceRunIdentity,
    pub workload_id: String,
    pub adapter_identity_hash: String,
    pub driver_identity_hash: String,
    pub report_hash: String,
    pub trace_hash: String,
    pub event_count: u64,
    pub dropped_event_count: u64,
    pub byte_length: u64,
    pub truncated: bool,
    pub timestamps_monotonic: bool,
}

impl PerformanceTraceManifest {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        self.identity.validate()?;
        if self.schema != PERFORMANCE_TRACE_MANIFEST_SCHEMA
            || !safe_symbol(&self.workload_id)
            || !sha256_identity(&self.adapter_identity_hash)
            || !sha256_identity(&self.driver_identity_hash)
            || !sha256_identity(&self.report_hash)
            || !sha256_identity(&self.trace_hash)
            || self.event_count == 0
            || self.byte_length == 0
            || self.dropped_event_count != 0
            || self.truncated
            || !self.timestamps_monotonic
        {
            return Err(PerformanceError::InvalidReport(
                "performance trace identity, bounds, or continuity is invalid".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerformanceThresholds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_p50: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_p95: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_p50: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_p95: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_p99: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerformanceMetricBudget {
    pub id: String,
    pub unit: PerformanceUnit,
    pub min_samples: usize,
    pub max_samples: usize,
    pub thresholds: PerformanceThresholds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerformanceBudget {
    pub schema: String,
    pub budget_id: String,
    pub target: String,
    pub profile: String,
    pub profile_hash: String,
    pub min_run_duration_us: u64,
    pub metrics: Vec<PerformanceMetricBudget>,
}

impl PerformanceBudget {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        if self.schema != PERFORMANCE_BUDGET_SCHEMA {
            return Err(PerformanceError::InvalidBudget(
                "performance budget schema is unsupported".into(),
            ));
        }
        if !safe_symbol(&self.budget_id)
            || !safe_symbol(&self.target)
            || !safe_symbol(&self.profile)
            || !sha256_identity(&self.profile_hash)
            || self.min_run_duration_us == 0
            || self.metrics.is_empty()
        {
            return Err(PerformanceError::InvalidBudget(
                "budget identity, run duration, or metric set is invalid".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        for metric in &self.metrics {
            if !safe_metric_id(&metric.id)
                || !ids.insert(metric.id.clone())
                || metric.min_samples == 0
                || metric.max_samples < metric.min_samples
            {
                return Err(PerformanceError::InvalidBudget(
                    "metric identity or sample limits are invalid".into(),
                ));
            }
            validate_thresholds(&metric.thresholds)?;
        }
        Ok(())
    }

    pub fn hash(&self) -> Result<Hash256, PerformanceError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| {
            PerformanceError::InvalidBudget("budget could not be serialized".into())
        })?;
        Ok(Hash256::from_sha256(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerformanceRunIdentity {
    pub source_revision: String,
    pub dirty: bool,
    pub target: String,
    pub profile: String,
    pub profile_hash: String,
    pub package_hash: String,
    pub build_fingerprint: String,
    pub session_id: String,
}

impl PerformanceRunIdentity {
    pub fn validate(&self) -> Result<(), PerformanceError> {
        if !git_revision(&self.source_revision)
            || !safe_symbol(&self.target)
            || !safe_symbol(&self.profile)
            || !sha256_identity(&self.profile_hash)
            || !sha256_identity(&self.package_hash)
            || !sha256_identity(&self.build_fingerprint)
            || !safe_symbol(&self.session_id)
        {
            return Err(PerformanceError::InvalidIdentity(
                "performance run identity is incomplete or unsafe".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerformanceMetricSummary {
    pub id: String,
    pub unit: PerformanceUnit,
    pub sample_count: usize,
    pub min: u64,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerformanceReport {
    pub schema: String,
    pub status: PerformanceStatus,
    pub budget_id: String,
    pub budget_hash: String,
    pub identity: PerformanceRunIdentity,
    pub run_duration_us: u64,
    pub metrics: Vec<PerformanceMetricSummary>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct PerformanceRecorder {
    budget: PerformanceBudget,
    samples: BTreeMap<String, Vec<u64>>,
}

impl PerformanceRecorder {
    pub fn new(budget: PerformanceBudget) -> Result<Self, PerformanceError> {
        budget.validate()?;
        let samples = budget
            .metrics
            .iter()
            .map(|metric| (metric.id.clone(), Vec::new()))
            .collect();
        Ok(Self { budget, samples })
    }

    pub fn budget(&self) -> &PerformanceBudget {
        &self.budget
    }

    pub fn record(&mut self, metric_id: &str, value: u64) -> Result<(), PerformanceError> {
        let budget = self
            .budget
            .metrics
            .iter()
            .find(|metric| metric.id == metric_id)
            .ok_or_else(|| {
                PerformanceError::InvalidMetric("sample references an undeclared metric".into())
            })?;
        let samples = self.samples.get_mut(metric_id).ok_or_else(|| {
            PerformanceError::InvalidMetric("metric recorder state is missing".into())
        })?;
        if samples.len() == budget.max_samples {
            return Err(PerformanceError::SampleBudget(
                "metric exceeded its bounded sample capacity".into(),
            ));
        }
        samples.push(value);
        Ok(())
    }

    pub fn finalize(
        self,
        identity: PerformanceRunIdentity,
        run_duration_us: u64,
    ) -> Result<PerformanceReport, PerformanceError> {
        identity.validate()?;
        if identity.target != self.budget.target
            || identity.profile != self.budget.profile
            || identity.profile_hash != self.budget.profile_hash
        {
            return Err(PerformanceError::InvalidIdentity(
                "run identity does not match its performance budget".into(),
            ));
        }
        let budget_hash = self.budget.hash()?.to_string();
        let mut metrics = Vec::with_capacity(self.budget.metrics.len());
        let mut diagnostics = Vec::new();
        if run_duration_us < self.budget.min_run_duration_us {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_PERFORMANCE_RUN_DURATION",
                "measured run is shorter than the profile-bound minimum duration",
            ));
        }
        for metric in &self.budget.metrics {
            let values = self.samples.get(&metric.id).ok_or_else(|| {
                PerformanceError::InvalidMetric("metric recorder state is missing".into())
            })?;
            if values.len() < metric.min_samples {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_PERFORMANCE_SAMPLE_COUNT",
                        "metric has fewer samples than its profile-bound minimum",
                    )
                    .with_field("metric", metric.id.clone()),
                );
                continue;
            }
            let summary = summarize(metric, values);
            append_threshold_diagnostics(metric, &summary, &mut diagnostics);
            metrics.push(summary);
        }
        metrics.sort_by(|left, right| left.id.cmp(&right.id));
        let status = if diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.severity,
                DiagnosticSeverity::Blocking | DiagnosticSeverity::Error
            )
        }) {
            PerformanceStatus::Blocked
        } else {
            PerformanceStatus::Pass
        };
        Ok(PerformanceReport {
            schema: PERFORMANCE_REPORT_SCHEMA.into(),
            status,
            budget_id: self.budget.budget_id,
            budget_hash,
            identity,
            run_duration_us,
            metrics,
            diagnostics,
        })
    }
}

pub fn validate_performance_report(
    budget: &PerformanceBudget,
    expected_identity: &PerformanceRunIdentity,
    report: &PerformanceReport,
) -> Result<(), PerformanceError> {
    budget.validate()?;
    expected_identity.validate()?;
    if report.schema != PERFORMANCE_REPORT_SCHEMA
        || report.status != PerformanceStatus::Pass
        || report.budget_id != budget.budget_id
        || report.budget_hash != budget.hash()?.to_string()
        || &report.identity != expected_identity
        || report.run_duration_us < budget.min_run_duration_us
        || report.metrics.len() != budget.metrics.len()
        || report.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.severity,
                DiagnosticSeverity::Blocking | DiagnosticSeverity::Error
            )
        })
    {
        return Err(PerformanceError::InvalidReport(
            "report identity, status, duration, or metric set is invalid".into(),
        ));
    }
    let summaries = report
        .metrics
        .iter()
        .map(|metric| (metric.id.as_str(), metric))
        .collect::<BTreeMap<_, _>>();
    if summaries.len() != report.metrics.len() {
        return Err(PerformanceError::InvalidReport(
            "report contains duplicate metric summaries".into(),
        ));
    }
    for metric in &budget.metrics {
        let summary = summaries.get(metric.id.as_str()).ok_or_else(|| {
            PerformanceError::InvalidReport("required metric summary is missing".into())
        })?;
        if summary.unit != metric.unit
            || summary.sample_count < metric.min_samples
            || summary.sample_count > metric.max_samples
            || summary.min > summary.p50
            || summary.p50 > summary.p95
            || summary.p95 > summary.p99
            || summary.p99 > summary.max
        {
            return Err(PerformanceError::InvalidReport(
                "metric summary shape is invalid".into(),
            ));
        }
        let mut diagnostics = Vec::new();
        append_threshold_diagnostics(metric, summary, &mut diagnostics);
        if !diagnostics.is_empty() {
            return Err(PerformanceError::InvalidReport(
                "metric summary exceeds its profile-bound threshold".into(),
            ));
        }
    }
    Ok(())
}

fn validate_thresholds(thresholds: &PerformanceThresholds) -> Result<(), PerformanceError> {
    let upper = [
        thresholds.max_p50,
        thresholds.max_p95,
        thresholds.max_p99,
        thresholds.max,
    ];
    if upper.iter().all(Option::is_none)
        && thresholds.min_p50.is_none()
        && thresholds.min_p95.is_none()
    {
        return Err(PerformanceError::InvalidBudget(
            "metric must declare at least one threshold".into(),
        ));
    }
    let present = upper.into_iter().flatten().collect::<Vec<_>>();
    if present.windows(2).any(|pair| pair[0] > pair[1])
        || matches!((thresholds.min_p50, thresholds.min_p95), (Some(p50), Some(p95)) if p50 > p95)
    {
        return Err(PerformanceError::InvalidBudget(
            "percentile thresholds are not monotonic".into(),
        ));
    }
    Ok(())
}

fn summarize(metric: &PerformanceMetricBudget, values: &[u64]) -> PerformanceMetricSummary {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    PerformanceMetricSummary {
        id: metric.id.clone(),
        unit: metric.unit,
        sample_count: sorted.len(),
        min: sorted[0],
        p50: percentile(&sorted, 50),
        p95: percentile(&sorted, 95),
        p99: percentile(&sorted, 99),
        max: *sorted.last().expect("non-empty samples were validated"),
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn append_threshold_diagnostics(
    budget: &PerformanceMetricBudget,
    summary: &PerformanceMetricSummary,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let thresholds = &budget.thresholds;
    let below = thresholds.min_p50.is_some_and(|value| summary.p50 < value)
        || thresholds.min_p95.is_some_and(|value| summary.p95 < value);
    let above = thresholds.max_p50.is_some_and(|value| summary.p50 > value)
        || thresholds.max_p95.is_some_and(|value| summary.p95 > value)
        || thresholds.max_p99.is_some_and(|value| summary.p99 > value)
        || thresholds.max.is_some_and(|value| summary.max > value);
    if below || above {
        diagnostics.push(
            Diagnostic::blocking(
                "ASTRA_PERFORMANCE_THRESHOLD",
                "measured metric violates its profile-bound threshold",
            )
            .with_field("metric", budget.id.clone()),
        );
    }
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_metric_id(value: &str) -> bool {
    safe_symbol(value) && value.contains('.')
}

fn sha256_identity(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_revision(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
