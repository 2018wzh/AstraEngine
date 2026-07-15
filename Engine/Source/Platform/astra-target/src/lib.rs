use std::collections::BTreeSet;

use astra_core::{Diagnostic, DiagnosticSeverity};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const TARGET_MANIFEST_SCHEMA: &str = "astra.target_manifest.v2";
pub const TARGET_VALIDATION_SCHEMA: &str = "astra.target_validation_report.v1";

#[derive(Debug, Error)]
pub enum TargetError {
    #[error("{0}")]
    Message(String),
    #[error("target descriptor parse failed: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Editor,
    Game,
    Program,
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TargetDescriptor {
    pub id: String,
    pub kind: TargetKind,
    #[serde(default, rename = "crate", skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub packaged: bool,
}

impl TargetDescriptor {
    pub fn default_game(project_id: impl Into<String>) -> Self {
        let project_id = project_id.into();
        Self {
            id: format!("{}-game", normalize_id(&project_id)),
            kind: TargetKind::Game,
            crate_name: Some("astra-runtime".to_string()),
            runtime_provider: Some("native_vn".to_string()),
            ui_provider: Some("astra.ui.yakui".to_string()),
            binary: None,
            default_profile: Some("desktop-release".to_string()),
            platforms: vec![
                "windows".to_string(),
                "linux".to_string(),
                "macos".to_string(),
                "ios".to_string(),
                "android".to_string(),
                "web".to_string(),
            ],
            packaged: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TargetManifest {
    pub schema: String,
    pub targets: Vec<TargetDescriptor>,
    #[serde(default, skip)]
    pub legacy_runtime_field: bool,
}

impl TargetManifest {
    pub fn new(targets: Vec<TargetDescriptor>) -> Self {
        Self {
            schema: TARGET_MANIFEST_SCHEMA.to_string(),
            targets,
            legacy_runtime_field: false,
        }
    }

    pub fn with_legacy_runtime_field(mut self, legacy_runtime_field: bool) -> Self {
        self.legacy_runtime_field = legacy_runtime_field;
        self
    }

    pub fn from_project_yaml(text: &str) -> Result<Self, TargetError> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|err| TargetError::Parse(err.to_string()))?;
        Self::from_project_value(&value)
    }

    pub fn from_project_value(value: &serde_yaml::Value) -> Result<Self, TargetError> {
        let legacy_runtime_field = value.get("runtime").is_some();
        if let Some(targets) = value.get("targets") {
            let targets: Vec<TargetDescriptor> = serde_yaml::from_value(targets.clone())
                .map_err(|err| TargetError::Parse(err.to_string()))?;
            return Ok(Self::new(targets).with_legacy_runtime_field(legacy_runtime_field));
        }

        let project_id = value
            .get("id")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or("com.example.astra");
        Ok(Self::new(vec![TargetDescriptor::default_game(project_id)])
            .with_legacy_runtime_field(legacy_runtime_field))
    }

    pub fn find(&self, id: &str) -> Option<&TargetDescriptor> {
        self.targets.iter().find(|target| target.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetValidationStatus {
    Pass,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TargetValidationReport {
    pub schema: String,
    pub status: TargetValidationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_target: Option<String>,
    pub target_count: usize,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

pub fn validate_manifest(
    manifest: &TargetManifest,
    selected: Option<&str>,
) -> TargetValidationReport {
    tracing::debug!(
        event = "target.validate.start",
        target_count = manifest.targets.len(),
        has_selected_target = selected.is_some(),
        "target validation started"
    );
    let mut diagnostics = Vec::new();
    if manifest.schema != TARGET_MANIFEST_SCHEMA {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_TARGET_SCHEMA",
            "target manifest schema must be astra.target_manifest.v2",
        ));
    }
    if manifest.legacy_runtime_field {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_TARGET_LEGACY_RUNTIME_FIELD",
            "top-level runtime is removed; game targets must declare runtime_provider",
        ));
    }
    if manifest.targets.is_empty() {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_TARGET_EMPTY",
            "target manifest must contain at least one target",
        ));
    }

    let mut ids = BTreeSet::new();
    for target in &manifest.targets {
        validate_target(target, &mut diagnostics);
        if !ids.insert(target.id.clone()) {
            diagnostics.push(
                Diagnostic::blocking("ASTRA_TARGET_DUPLICATE", "target id is duplicated")
                    .with_field("target", &target.id),
            );
        }
    }

    if let Some(selected) = selected {
        if manifest.find(selected).is_none() {
            diagnostics.push(
                Diagnostic::blocking("ASTRA_TARGET_NOT_FOUND", "selected target is not defined")
                    .with_field("target", selected),
            );
        }
    }

    let status = if diagnostics.iter().any(|diag| {
        matches!(
            diag.severity,
            DiagnosticSeverity::Blocking | DiagnosticSeverity::Error
        )
    }) {
        TargetValidationStatus::Blocked
    } else if diagnostics
        .iter()
        .any(|diag| diag.severity == DiagnosticSeverity::Warning)
    {
        TargetValidationStatus::Warning
    } else {
        TargetValidationStatus::Pass
    };

    let report = TargetValidationReport {
        schema: TARGET_VALIDATION_SCHEMA.to_string(),
        status,
        selected_target: selected.map(str::to_string),
        target_count: manifest.targets.len(),
        diagnostics,
    };
    match report.status {
        TargetValidationStatus::Pass => tracing::info!(
            event = "target.validate.complete",
            status = "pass",
            target_count = report.target_count,
            diagnostic_count = report.diagnostics.len(),
            "target validation completed"
        ),
        TargetValidationStatus::Warning => tracing::warn!(
            event = "target.validate.complete",
            status = "warning",
            target_count = report.target_count,
            diagnostic_count = report.diagnostics.len(),
            "target validation completed with warnings"
        ),
        TargetValidationStatus::Blocked => tracing::error!(
            event = "target.validate.complete",
            status = "blocked",
            target_count = report.target_count,
            diagnostic_count = report.diagnostics.len(),
            "target validation blocked"
        ),
    }
    report
}

fn validate_target(target: &TargetDescriptor, diagnostics: &mut Vec<Diagnostic>) {
    if target.id.trim().is_empty() {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_TARGET_ID",
            "target id must not be empty",
        ));
    }
    if target
        .platforms
        .iter()
        .any(|platform| platform.trim().is_empty())
    {
        diagnostics.push(
            Diagnostic::blocking("ASTRA_TARGET_PLATFORM", "platform id must not be empty")
                .with_field("target", &target.id),
        );
    }

    match target.kind {
        TargetKind::Game => {
            if !target.packaged {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_TARGET_GAME_PACKAGE",
                        "game target must be packaged",
                    )
                    .with_field("target", &target.id),
                );
            }
            if target.default_profile.is_none() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_TARGET_PROFILE",
                        "game target needs a default profile",
                    )
                    .with_field("target", &target.id),
                );
            }
            if target.runtime_provider.as_deref().is_none_or(str::is_empty) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_TARGET_RUNTIME_PROVIDER",
                        "game target needs an explicit runtime_provider",
                    )
                    .with_field("target", &target.id),
                );
            }
            if target.runtime_provider.as_deref() == Some("native_vn")
                && target.ui_provider.as_deref() != Some("astra.ui.yakui")
            {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_TARGET_UI_PROVIDER",
                        "native_vn game target requires the unique ui_provider astra.ui.yakui",
                    )
                    .with_field("target", &target.id),
                );
            }
            if target.platforms.is_empty() {
                diagnostics.push(
                    Diagnostic::blocking("ASTRA_TARGET_PLATFORMS", "game target needs platforms")
                        .with_field("target", &target.id),
                );
            }
        }
        TargetKind::Editor => {
            if target.packaged {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_TARGET_EDITOR_PACKAGE",
                        "editor target must not be packaged runtime",
                    )
                    .with_field("target", &target.id),
                );
            }
            if target.binary.is_none() {
                diagnostics.push(
                    Diagnostic::blocking("ASTRA_TARGET_BINARY", "editor target needs a binary")
                        .with_field("target", &target.id),
                );
            }
        }
        TargetKind::Program => {
            if target.binary.is_none() {
                diagnostics.push(
                    Diagnostic::blocking("ASTRA_TARGET_BINARY", "program target needs a binary")
                        .with_field("target", &target.id),
                );
            }
        }
        TargetKind::Client | TargetKind::Server => diagnostics.push(
            Diagnostic::warning(
                "ASTRA_TARGET_KIND_RESERVED",
                "client and server targets are reserved schema values for a later network stage",
            )
            .with_field("target", &target.id),
        ),
    }
}

fn normalize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_project_targets_and_reserved_kinds() {
        let manifest = TargetManifest::from_project_yaml(
            r#"
schema: astra.project.v1
id: com.example.nativevn
targets:
  - id: nativevn-game
    kind: game
    crate: astra-vn
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    default_profile: desktop-release
    platforms: [windows, linux, macos, ios, android, web]
    packaged: true
  - id: nativevn-server
    kind: server
"#,
        )
        .unwrap();
        let report = validate_manifest(&manifest, Some("nativevn-game"));
        assert_eq!(report.status, TargetValidationStatus::Warning);
        assert_eq!(report.target_count, 2);
        assert!(report
            .diagnostics
            .iter()
            .any(|diag| diag.code == "ASTRA_TARGET_KIND_RESERVED"));
    }

    #[test]
    fn blocks_invalid_game_target() {
        let manifest = TargetManifest::new(vec![TargetDescriptor {
            id: "bad".to_string(),
            kind: TargetKind::Game,
            crate_name: None,
            runtime_provider: None,
            ui_provider: None,
            binary: None,
            default_profile: None,
            platforms: Vec::new(),
            packaged: false,
        }]);
        let report = validate_manifest(&manifest, Some("missing"));
        assert_eq!(report.status, TargetValidationStatus::Blocked);
        assert!(report
            .diagnostics
            .iter()
            .any(|diag| diag.code == "ASTRA_TARGET_NOT_FOUND"));
        assert!(report
            .diagnostics
            .iter()
            .any(|diag| diag.code == "ASTRA_TARGET_RUNTIME_PROVIDER"));
    }

    #[test]
    fn blocks_legacy_top_level_runtime_field_for_game_targets() {
        let manifest = TargetManifest::from_project_yaml(
            r#"
schema: astra.project.v1
id: com.example.nativevn
runtime: astra-vn
targets:
  - id: nativevn-game
    kind: game
    crate: astra-vn
    default_profile: desktop-release
    platforms: [windows]
    packaged: true
"#,
        )
        .unwrap();
        let report = validate_manifest(&manifest, Some("nativevn-game"));
        assert_eq!(report.status, TargetValidationStatus::Blocked);
        assert!(report
            .diagnostics
            .iter()
            .any(|diag| diag.code == "ASTRA_TARGET_LEGACY_RUNTIME_FIELD"));
    }
}
