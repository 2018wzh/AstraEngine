use astra_core::{Diagnostic, Hash256};
use astra_package::{PackageManifest, PackageReader};
use astra_platform::{PlatformCapabilityReport, PlatformValidationStatus};
use astra_target::{validate_manifest, TargetKind, TargetManifest, TargetValidationStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageValidateRequest {
    pub package_bytes: Vec<u8>,
    pub profile: String,
    pub require_ffmpeg: bool,
    pub target: Option<String>,
    pub require_platform_report: bool,
    pub platform_report: Option<PlatformCapabilityReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReleaseReport {
    pub schema: String,
    pub package_id: String,
    pub profile: String,
    pub status: CheckStatus,
    pub package_hash: String,
    pub checks: Vec<ReleaseCheckRecord>,
}

impl ReleaseReport {
    pub fn explain(&self) -> String {
        let mut lines = vec![format!(
            "release report {} for {} [{}]: {:?}",
            self.schema, self.package_id, self.profile, self.status
        )];
        for check in &self.checks {
            lines.push(format!(
                "- {} ({:?}): {:?} - {}",
                check.id, check.domain, check.status, check.summary
            ));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReleaseCheckRecord {
    pub id: String,
    pub domain: ReleaseDomain,
    pub status: CheckStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Diagnostic>,
    #[serde(default)]
    pub evidence: Vec<ReleaseEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseDomain {
    Runtime,
    Target,
    Plugin,
    Package,
    Media,
    Scenario,
    Platform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReleaseEvidence {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct ReleaseValidator;

impl ReleaseValidator {
    pub fn validate_package(
        &self,
        request: PackageValidateRequest,
    ) -> Result<ReleaseReport, ReleaseError> {
        let package_hash = Hash256::from_sha256(&request.package_bytes).to_string();
        let mut checks = Vec::new();
        let mut package_id = "unknown".to_string();

        match PackageReader::open(&request.package_bytes) {
            Ok(package) => {
                let package_manifest = package
                    .container()
                    .decode_postcard::<PackageManifest>("package.manifest")
                    .ok();
                if let Some(manifest) = &package_manifest {
                    package_id = manifest.package_id.clone();
                }
                let section_count = package.container().entries().len();
                checks.push(ReleaseCheckRecord {
                    id: "package.integrity".to_string(),
                    domain: ReleaseDomain::Package,
                    status: CheckStatus::Pass,
                    summary: "container footer, section bounds and hashes verified".to_string(),
                    diagnostic: None,
                    evidence: vec![
                        evidence("section_count", section_count),
                        evidence("package_hash", &package_hash),
                    ],
                });
                for section in [
                    "schema.registry",
                    "asset.registry",
                    "media.manifest",
                    "provider.policy",
                    "scenario.refs",
                    "target.manifest",
                    "platform.eligibility",
                ] {
                    checks.push(section_check(&package, section));
                }
                checks.push(plugin_extension_registry_check(&package));
                checks.push(plugin_dependency_graph_check(&package));
                checks.push(target_manifest_check(&package, request.target.as_deref()));
                checks.push(cooked_project_input_check(
                    &package,
                    &request.profile,
                    package_manifest.as_ref(),
                ));
                checks.push(media_check(request.require_ffmpeg));
                checks.push(platform_report_check(
                    request.platform_report.as_ref(),
                    request.require_platform_report,
                ));
                checks.push(ReleaseCheckRecord {
                    id: "scenario.refs".to_string(),
                    domain: ReleaseDomain::Scenario,
                    status: CheckStatus::Pass,
                    summary: "scenario refs section is present".to_string(),
                    diagnostic: None,
                    evidence: vec![evidence("section", "scenario.refs")],
                });
            }
            Err(err) => {
                checks.push(ReleaseCheckRecord {
                    id: "package.integrity".to_string(),
                    domain: ReleaseDomain::Package,
                    status: CheckStatus::Blocked,
                    summary: "package container could not be opened".to_string(),
                    diagnostic: Some(Diagnostic::blocking(
                        "ASTRA_PACKAGE_INTEGRITY",
                        err.to_string(),
                    )),
                    evidence: vec![evidence("package_hash", &package_hash)],
                });
                checks.push(media_check(request.require_ffmpeg));
                checks.push(platform_report_check(
                    request.platform_report.as_ref(),
                    request.require_platform_report,
                ));
            }
        }

        let status = if checks
            .iter()
            .any(|check| check.status == CheckStatus::Blocked)
        {
            CheckStatus::Blocked
        } else if checks
            .iter()
            .any(|check| check.status == CheckStatus::Warning)
        {
            CheckStatus::Warning
        } else {
            CheckStatus::Pass
        };

        Ok(ReleaseReport {
            schema: "astra.release_report.v1".to_string(),
            package_id,
            profile: request.profile,
            status,
            package_hash,
            checks,
        })
    }
}

fn section_check(package: &PackageReader, section: &str) -> ReleaseCheckRecord {
    if package.has_section(section) {
        ReleaseCheckRecord {
            id: format!("{section}.present"),
            domain: match section {
                "media.manifest" => ReleaseDomain::Media,
                "target.manifest" => ReleaseDomain::Target,
                "platform.eligibility" => ReleaseDomain::Platform,
                "provider.policy" => ReleaseDomain::Plugin,
                _ => ReleaseDomain::Package,
            },
            status: CheckStatus::Pass,
            summary: format!("{section} section is present"),
            diagnostic: None,
            evidence: vec![evidence("section", section)],
        }
    } else {
        ReleaseCheckRecord {
            id: format!("{section}.present"),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: format!("{section} section is missing"),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PACKAGE_SECTION_MISSING",
                format!("missing package section {section}"),
            )),
            evidence: vec![evidence("section", section)],
        }
    }
}

fn plugin_extension_registry_check(package: &PackageReader) -> ReleaseCheckRecord {
    let registry = match read_json_section(package, "plugin.extension_registry") {
        Ok(value) => value,
        Err((code, message)) => {
            return plugin_blocked(
                "plugin.extension_registry",
                code,
                message,
                vec![evidence("section", "plugin.extension_registry")],
            )
        }
    };
    let provider_policy = match read_json_section(package, "provider.policy") {
        Ok(value) => value,
        Err((code, message)) => {
            return plugin_blocked(
                "plugin.extension_registry",
                code,
                message,
                vec![evidence("section", "provider.policy")],
            )
        }
    };
    let providers = registry
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let bindings = registry
        .get("bindings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let conflicts = registry
        .get("conflicts")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    if !conflicts.is_empty() {
        return plugin_blocked(
            "plugin.extension_registry",
            "ASTRA_PLUGIN_EXTENSION_CONFLICT",
            "plugin extension registry contains unresolved conflicts",
            vec![evidence("conflict_count", conflicts.len())],
        );
    }

    for binding in bindings.iter().chain(
        provider_policy
            .get("bindings")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten(),
    ) {
        let provider_id = binding
            .get("provider_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let Some(provider) = providers.iter().find(|provider| {
            provider
                .get("provider_id")
                .and_then(serde_json::Value::as_str)
                == Some(provider_id)
        }) else {
            return plugin_blocked(
                "plugin.extension_registry",
                "ASTRA_PLUGIN_PROVIDER_BINDING_MISSING",
                format!("provider binding {provider_id} is not registered"),
                vec![evidence("provider_id", provider_id)],
            );
        };
        if !provider
            .get("packaged")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return plugin_blocked(
                "plugin.extension_registry",
                "ASTRA_PLUGIN_PACKAGED_INELIGIBLE",
                format!("provider binding {provider_id} is not packaged eligible"),
                vec![evidence("provider_id", provider_id)],
            );
        }
    }

    ReleaseCheckRecord {
        id: "plugin.extension_registry".to_string(),
        domain: ReleaseDomain::Plugin,
        status: CheckStatus::Pass,
        summary: "plugin extension registry has resolved bindings and no conflicts".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("provider_count", providers.len()),
            evidence("binding_count", bindings.len()),
        ],
    }
}

fn plugin_dependency_graph_check(package: &PackageReader) -> ReleaseCheckRecord {
    let graph = match read_json_section(package, "plugin.dependency_graph") {
        Ok(value) => value,
        Err((code, message)) => {
            return plugin_blocked(
                "plugin.dependency_graph",
                code,
                message,
                vec![evidence("section", "plugin.dependency_graph")],
            )
        }
    };
    let dependencies = graph
        .get("dependencies")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(dependency) = dependencies.iter().find(|dependency| {
        dependency
            .get("required")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            && !dependency
                .get("resolved")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
    }) {
        let plugin_id = dependency
            .get("plugin_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        return plugin_blocked(
            "plugin.dependency_graph",
            "ASTRA_PLUGIN_DEPENDENCY_UNRESOLVED",
            format!("required plugin dependency {plugin_id} is unresolved"),
            vec![evidence("plugin_id", plugin_id)],
        );
    }

    ReleaseCheckRecord {
        id: "plugin.dependency_graph".to_string(),
        domain: ReleaseDomain::Plugin,
        status: CheckStatus::Pass,
        summary: "plugin dependency graph has no unresolved required dependencies".to_string(),
        diagnostic: None,
        evidence: vec![evidence("dependency_count", dependencies.len())],
    }
}

fn read_json_section(
    package: &PackageReader,
    section: &str,
) -> Result<serde_json::Value, (&'static str, String)> {
    let bytes = package
        .container()
        .read_bounded(section, 256 * 1024)
        .map_err(|err| ("ASTRA_PLUGIN_SECTION_MISSING", err.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|err| ("ASTRA_PLUGIN_SECTION_JSON", err.to_string()))
}

fn plugin_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Plugin,
        status: CheckStatus::Blocked,
        diagnostic: Some(Diagnostic::blocking(code, summary.clone())),
        summary,
        evidence: evidence_values,
    }
}

fn target_manifest_check(package: &PackageReader, selected: Option<&str>) -> ReleaseCheckRecord {
    let bytes = match package
        .container()
        .read_bounded("target.manifest", 256 * 1024)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "target.manifest".to_string(),
                domain: ReleaseDomain::Target,
                status: CheckStatus::Blocked,
                summary: "target manifest section could not be read".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_TARGET_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "target.manifest")],
            };
        }
    };
    let manifest: TargetManifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "target.manifest".to_string(),
                domain: ReleaseDomain::Target,
                status: CheckStatus::Blocked,
                summary: "target manifest is not valid JSON".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_TARGET_MANIFEST_JSON",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "target.manifest")],
            };
        }
    };
    let report = validate_manifest(&manifest, selected);
    let package_shape_diagnostic = package_target_shape_diagnostic(&manifest);
    let status = if package_shape_diagnostic.is_some() {
        CheckStatus::Blocked
    } else {
        match report.status {
            TargetValidationStatus::Pass => CheckStatus::Pass,
            TargetValidationStatus::Warning => CheckStatus::Warning,
            TargetValidationStatus::Blocked => CheckStatus::Blocked,
        }
    };
    ReleaseCheckRecord {
        id: "target.manifest".to_string(),
        domain: ReleaseDomain::Target,
        status,
        summary: "target manifest contains one packaged Game target".to_string(),
        diagnostic: package_shape_diagnostic.or_else(|| {
            report
                .diagnostics
                .iter()
                .find(|diagnostic| {
                    matches!(
                        diagnostic.severity,
                        astra_core::DiagnosticSeverity::Blocking
                            | astra_core::DiagnosticSeverity::Error
                    )
                })
                .cloned()
        }),
        evidence: vec![
            evidence("target_count", report.target_count),
            evidence("selected_target", selected.unwrap_or("")),
        ],
    }
}

fn cooked_project_input_check(
    package: &PackageReader,
    profile: &str,
    manifest: Option<&PackageManifest>,
) -> ReleaseCheckRecord {
    let required = release_profile_requires_cooked_project(profile);
    let Some(entry) = package.container().section_entry("compiled.project") else {
        return if required {
            ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "release package must include a cooked project artifact".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_MISSING",
                    "release profile package validation requires compiled.project from cook/project input",
                )),
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("profile", profile),
                ],
            }
        } else {
            ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Pass,
                summary: "profile does not require a cooked project artifact".to_string(),
                diagnostic: None,
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("profile", profile),
                    evidence("required", required),
                ],
            }
        };
    };

    if entry.schema != "astra.cooked_project.v1" {
        return ReleaseCheckRecord {
            id: "package.cooked_project".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: "compiled.project section uses an unexpected schema".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PACKAGE_COOKED_PROJECT_SCHEMA",
                format!(
                    "compiled.project schema must be astra.cooked_project.v1, got {}",
                    entry.schema
                ),
            )),
            evidence: vec![
                evidence("section", "compiled.project"),
                evidence("schema", &entry.schema),
            ],
        };
    }

    let bytes = match package
        .container()
        .read_bounded("compiled.project", 256 * 1024)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project section could not be read".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_READ",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "compiled.project")],
            };
        }
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project section is not valid JSON".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_JSON",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "compiled.project")],
            };
        }
    };
    for field in ["schema", "package_id", "profile", "target", "project_hash"] {
        if value
            .get(field)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .is_empty()
        {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project section is missing required cook metadata".to_string(),
                diagnostic: Some(
                    Diagnostic::blocking(
                        "ASTRA_PACKAGE_COOKED_PROJECT_METADATA",
                        "compiled.project must record schema, package_id, profile, target and project_hash",
                    )
                    .with_field("field", field),
                ),
                evidence: vec![evidence("section", "compiled.project")],
            };
        }
    }
    if value.get("schema").and_then(serde_json::Value::as_str) != Some("astra.cooked_project.v1") {
        return ReleaseCheckRecord {
            id: "package.cooked_project".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: "compiled.project payload schema is invalid".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PACKAGE_COOKED_PROJECT_PAYLOAD_SCHEMA",
                "compiled.project payload schema must be astra.cooked_project.v1",
            )),
            evidence: vec![evidence("section", "compiled.project")],
        };
    }

    if let Some(manifest) = manifest {
        if manifest.profile != profile {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "validation profile does not match package manifest".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_PROFILE_MISMATCH",
                    "release validation profile must match package.manifest profile",
                )),
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("package_profile", &manifest.profile),
                    evidence("validation_profile", profile),
                ],
            };
        }
        let project_package_id = value
            .get("package_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let project_profile = value
            .get("profile")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if project_package_id != manifest.package_id || project_profile != manifest.profile {
            return ReleaseCheckRecord {
                id: "package.cooked_project".to_string(),
                domain: ReleaseDomain::Package,
                status: CheckStatus::Blocked,
                summary: "compiled.project metadata does not match package manifest".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_PACKAGE_COOKED_PROJECT_MISMATCH",
                    "compiled.project package_id and profile must match package.manifest",
                )),
                evidence: vec![
                    evidence("section", "compiled.project"),
                    evidence("package_profile", &manifest.profile),
                    evidence("cooked_profile", project_profile),
                ],
            };
        }
    }

    ReleaseCheckRecord {
        id: "package.cooked_project".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "package includes a cooked project artifact from cook/project input".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "compiled.project"),
            evidence("schema", "astra.cooked_project.v1"),
            evidence(
                "target",
                value
                    .get("target")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            ),
            evidence("profile", profile),
        ],
    }
}

fn release_profile_requires_cooked_project(profile: &str) -> bool {
    matches!(profile, "release" | "desktop-release" | "web-release")
        || profile.ends_with("-release")
}

fn package_target_shape_diagnostic(manifest: &TargetManifest) -> Option<Diagnostic> {
    if manifest.targets.len() != 1 {
        return Some(Diagnostic::blocking(
            "ASTRA_TARGET_PACKAGE_SHAPE",
            "package target manifest must contain exactly one target",
        ));
    }
    let target = &manifest.targets[0];
    if target.kind != TargetKind::Game || !target.packaged {
        return Some(
            Diagnostic::blocking(
                "ASTRA_TARGET_PACKAGE_GAME",
                "package target manifest must contain one packaged game target",
            )
            .with_field("target", &target.id),
        );
    }
    None
}

fn media_check(require_ffmpeg: bool) -> ReleaseCheckRecord {
    let symphonia = astra_media::SymphoniaAudioDecodeProvider;
    let symphonia_available = symphonia.probe_available();
    if require_ffmpeg {
        ReleaseCheckRecord {
            id: "media.decode.ffmpeg".to_string(),
            domain: ReleaseDomain::Media,
            status: CheckStatus::Blocked,
            summary: "desktop-release requires FFmpeg feature but this build did not enable it"
                .to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_MEDIA_FFMPEG_REQUIRED",
                "FFmpeg decode fallback is feature-gated",
            )),
            evidence: vec![evidence("symphonia_available", symphonia_available)],
        }
    } else {
        ReleaseCheckRecord {
            id: "media.decode.platform_fallback".to_string(),
            domain: ReleaseDomain::Media,
            status: CheckStatus::Warning,
            summary:
                "FFmpeg fallback is optional for this profile; platform decode remains preferred"
                    .to_string(),
            diagnostic: Some(Diagnostic::warning(
                "ASTRA_MEDIA_FFMPEG_OPTIONAL",
                "FFmpeg feature is not required for this validation profile",
            )),
            evidence: vec![evidence("symphonia_available", symphonia_available)],
        }
    }
}

fn platform_report_check(
    report: Option<&PlatformCapabilityReport>,
    require_report: bool,
) -> ReleaseCheckRecord {
    let Some(report) = report else {
        return ReleaseCheckRecord {
            id: "platform.capability_report".to_string(),
            domain: ReleaseDomain::Platform,
            status: if require_report {
                CheckStatus::Blocked
            } else {
                CheckStatus::Warning
            },
            summary: "platform capability report was not supplied".to_string(),
            diagnostic: Some(if require_report {
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_REPORT_MISSING",
                    "platform probe evidence is required for this release profile",
                )
            } else {
                Diagnostic::warning(
                    "ASTRA_PLATFORM_REPORT_MISSING",
                    "platform probe evidence is required before native platform completion",
                )
            }),
            evidence: vec![],
        };
    };

    let (status, diagnostics) = astra_platform::validate_capability_report(report);
    let mut evidence_values = vec![
        evidence("platform", report.platform),
        evidence(
            "sdk_status",
            format!("{:?}", report.sdk_status).to_lowercase(),
        ),
        evidence("smoke_count", report.smoke.len()),
    ];
    for check in &report.smoke {
        evidence_values.push(evidence(
            format!("smoke.{}.status", check.id),
            format!("{:?}", check.status).to_lowercase(),
        ));
        for entry in &check.evidence {
            evidence_values.push(evidence(
                format!("smoke.{}.{}", check.id, entry.key),
                &entry.value,
            ));
        }
    }

    ReleaseCheckRecord {
        id: "platform.capability_report".to_string(),
        domain: ReleaseDomain::Platform,
        status: match status {
            PlatformValidationStatus::Pass => CheckStatus::Pass,
            PlatformValidationStatus::Warning => CheckStatus::Warning,
            PlatformValidationStatus::Blocked => CheckStatus::Blocked,
        },
        summary: "platform capability report matches the requested native SDK state".to_string(),
        diagnostic: diagnostics.first().cloned(),
        evidence: evidence_values,
    }
}

fn evidence(key: impl Into<String>, value: impl ToString) -> ReleaseEvidence {
    ReleaseEvidence {
        key: key.into(),
        value: value.to_string(),
    }
}
