use std::collections::{BTreeMap, BTreeSet};

use astra_asset::{AssetCatalog, VfsBackendKind, VfsManifest, VfsSourceRef, VfsUri};
use astra_core::{Diagnostic, Hash256};
use astra_package::{PackageManifest, PackageReader};
use astra_platform::{
    PlatformCapabilityReport, PlatformHostConformanceReport, PlatformValidationStatus,
};
use astra_player_core::{PlayerAutomationReport, PlayerAutomationStatus};
use astra_plugin_abi::{
    RuntimeOpenRequest, RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeStepInput,
};
use astra_target::{validate_manifest, TargetKind, TargetManifest, TargetValidationStatus};
use astra_vn::{
    decode_compiled_story, NativeVnRuntimeProvider, SystemStoryManifest,
    SystemStoryValidationStatus, VnAdvancedPresentationManifest, VnCommercialBaselineManifest,
    VnExtensionManifest, VnPolicyBundleManifest, VnPolicyBundleSourceCache,
    VnPresentationProviderManifest, VnProfileManifest, VnRunConfig, VnStandardCommandManifest,
    VnSystemUiProfileManifest,
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
    pub fn validate_package_with_platform_evidence(
        &self,
        request: PackageValidateRequest,
        conformance_report: Option<PlatformHostConformanceReport>,
        player_report: Option<PlayerAutomationReport>,
    ) -> Result<ReleaseReport, ReleaseError> {
        let capability_report = request.platform_report.clone();
        let require_platform_report = request.require_platform_report;
        let target = request.target.clone();
        let profile = request.profile.clone();
        let mut report = self.validate_package(request)?;
        report.checks.push(platform_conformance_check(
            capability_report.as_ref(),
            conformance_report.as_ref(),
            &report.package_hash,
            require_platform_report,
        ));
        if let Some(player_report) = player_report {
            report.checks.push(player_full_playable_check(
                &player_report,
                &report.package_hash,
                &profile,
                target.as_deref(),
            ));
            report.checks.push(platform_evidence_continuity_check(
                capability_report.as_ref(),
                conformance_report.as_ref(),
                &player_report,
            ));
        }
        report.status = release_status(&report.checks);
        Ok(report)
    }

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
        tracing::info!(
            event = "release.validate.start",
            profile = %request.profile,
            package_byte_size = request.package_bytes.len(),
            has_target = request.target.is_some(),
            "release validation started"
        );
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
                    "asset.vfs_manifest",
                    "asset.catalog",
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
                checks.extend(runtime_provider_checks(
                    &package,
                    request.target.as_deref(),
                    &request.profile,
                ));
                checks.extend(vfs_checks(&package));
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
                checks.push(platform_profile_binding_check(
                    &package,
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

        let report = ReleaseReport {
            schema: "astra.release_report.v1".to_string(),
            package_id,
            profile: request.profile,
            status,
            package_hash,
            checks,
        };
        match report.status {
            CheckStatus::Pass => tracing::info!(
                event = "release.validate.complete",
                status = "pass",
                check_count = report.checks.len(),
                package_hash = %report.package_hash,
                "release validation completed"
            ),
            CheckStatus::Warning => tracing::warn!(
                event = "release.validate.complete",
                status = "warning",
                check_count = report.checks.len(),
                package_hash = %report.package_hash,
                "release validation completed with warnings"
            ),
            CheckStatus::Blocked => tracing::error!(
                event = "release.validate.complete",
                status = "blocked",
                check_count = report.checks.len(),
                package_hash = %report.package_hash,
                "release validation blocked"
            ),
        }
        Ok(report)
    }
}

fn platform_conformance_check(
    capability: Option<&PlatformCapabilityReport>,
    conformance: Option<&PlatformHostConformanceReport>,
    package_hash: &str,
    required: bool,
) -> ReleaseCheckRecord {
    let (Some(capability), Some(conformance)) = (capability, conformance) else {
        return ReleaseCheckRecord {
            id: "platform.host_conformance".to_string(),
            domain: ReleaseDomain::Platform,
            status: if required {
                CheckStatus::Blocked
            } else {
                CheckStatus::Warning
            },
            summary: "platform host conformance report was not supplied".to_string(),
            diagnostic: Some(if required {
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_CONFORMANCE_MISSING",
                    "a host conformance report is required for this release profile",
                )
            } else {
                Diagnostic::warning(
                    "ASTRA_PLATFORM_CONFORMANCE_MISSING",
                    "host conformance evidence is pending",
                )
            }),
            evidence: Vec::new(),
        };
    };
    let (validation, diagnostics) =
        astra_platform::validate_conformance_report(capability, conformance);
    let package_matches = conformance.package_hash == package_hash;
    let status = if validation == PlatformValidationStatus::Pass && package_matches {
        CheckStatus::Pass
    } else {
        CheckStatus::Blocked
    };
    ReleaseCheckRecord {
        id: "platform.host_conformance".to_string(),
        domain: ReleaseDomain::Platform,
        status,
        summary: "host conformance evidence is bound to the release identity".to_string(),
        diagnostic: if status == CheckStatus::Pass {
            None
        } else {
            Some(diagnostics.first().cloned().unwrap_or_else(|| {
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_CONFORMANCE_PACKAGE_IDENTITY",
                    "host conformance package hash does not match the release package",
                )
            }))
        },
        evidence: vec![
            evidence("profile_hash", &conformance.profile_hash),
            evidence("package_hash", &conformance.package_hash),
            evidence("build_fingerprint", &conformance.build_fingerprint),
            evidence("session_id", &conformance.session_id),
            evidence("check_count", conformance.checks.len()),
        ],
    }
}

fn platform_evidence_continuity_check(
    capability: Option<&PlatformCapabilityReport>,
    conformance: Option<&PlatformHostConformanceReport>,
    player: &PlayerAutomationReport,
) -> ReleaseCheckRecord {
    let player_evidence = |key: &str| {
        player
            .checks
            .iter()
            .flat_map(|check| &check.evidence)
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
    };
    let matches = capability
        .zip(conformance)
        .is_some_and(|(capability, conformance)| {
            conformance.profile_hash == capability.profile_hash
                && conformance.build_fingerprint == capability.build_fingerprint
                && conformance.package_hash == player.package_hash
                && player_evidence("profile_hash") == Some(conformance.profile_hash.as_str())
                && player_evidence("build_fingerprint")
                    == Some(conformance.build_fingerprint.as_str())
                && player_evidence("session_id") == Some(conformance.session_id.as_str())
        });
    ReleaseCheckRecord {
        id: "platform.evidence_continuity".to_string(),
        domain: ReleaseDomain::Platform,
        status: if matches {
            CheckStatus::Pass
        } else {
            CheckStatus::Blocked
        },
        summary: "capability, host conformance and player automation identities are continuous"
            .to_string(),
        diagnostic: (!matches).then(|| {
            Diagnostic::blocking(
                "ASTRA_PLATFORM_EVIDENCE_CONTINUITY",
                "platform reports do not share profile, package, build and session identity",
            )
        }),
        evidence: player_evidence("session_id")
            .map(|session_id| vec![evidence("session_id", session_id)])
            .unwrap_or_default(),
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

fn runtime_provider_checks(
    package: &PackageReader,
    selected_target: Option<&str>,
    profile: &str,
) -> Vec<ReleaseCheckRecord> {
    vec![
        runtime_provider_binding_check(package, selected_target, profile),
        runtime_provider_native_vn_check(package),
    ]
}

fn runtime_provider_binding_check(
    package: &PackageReader,
    selected_target: Option<&str>,
    profile: &str,
) -> ReleaseCheckRecord {
    let provider_policy = match read_json_section(package, "provider.policy") {
        Ok(value) => value,
        Err((code, message)) => {
            return runtime_blocked(
                "runtime_provider.binding",
                code,
                message,
                vec![evidence("section", "provider.policy")],
            )
        }
    };
    let registry = match read_json_section(package, "plugin.extension_registry") {
        Ok(value) => value,
        Err((code, message)) => {
            return runtime_blocked(
                "runtime_provider.binding",
                code,
                message,
                vec![evidence("section", "plugin.extension_registry")],
            )
        }
    };
    let target_manifest = match package
        .container()
        .read_bounded("target.manifest", 256 * 1024)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<TargetManifest>(&bytes).ok())
    {
        Some(manifest) => manifest,
        None => {
            return runtime_blocked(
                "runtime_provider.binding",
                "ASTRA_RUNTIME_PROVIDER_TARGET_MANIFEST",
                "runtime provider binding requires a valid target manifest",
                vec![evidence("section", "target.manifest")],
            )
        }
    };
    let Some(target) = select_game_target(&target_manifest, selected_target) else {
        return runtime_blocked(
            "runtime_provider.binding",
            "ASTRA_RUNTIME_PROVIDER_TARGET_MISSING",
            "runtime provider binding requires one selected game target",
            vec![evidence("selected_target", selected_target.unwrap_or(""))],
        );
    };
    let runtime_provider = target.runtime_provider.as_deref().unwrap_or_default();
    if runtime_provider != "native_vn" {
        return runtime_blocked(
            "runtime_provider.binding",
            "ASTRA_RUNTIME_PROVIDER_BINDING_MISSING",
            "selected game target must bind native_vn runtime provider",
            vec![
                evidence("target", &target.id),
                evidence("runtime_provider", runtime_provider),
            ],
        );
    }
    let bindings = provider_policy
        .get("bindings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_policy_binding = bindings.iter().any(|binding| {
        binding.get("slot").and_then(serde_json::Value::as_str) == Some("game_runtime_provider")
            && binding
                .get("provider_id")
                .and_then(serde_json::Value::as_str)
                == Some("astra.runtime.native_vn")
    });
    if !has_policy_binding {
        return runtime_blocked(
            "runtime_provider.binding",
            "ASTRA_RUNTIME_PROVIDER_BINDING_MISSING",
            "provider.policy must bind game_runtime_provider to astra.runtime.native_vn",
            vec![evidence("target", &target.id), evidence("profile", profile)],
        );
    }
    let providers = registry
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(provider) = providers.iter().find(|provider| {
        provider.get("slot").and_then(serde_json::Value::as_str) == Some("game_runtime_provider")
            && provider
                .get("provider_id")
                .and_then(serde_json::Value::as_str)
                == Some("astra.runtime.native_vn")
    }) else {
        return runtime_blocked(
            "runtime_provider.binding",
            "ASTRA_RUNTIME_PROVIDER_REGISTRY_MISSING",
            "plugin registry must register astra.runtime.native_vn for game_runtime_provider",
            vec![evidence("provider_id", "astra.runtime.native_vn")],
        );
    };
    if !provider
        .get("packaged")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return runtime_blocked(
            "runtime_provider.binding",
            "ASTRA_RUNTIME_PROVIDER_UNPACKAGED",
            "runtime provider must be packaged eligible",
            vec![evidence("provider_id", "astra.runtime.native_vn")],
        );
    }

    ReleaseCheckRecord {
        id: "runtime_provider.binding".to_string(),
        domain: ReleaseDomain::Runtime,
        status: CheckStatus::Pass,
        summary: "target manifest, provider policy and plugin registry bind NativeVN runtime"
            .to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("target", &target.id),
            evidence("runtime_provider", "native_vn"),
            evidence("provider_id", "astra.runtime.native_vn"),
        ],
    }
}

fn runtime_provider_native_vn_check(package: &PackageReader) -> ReleaseCheckRecord {
    let provider_policy = match read_json_section(package, "provider.policy") {
        Ok(value) => value,
        Err((code, message)) => {
            return runtime_blocked(
                "runtime_provider.native_vn",
                code,
                message,
                vec![evidence("section", "provider.policy")],
            )
        }
    };
    let Some(descriptor) = provider_policy.get("runtime_provider") else {
        return runtime_blocked(
            "runtime_provider.native_vn",
            "ASTRA_RUNTIME_PROVIDER_DESCRIPTOR_MISSING",
            "provider.policy must include NativeVN runtime provider descriptor",
            vec![evidence("provider_id", "astra.runtime.native_vn")],
        );
    };
    let runtime_id = descriptor
        .get("runtime_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let provider_id = descriptor
        .get("provider_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if runtime_id != "native_vn" || provider_id != "astra.runtime.native_vn" {
        return runtime_blocked(
            "runtime_provider.native_vn",
            "ASTRA_RUNTIME_PROVIDER_DESCRIPTOR",
            "NativeVN runtime provider descriptor id does not match target binding",
            vec![
                evidence("runtime_id", runtime_id),
                evidence("provider_id", provider_id),
            ],
        );
    }
    let package_sections = descriptor
        .get("package_sections")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let required_sections = [
        "vn.compiled_story",
        "vn.profile_manifest",
        "vn.policy_bundle_manifest",
        "vn.extension_manifest",
        "vn.standard_command_manifest",
        "vn.presentation_provider_manifest",
        "vn.commercial_baseline_manifest",
        "vn.system_story_manifest",
        "vn.system_ui_profile_manifest",
    ];
    for section in required_sections {
        if !package_sections.iter().any(|value| value == section) {
            return runtime_blocked(
                "runtime_provider.native_vn",
                "ASTRA_RUNTIME_PROVIDER_SECTION_DECLARATION",
                format!("NativeVN runtime provider descriptor must declare {section}"),
                vec![evidence("section", section)],
            );
        }
    }
    let release_checks = descriptor
        .get("release_checks")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !release_checks
        .iter()
        .any(|value| value == "runtime_provider.native_vn")
    {
        return runtime_blocked(
            "runtime_provider.native_vn",
            "ASTRA_RUNTIME_PROVIDER_RELEASE_CHECK",
            "NativeVN runtime provider descriptor must declare runtime_provider.native_vn",
            vec![evidence("provider_id", "astra.runtime.native_vn")],
        );
    }

    let behavioral_evidence = match native_vn_behavioral_evidence(package) {
        Ok(evidence) => evidence,
        Err((code, message)) => {
            return runtime_blocked(
                "runtime_provider.native_vn",
                code,
                message,
                vec![evidence("provider_id", provider_id)],
            )
        }
    };
    let mut release_evidence = vec![
        evidence("runtime_id", runtime_id),
        evidence("provider_id", provider_id),
        evidence("package_section_count", package_sections.len()),
        evidence("release_check_count", release_checks.len()),
    ];
    release_evidence.extend(behavioral_evidence);

    ReleaseCheckRecord {
        id: "runtime_provider.native_vn".to_string(),
        domain: ReleaseDomain::Runtime,
        status: CheckStatus::Pass,
        summary: "NativeVN runtime provider completed package-bound lifecycle conformance"
            .to_string(),
        diagnostic: None,
        evidence: release_evidence,
    }
}

fn native_vn_behavioral_evidence(
    package: &PackageReader,
) -> Result<Vec<ReleaseEvidence>, (&'static str, String)> {
    let compiled = decode_compiled_story(package).map_err(|err| {
        (
            "ASTRA_RUNTIME_PROVIDER_BEHAVIOR_PACKAGE",
            format!("decode vn.compiled_story for provider conformance: {err}"),
        )
    })?;
    let mut provider = NativeVnRuntimeProvider::default();
    let open = provider
        .open_compiled_story(
            compiled.clone(),
            VnRunConfig::classic("und"),
            RuntimeOpenRequest {
                target_id: "release.conformance".to_string(),
                profile: "classic".to_string(),
                locale: "und".to_string(),
                seed: 0xA57A,
                package_hash: compiled.story_hash.to_string(),
                sections: Vec::new(),
            },
        )
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_OPEN", err.to_string()))?;
    provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 0,
            action: "launch_default".to_string(),
            payload: serde_json::json!({}),
        })
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STEP", err.to_string()))?;
    let before_state = provider
        .state(&open.session_id)
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STATE", err.to_string()))?;
    let before_hash = astra_core::Hash128::from_blake3(
        &postcard::to_allocvec(&before_state)
            .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STATE", err.to_string()))?,
    );
    let runtime_snapshot = provider
        .runtime_snapshot(&open.session_id)
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STATE", err.to_string()))?;
    let save = provider
        .save(RuntimeSaveRequest {
            session_id: open.session_id.clone(),
            slot: "release.conformance".to_string(),
        })
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_SAVE", err.to_string()))?;
    let save_section_count = save.sections.len();
    provider
        .restore(RuntimeRestoreRequest {
            session_id: open.session_id.clone(),
            sections: save.sections,
        })
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_RESTORE", err.to_string()))?;
    let restored_state = provider
        .state(&open.session_id)
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STATE", err.to_string()))?;
    let restored_hash = astra_core::Hash128::from_blake3(
        &postcard::to_allocvec(&restored_state)
            .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_STATE", err.to_string()))?,
    );
    if before_hash != restored_hash {
        return Err((
            "ASTRA_RUNTIME_PROVIDER_BEHAVIOR_RESTORE_HASH",
            "provider restore did not reproduce the saved VN state hash".to_string(),
        ));
    }
    let event_bytes = postcard::to_allocvec(runtime_snapshot.events.trace()).map_err(|err| {
        (
            "ASTRA_RUNTIME_PROVIDER_BEHAVIOR_EVENT_HASH",
            err.to_string(),
        )
    })?;
    let presentation_bytes =
        postcard::to_allocvec(&(runtime_snapshot.presentation, runtime_snapshot.effects)).map_err(
            |err| {
                (
                    "ASTRA_RUNTIME_PROVIDER_BEHAVIOR_PRESENTATION_HASH",
                    err.to_string(),
                )
            },
        )?;
    provider
        .shutdown(open.session_id)
        .map_err(|err| ("ASTRA_RUNTIME_PROVIDER_BEHAVIOR_SHUTDOWN", err.to_string()))?;
    Ok(vec![
        evidence("behavior_state_hash", before_hash),
        evidence(
            "behavior_event_hash",
            astra_core::Hash128::from_blake3(&event_bytes),
        ),
        evidence(
            "behavior_presentation_hash",
            astra_core::Hash128::from_blake3(&presentation_bytes),
        ),
        evidence("behavior_save_section_count", save_section_count),
    ])
}

fn select_game_target<'a>(
    manifest: &'a TargetManifest,
    selected_target: Option<&str>,
) -> Option<&'a astra_target::TargetDescriptor> {
    if let Some(selected) = selected_target {
        return manifest
            .targets
            .iter()
            .find(|target| target.id == selected && target.kind == TargetKind::Game);
    }
    manifest
        .targets
        .iter()
        .find(|target| target.kind == TargetKind::Game)
}

fn vfs_checks(package: &PackageReader) -> Vec<ReleaseCheckRecord> {
    vec![
        vfs_uri_format_check(package),
        vfs_prefix_registry_check(package),
        vfs_package_mount_check(package),
        vfs_overlay_mount_check(package),
        vfs_catalog_check(package),
    ]
}

fn vfs_uri_format_check(package: &PackageReader) -> ReleaseCheckRecord {
    if package.has_section("asset.registry") {
        return package_blocked(
            "vfs.uri_format",
            "ASTRA_VFS_ASSET_REGISTRY_REMOVED",
            "legacy asset.registry section is not accepted after VFS migration",
            vec![evidence("section", "asset.registry")],
        );
    }

    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.uri_format"),
    };
    for entry in &manifest.entries {
        if let Err(err) = VfsUri::parse(entry.uri.as_str()) {
            return package_blocked(
                "vfs.uri_format",
                "ASTRA_VFS_URI_INVALID",
                format!("VFS entry URI is invalid: {err}"),
                vec![evidence("vfs_uri", entry.uri.as_str())],
            );
        }
    }
    for whiteout in &manifest.whiteouts {
        if let Err(err) = VfsUri::parse(whiteout.uri.as_str()) {
            return package_blocked(
                "vfs.uri_format",
                "ASTRA_VFS_URI_INVALID",
                format!("VFS whiteout URI is invalid: {err}"),
                vec![evidence("vfs_uri", whiteout.uri.as_str())],
            );
        }
    }

    ReleaseCheckRecord {
        id: "vfs.uri_format".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "all VFS locators use provider:/path format".to_string(),
        diagnostic: None,
        evidence: vec![evidence("entry_count", manifest.entries.len())],
    }
}

fn vfs_prefix_registry_check(package: &PackageReader) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.prefix_registry"),
    };
    if let Some(path) = forbidden_vfs_report_field(package, "asset.vfs_manifest") {
        return package_blocked(
            "vfs.prefix_registry",
            "ASTRA_VFS_REPORT_PAYLOAD_LEAK",
            "asset.vfs_manifest contains a local-root or payload-like field",
            vec![evidence("field", path)],
        );
    }
    if manifest.prefixes.is_empty() {
        return package_blocked(
            "vfs.prefix_registry",
            "ASTRA_VFS_PREFIX_MISSING",
            "asset.vfs_manifest must declare at least one prefix",
            vec![evidence("section", "asset.vfs_manifest")],
        );
    }
    let validation = manifest.validate();
    if let Some(diagnostic) = validation.first() {
        return ReleaseCheckRecord {
            id: "vfs.prefix_registry".to_string(),
            domain: ReleaseDomain::Package,
            status: CheckStatus::Blocked,
            summary: "asset VFS prefix registry is invalid".to_string(),
            diagnostic: Some(diagnostic.clone()),
            evidence: vec![evidence("prefix_count", manifest.prefixes.len())],
        };
    }

    let providers = match vfs_registered_providers(package) {
        Ok(providers) => providers,
        Err(check) => return (*check).with_id("vfs.prefix_registry"),
    };
    let mut seen_prefixes = BTreeSet::new();
    for prefix in &manifest.prefixes {
        if !seen_prefixes.insert(prefix.prefix.as_str()) {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PREFIX_CONFLICT",
                format!("VFS prefix {} is declared more than once", prefix.prefix),
                vec![evidence("prefix", &prefix.prefix)],
            );
        }
        let Some(provider) = providers.get(prefix.provider_id.as_str()) else {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PROVIDER_MISSING",
                format!(
                    "VFS prefix {} provider {} is not registered in vfs_provider slot",
                    prefix.prefix, prefix.provider_id
                ),
                vec![
                    evidence("prefix", &prefix.prefix),
                    evidence("provider_id", &prefix.provider_id),
                ],
            );
        };
        if !provider.packaged {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PROVIDER_UNPACKAGED",
                format!(
                    "VFS prefix {} provider {} is not packaged eligible",
                    prefix.prefix, prefix.provider_id
                ),
                vec![
                    evidence("prefix", &prefix.prefix),
                    evidence("provider_id", &prefix.provider_id),
                ],
            );
        }
        let required_capability = vfs_backend_capability(prefix.backend);
        if !provider
            .capability
            .as_deref()
            .is_some_and(|capability| vfs_capability_matches(capability, required_capability))
        {
            return package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_PROVIDER_CAPABILITY_MISMATCH",
                format!(
                    "VFS prefix {} provider {} does not match backend capability {}",
                    prefix.prefix, prefix.provider_id, required_capability
                ),
                vec![
                    evidence("prefix", &prefix.prefix),
                    evidence("provider_id", &prefix.provider_id),
                    evidence("required_capability", required_capability),
                ],
            );
        }
    }

    ReleaseCheckRecord {
        id: "vfs.prefix_registry".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "VFS prefixes bind to packaged vfs_provider registrations".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("prefix_count", manifest.prefixes.len()),
            evidence("provider_count", providers.len()),
        ],
    }
}

fn vfs_package_mount_check(package: &PackageReader) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.package_mount"),
    };
    let layers = manifest
        .layers
        .iter()
        .map(|layer| (layer.layer_id.as_str(), layer))
        .collect::<BTreeMap<_, _>>();
    for entry in &manifest.entries {
        if let Some(layer) = layers.get(entry.layer_id.as_str()) {
            if !layer.profiles.is_empty()
                && !layer.profiles.iter().any(|profile| !profile.is_empty())
            {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_PROFILE_INVALID",
                    "VFS layer profile eligibility contains an empty profile",
                    vec![evidence("layer_id", &entry.layer_id)],
                );
            }
        }
        if let VfsSourceRef::PackageSection { section_id } = &entry.source {
            let Some(section) = package.container().section_entry(section_id) else {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_PACKAGE_SECTION_MISSING",
                    format!("VFS package entry references missing section {section_id}"),
                    vec![
                        evidence("vfs_uri", entry.uri.as_str()),
                        evidence("section_id", section_id),
                    ],
                );
            };
            if entry.offset != 0 || entry.size != section.decoded_length {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_BOUNDS_INVALID",
                    format!("VFS package entry bounds do not match section {section_id}"),
                    vec![
                        evidence("vfs_uri", entry.uri.as_str()),
                        evidence("section_id", section_id),
                        evidence("entry_size", entry.size),
                        evidence("section_size", section.decoded_length),
                    ],
                );
            }
            if entry.hash != section.hash {
                return package_blocked(
                    "vfs.package_mount",
                    "ASTRA_VFS_HASH_MISMATCH",
                    format!("VFS package entry hash does not match section {section_id}"),
                    vec![
                        evidence("vfs_uri", entry.uri.as_str()),
                        evidence("section_id", section_id),
                    ],
                );
            }
        }
    }

    ReleaseCheckRecord {
        id: "vfs.package_mount".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "package-backed VFS entries match container section bounds and hashes".to_string(),
        diagnostic: None,
        evidence: vec![evidence("entry_count", manifest.entries.len())],
    }
}

fn vfs_overlay_mount_check(package: &PackageReader) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.overlay_mount"),
    };
    let layer_ids = manifest
        .layers
        .iter()
        .map(|layer| layer.layer_id.as_str())
        .collect::<BTreeSet<_>>();
    for whiteout in &manifest.whiteouts {
        if !layer_ids.contains(whiteout.layer_id.as_str())
            || whiteout.allowlist_id.trim().is_empty()
            || whiteout.reason.trim().is_empty()
        {
            return package_blocked(
                "vfs.overlay_mount",
                "ASTRA_VFS_WHITEOUT_UNAUTHORIZED",
                "VFS whiteout requires a valid layer, allowlist and reason",
                vec![
                    evidence("vfs_uri", whiteout.uri.as_str()),
                    evidence("layer_id", &whiteout.layer_id),
                ],
            );
        }
    }
    ReleaseCheckRecord {
        id: "vfs.overlay_mount".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "VFS overlay layers and whiteouts are explicitly authorized".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("layer_count", manifest.layers.len()),
            evidence("whiteout_count", manifest.whiteouts.len()),
        ],
    }
}

fn vfs_catalog_check(package: &PackageReader) -> ReleaseCheckRecord {
    let manifest = match decode_vfs_manifest(package) {
        Ok(manifest) => manifest,
        Err(check) => return (*check).with_id("vfs.catalog"),
    };
    let catalog = match decode_asset_catalog(package) {
        Ok(catalog) => catalog,
        Err(check) => return (*check).with_id("vfs.catalog"),
    };
    if let Some(path) = forbidden_vfs_report_field(package, "asset.catalog") {
        return package_blocked(
            "vfs.catalog",
            "ASTRA_VFS_CATALOG_PAYLOAD_LEAK",
            "asset.catalog contains a local-root or payload-like field",
            vec![evidence("field", path)],
        );
    }
    if catalog.schema != "astra.asset_catalog.v1" {
        return package_blocked(
            "vfs.catalog",
            "ASTRA_VFS_CATALOG_SCHEMA",
            "asset.catalog schema must be astra.asset_catalog.v1",
            vec![evidence("schema", catalog.schema)],
        );
    }
    let entry_uris = manifest
        .entries
        .iter()
        .map(|entry| entry.uri.as_str())
        .collect::<BTreeSet<_>>();
    let mut asset_ids = BTreeSet::new();
    for asset in &catalog.assets {
        if asset.asset_id.trim().is_empty() || !asset_ids.insert(asset.asset_id.as_str()) {
            return package_blocked(
                "vfs.catalog",
                "ASTRA_VFS_CATALOG_ASSET_ID",
                "asset.catalog asset ids must be non-empty and unique",
                vec![evidence("asset_id", &asset.asset_id)],
            );
        }
        if !entry_uris.contains(asset.uri.as_str()) {
            return package_blocked(
                "vfs.catalog",
                "ASTRA_VFS_CATALOG_URI_MISSING",
                format!(
                    "asset.catalog references VFS URI {} without an entry",
                    asset.uri
                ),
                vec![evidence("vfs_uri", asset.uri.as_str())],
            );
        }
    }

    ReleaseCheckRecord {
        id: "vfs.catalog".to_string(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Pass,
        summary: "asset catalog references package VFS entries".to_string(),
        diagnostic: None,
        evidence: vec![evidence("asset_count", catalog.assets.len())],
    }
}

fn decode_vfs_manifest(package: &PackageReader) -> Result<VfsManifest, Box<ReleaseCheckRecord>> {
    let bytes = package
        .container()
        .read_bounded("asset.vfs_manifest", 1024 * 1024)
        .map_err(|err| {
            Box::new(package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_MANIFEST_MISSING",
                format!("asset.vfs_manifest section is required: {err}"),
                vec![evidence("section", "asset.vfs_manifest")],
            ))
        })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        Box::new(package_blocked(
            "vfs.prefix_registry",
            "ASTRA_VFS_MANIFEST_JSON",
            format!("asset.vfs_manifest is not valid JSON: {err}"),
            vec![evidence("section", "asset.vfs_manifest")],
        ))
    })
}

fn decode_asset_catalog(package: &PackageReader) -> Result<AssetCatalog, Box<ReleaseCheckRecord>> {
    let bytes = package
        .container()
        .read_bounded("asset.catalog", 1024 * 1024)
        .map_err(|err| {
            Box::new(package_blocked(
                "vfs.catalog",
                "ASTRA_VFS_CATALOG_MISSING",
                format!("asset.catalog section is required: {err}"),
                vec![evidence("section", "asset.catalog")],
            ))
        })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        Box::new(package_blocked(
            "vfs.catalog",
            "ASTRA_VFS_CATALOG_JSON",
            format!("asset.catalog is not valid JSON: {err}"),
            vec![evidence("section", "asset.catalog")],
        ))
    })
}

#[derive(Debug, Clone)]
struct RegisteredVfsProvider {
    capability: Option<String>,
    packaged: bool,
}

fn vfs_registered_providers(
    package: &PackageReader,
) -> Result<BTreeMap<String, RegisteredVfsProvider>, Box<ReleaseCheckRecord>> {
    let registry =
        read_json_section(package, "plugin.extension_registry").map_err(|(code, message)| {
            Box::new(plugin_blocked(
                "vfs.prefix_registry",
                code,
                message,
                vec![evidence("section", "plugin.extension_registry")],
            ))
        })?;
    let mut providers = BTreeMap::new();
    for provider in registry
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if provider.get("slot").and_then(serde_json::Value::as_str) != Some("vfs_provider") {
            continue;
        }
        let Some(provider_id) = provider
            .get("provider_id")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        providers.insert(
            provider_id.to_string(),
            RegisteredVfsProvider {
                capability: provider
                    .get("capability")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                packaged: provider
                    .get("packaged")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            },
        );
    }
    Ok(providers)
}

fn vfs_backend_capability(backend: VfsBackendKind) -> &'static str {
    match backend {
        VfsBackendKind::Package => "vfs.backend.package",
        VfsBackendKind::LocalAuthorized => "vfs.backend.local_authorized",
        VfsBackendKind::Overlay => "vfs.backend.overlay",
        VfsBackendKind::Memory => "vfs.backend.memory",
        VfsBackendKind::LegacyPack => "vfs.backend.legacy_pack",
    }
}

fn vfs_capability_matches(actual: &str, required: &str) -> bool {
    actual == required
        || (required == "vfs.backend.local_authorized" && actual == "vfs.backend.local")
        || actual
            .strip_prefix(required)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn forbidden_vfs_report_field(package: &PackageReader, section: &str) -> Option<String> {
    let bytes = package
        .container()
        .read_bounded(section, 1024 * 1024)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    forbidden_vfs_field_path(&value)
}

fn forbidden_vfs_field_path(value: &serde_json::Value) -> Option<String> {
    fn walk(value: &serde_json::Value, path: &mut Vec<String>, found: &mut Option<String>) {
        if found.is_some() {
            return;
        }
        match value {
            serde_json::Value::Object(object) => {
                for (key, child) in object {
                    path.push(key.clone());
                    if is_forbidden_vfs_key(key, path, child) {
                        *found = Some(path.join("."));
                        return;
                    }
                    walk(child, path, found);
                    path.pop();
                }
            }
            serde_json::Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    path.push(index.to_string());
                    walk(child, path, found);
                    path.pop();
                }
            }
            serde_json::Value::String(value) if looks_like_host_absolute_path(value) => {
                *found = Some(path.join("."));
            }
            _ => {}
        }
    }

    let mut path = Vec::new();
    let mut found = None;
    walk(value, &mut path, &mut found);
    found
}

fn is_forbidden_vfs_key(key: &str, path: &[String], value: &serde_json::Value) -> bool {
    if key == "payload" {
        return !(path.len() == 2
            && path[0] == "redaction"
            && path[1] == "payload"
            && value.as_str() == Some("omitted"));
    }
    matches!(
        key,
        "root"
            | "host_root"
            | "local_root"
            | "absolute_path"
            | "text"
            | "script_text"
            | "source_text"
            | "content"
            | "payload_bytes"
            | "raw_payload"
            | "source_payload"
            | "commercial_text"
            | "bytecode"
            | "bytes"
    )
}

fn looks_like_host_absolute_path(value: &str) -> bool {
    let normalized = value.replace('\\', "/");
    normalized.starts_with('/')
        || normalized.starts_with("~/")
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':')
}

fn package_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Blocked,
        diagnostic: Some(Diagnostic::blocking(code, summary.clone())),
        summary,
        evidence: evidence_values,
    }
}

trait ReleaseCheckRecordExt {
    fn with_id(self, id: &'static str) -> Self;
}

impl ReleaseCheckRecordExt for ReleaseCheckRecord {
    fn with_id(mut self, id: &'static str) -> Self {
        self.id = id.to_string();
        self
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

fn runtime_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Runtime,
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
    let compiled = match decode_compiled_story(package) {
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
    if compiled.schema != "astra.vn.compiled_story"
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
    let compiled = match decode_compiled_story(package) {
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
        evidence("target", &report.target),
        evidence("profile_id", &report.profile_id),
        evidence("profile_hash", &report.profile_hash),
        evidence("build_fingerprint", &report.build_fingerprint),
        evidence(
            "sdk_status",
            format!("{:?}", report.sdk_status).to_lowercase(),
        ),
    ];
    for (domain, selection) in [
        ("renderer", &report.renderer),
        ("decode", &report.decode),
        ("audio", &report.audio),
        ("save", &report.save),
    ] {
        evidence_values.push(evidence(
            format!("provider.{domain}.declared"),
            selection.declared.join(","),
        ));
        evidence_values.push(evidence(
            format!("provider.{domain}.available"),
            selection.available.join(","),
        ));
        evidence_values.push(evidence(
            format!("provider.{domain}.selected"),
            selection.selected.as_deref().unwrap_or_default(),
        ));
    }

    ReleaseCheckRecord {
        id: "platform.capability_report".to_string(),
        domain: ReleaseDomain::Platform,
        status: match status {
            PlatformValidationStatus::Pass => CheckStatus::Pass,
            PlatformValidationStatus::Warning => CheckStatus::Warning,
            PlatformValidationStatus::Blocked => CheckStatus::Blocked,
        },
        summary: "platform capability report binds declared, available and selected providers"
            .to_string(),
        diagnostic: diagnostics.first().cloned(),
        evidence: evidence_values,
    }
}

fn platform_profile_binding_check(
    package: &PackageReader,
    capability: Option<&PlatformCapabilityReport>,
    required: bool,
) -> ReleaseCheckRecord {
    let blocked = |code: &'static str, summary: &'static str| ReleaseCheckRecord {
        id: "platform.profile_binding".to_string(),
        domain: ReleaseDomain::Platform,
        status: if required {
            CheckStatus::Blocked
        } else {
            CheckStatus::Warning
        },
        summary: summary.to_string(),
        diagnostic: Some(if required {
            Diagnostic::blocking(code, summary)
        } else {
            Diagnostic::warning(code, summary)
        }),
        evidence: Vec::new(),
    };
    if !package.has_section("platform.profiles") {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_SECTION_MISSING",
            "package does not contain cooked platform profiles",
        );
    }
    let Some(capability) = capability else {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_CAPABILITY_MISSING",
            "platform profile binding requires a capability report",
        );
    };
    let value = match read_json_section(package, "platform.profiles") {
        Ok(value) => value,
        Err(_) => {
            return blocked(
                "ASTRA_PLATFORM_PROFILE_SECTION_INVALID",
                "cooked platform profile section is invalid",
            )
        }
    };
    if !matches!(
        value.get("schema").and_then(serde_json::Value::as_str),
        Some("astra.platform_profiles.v1" | "astra.platform_profiles.v2")
    ) {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_SECTION_INVALID",
            "cooked platform profile section schema is unsupported",
        );
    }
    let profiles: Vec<astra_platform::PlatformHostProfile> = match value
        .get("profiles")
        .cloned()
        .and_then(|profiles| serde_json::from_value::<Vec<serde_json::Value>>(profiles).ok())
        .and_then(|profiles| {
            profiles
                .into_iter()
                .map(astra_platform::migrate_host_profile_json)
                .collect::<Result<Vec<_>, _>>()
                .ok()
        }) {
        Some(profiles) => profiles,
        None => {
            return blocked(
                "ASTRA_PLATFORM_PROFILE_SECTION_INVALID",
                "cooked platform profile list is invalid",
            )
        }
    };
    let matched = profiles.iter().find(|profile| {
        profile.platform == capability.platform
            && profile.target == capability.target
            && profile.id == capability.profile_id
            && profile.hash().ok().as_deref() == Some(capability.profile_hash.as_str())
    });
    let Some(profile) = matched else {
        return blocked(
            "ASTRA_PLATFORM_PROFILE_IDENTITY",
            "capability report does not match a cooked platform profile",
        );
    };
    if let Err(error) = astra_platform::validate_host_profile(profile) {
        return ReleaseCheckRecord {
            id: "platform.profile_binding".to_string(),
            domain: ReleaseDomain::Platform,
            status: CheckStatus::Blocked,
            summary: "cooked platform profile violates release policy".to_string(),
            diagnostic: Some(Diagnostic::blocking(
                "ASTRA_PLATFORM_PROFILE_POLICY",
                error.to_string(),
            )),
            evidence: Vec::new(),
        };
    }
    ReleaseCheckRecord {
        id: "platform.profile_binding".to_string(),
        domain: ReleaseDomain::Platform,
        status: CheckStatus::Pass,
        summary: "capability report matches the cooked platform profile".to_string(),
        diagnostic: None,
        evidence: vec![
            evidence("profile_id", &capability.profile_id),
            evidence("profile_hash", &capability.profile_hash),
        ],
    }
}

fn evidence(key: impl Into<String>, value: impl ToString) -> ReleaseEvidence {
    ReleaseEvidence {
        key: key.into(),
        value: value.to_string(),
    }
}
