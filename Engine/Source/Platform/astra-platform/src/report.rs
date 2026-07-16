use std::collections::BTreeSet;

use astra_core::{Diagnostic, DiagnosticSeverity};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{PlatformError, PlatformHostProfile, PlatformId, SdkStatus};

pub const PLATFORM_CAPABILITY_REPORT_SCHEMA: &str = "astra.platform_capability_report.v2";
pub const PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA: &str =
    "astra.platform_host_conformance_report.v1";

pub fn build_fingerprint(
    crate_name: &str,
    crate_version: &str,
    features: impl IntoIterator<Item = impl AsRef<str>>,
) -> String {
    let features = features
        .into_iter()
        .map(|feature| feature.as_ref().to_string())
        .collect::<BTreeSet<_>>();
    let identity = format!(
        "{crate_name}\n{crate_version}\n{}",
        features.into_iter().collect::<Vec<_>>().join(",")
    );
    astra_core::Hash256::from_sha256(identity.as_bytes()).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilitySelection {
    pub declared: Vec<String>,
    pub available: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
}

impl CapabilitySelection {
    fn resolve(declared: &[String], available: &BTreeSet<String>) -> Self {
        let selected = declared
            .iter()
            .find(|provider| available.contains(provider.as_str()))
            .cloned();
        Self {
            declared: declared.to_vec(),
            available: declared
                .iter()
                .filter(|provider| available.contains(provider.as_str()))
                .cloned()
                .collect(),
            selected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlatformCapabilityReport {
    pub schema: String,
    pub platform: PlatformId,
    pub target: String,
    pub profile_id: String,
    pub profile_hash: String,
    pub build_fingerprint: String,
    pub sdk_status: SdkStatus,
    pub renderer: CapabilitySelection,
    pub decode: CapabilitySelection,
    pub audio: CapabilitySelection,
    pub save: CapabilitySelection,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

impl PlatformCapabilityReport {
    pub fn from_profile(
        profile: &PlatformHostProfile,
        build_fingerprint: impl Into<String>,
        available: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, PlatformError> {
        let available = available
            .into_iter()
            .map(Into::into)
            .collect::<BTreeSet<_>>();
        Ok(Self {
            schema: PLATFORM_CAPABILITY_REPORT_SCHEMA.to_string(),
            platform: profile.platform,
            target: profile.target.clone(),
            profile_id: profile.id.clone(),
            profile_hash: profile.hash()?,
            build_fingerprint: build_fingerprint.into(),
            sdk_status: SdkStatus::Present,
            renderer: CapabilitySelection::resolve(&profile.renderer.providers, &available),
            decode: CapabilitySelection::resolve(&profile.decode.providers, &available),
            audio: CapabilitySelection::resolve(&profile.audio.providers, &available),
            save: CapabilitySelection::resolve(&profile.save.providers, &available),
            diagnostics: Vec::new(),
        })
    }

    pub fn unavailable(
        platform: PlatformId,
        target: Option<&str>,
        sdk_status: SdkStatus,
        build_fingerprint: impl Into<String>,
    ) -> Self {
        let identity = format!("{}:{}:unavailable", platform, target.unwrap_or_default());
        let empty = || CapabilitySelection {
            declared: Vec::new(),
            available: Vec::new(),
            selected: None,
        };
        Self {
            schema: PLATFORM_CAPABILITY_REPORT_SCHEMA.to_string(),
            platform,
            target: target.unwrap_or_default().to_string(),
            profile_id: "stage6-unavailable".to_string(),
            profile_hash: astra_core::Hash256::from_sha256(identity.as_bytes()).to_string(),
            build_fingerprint: build_fingerprint.into(),
            sdk_status,
            renderer: empty(),
            decode: empty(),
            audio: empty(),
            save: empty(),
            diagnostics: vec![Diagnostic::blocking(
                "ASTRA_PLATFORM_NOT_IMPLEMENTED",
                "platform host is not implemented in this release",
            )
            .with_field("platform", platform.as_str())],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConformanceCheck {
    pub id: String,
    pub status: ConformanceStatus,
    pub evidence: Vec<ConformanceEvidence>,
}

impl ConformanceCheck {
    pub fn pass(
        id: impl Into<String>,
        evidence: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            id: id.into(),
            status: ConformanceStatus::Pass,
            evidence: evidence
                .into_iter()
                .map(|(key, value)| ConformanceEvidence {
                    key: key.into(),
                    value: value.into(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConformanceEvidence {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlatformHostConformanceReport {
    pub schema: String,
    pub status: ConformanceStatus,
    pub platform: PlatformId,
    pub target: String,
    pub profile_hash: String,
    pub package_hash: String,
    pub build_fingerprint: String,
    pub session_id: String,
    pub checks: Vec<ConformanceCheck>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlatformValidationStatus {
    Pass,
    Warning,
    Blocked,
}

pub fn validate_capability_report(
    report: &PlatformCapabilityReport,
) -> (PlatformValidationStatus, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if report.schema != PLATFORM_CAPABILITY_REPORT_SCHEMA {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_PLATFORM_SCHEMA",
            "platform capability report schema must be v2",
        ));
    }
    if report.sdk_status != SdkStatus::Present {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_PLATFORM_SDK_MISSING",
            "platform SDK is unavailable",
        ));
    }
    if !report.profile_hash.starts_with("sha256:")
        || !report.build_fingerprint.starts_with("sha256:")
    {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_PLATFORM_IDENTITY",
            "platform profile and build identities must use sha256",
        ));
    }
    for (domain, selection) in [
        ("renderer", &report.renderer),
        ("decode", &report.decode),
        ("audio", &report.audio),
        ("save", &report.save),
    ] {
        let valid = selection.selected.as_ref().is_some_and(|selected| {
            selection.declared.contains(selected) && selection.available.contains(selected)
        });
        if !valid {
            diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_PROVIDER_UNAVAILABLE",
                    "declared platform provider is unavailable",
                )
                .with_field("domain", domain),
            );
        }
    }
    diagnostics.extend(
        report
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.severity,
                    DiagnosticSeverity::Blocking
                        | DiagnosticSeverity::Error
                        | DiagnosticSeverity::Warning
                )
            })
            .cloned(),
    );
    (validation_status(&diagnostics), diagnostics)
}

pub fn required_conformance_checks(platform: PlatformId) -> &'static [&'static str] {
    match platform {
        PlatformId::Windows => &[
            "host.lifecycle",
            "window.create_destroy",
            "surface.present_readback",
            "input.native_consumption",
            "audio.output_meter",
            "decode.platform",
            "save.atomic_reopen",
            "package.hash_range",
            "resource.zero_leaks",
        ],
        PlatformId::Web => &[
            "host.lifecycle",
            "window.canvas",
            "surface.webgpu_present_readback",
            "input.dom_consumption",
            "audio.webaudio_meter",
            "decode.webcodecs",
            "save.opfs_atomic_reopen",
            "package.hash_range",
            "resource.zero_leaks",
        ],
        PlatformId::Linux => &[
            "host.lifecycle",
            "runtime.steam_sniper",
            "window.wayland_create_destroy",
            "surface.vulkan_present_readback",
            "surface.portal_capture",
            "input.uinput_consumption",
            "input.ime_consumption",
            "input.gamepad_consumption",
            "audio.alsa_output_meter",
            "decode.gstreamer",
            "save.xdg_atomic_reopen",
            "package.portal_authorized",
            "package.hash_range",
            "resource.zero_leaks",
        ],
        PlatformId::Macos => &[
            "host.lifecycle",
            "runtime.macos_13",
            "window.appkit_create_destroy",
            "surface.metal_present_readback",
            "surface.screencapturekit",
            "input.cgevent_consumption",
            "input.ime_consumption",
            "input.gamepad_consumption",
            "accessibility.accesskit",
            "audio.coreaudio_output_meter",
            "decode.avfoundation",
            "save.application_support_atomic_reopen",
            "package.user_authorized",
            "package.hash_range",
            "distribution.codesign",
            "distribution.notarization",
            "resource.zero_leaks",
        ],
        PlatformId::Android => &[
            "host.lifecycle",
            "window.activity",
            "surface.vulkan_present_readback",
            "input.android_consumption",
            "accessibility.talkback_semantics",
            "audio.android_focus_meter",
            "decode.mediacodec_audio_video",
            "save.android_atomic_reopen",
            "package.bundled_saf_hash_range",
            "host.resume_recreate",
            "resource.zero_leaks",
        ],
        PlatformId::Ios => &[],
    }
}

pub fn validate_conformance_report(
    capability: &PlatformCapabilityReport,
    report: &PlatformHostConformanceReport,
) -> (PlatformValidationStatus, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if report.schema != PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_PLATFORM_CONFORMANCE_SCHEMA",
            "platform host conformance report schema is unsupported",
        ));
    }
    if report.status != ConformanceStatus::Pass {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_PLATFORM_CONFORMANCE_STATUS",
            "platform host conformance report did not pass",
        ));
    }
    if report.platform != capability.platform
        || report.target != capability.target
        || report.profile_hash != capability.profile_hash
        || report.build_fingerprint != capability.build_fingerprint
        || !report.package_hash.starts_with("sha256:")
        || report.session_id.is_empty()
    {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_PLATFORM_CONFORMANCE_IDENTITY",
            "platform capability and conformance identities do not match",
        ));
    }
    for required in required_conformance_checks(report.platform) {
        match report.checks.iter().find(|check| check.id == *required) {
            Some(check)
                if check.status == ConformanceStatus::Pass && !check.evidence.is_empty() => {}
            _ => diagnostics.push(
                Diagnostic::blocking(
                    "ASTRA_PLATFORM_CONFORMANCE_CHECK",
                    "required platform host conformance check is missing or blocked",
                )
                .with_field("check", *required),
            ),
        }
    }
    diagnostics.extend(report.diagnostics.iter().cloned());
    (validation_status(&diagnostics), diagnostics)
}

fn validation_status(diagnostics: &[Diagnostic]) -> PlatformValidationStatus {
    if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            DiagnosticSeverity::Blocking | DiagnosticSeverity::Error
        )
    }) {
        PlatformValidationStatus::Blocked
    } else if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
    {
        PlatformValidationStatus::Warning
    } else {
        PlatformValidationStatus::Pass
    }
}
