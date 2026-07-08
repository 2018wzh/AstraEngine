use astra_core::{Diagnostic, Hash256};
use astra_package::{PackageManifest, PackageReader};
use astra_platform::{PlatformCapabilityReport, PlatformValidationStatus};
use astra_player_core::{PlayerAutomationReport, PlayerAutomationStatus};
use astra_target::{validate_manifest, TargetKind, TargetManifest, TargetValidationStatus};
use astra_vn::{
    CompiledStory, SystemStoryManifest, SystemStoryValidationStatus,
    VnAdvancedPresentationManifest, VnCommercialBaselineManifest, VnExtensionManifest,
    VnPolicyBundleManifest, VnPolicyBundleSourceCache, VnPresentationProviderManifest,
    VnProfileManifest, VnStandardCommandManifest, VnSystemUiProfileManifest,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const TSUI_REFERENCE_TITLE_HASH: &str =
    "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca";
const TSUI_REFERENCE_GAME_HASH: &str =
    "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84";
const TSUI_REFERENCE_TITLE_DIMENSIONS: (i64, i64) = (1386, 1040);
const TSUI_REFERENCE_GAME_DIMENSIONS: (i64, i64) = (1403, 1053);

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
    Player,
    Vn,
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
    pub fn validate_package_with_player_report(
        &self,
        request: PackageValidateRequest,
        player_report: Option<PlayerAutomationReport>,
    ) -> Result<ReleaseReport, ReleaseError> {
        let target = request.target.clone();
        let profile = request.profile.clone();
        let mut report = self.validate_package(request)?;
        if let Some(player_report) = player_report {
            report.checks.push(player_full_playable_check(
                &player_report,
                &report.package_hash,
                &profile,
                target.as_deref(),
            ));
            report.status = release_status(&report.checks);
        }
        Ok(report)
    }

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
                checks.extend(vn_checks(
                    &package,
                    &request.profile,
                    request.target.as_deref(),
                ));
                checks.extend(tsuinosora_checks(
                    &package,
                    &request.profile,
                    request.target.as_deref(),
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

        let status = release_status(&checks);

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

fn release_status(checks: &[ReleaseCheckRecord]) -> CheckStatus {
    if checks
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
    }
}

fn player_full_playable_check(
    report: &PlayerAutomationReport,
    package_hash: &str,
    profile: &str,
    target: Option<&str>,
) -> ReleaseCheckRecord {
    let target_matches = target.is_none_or(|target| report.target == target);
    if report.schema == "astra.player_automation_report.v1"
        && report.status == PlayerAutomationStatus::Pass
        && report.full_playable_passed()
        && report.package_hash == package_hash
        && report.profile == profile
        && target_matches
        && report.transcript_hash.starts_with("sha256:")
        && !report.route_coverage.is_empty()
    {
        ReleaseCheckRecord {
            id: "player.full_playable".to_string(),
            domain: ReleaseDomain::Player,
            status: CheckStatus::Pass,
            summary: "live player automation report proves full playable route coverage"
                .to_string(),
            diagnostic: None,
            evidence: vec![
                evidence("package_hash", package_hash),
                evidence("profile", profile),
                evidence("target", &report.target),
                evidence("transcript_hash", &report.transcript_hash),
                evidence("route_count", report.route_coverage.len()),
            ],
        }
    } else {
        ReleaseCheckRecord {
            id: "player.full_playable".to_string(),
            domain: ReleaseDomain::Player,
            status: CheckStatus::Blocked,
            summary: "live player automation report is missing or does not match package identity"
                .to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PLAYER_FULL_PLAYABLE_EVIDENCE",
                "player.full_playable requires a passing live automation report for this package",
            )),
            evidence: vec![
                evidence("expected_package_hash", package_hash),
                evidence("actual_package_hash", &report.package_hash),
                evidence("expected_profile", profile),
                evidence("actual_profile", &report.profile),
                evidence("route_count", report.route_coverage.len()),
            ],
        }
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

fn release_profile_requires_vn(profile: &str) -> bool {
    matches!(profile, "classic" | "modern")
        || VnAdvancedPresentationManifest::profile_requires_advanced(profile)
}

fn vn_checks(
    package: &PackageReader,
    profile: &str,
    selected_target: Option<&str>,
) -> Vec<ReleaseCheckRecord> {
    if !release_profile_requires_vn(profile)
        && !package.has_section("vn.compiled_story")
        && !package.has_section("vn.profile_manifest")
    {
        return Vec::new();
    }
    let mut checks = vec![
        vn_compiled_story_check(package, profile),
        vn_profile_manifest_check(package, profile, selected_target),
        vn_policy_bundle_check(package, profile),
        vn_extension_bindings_check(package, profile),
        vn_standard_commands_check(package, profile),
        vn_presentation_provider_check(package, profile),
        vn_commercial_baseline_check(package, profile),
        vn_system_ui_profile_check(package, profile),
    ];
    if VnAdvancedPresentationManifest::profile_requires_advanced(profile) {
        checks.push(vn_advanced_presentation_check(package, profile));
    }
    checks
}

fn vn_compiled_story_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let compiled = match package
        .container()
        .decode_postcard::<CompiledStory>("vn.compiled_story")
    {
        Ok(compiled) => compiled,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.compiled_story".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.compiled_story section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_COMPILED_STORY",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.compiled_story"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    if compiled.schema != "astra.vn.compiled_story.v1"
        || compiled.stories.is_empty()
        || compiled.states.is_empty()
    {
        return ReleaseCheckRecord {
            id: "vn.compiled_story".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "compiled VN story does not contain runnable story/state data".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_COMPILED_STORY_SHAPE",
                "compiled VN story needs schema, stories and states",
            )),
            evidence: vec![evidence("section", "vn.compiled_story")],
        };
    }
    if compiled.story_manifest.schema != "astra.vn.story_manifest.v1"
        || compiled.variable_manifest.schema != "astra.vn.variable_manifest.v1"
        || compiled.command_manifest.schema != "astra.vn.command_manifest.v1"
        || compiled.story_manifest.stories.is_empty()
        || compiled.command_manifest.commands.is_empty()
        || compiled
            .command_manifest
            .commands
            .iter()
            .any(|command| command.source.is_none())
    {
        return ReleaseCheckRecord {
            id: "vn.compiled_story".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "compiled VN story is missing manifest/source-map evidence".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_COMPILED_STORY_MANIFEST",
                "compiled VN story must include story, variable and command manifests with command source refs",
            )),
            evidence: vec![
                evidence("section", "vn.compiled_story"),
                evidence("story_manifest_count", compiled.story_manifest.stories.len()),
                evidence("variable_scope_count", compiled.variable_manifest.scopes.len()),
                evidence("command_manifest_count", compiled.command_manifest.commands.len()),
            ],
        };
    }
    ReleaseCheckRecord {
        id: "vn.compiled_story".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "compiled VN story is present and decodable".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("story_hash", compiled.story_hash),
            evidence("story_count", compiled.stories.len()),
            evidence("state_count", compiled.states.len()),
            evidence("route_node_count", compiled.route_graph.nodes.len()),
            evidence(
                "command_manifest_count",
                compiled.command_manifest.commands.len(),
            ),
            evidence(
                "variable_scope_count",
                compiled.variable_manifest.scopes.len(),
            ),
        ],
    }
}

fn vn_profile_manifest_check(
    package: &PackageReader,
    profile: &str,
    selected_target: Option<&str>,
) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnProfileManifest>("vn.profile_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.profile_manifest".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.profile_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_PROFILE_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.profile_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    if manifest.schema != "astra.vn.profile_manifest.v1"
        || !manifest.profiles.iter().any(|p| p == profile)
    {
        return ReleaseCheckRecord {
            id: "vn.profile_manifest".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN profile manifest does not declare the validation profile".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_PROFILE_MISSING",
                "vn.profile_manifest must include the validation profile",
            )),
            evidence: vec![evidence("profile", profile)],
        };
    }
    if let Some(selected_target) = selected_target {
        if manifest.target != selected_target {
            return ReleaseCheckRecord {
                id: "vn.profile_manifest".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "VN profile manifest target does not match selected target".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_PROFILE_TARGET",
                    "vn.profile_manifest target must match selected target",
                )),
                evidence: vec![
                    evidence("manifest_target", manifest.target),
                    evidence("selected_target", selected_target),
                ],
            };
        }
    }
    ReleaseCheckRecord {
        id: "vn.profile_manifest".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN profile manifest matches target and validation profile".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("target", manifest.target),
            evidence("profile", profile),
            evidence("profile_count", manifest.profiles.len()),
        ],
    }
}

fn vn_policy_bundle_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnPolicyBundleManifest>("vn.policy_bundle_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.policy_bundle".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.policy_bundle_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_POLICY_BUNDLE",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.policy_bundle_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let cache = match package
        .container()
        .decode_postcard::<VnPolicyBundleSourceCache>("vn.policy_bundle_source_cache")
    {
        Ok(cache) => cache,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.policy_bundle".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.policy_bundle_source_cache section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_POLICY_CACHE",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.policy_bundle_source_cache"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_standard_with_cache(&cache);
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.policy_bundle".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary:
                "VN policy bundle manifest and source cache do not satisfy the standard policy gate"
                    .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.policy_bundle_manifest"),
                evidence("source_cache_section", "vn.policy_bundle_source_cache"),
                evidence("profile", profile),
                evidence("bundle_count", manifest.bundles.len()),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.policy_bundle".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN standard policy bundle is present and locked".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.policy_bundle_manifest"),
            evidence("source_cache_section", "vn.policy_bundle_source_cache"),
            evidence("profile", profile),
            evidence("bundle_count", manifest.bundles.len()),
        ],
    }
}

fn vn_extension_bindings_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnExtensionManifest>("vn.extension_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.extension_bindings".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.extension_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_EXTENSION_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.extension_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_required();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.extension_bindings".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN extension manifest is missing required provider bindings".to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.extension_manifest"),
                evidence("profile", profile),
                evidence("binding_count", manifest.bindings.len()),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.extension_bindings".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN extension manifest declares required explicit bindings".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.extension_manifest"),
            evidence("profile", profile),
            evidence("binding_count", manifest.bindings.len()),
        ],
    }
}

fn vn_standard_commands_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnStandardCommandManifest>("vn.standard_command_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.standard_commands".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.standard_command_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_STANDARD_COMMAND_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.standard_command_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    let compiled = match package
        .container()
        .decode_postcard::<CompiledStory>("vn.compiled_story")
    {
        Ok(compiled) => compiled,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.standard_commands".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.compiled_story is required to validate standard command usage"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_COMPILED_STORY",
                    err.to_string(),
                )),
                evidence: vec![evidence("section", "vn.compiled_story")],
            };
        }
    };

    let report = manifest.validate_usage(&compiled);
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.standard_commands".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN standard command manifest or compiled command usage failed validation"
                .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.standard_command_manifest"),
                evidence("profile", profile),
                evidence("command_count", report.command_count),
                evidence("checked_usage_count", report.checked_usage_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.standard_commands".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN standard command manifest covers compiled presentation usage".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.standard_command_manifest"),
            evidence("profile", profile),
            evidence("command_count", report.command_count),
            evidence("checked_usage_count", report.checked_usage_count),
        ],
    }
}

fn vn_presentation_provider_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnPresentationProviderManifest>("vn.presentation_provider_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.presentation_provider".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.presentation_provider_manifest section is missing or invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_PRESENTATION_PROVIDER_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.presentation_provider_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_standard();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.presentation_provider".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary:
                "VN presentation provider manifest lacks required filter/fallback/wait support"
                    .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.presentation_provider_manifest"),
                evidence("profile", profile),
                evidence("filter_count", report.filter_count),
                evidence("fallback_count", report.fallback_count),
                evidence("wait_capability_count", report.wait_capability_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.presentation_provider".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN presentation provider declares filter fallback and await capabilities"
            .to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.presentation_provider_manifest"),
            evidence("profile", profile),
            evidence("filter_count", report.filter_count),
            evidence("fallback_count", report.fallback_count),
            evidence("wait_capability_count", report.wait_capability_count),
        ],
    }
}

fn vn_commercial_baseline_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnCommercialBaselineManifest>("vn.commercial_baseline_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.commercial_baseline".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.commercial_baseline_manifest section is missing or invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_COMMERCIAL_BASELINE_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.commercial_baseline_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_required();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.commercial_baseline".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN commercial baseline is missing required automated feature evidence"
                .to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.commercial_baseline_manifest"),
                evidence("profile", profile),
                evidence("story_hash", manifest.story_hash),
                evidence("required_count", report.required_count),
                evidence("feature_count", report.feature_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.commercial_baseline".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN commercial baseline declares required automated feature evidence".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.commercial_baseline_manifest"),
            evidence("profile", profile),
            evidence("story_hash", manifest.story_hash),
            evidence("required_count", report.required_count),
            evidence("feature_count", report.feature_count),
        ],
    }
}

fn vn_system_ui_profile_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<SystemStoryManifest>("vn.system_story_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.system_ui_profile".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.system_story_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_SYSTEM_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.system_story_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    if manifest.schema != "astra.vn.system_story_manifest.v1" {
        return ReleaseCheckRecord {
            id: "vn.system_ui_profile".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN system story manifest has an unexpected schema".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_VN_SYSTEM_MANIFEST_SCHEMA",
                "vn.system_story_manifest must use astra.vn.system_story_manifest.v1",
            )),
            evidence: vec![evidence("section", "vn.system_story_manifest")],
        };
    }
    let required = SystemStoryManifest::commercial_required_pages();
    let report = manifest.validate_required(&required);
    if report.status == SystemStoryValidationStatus::Blocked {
        let missing_count = report.diagnostics.len();
        return ReleaseCheckRecord {
            id: "vn.system_ui_profile".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN system UI profile is missing required system story entries".to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.system_story_manifest"),
                evidence("profile", profile),
                evidence("page_count", manifest.entries.len()),
                evidence("required_count", required.len()),
                evidence("missing_count", missing_count),
            ],
        };
    }

    let profile_manifest = match package
        .container()
        .decode_postcard::<VnSystemUiProfileManifest>("vn.system_ui_profile_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.system_ui_profile".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.system_ui_profile_manifest section is missing or invalid".to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_SYSTEM_UI_PROFILE_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.system_ui_profile_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };
    let profile_report = profile_manifest.validate();
    if profile_report.status == SystemStoryValidationStatus::Blocked {
        return ReleaseCheckRecord {
            id: "vn.system_ui_profile".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN system UI profile is missing migration, unlock or localization coverage"
                .to_string(),
            diagnostic: profile_report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.system_ui_profile_manifest"),
                evidence("profile", profile),
                evidence("diagnostic_count", profile_report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.system_ui_profile".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN system UI profile declares required stories and coverage policies".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.system_story_manifest"),
            evidence("profile_section", "vn.system_ui_profile_manifest"),
            evidence("profile", profile),
            evidence("page_count", manifest.entries.len()),
            evidence("required_count", required.len()),
            evidence("unlock_source_count", profile_manifest.unlock_sources.len()),
            evidence(
                "localization_locale_count",
                profile_manifest.localization.locales.len(),
            ),
            evidence("save_migrator", profile_manifest.save_migration.migrator_id),
        ],
    }
}

fn vn_advanced_presentation_check(package: &PackageReader, profile: &str) -> ReleaseCheckRecord {
    let manifest = match package
        .container()
        .decode_postcard::<VnAdvancedPresentationManifest>("vn.advanced_presentation_manifest")
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return ReleaseCheckRecord {
                id: "vn.advanced_presentation".to_string(),
                domain: ReleaseDomain::Vn,
                status: CheckStatus::Blocked,
                summary: "vn.advanced_presentation_manifest section is missing or invalid"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_VN_ADVANCED_PRESENTATION_MANIFEST",
                    err.to_string(),
                )),
                evidence: vec![
                    evidence("section", "vn.advanced_presentation_manifest"),
                    evidence("profile", profile),
                ],
            };
        }
    };

    let report = manifest.validate_required();
    if !report.passed {
        return ReleaseCheckRecord {
            id: "vn.advanced_presentation".to_string(),
            domain: ReleaseDomain::Vn,
            status: CheckStatus::Blocked,
            summary: "VN advanced presentation profile is missing automated evidence".to_string(),
            diagnostic: report.diagnostics.first().cloned(),
            evidence: vec![
                evidence("section", "vn.advanced_presentation_manifest"),
                evidence("profile", profile),
                evidence("story_hash", manifest.story_hash),
                evidence("required_count", report.required_count),
                evidence("evidence_count", report.evidence_count),
                evidence("diagnostic_count", report.diagnostics.len()),
            ],
        };
    }

    ReleaseCheckRecord {
        id: "vn.advanced_presentation".to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "VN advanced presentation profile declares required automated evidence"
            .to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", "vn.advanced_presentation_manifest"),
            evidence("profile", profile),
            evidence("story_hash", manifest.story_hash),
            evidence("required_count", report.required_count),
            evidence("evidence_count", report.evidence_count),
            evidence("timeline_count", manifest.timeline_ids.len()),
        ],
    }
}

fn tsuinosora_checks(
    package: &PackageReader,
    profile: &str,
    selected_target: Option<&str>,
) -> Vec<ReleaseCheckRecord> {
    let Some(target) = tsuinosora_target(package, selected_target) else {
        return Vec::new();
    };
    let mut checks = vec![
        tsuinosora_reference_evidence_check(package),
        tsuinosora_asset_analysis_check(package),
        tsuinosora_conversion_manifest_check(package),
        tsuinosora_mount_policy_check(package, &target),
    ];
    if profile == "modern" {
        checks.push(tsuinosora_modern_profile_check(package));
    }
    if release_profile_requires_cooked_project(profile) {
        checks.push(tsuinosora_manual_signoff_check(package));
    }
    checks
}

fn tsuinosora_target(package: &PackageReader, selected_target: Option<&str>) -> Option<String> {
    if let Some(target) = selected_target {
        if target.starts_with("tsuinosora-") {
            return Some(target.to_string());
        }
    }
    let bytes = package
        .container()
        .read_bounded("target.manifest", 256 * 1024)
        .ok()?;
    let manifest: TargetManifest = serde_json::from_slice(&bytes).ok()?;
    manifest
        .targets
        .iter()
        .find(|target| target.id.starts_with("tsuinosora-"))
        .map(|target| target.id.clone())
}

fn tsuinosora_reference_evidence_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.reference_evidence";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(
        section,
        &value,
        &[
            "tsuinosora.visual_reference_report.v1",
            "tsuinosora.reference_evidence.v1",
        ],
    ) {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let references = value
        .get("references")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if references.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REFERENCE_EVIDENCE",
            "TsuiNoSora reference evidence must include title/game reference hashes",
            vec![evidence("section", section), evidence("reference_count", 0)],
        );
    }
    let missing_hash = references.iter().any(|reference| {
        reference
            .get("hash")
            .and_then(serde_json::Value::as_str)
            .is_none()
    });
    if missing_hash {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REFERENCE_HASH",
            "TsuiNoSora reference evidence entries must expose hashes",
            vec![evidence("section", section)],
        );
    }
    for (logical_id, expected_hash, (expected_width, expected_height)) in [
        (
            "title",
            TSUI_REFERENCE_TITLE_HASH,
            TSUI_REFERENCE_TITLE_DIMENSIONS,
        ),
        (
            "game",
            TSUI_REFERENCE_GAME_HASH,
            TSUI_REFERENCE_GAME_DIMENSIONS,
        ),
    ] {
        let Some(reference) = references.iter().find(|reference| {
            reference
                .get("logical_id")
                .and_then(serde_json::Value::as_str)
                == Some(logical_id)
        }) else {
            return tsuinosora_blocked(
                section,
                "ASTRA_TSUI_REFERENCE_EVIDENCE",
                format!("TsuiNoSora reference evidence must include {logical_id}"),
                vec![
                    evidence("section", section),
                    evidence("logical_id", logical_id),
                ],
            );
        };
        let actual_hash = reference
            .get("hash")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if actual_hash != expected_hash {
            return tsuinosora_blocked(
                section,
                "ASTRA_TSUI_REFERENCE_HASH_MISMATCH",
                format!("TsuiNoSora {logical_id} reference hash does not match authority"),
                vec![
                    evidence("section", section),
                    evidence("logical_id", logical_id),
                    evidence("expected_hash", expected_hash),
                    evidence("actual_hash", actual_hash),
                ],
            );
        }
        let width = reference
            .get("dimensions")
            .and_then(|dimensions| dimensions.get("width"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();
        let height = reference
            .get("dimensions")
            .and_then(|dimensions| dimensions.get("height"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default();
        if width != expected_width || height != expected_height {
            return tsuinosora_blocked(
                section,
                "ASTRA_TSUI_REFERENCE_DIMENSION_MISMATCH",
                format!("TsuiNoSora {logical_id} reference dimensions do not match authority"),
                vec![
                    evidence("section", section),
                    evidence("logical_id", logical_id),
                    evidence("expected_width", expected_width),
                    evidence("expected_height", expected_height),
                    evidence("actual_width", width),
                    evidence("actual_height", height),
                ],
            );
        }
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora visual reference evidence is present and sanitized".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("reference_count", references.len()),
        ],
    }
}

fn tsuinosora_asset_analysis_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.asset_analysis";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(section, &value, &["tsuinosora.asset_analysis.v1"])
    {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    let quarantine_count = value
        .get("quarantine")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if quarantine_count > 0 {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ASSET_QUARANTINE",
            "TsuiNoSora asset analysis contains quarantined assets",
            vec![
                evidence("section", section),
                evidence("quarantine_count", quarantine_count),
            ],
        );
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let asset_count = value
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if asset_count == 0 {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ASSET_ANALYSIS_EMPTY",
            "TsuiNoSora asset analysis must include analyzed asset evidence",
            vec![evidence("section", section), evidence("asset_count", 0)],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora asset analysis passed with no quarantine".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("asset_count", asset_count),
            evidence("quarantine_count", quarantine_count),
        ],
    }
}

fn tsuinosora_conversion_manifest_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.conversion_manifest";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(
        section,
        &value,
        &[
            "tsuinosora.conversion_report.v1",
            "tsuinosora.conversion_manifest.v1",
        ],
    ) {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let routes = value
        .get("routes")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if routes.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ROUTE_COVERAGE",
            "TsuiNoSora conversion manifest must include covered routes",
            vec![evidence("section", section), evidence("route_count", 0)],
        );
    }
    let uncovered = routes.iter().filter(|route| {
        route.get("coverage").and_then(serde_json::Value::as_str) != Some("covered")
    });
    let uncovered_count = uncovered.count();
    if uncovered_count > 0 {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_ROUTE_COVERAGE",
            "TsuiNoSora conversion manifest contains routes without proven coverage",
            vec![
                evidence("section", section),
                evidence("route_count", routes.len()),
                evidence("uncovered_count", uncovered_count),
            ],
        );
    }
    let resources = value
        .get("resources")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if resources.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_CONVERSION_RESOURCE_EVIDENCE",
            "TsuiNoSora conversion manifest must include converted resource evidence",
            vec![evidence("section", section), evidence("resource_count", 0)],
        );
    }
    if let Some(field) = tsuinosora_invalid_conversion_resource_field(resources) {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_CONVERSION_RESOURCE_EVIDENCE",
            "TsuiNoSora conversion manifest resources must include path, hash and byte size evidence",
            vec![
                evidence("section", section),
                evidence("resource_count", resources.len()),
                evidence("field", field),
            ],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora conversion manifest proves route and resource coverage".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("route_count", routes.len()),
            evidence("resource_count", resources.len()),
        ],
    }
}

fn tsuinosora_invalid_conversion_resource_field(resources: &[serde_json::Value]) -> Option<String> {
    for (index, resource) in resources.iter().enumerate() {
        for field in ["source", "native_path", "classification"] {
            if resource
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Some(format!("resources.{index}.{field}"));
            }
        }
        for field in ["source_hash", "converted_hash"] {
            let value = resource
                .get(field)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if !is_release_sha256(value) {
                return Some(format!("resources.{index}.{field}"));
            }
        }
        if resource
            .get("byte_size")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|byte_size| byte_size == 0)
        {
            return Some(format!("resources.{index}.byte_size"));
        }
    }
    None
}

fn is_release_sha256(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn tsuinosora_mount_policy_check(package: &PackageReader, target: &str) -> ReleaseCheckRecord {
    let section = "tsuinosora.mount_policy";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(section, &value, &["tsuinosora.mount_policy.v1"]) {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    if value
        .get("target")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|manifest_target| manifest_target != target)
    {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_MOUNT_TARGET",
            "TsuiNoSora mount policy target must match the selected target",
            vec![
                evidence("section", section),
                evidence("selected_target", target),
            ],
        );
    }
    let aliases = value
        .get("aliases")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if aliases.is_empty() {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_MOUNT_ALIAS",
            "TsuiNoSora mount policy must declare mount aliases",
            vec![evidence("section", section)],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora mount policy is sanitized and target-bound".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("target", target),
            evidence("alias_count", aliases.len()),
        ],
    }
}

fn tsuinosora_modern_profile_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.modern_profile_report";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) =
        tsuinosora_schema_check(section, &value, &["tsuinosora.modern_profile_report.v1"])
    {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let features = value
        .get("features")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if features.is_empty()
        || features.iter().any(|feature| {
            !feature
                .get("independent_switch")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                || feature
                    .get("affects_core_state")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
                || feature
                    .get("fallback_hash")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(str::is_empty)
        })
    {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_MODERN_FEATURE",
            "TsuiNoSora modern profile features must be reversible, independent and backed by fallback hashes",
            vec![
                evidence("section", section),
                evidence("feature_count", features.len()),
            ],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora modern profile report is reversible and fallback-backed".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("feature_count", features.len()),
        ],
    }
}

fn tsuinosora_manual_signoff_check(package: &PackageReader) -> ReleaseCheckRecord {
    let section = "tsuinosora.manual_signoff";
    let value = match read_tsuinosora_json_section(package, section) {
        Ok(value) => value,
        Err(check) => return *check,
    };
    if let Some(check) = tsuinosora_schema_check(section, &value, &["tsuinosora.manual_signoff.v1"])
    {
        return check;
    }
    if let Some(check) = tsuinosora_path_leak_check(section, &value) {
        return check;
    }
    if let Some(check) = tsuinosora_status_check(section, &value) {
        return check;
    }
    let checks = value
        .get("checks")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let blockers = value
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let failed_count = checks
        .iter()
        .filter(|check| {
            !matches!(
                check.get("result").and_then(serde_json::Value::as_str),
                Some("pass" | "pass_with_diagnostics")
            )
        })
        .count();
    let required_checks = [
        "manual.full_playthrough",
        "manual.audio_listening",
        "manual.visual_review",
        "manual.alias_replacement",
    ];
    let missing_required_count = required_checks
        .iter()
        .filter(|required| {
            !checks.iter().any(|check| {
                let check_id = check.get("check_id").and_then(serde_json::Value::as_str);
                check_id == Some(**required)
                    && matches!(
                        check.get("result").and_then(serde_json::Value::as_str),
                        Some("pass" | "pass_with_diagnostics")
                    )
            })
        })
        .count();
    if checks.is_empty() || failed_count > 0 || !blockers.is_empty() || missing_required_count > 0 {
        return tsuinosora_blocked(
            section,
            "ASTRA_TSUI_MANUAL_SIGNOFF",
            "TsuiNoSora release profile requires completed manual signoff checks",
            vec![
                evidence("section", section),
                evidence("check_count", checks.len()),
                evidence("failed_count", failed_count),
                evidence("blocker_count", blockers.len()),
                evidence("missing_required_count", missing_required_count),
            ],
        );
    }
    ReleaseCheckRecord {
        id: section.to_string(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Pass,
        summary: "TsuiNoSora manual signoff is complete for release validation".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("section", section),
            evidence("check_count", checks.len()),
            evidence("required_check_count", required_checks.len()),
        ],
    }
}

fn read_tsuinosora_json_section(
    package: &PackageReader,
    section: &str,
) -> Result<serde_json::Value, Box<ReleaseCheckRecord>> {
    let bytes = match package.container().read_bounded(section, 512 * 1024) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err(Box::new(tsuinosora_blocked(
                section,
                "ASTRA_TSUI_SECTION_MISSING",
                format!("missing or unreadable TsuiNoSora package section {section}: {err}"),
                vec![evidence("section", section)],
            )));
        }
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|err| {
        Box::new(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_SECTION_JSON",
            format!("TsuiNoSora package section {section} is not valid JSON: {err}"),
            vec![evidence("section", section)],
        ))
    })?;
    if let Some(check) = tsuinosora_payload_leak_check(section, &value) {
        return Err(Box::new(check));
    }
    Ok(value)
}

fn tsuinosora_schema_check(
    section: &str,
    value: &serde_json::Value,
    expected: &[&str],
) -> Option<ReleaseCheckRecord> {
    let schema = value.get("schema").and_then(serde_json::Value::as_str);
    if schema.is_none_or(|schema| !expected.contains(&schema)) {
        return Some(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_SCHEMA",
            "TsuiNoSora package section uses an unexpected schema",
            vec![
                evidence("section", section),
                evidence("schema", schema.unwrap_or("missing")),
                evidence("expected", expected.join("|")),
            ],
        ));
    }
    None
}

fn tsuinosora_status_check(section: &str, value: &serde_json::Value) -> Option<ReleaseCheckRecord> {
    let status = value.get("status").and_then(serde_json::Value::as_str);
    if status != Some("pass") {
        return Some(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REPORT_BLOCKED",
            "TsuiNoSora report status is not pass",
            vec![
                evidence("section", section),
                evidence("status", status.unwrap_or("missing")),
            ],
        ));
    }
    None
}

fn tsuinosora_path_leak_check(
    section: &str,
    value: &serde_json::Value,
) -> Option<ReleaseCheckRecord> {
    if json_has_local_path(value) {
        return Some(tsuinosora_blocked(
            section,
            "ASTRA_TSUI_REPORT_PATH_LEAK",
            "TsuiNoSora package report contains a local path-like value",
            vec![evidence("section", section)],
        ));
    }
    None
}

fn tsuinosora_payload_leak_check(
    section: &str,
    value: &serde_json::Value,
) -> Option<ReleaseCheckRecord> {
    let field = forbidden_tsuinosora_payload_field(value)?;
    Some(tsuinosora_blocked(
        section,
        "ASTRA_TSUI_REPORT_PAYLOAD_LEAK",
        "TsuiNoSora package report contains a commercial payload-like field",
        vec![evidence("section", section), evidence("field", field)],
    ))
}

fn forbidden_tsuinosora_payload_field(value: &serde_json::Value) -> Option<String> {
    fn visit(value: &serde_json::Value, path: &mut Vec<String>) -> Option<String> {
        match value {
            serde_json::Value::Array(values) => {
                for (index, child) in values.iter().enumerate() {
                    path.push(index.to_string());
                    if let Some(field) = visit(child, path) {
                        return Some(field);
                    }
                    path.pop();
                }
                None
            }
            serde_json::Value::Object(values) => {
                for (key, child) in values {
                    path.push(key.clone());
                    if is_forbidden_tsuinosora_payload_key(key, path, child) {
                        return Some(path.join("."));
                    }
                    if let Some(field) = visit(child, path) {
                        return Some(field);
                    }
                    path.pop();
                }
                None
            }
            _ => None,
        }
    }

    visit(value, &mut Vec::new())
}

fn is_forbidden_tsuinosora_payload_key(
    key: &str,
    path: &[String],
    value: &serde_json::Value,
) -> bool {
    let key = key.to_ascii_lowercase();
    if key == "payload" {
        return !(path == ["redaction".to_string(), "payload".to_string()]
            && value.as_str() == Some("omitted"));
    }
    matches!(
        key.as_str(),
        "body"
            | "bytecode"
            | "bytes"
            | "commercial_text"
            | "content"
            | "lingo_source"
            | "payload_bytes"
            | "raw_payload"
            | "script_text"
            | "source_payload"
            | "source_text"
            | "text"
    )
}

fn json_has_local_path(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => looks_like_local_path(value),
        serde_json::Value::Array(values) => values.iter().any(json_has_local_path),
        serde_json::Value::Object(values) => values.values().any(json_has_local_path),
        _ => false,
    }
}

fn looks_like_local_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value.as_bytes().windows(3).any(|pair| {
            pair[0].is_ascii_alphabetic() && pair[1] == b':' && matches!(pair[2], b'/' | b'\\')
        })
}

fn tsuinosora_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Vn,
        status: CheckStatus::Blocked,
        summary: summary.clone(),
        diagnostic: Some(Diagnostic::blocking(code, summary)),
        evidence: evidence_values,
    }
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
