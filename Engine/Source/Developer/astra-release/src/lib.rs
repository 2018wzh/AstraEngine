use std::collections::{BTreeMap, BTreeSet};

use astra_asset::{AssetCatalog, ResolveContext, VfsManifest, VfsSourceRef, VfsUri};
use astra_core::{
    validate_performance_report, Diagnostic, Hash256, PerformanceBudget, PerformanceReport,
};
use astra_media::{
    FontBindingContext, FontPackageManifest, TextLayoutConfig, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_package::{CookSummaryManifest, PackageManifest, PackageReader};
use astra_platform::{
    PlatformCapabilityReport, PlatformHostConformanceReport, PlatformValidationStatus,
};
use astra_platform_android::AndroidBundleManifest;
use astra_player_core::{
    PlayerAutomationReport, PlayerAutomationStatus, PlayerPlatform, PlayerPresentationReport,
    PLAYER_PRESENTATION_REPORT_SCHEMA,
};
use astra_plugin_abi::{PluginExtensionRegistrySnapshot, ProviderPolicy};
use astra_target::{validate_manifest, TargetKind, TargetManifest, TargetValidationStatus};
use astra_vn::VnHostLimits;
use astra_vn_package::{
    decode_compiled_project, load_player_locale_config, load_presentation_provider_manifest,
    VnAdvancedPresentationManifest, VnCommercialBaselineManifest, VnExtensionManifest,
    VnProfileManifest, VnStandardCommandManifest,
};
use astra_vn_policy::{VnPolicyBundleManifest, VnPolicyBundleSourceCache};

use astra_vn_script::{
    SystemStoryManifest, SystemStoryValidationStatus, VnSystemUiProfileManifest,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod checks_headless;
mod checks_media;
mod checks_player;
mod checks_plugin;
mod checks_target;
mod checks_tsui;
mod checks_vfs;
mod checks_vn;
use checks_headless::*;
use checks_media::*;
use checks_player::*;
use checks_plugin::*;
use checks_target::*;
use checks_tsui::*;
use checks_vfs::*;
use checks_vn::*;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductPerformanceEvidence {
    pub budget: PerformanceBudget,
    pub report: PerformanceReport,
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
    pub fn validate_android_bundle_manifest(
        &self,
        manifest: &AndroidBundleManifest,
        package_hash: &str,
        target: &str,
        profile: &str,
        conformance_report: Option<&PlatformHostConformanceReport>,
        player_report: Option<&PlayerAutomationReport>,
    ) -> ReleaseCheckRecord {
        let manifest_valid = manifest.validate().is_ok()
            && manifest.package_hash == package_hash
            && manifest.target == target
            && manifest.profile == profile;
        let conformance_valid = conformance_report.is_some_and(|report| {
            report.platform == astra_platform::PlatformId::Android
                && report.package_hash == manifest.package_hash
                && report.build_fingerprint == manifest.build_fingerprint
                && report.target == manifest.target
                && report.status == astra_platform::ConformanceStatus::Pass
        });
        let player_valid = player_report.is_some_and(|report| {
            report.platform == PlayerPlatform::Android
                && report.package_hash == manifest.package_hash
                && report.target == manifest.target
                && report.profile == manifest.profile
                && report.full_playable_passed()
        });
        if manifest_valid && conformance_valid && player_valid {
            ReleaseCheckRecord {
                id: "android.bundle_identity".to_string(),
                domain: ReleaseDomain::Platform,
                status: CheckStatus::Pass,
                summary: "Android APK/AAB identity is bound to live host and Player evidence"
                    .to_string(),
                diagnostic: None,
                evidence: vec![
                    ReleaseEvidence {
                        key: "build_fingerprint".to_string(),
                        value: manifest.build_fingerprint.clone(),
                    },
                    ReleaseEvidence {
                        key: "package_hash".to_string(),
                        value: manifest.package_hash.clone(),
                    },
                ],
            }
        } else {
            ReleaseCheckRecord {
                id: "android.bundle_identity".to_string(),
                domain: ReleaseDomain::Platform,
                status: CheckStatus::Blocked,
                summary: "Android bundle, host, and Player identities are incomplete or differ"
                    .to_string(),
                diagnostic: Some(Diagnostic::blocking(
                    "ASTRA_ANDROID_RELEASE_IDENTITY_MISMATCH",
                    "Android release requires the same package, target, profile, build, and live session evidence",
                )),
                evidence: Vec::new(),
            }
        }
    }

    pub fn validate_package_with_product_evidence(
        &self,
        request: PackageValidateRequest,
        conformance_report: Option<PlatformHostConformanceReport>,
        player_report: Option<PlayerAutomationReport>,
        presentation_report: Option<PlayerPresentationReport>,
        performance_evidence: Vec<ProductPerformanceEvidence>,
    ) -> Result<ReleaseReport, ReleaseError> {
        let target = request.target.clone();
        let profile = request.profile.clone();
        let capability_report = request.platform_report.clone();
        let mut report = self.validate_package_with_platform_evidence(
            request,
            conformance_report.clone(),
            player_report.clone(),
        )?;
        report.checks.push(product_performance_check(
            &performance_evidence,
            &report.package_hash,
            target.as_deref(),
            &profile,
            capability_report.as_ref(),
            conformance_report.as_ref(),
            player_report.as_ref(),
        ));
        report.checks.push(player_presentation_check(
            presentation_report.as_ref(),
            &report.package_hash,
            target.as_deref(),
            &profile,
            capability_report.as_ref(),
            conformance_report.as_ref(),
            player_report.as_ref(),
        ));
        report.status = release_status(&report.checks);
        Ok(report)
    }

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
        let mut package_hash = Hash256::from_sha256(&request.package_bytes).to_string();
        let mut checks = Vec::new();
        let mut package_id = "unknown".to_string();

        match PackageReader::open(&request.package_bytes) {
            Ok(package) => {
                package_hash = package.package_hash().to_string();
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
                checks.push(headless_shipping_isolation_check(&package));
                checks.push(engine_test_profile_isolation_check(
                    &package,
                    &request.profile,
                ));
                for section in [
                    "schema.registry",
                    "cook.summary",
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
                checks.extend(product_context_checks(
                    &package,
                    request.target.as_deref(),
                    &request.profile,
                ));
                checks.extend(vfs_checks(
                    &package,
                    request.target.as_deref(),
                    &request.profile,
                ));
                checks.push(font_package_check(
                    &package,
                    request.target.as_deref(),
                    &request.profile,
                ));
                checks.push(target_manifest_check(&package, request.target.as_deref()));
                checks.push(headless_release_boundary_check(
                    &package,
                    request.target.as_deref(),
                    &request.profile,
                ));
                checks.push(cooked_project_input_check(
                    &package,
                    &request.profile,
                    package_manifest.as_ref(),
                ));
                checks.push(cook_graph_identity_check(&package));
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
