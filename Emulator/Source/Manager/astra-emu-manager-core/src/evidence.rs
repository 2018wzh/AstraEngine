use std::collections::BTreeMap;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryMigrationEvidence {
    pub schema: String,
    pub from_version: u32,
    pub to_version: u32,
    pub applied_migration_ids: Vec<String>,
    pub database_identity_hash: Hash256,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeCandidateEvidence {
    pub family_id: String,
    pub provider_id: String,
    pub confidence_milli: u16,
    pub marker_hashes: Vec<Hash256>,
    pub blocker_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeEvidence {
    pub schema: String,
    pub case_alias: String,
    pub case_fingerprint: Hash256,
    pub explicit_family_id: Option<String>,
    pub selected_family_id: Option<String>,
    pub candidates: Vec<ProbeCandidateEvidence>,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FvpSyscallCoverageEvidence {
    pub schema: String,
    pub rfvp_revision: String,
    pub release_syscall_total: u16,
    pub covered_syscall_ids: Vec<String>,
    pub missing_syscall_ids: Vec<String>,
    pub opcode_counts: BTreeMap<String, u64>,
    pub full_flow_hash: Option<Hash256>,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FvpParityEvidence {
    pub schema: String,
    pub rfvp_revision: String,
    pub fixture_id: String,
    pub fixture_hash: Hash256,
    pub astra_trace_hash: Hash256,
    pub reference_trace_hash: Hash256,
    pub compared_event_count: u64,
    pub first_divergence_sequence: Option<u64>,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameParityFrameV1 {
    pub frame_index: u64,
    pub semantic_astra_hash: Hash256,
    pub semantic_reference_hash: Hash256,
    pub rgba_astra_hash: Option<Hash256>,
    pub rgba_reference_hash: Option<Hash256>,
    pub audio_astra_hash: Option<Hash256>,
    pub audio_reference_hash: Option<Hash256>,
    pub video_pts: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameParityReportV1 {
    pub schema: String,
    pub reference_revision: String,
    pub reference_observer_patch_hash: Hash256,
    pub build_identity: String,
    pub profile: String,
    pub game_identity_hash: Hash256,
    pub input_sequence_hash: Hash256,
    pub frames: Vec<FrameParityFrameV1>,
    pub compared_event_count: u64,
    pub first_divergence_sequence: Option<u64>,
    pub difference_window_before: u32,
    pub difference_window_after: u32,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TranslationEvidence {
    pub schema: String,
    pub provider_identity: String,
    pub endpoint_identity_hash: Hash256,
    pub model_identity_hash: Hash256,
    pub consent_present: bool,
    pub persistent_cache_enabled: bool,
    pub request_count: u64,
    pub source_hashes: Vec<Hash256>,
    pub latency_ms_total: u64,
    pub error_codes: Vec<String>,
    pub redaction_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetadataEvidenceV1 {
    pub schema: String,
    pub release_use: String,
    pub enabled_providers: Vec<String>,
    pub vndb_commercial_license_id: Option<String>,
    pub consent_separated: bool,
    pub secret_references_only: bool,
    pub sensitive_cover_default: bool,
    pub local_path_present: bool,
    pub query_body_present: bool,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiHostIdentityEvidence {
    pub schema: String,
    pub program_target_id: String,
    pub platform: String,
    pub slint_version: String,
    pub slint_license_mode: String,
    pub wgpu_version: String,
    pub winit_version: String,
    pub renderer_adapter_hash: Hash256,
    pub shared_device_queue: bool,
    pub cpu_frame_readback: bool,
    pub cross_device_texture_copy: bool,
    pub build_identity_hash: Hash256,
    pub package_hash: Hash256,
    pub session_id_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AndroidNativePluginManifest {
    pub schema: String,
    pub apk_signer_digest: Hash256,
    pub package_name: String,
    pub version_code: u64,
    pub abi: String,
    pub family_id: String,
    pub library_file_name: String,
    pub library_hash: Hash256,
    /// Hash of the canonical family identity before the Android native-manifest
    /// hash is attached. This deliberately avoids a circular hash dependency:
    /// the signed family manifest binds this native manifest, while this field
    /// binds the invariant family/plugin/provider/binary identity.
    pub family_base_identity_hash: Hash256,
    pub min_api: u32,
    pub target_api: u32,
}

impl AndroidNativePluginManifest {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn identity_hash(&self) -> Result<Hash256, serde_json::Error> {
        self.canonical_bytes()
            .map(|bytes| Hash256::from_sha256(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmuReleaseManifestV1 {
    pub schema: String,
    pub runtime_provider_id: String,
    pub family_id: String,
    pub family_provider_id: String,
    pub ui_provider_id: String,
    pub required_platforms: Vec<String>,
    pub evidence_sections: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmuProviderBindingEvidenceV1 {
    pub schema: String,
    pub target_id: String,
    pub profile: String,
    pub runtime_provider_id: String,
    pub family_id: String,
    pub family_provider_id: String,
    pub ui_provider_id: String,
    pub engine_fingerprint: String,
    pub rustc_fingerprint: String,
    pub feature_fingerprint: String,
    pub binary_hash: Hash256,
    pub package_eligible: bool,
    pub signer_identity_hash: Hash256,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrustedLuauEvidenceV1 {
    pub schema: String,
    pub policy_id: String,
    pub capability_ids: Vec<String>,
    pub denied_capability_ids: Vec<String>,
    pub evaluated_script_hashes: Vec<Hash256>,
    pub violation_codes: Vec<String>,
    pub commercial_payload_present: bool,
    pub local_path_present: bool,
    pub deterministic_effect_hash: Hash256,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmuPlatformRunEvidenceV1 {
    pub schema: String,
    pub platform: String,
    pub architecture: String,
    pub host_kind: String,
    pub build_identity_hash: Hash256,
    pub profile_hash: Hash256,
    pub package_hash: Hash256,
    pub session_id_hash: Hash256,
    pub input_sequence_hash: Hash256,
    pub consumed_input_trace_hash: Hash256,
    pub visual_trace_hash: Hash256,
    pub audio_meter_hash: Hash256,
    pub route_terminal_hash: Hash256,
    pub lifecycle_steps: Vec<String>,
    pub evidence_level: String,
    pub status: String,
    pub diagnostic_codes: Vec<String>,
}

pub const RFVP_REFERENCE_REVISION: &str = "3b5ea6c96a925c12f95aef8554905e8fecbc77c3";
pub const FVP_RELEASE_SYSCALL_CATALOG_HASH: &str =
    "sha256:c53cb15a5a1fe29d11c8cf8b0cf14a20c2dab7d85dace74f3b35345d5aa97d6a";
pub const EMU_RELEASE_PLATFORMS: [&str; 6] = [
    "android-arm64",
    "android-x86_64",
    "ios",
    "linux",
    "macos",
    "windows",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstraEmuEvidenceBundleV1 {
    pub manifest: EmuReleaseManifestV1,
    pub provider_binding: EmuProviderBindingEvidenceV1,
    pub ui_host_identity: UiHostIdentityEvidence,
    pub fvp_coverage: FvpSyscallCoverageEvidence,
    pub fvp_parity: FvpParityEvidence,
    pub trusted_luau: TrustedLuauEvidenceV1,
    pub translation: TranslationEvidence,
    pub metadata: MetadataEvidenceV1,
    pub platforms: BTreeMap<String, EmuPlatformRunEvidenceV1>,
}

impl AstraEmuEvidenceBundleV1 {
    pub fn validate(&self, target: &str, profile: &str) -> Result<(), String> {
        validate_release_manifest(&self.manifest)?;
        validate_provider_binding(&self.provider_binding, target, profile)?;
        validate_ui_host_identity(&self.ui_host_identity)?;
        validate_fvp_coverage(&self.fvp_coverage)?;
        validate_fvp_parity(&self.fvp_parity)?;
        validate_trusted_luau(&self.trusted_luau)?;
        validate_translation_evidence(&self.translation)?;
        validate_metadata_evidence(&self.metadata)?;
        if self.platforms.len() != EMU_RELEASE_PLATFORMS.len() {
            return Err("ASTRA_EMU_PLATFORM_EVIDENCE_SET".into());
        }
        for platform in EMU_RELEASE_PLATFORMS {
            let value = self
                .platforms
                .get(platform)
                .ok_or_else(|| "ASTRA_EMU_PLATFORM_EVIDENCE_SET".to_owned())?;
            validate_platform_evidence(value, platform)?;
        }
        let package_hash = self.ui_host_identity.package_hash;
        if self
            .platforms
            .values()
            .any(|platform| platform.package_hash != package_hash)
        {
            return Err("ASTRA_EMU_PACKAGE_IDENTITY_DRIFT".into());
        }
        let ui_platform = self
            .platforms
            .get(&self.ui_host_identity.platform)
            .ok_or_else(|| "ASTRA_EMU_UI_PLATFORM_IDENTITY".to_owned())?;
        if ui_platform.build_identity_hash != self.ui_host_identity.build_identity_hash
            || ui_platform.session_id_hash != self.ui_host_identity.session_id_hash
        {
            return Err("ASTRA_EMU_UI_PLATFORM_IDENTITY".into());
        }
        let windows = &self.platforms["windows"];
        let android = &self.platforms["android-x86_64"];
        if windows.input_sequence_hash != android.input_sequence_hash
            || windows.route_terminal_hash != android.route_terminal_hash
        {
            return Err("ASTRA_EMU_CROSS_PLATFORM_RUN_DRIFT".into());
        }
        Ok(())
    }
}

pub fn validate_release_manifest(value: &EmuReleaseManifestV1) -> Result<(), String> {
    let required = [
        "provider_binding",
        "ui_host_identity",
        "fvp_coverage",
        "fvp_parity",
        "trusted_luau",
        "translation",
        "metadata",
    ];
    let platforms = value
        .required_platforms
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if value.schema != "astra.emu.release_manifest.v1"
        || value.runtime_provider_id != "astra.runtime.astraemu"
        || value.family_id != "fvp"
        || value.family_provider_id != "astra.emu.family.fvp"
        || value.ui_provider_id != "astra.emu.ui.slint"
        || required.iter().any(|key| {
            !value
                .evidence_sections
                .get(*key)
                .is_some_and(|id| safe_id(id))
        })
        || platforms != EMU_RELEASE_PLATFORMS.into_iter().collect()
        || value.required_platforms.len() != platforms.len()
    {
        return Err("ASTRA_EMU_RELEASE_MANIFEST_INVALID".into());
    }
    for platform in EMU_RELEASE_PLATFORMS {
        if !value
            .evidence_sections
            .get(&format!("platform.{platform}"))
            .is_some_and(|id| safe_id(id))
        {
            return Err("ASTRA_EMU_RELEASE_MANIFEST_INVALID".into());
        }
    }
    Ok(())
}

pub fn validate_metadata_evidence(value: &MetadataEvidenceV1) -> Result<(), String> {
    let providers = value
        .enabled_providers
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let known = providers
        .iter()
        .all(|provider| matches!(*provider, "vndb" | "bangumi"));
    let commercial_vndb_permitted = value.release_use != "commercial"
        || !providers.contains("vndb")
        || value
            .vndb_commercial_license_id
            .as_deref()
            .is_some_and(safe_id);
    if value.schema != "astra.emu.metadata_evidence.v1"
        || !matches!(
            value.release_use.as_str(),
            "development" | "non_commercial" | "commercial"
        )
        || providers.len() != value.enabled_providers.len()
        || !known
        || !commercial_vndb_permitted
        || !value.consent_separated
        || !value.secret_references_only
        || value.sensitive_cover_default
        || value.local_path_present
        || value.query_body_present
        || value.status != "passed"
        || !value.diagnostic_codes.is_empty()
    {
        return Err("ASTRA_EMU_METADATA_EVIDENCE_INVALID".into());
    }
    Ok(())
}

pub fn validate_provider_binding(
    value: &EmuProviderBindingEvidenceV1,
    target: &str,
    profile: &str,
) -> Result<(), String> {
    if value.schema != "astra.emu.provider_binding.v1"
        || value.target_id != target
        || value.profile != profile
        || value.runtime_provider_id != "astra.runtime.astraemu"
        || value.family_id != "fvp"
        || value.family_provider_id != "astra.emu.family.fvp"
        || value.ui_provider_id != "astra.emu.ui.slint"
        || value.engine_fingerprint.is_empty()
        || value.rustc_fingerprint.is_empty()
        || value.feature_fingerprint.is_empty()
        || is_zero(value.binary_hash)
        || is_zero(value.signer_identity_hash)
        || !value.package_eligible
        || value.status != "pass"
        || !value.diagnostic_codes.is_empty()
    {
        return Err("ASTRA_EMU_PROVIDER_BINDING_INVALID".into());
    }
    Ok(())
}

pub fn validate_ui_host_identity(value: &UiHostIdentityEvidence) -> Result<(), String> {
    if value.schema != "astra.emu.ui_host_identity.v1"
        || value.program_target_id != "astra-emu-manager"
        || !EMU_RELEASE_PLATFORMS.contains(&value.platform.as_str())
        || value.slint_version != "1.17.1"
        || value.slint_license_mode != "royalty-free-2.0"
        || value.wgpu_version != "29.0.4"
        || !value.winit_version.starts_with("0.30.")
        || !value.shared_device_queue
        || value.cpu_frame_readback
        || value.cross_device_texture_copy
        || is_zero(value.renderer_adapter_hash)
        || is_zero(value.build_identity_hash)
        || is_zero(value.package_hash)
        || is_zero(value.session_id_hash)
    {
        return Err("ASTRA_EMU_UI_HOST_IDENTITY_INVALID".into());
    }
    Ok(())
}

pub fn validate_fvp_coverage(value: &FvpSyscallCoverageEvidence) -> Result<(), String> {
    let covered = value
        .covered_syscall_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut ids = value.covered_syscall_ids.clone();
    ids.sort_unstable();
    let catalog_hash = Hash256::from_sha256(format!("{}\n", ids.join("\n")).as_bytes());
    let all_opcodes_covered = (0_u8..=0x27).all(|opcode| {
        value
            .opcode_counts
            .get(&format!("0x{opcode:02x}"))
            .is_some_and(|count| *count > 0)
    });
    if value.schema != "astra.emu.fvp_syscall_coverage.v1"
        || value.rfvp_revision != RFVP_REFERENCE_REVISION
        || value.release_syscall_total != 148
        || covered.len() != 148
        || value.covered_syscall_ids.len() != 148
        || catalog_hash.to_string() != FVP_RELEASE_SYSCALL_CATALOG_HASH
        || !all_opcodes_covered
        || !value.missing_syscall_ids.is_empty()
        || value.full_flow_hash.is_none_or(is_zero)
        || value.status != "pass"
        || !value.diagnostic_codes.is_empty()
    {
        return Err("ASTRA_EMU_FVP_COVERAGE_INCOMPLETE".into());
    }
    Ok(())
}

pub fn validate_fvp_parity(value: &FvpParityEvidence) -> Result<(), String> {
    if value.schema != "astra.emu.fvp_parity.v1"
        || value.rfvp_revision != RFVP_REFERENCE_REVISION
        || !safe_id(&value.fixture_id)
        || is_zero(value.fixture_hash)
        || is_zero(value.astra_trace_hash)
        || value.astra_trace_hash != value.reference_trace_hash
        || value.compared_event_count == 0
        || value.first_divergence_sequence.is_some()
        || value.status != "pass"
        || !value.diagnostic_codes.is_empty()
    {
        return Err("ASTRA_EMU_FVP_PARITY_DIVERGENCE".into());
    }
    Ok(())
}

pub fn validate_frame_parity(value: &FrameParityReportV1) -> Result<(), String> {
    let frames_match = !value.frames.is_empty()
        && value
            .frames
            .windows(2)
            .all(|pair| pair[0].frame_index + 1 == pair[1].frame_index)
        && value.frames.iter().all(|frame| {
            frame.semantic_astra_hash == frame.semantic_reference_hash
                && frame.rgba_astra_hash == frame.rgba_reference_hash
                && frame.audio_astra_hash == frame.audio_reference_hash
        });
    if value.schema != "astra.frame_parity_report.v1"
        || value.reference_revision != RFVP_REFERENCE_REVISION
        || is_zero(value.reference_observer_patch_hash)
        || !safe_id(&value.build_identity)
        || !safe_id(&value.profile)
        || is_zero(value.game_identity_hash)
        || is_zero(value.input_sequence_hash)
        || !frames_match
        || value.compared_event_count == 0
        || value.first_divergence_sequence.is_some()
        || value.difference_window_before != 30
        || value.difference_window_after != 60
        || value.status != "pass"
        || !value.diagnostic_codes.is_empty()
    {
        return Err("ASTRA_EMU_FRAME_PARITY_DIVERGENCE".into());
    }
    Ok(())
}

pub fn validate_trusted_luau(value: &TrustedLuauEvidenceV1) -> Result<(), String> {
    let granted = value
        .capability_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let denied = value
        .denied_capability_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if value.schema != "astra.emu.trusted_luau_evidence.v1"
        || value.policy_id.is_empty()
        || [
            "vfs.read",
            "patch.overlay",
            "decode_transform",
            "text_hook",
            "media_hook",
            "deterministic_effect",
        ]
        .iter()
        .any(|id| !granted.contains(id))
        || ["filesystem", "network", "system", "native_handle"]
            .iter()
            .any(|id| !denied.contains(id))
        || value.evaluated_script_hashes.is_empty()
        || value
            .evaluated_script_hashes
            .iter()
            .any(|hash| is_zero(*hash))
        || value
            .evaluated_script_hashes
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != value.evaluated_script_hashes.len()
        || !value.violation_codes.is_empty()
        || value.commercial_payload_present
        || value.local_path_present
        || is_zero(value.deterministic_effect_hash)
        || value.status != "pass"
    {
        return Err("ASTRA_EMU_TRUSTED_LUAU_EVIDENCE_INVALID".into());
    }
    Ok(())
}

pub fn validate_translation_evidence(value: &TranslationEvidence) -> Result<(), String> {
    if value.schema != "astra.emu.translation_evidence.v1"
        || value.provider_identity != "ecnu-openai-compatible"
        || !value.consent_present
        || is_zero(value.endpoint_identity_hash)
        || is_zero(value.model_identity_hash)
        || value.request_count == 0
        || value.source_hashes.is_empty()
        || value.source_hashes.iter().any(|hash| is_zero(*hash))
        || value
            .source_hashes
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != value.source_hashes.len()
        || value.latency_ms_total == 0
        || !value.error_codes.is_empty()
        || value.redaction_status != "pass"
    {
        return Err("ASTRA_EMU_TRANSLATION_EVIDENCE_INVALID".into());
    }
    Ok(())
}

pub fn validate_platform_evidence(
    value: &EmuPlatformRunEvidenceV1,
    platform: &str,
) -> Result<(), String> {
    let expected = match platform {
        "windows" => ("x86_64", "native", "E3"),
        "android-x86_64" => ("x86_64", "android-native", "E3"),
        "android-arm64" => ("arm64-v8a", "android-native", "E2"),
        "linux" => ("x86_64", "native", "E2"),
        "macos" => ("universal", "native", "E2"),
        "ios" => ("arm64", "ios-static-registry", "E2"),
        _ => return Err("ASTRA_EMU_PLATFORM_EVIDENCE_INVALID".into()),
    };
    let lifecycle = value
        .lifecycle_steps
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let required_lifecycle: &[&str] = if platform == "android-arm64" {
        &["package", "native_manifest"]
    } else {
        &["create", "open", "step", "save", "restore", "shutdown"]
    };
    let e3 = expected.2 == "E3";
    if value.schema != "astra.emu.platform_run_evidence.v1"
        || value.platform != platform
        || value.architecture != expected.0
        || value.host_kind != expected.1
        || value.evidence_level != expected.2
        || value.status != "pass"
        || !value.diagnostic_codes.is_empty()
        || is_zero(value.build_identity_hash)
        || is_zero(value.profile_hash)
        || is_zero(value.package_hash)
        || is_zero(value.session_id_hash)
        || required_lifecycle
            .iter()
            .any(|step| !lifecycle.contains(step))
        || (e3
            && [
                value.input_sequence_hash,
                value.consumed_input_trace_hash,
                value.visual_trace_hash,
                value.audio_meter_hash,
                value.route_terminal_hash,
            ]
            .into_iter()
            .any(is_zero))
    {
        return Err("ASTRA_EMU_PLATFORM_EVIDENCE_INVALID".into());
    }
    Ok(())
}

fn is_zero(value: Hash256) -> bool {
    value.as_bytes().iter().all(|byte| *byte == 0)
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod metadata_evidence_tests {
    use super::*;

    fn commercial_vndb() -> MetadataEvidenceV1 {
        MetadataEvidenceV1 {
            schema: "astra.emu.metadata_evidence.v1".into(),
            release_use: "commercial".into(),
            enabled_providers: vec!["vndb".into(), "bangumi".into()],
            vndb_commercial_license_id: None,
            consent_separated: true,
            secret_references_only: true,
            sensitive_cover_default: false,
            local_path_present: false,
            query_body_present: false,
            status: "passed".into(),
            diagnostic_codes: Vec::new(),
        }
    }

    #[test]
    fn commercial_vndb_release_is_blocked_without_explicit_license() {
        let mut evidence = commercial_vndb();
        assert_eq!(
            validate_metadata_evidence(&evidence),
            Err("ASTRA_EMU_METADATA_EVIDENCE_INVALID".into())
        );
        evidence.vndb_commercial_license_id = Some("licensed-distribution-1".into());
        assert!(validate_metadata_evidence(&evidence).is_ok());
    }
}
