use astra_core::Diagnostic;
use astra_emu_manager_core::{
    validate_fvp_coverage, validate_fvp_parity, validate_metadata_evidence,
    validate_platform_evidence, validate_provider_binding, validate_release_manifest,
    validate_translation_evidence, validate_trusted_luau, validate_ui_host_identity,
    AstraEmuEvidenceBundleV1, EmuPlatformRunEvidenceV1, EmuProviderBindingEvidenceV1,
    EmuReleaseManifestV1, FvpParityEvidence, FvpSyscallCoverageEvidence, MetadataEvidenceV1,
    TranslationEvidence, TrustedLuauEvidenceV1, UiHostIdentityEvidence,
};
use astra_package::{PackageReader, SectionEntry};
use serde::de::DeserializeOwned;

use crate::{evidence, CheckStatus, ReleaseCheckRecord, ReleaseDomain};

const MANIFEST_SECTION: &str = "emu.release_manifest";

pub(super) fn emu_release_checks(
    package: &PackageReader,
    profile: &str,
    target: Option<&str>,
) -> Vec<ReleaseCheckRecord> {
    let Some(entry) = section(package, MANIFEST_SECTION) else {
        return Vec::new();
    };
    let manifest = match decode::<EmuReleaseManifestV1>(package, entry) {
        Ok(manifest) => manifest,
        Err(check) => return vec![*check],
    };
    let mut checks = vec![manifest_check(&manifest)];
    checks.push(binding_check(package, &manifest, profile, target));
    checks.push(ui_check(package, &manifest));
    checks.push(fvp_coverage_check(package, &manifest));
    checks.push(fvp_parity_check(package, &manifest));
    checks.push(luau_check(package, &manifest));
    checks.push(translation_check(package, &manifest));
    checks.push(metadata_check(package, &manifest));
    checks.extend(platform_checks(package, &manifest));
    checks.push(continuity_check(package, &manifest, profile, target));
    checks
}

fn metadata_check(package: &PackageReader, manifest: &EmuReleaseManifestV1) -> ReleaseCheckRecord {
    let decoded = decode_named::<MetadataEvidenceV1>(package, manifest, "metadata");
    let Ok(value) = decoded else {
        return *decoded.unwrap_err();
    };
    let valid = validate_metadata_evidence(&value).is_ok();
    record(
        "emu.metadata_license",
        valid,
        "metadata provider consent, privacy and commercial license policy are explicit",
        "ASTRA_EMU_METADATA_EVIDENCE_INVALID",
    )
}

fn manifest_check(manifest: &EmuReleaseManifestV1) -> ReleaseCheckRecord {
    let valid = validate_release_manifest(manifest).is_ok();
    record(
        "emu.release_manifest",
        valid,
        "AstraEMU release identities and evidence section bindings are explicit",
        "ASTRA_EMU_RELEASE_MANIFEST_INVALID",
    )
}

fn binding_check(
    package: &PackageReader,
    manifest: &EmuReleaseManifestV1,
    profile: &str,
    target: Option<&str>,
) -> ReleaseCheckRecord {
    let decoded =
        decode_named::<EmuProviderBindingEvidenceV1>(package, manifest, "provider_binding");
    let Ok(binding) = decoded else {
        return *decoded.unwrap_err();
    };
    let expected_target = target.unwrap_or(&binding.target_id);
    let valid = validate_provider_binding(&binding, expected_target, profile).is_ok()
        && binding.runtime_provider_id == manifest.runtime_provider_id
        && binding.family_id == manifest.family_id
        && binding.family_provider_id == manifest.family_provider_id
        && binding.ui_provider_id == manifest.ui_provider_id;
    record(
        "emu.provider_binding",
        valid,
        "runtime, family and UI providers are uniquely package-bound",
        "ASTRA_EMU_PROVIDER_BINDING_INVALID",
    )
}

fn ui_check(package: &PackageReader, manifest: &EmuReleaseManifestV1) -> ReleaseCheckRecord {
    let decoded = decode_named::<UiHostIdentityEvidence>(package, manifest, "ui_host_identity");
    let Ok(ui) = decoded else {
        return *decoded.unwrap_err();
    };
    let valid = validate_ui_host_identity(&ui).is_ok();
    record(
        "emu.ui_host_identity",
        valid,
        "Slint host and shared WGPU device identity satisfy the UI contract",
        "ASTRA_EMU_UI_HOST_IDENTITY_INVALID",
    )
}

fn fvp_coverage_check(
    package: &PackageReader,
    manifest: &EmuReleaseManifestV1,
) -> ReleaseCheckRecord {
    let decoded = decode_named::<FvpSyscallCoverageEvidence>(package, manifest, "fvp_coverage");
    let Ok(value) = decoded else {
        return *decoded.unwrap_err();
    };
    let valid = validate_fvp_coverage(&value).is_ok();
    record(
        "emu.fvp_coverage",
        valid,
        "FVP full-flow and 148-syscall evidence is complete",
        "ASTRA_EMU_FVP_COVERAGE_INCOMPLETE",
    )
}

fn fvp_parity_check(
    package: &PackageReader,
    manifest: &EmuReleaseManifestV1,
) -> ReleaseCheckRecord {
    let decoded = decode_named::<FvpParityEvidence>(package, manifest, "fvp_parity");
    let Ok(value) = decoded else {
        return *decoded.unwrap_err();
    };
    let valid = validate_fvp_parity(&value).is_ok();
    record(
        "emu.fvp_parity",
        valid,
        "FVP behavior matches the fixed rfvp reference trace",
        "ASTRA_EMU_FVP_PARITY_DIVERGENCE",
    )
}

fn luau_check(package: &PackageReader, manifest: &EmuReleaseManifestV1) -> ReleaseCheckRecord {
    let decoded = decode_named::<TrustedLuauEvidenceV1>(package, manifest, "trusted_luau");
    let Ok(value) = decoded else {
        return *decoded.unwrap_err();
    };
    let valid = validate_trusted_luau(&value).is_ok();
    record(
        "emu.trusted_luau",
        valid,
        "Trusted Luau capability and redaction evidence is isolated",
        "ASTRA_EMU_TRUSTED_LUAU_EVIDENCE_INVALID",
    )
}

fn translation_check(
    package: &PackageReader,
    manifest: &EmuReleaseManifestV1,
) -> ReleaseCheckRecord {
    let decoded = decode_named::<TranslationEvidence>(package, manifest, "translation");
    let Ok(value) = decoded else {
        return *decoded.unwrap_err();
    };
    let valid = validate_translation_evidence(&value).is_ok();
    record(
        "emu.translation",
        valid,
        "translation consent, provider identity, cache policy and redaction are evidenced",
        "ASTRA_EMU_TRANSLATION_EVIDENCE_INVALID",
    )
}

fn platform_checks(
    package: &PackageReader,
    manifest: &EmuReleaseManifestV1,
) -> Vec<ReleaseCheckRecord> {
    manifest
        .required_platforms
        .iter()
        .map(|platform| {
            let key = format!("platform.{platform}");
            let decoded = decode_named::<EmuPlatformRunEvidenceV1>(package, manifest, &key);
            let Ok(value) = decoded else {
                return *decoded.unwrap_err();
            };
            let valid = validate_platform_evidence(&value, platform).is_ok();
            record(
                &format!("emu.platform.{platform}"),
                valid,
                "platform runtime, input, visual, audio and lifecycle evidence is bound to one run",
                "ASTRA_EMU_PLATFORM_EVIDENCE_INVALID",
            )
        })
        .collect()
}

fn continuity_check(
    package: &PackageReader,
    manifest: &EmuReleaseManifestV1,
    profile: &str,
    target: Option<&str>,
) -> ReleaseCheckRecord {
    let Ok(provider_binding) =
        decode_named::<EmuProviderBindingEvidenceV1>(package, manifest, "provider_binding")
    else {
        return blocked(
            "emu.evidence.continuity",
            "ASTRA_EMU_EVIDENCE_CONTINUITY_INVALID",
        );
    };
    let expected_target = target.unwrap_or(&provider_binding.target_id).to_owned();
    let values = (
        decode_named::<UiHostIdentityEvidence>(package, manifest, "ui_host_identity"),
        decode_named::<FvpSyscallCoverageEvidence>(package, manifest, "fvp_coverage"),
        decode_named::<FvpParityEvidence>(package, manifest, "fvp_parity"),
        decode_named::<TrustedLuauEvidenceV1>(package, manifest, "trusted_luau"),
        decode_named::<TranslationEvidence>(package, manifest, "translation"),
        decode_named::<MetadataEvidenceV1>(package, manifest, "metadata"),
    );
    let (
        Ok(ui_host_identity),
        Ok(fvp_coverage),
        Ok(fvp_parity),
        Ok(trusted_luau),
        Ok(translation),
        Ok(metadata),
    ) = values
    else {
        return blocked(
            "emu.evidence.continuity",
            "ASTRA_EMU_EVIDENCE_CONTINUITY_INVALID",
        );
    };
    let mut platforms = std::collections::BTreeMap::new();
    for platform in &manifest.required_platforms {
        let key = format!("platform.{platform}");
        let Ok(value) = decode_named::<EmuPlatformRunEvidenceV1>(package, manifest, &key) else {
            return blocked(
                "emu.evidence.continuity",
                "ASTRA_EMU_EVIDENCE_CONTINUITY_INVALID",
            );
        };
        platforms.insert(platform.clone(), value);
    }
    let bundle = AstraEmuEvidenceBundleV1 {
        manifest: manifest.clone(),
        provider_binding,
        ui_host_identity,
        fvp_coverage,
        fvp_parity,
        trusted_luau,
        translation,
        metadata,
        platforms,
    };
    record(
        "emu.evidence.continuity",
        bundle.validate(&expected_target, profile).is_ok(),
        "AstraEMU evidence identities are continuous across package, UI and platform runs",
        "ASTRA_EMU_EVIDENCE_CONTINUITY_INVALID",
    )
}

fn decode_named<T: DeserializeOwned>(
    package: &PackageReader,
    manifest: &EmuReleaseManifestV1,
    key: &str,
) -> Result<T, Box<ReleaseCheckRecord>> {
    let Some(id) = manifest.evidence_sections.get(key) else {
        return Err(Box::new(blocked(
            "emu.evidence_section",
            "ASTRA_EMU_EVIDENCE_SECTION_MISSING",
        )));
    };
    let Some(entry) = section(package, id) else {
        return Err(Box::new(blocked(
            "emu.evidence_section",
            "ASTRA_EMU_EVIDENCE_SECTION_MISSING",
        )));
    };
    decode(package, entry)
}

fn decode<T: DeserializeOwned>(
    package: &PackageReader,
    entry: &SectionEntry,
) -> Result<T, Box<ReleaseCheckRecord>> {
    package
        .container()
        .decode_postcard::<T>(&entry.id)
        .map_err(|_| {
            Box::new(blocked(
                "emu.evidence_decode",
                "ASTRA_EMU_EVIDENCE_DECODE_FAILED",
            ))
        })
}

fn section<'a>(package: &'a PackageReader, id: &str) -> Option<&'a SectionEntry> {
    package
        .container()
        .entries()
        .iter()
        .find(|entry| entry.id == id)
}

fn record(id: &str, valid: bool, summary: &str, diagnostic: &'static str) -> ReleaseCheckRecord {
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Emu,
        status: if valid {
            CheckStatus::Pass
        } else {
            CheckStatus::Blocked
        },
        summary: summary.into(),
        diagnostic: (!valid).then(|| Diagnostic::blocking(diagnostic, summary)),
        evidence: vec![evidence("validated", valid)],
    }
}

fn blocked(id: &str, diagnostic: &'static str) -> ReleaseCheckRecord {
    record(
        id,
        false,
        "required AstraEMU evidence is missing or malformed",
        diagnostic,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_core::Hash256;
    use astra_emu_manager_core::{
        EmuPlatformRunEvidenceV1, EmuProviderBindingEvidenceV1, FvpParityEvidence,
        FvpSyscallCoverageEvidence, MetadataEvidenceV1, TranslationEvidence, TrustedLuauEvidenceV1,
        UiHostIdentityEvidence, RFVP_REFERENCE_REVISION,
    };
    use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
    use std::collections::BTreeMap;

    #[astra_headless_test::test]
    fn complete_astraemu_evidence_set_passes_every_release_check() {
        let package = package_with_complete_evidence();
        let reader = PackageReader::open(&package).unwrap();
        let checks = emu_release_checks(&reader, "fvp-v1", Some("astra-emu-case"));
        assert_eq!(checks.len(), 15);
        assert!(
            checks.iter().all(|check| check.status == CheckStatus::Pass),
            "unexpected blocked checks: {:?}",
            checks
                .iter()
                .filter(|check| check.status != CheckStatus::Pass)
                .map(|check| (&check.id, &check.diagnostic))
                .collect::<Vec<_>>()
        );
    }

    fn package_with_complete_evidence() -> Vec<u8> {
        let platforms = [
            ("windows", "x86_64", "native", "E3"),
            ("android-x86_64", "x86_64", "android-native", "E3"),
            ("android-arm64", "arm64-v8a", "android-native", "E2"),
            ("linux", "x86_64", "native", "E2"),
            ("macos", "universal", "native", "E2"),
            ("ios", "arm64", "ios-static-registry", "E2"),
        ];
        let mut evidence_sections = BTreeMap::from([
            ("provider_binding".into(), "emu.evidence.binding".into()),
            ("ui_host_identity".into(), "emu.evidence.ui".into()),
            ("fvp_coverage".into(), "emu.evidence.coverage".into()),
            ("fvp_parity".into(), "emu.evidence.parity".into()),
            ("trusted_luau".into(), "emu.evidence.luau".into()),
            ("translation".into(), "emu.evidence.translation".into()),
            ("metadata".into(), "emu.evidence.metadata".into()),
        ]);
        let mut sections = vec![postcard_section(
            "emu.evidence.binding",
            "astra.emu.provider_binding.v1",
            &EmuProviderBindingEvidenceV1 {
                schema: "astra.emu.provider_binding.v1".into(),
                target_id: "astra-emu-case".into(),
                profile: "fvp-v1".into(),
                runtime_provider_id: "astra.runtime.astraemu".into(),
                family_id: "fvp".into(),
                family_provider_id: "astra.emu.family.fvp".into(),
                ui_provider_id: "astra.emu.ui.slint".into(),
                engine_fingerprint: "engine-v1".into(),
                rustc_fingerprint: "rustc-v1".into(),
                feature_fingerprint: "feature-v1".into(),
                binary_hash: hash(b"binary"),
                package_eligible: true,
                signer_identity_hash: hash(b"signer"),
                status: "pass".into(),
                diagnostic_codes: Vec::new(),
            },
        )];
        sections.push(postcard_section(
            "emu.evidence.ui",
            "astra.emu.ui_host_identity.v1",
            &UiHostIdentityEvidence {
                schema: "astra.emu.ui_host_identity.v1".into(),
                program_target_id: "astra-emu-manager".into(),
                platform: "windows".into(),
                slint_version: "1.17.1".into(),
                slint_license_mode: "royalty-free-2.0".into(),
                wgpu_version: "29.0.4".into(),
                winit_version: "0.30.13".into(),
                renderer_adapter_hash: hash(b"adapter"),
                shared_device_queue: true,
                cpu_frame_readback: false,
                cross_device_texture_copy: false,
                build_identity_hash: hash(b"build-windows"),
                package_hash: hash(b"package"),
                session_id_hash: hash(b"session-windows"),
            },
        ));
        sections.push(postcard_section(
            "emu.evidence.coverage",
            "astra.emu.fvp_syscall_coverage.v1",
            &FvpSyscallCoverageEvidence {
                schema: "astra.emu.fvp_syscall_coverage.v1".into(),
                rfvp_revision: RFVP_REFERENCE_REVISION.into(),
                release_syscall_total: 148,
                covered_syscall_ids: astra_emu_fvp::release_syscall_ids(),
                missing_syscall_ids: Vec::new(),
                opcode_counts: astra_emu_fvp::release_opcode_ids()
                    .into_iter()
                    .map(|id| (id, 1))
                    .collect(),
                full_flow_hash: Some(hash(b"full-flow")),
                status: "pass".into(),
                diagnostic_codes: Vec::new(),
            },
        ));
        let trace = hash(b"trace");
        sections.push(postcard_section(
            "emu.evidence.parity",
            "astra.emu.fvp_parity.v1",
            &FvpParityEvidence {
                schema: "astra.emu.fvp_parity.v1".into(),
                rfvp_revision: RFVP_REFERENCE_REVISION.into(),
                fixture_id: "sanitized.full-flow".into(),
                fixture_hash: hash(b"fixture"),
                astra_trace_hash: trace,
                reference_trace_hash: trace,
                compared_event_count: 1,
                first_divergence_sequence: None,
                status: "pass".into(),
                diagnostic_codes: Vec::new(),
            },
        ));
        sections.push(postcard_section(
            "emu.evidence.luau",
            "astra.emu.trusted_luau_evidence.v1",
            &TrustedLuauEvidenceV1 {
                schema: "astra.emu.trusted_luau_evidence.v1".into(),
                policy_id: "astra.emu.trusted.v1".into(),
                capability_ids: [
                    "vfs.read",
                    "patch.overlay",
                    "decode_transform",
                    "text_hook",
                    "media_hook",
                    "deterministic_effect",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
                denied_capability_ids: ["filesystem", "network", "system", "native_handle"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                evaluated_script_hashes: vec![hash(b"patch")],
                violation_codes: Vec::new(),
                commercial_payload_present: false,
                local_path_present: false,
                deterministic_effect_hash: hash(b"effects"),
                status: "pass".into(),
            },
        ));
        sections.push(postcard_section(
            "emu.evidence.translation",
            "astra.emu.translation_evidence.v1",
            &TranslationEvidence {
                schema: "astra.emu.translation_evidence.v1".into(),
                provider_identity: "ecnu-openai-compatible".into(),
                endpoint_identity_hash: hash(b"endpoint"),
                model_identity_hash: hash(b"model"),
                consent_present: true,
                persistent_cache_enabled: false,
                request_count: 1,
                source_hashes: vec![hash(b"source")],
                latency_ms_total: 1,
                error_codes: Vec::new(),
                redaction_status: "pass".into(),
            },
        ));
        sections.push(postcard_section(
            "emu.evidence.metadata",
            "astra.emu.metadata_evidence.v1",
            &MetadataEvidenceV1 {
                schema: "astra.emu.metadata_evidence.v1".into(),
                release_use: "non_commercial".into(),
                enabled_providers: vec!["vndb".into(), "bangumi".into()],
                vndb_commercial_license_id: None,
                consent_separated: true,
                secret_references_only: true,
                sensitive_cover_default: false,
                local_path_present: false,
                query_body_present: false,
                status: "passed".into(),
                diagnostic_codes: Vec::new(),
            },
        ));
        for (platform, architecture, host_kind, level) in platforms {
            let key = format!("platform.{platform}");
            let section_id = format!("emu.evidence.platform.{platform}");
            evidence_sections.insert(key, section_id.clone());
            sections.push(postcard_section(
                &section_id,
                "astra.emu.platform_run_evidence.v1",
                &EmuPlatformRunEvidenceV1 {
                    schema: "astra.emu.platform_run_evidence.v1".into(),
                    platform: platform.into(),
                    architecture: architecture.into(),
                    host_kind: host_kind.into(),
                    build_identity_hash: hash(format!("build-{platform}").as_bytes()),
                    profile_hash: hash(format!("profile-{platform}").as_bytes()),
                    package_hash: hash(b"package"),
                    session_id_hash: hash(format!("session-{platform}").as_bytes()),
                    input_sequence_hash: if matches!(platform, "windows" | "android-x86_64") {
                        hash(b"shared-e3-input")
                    } else {
                        hash(format!("input-{platform}").as_bytes())
                    },
                    consumed_input_trace_hash: hash(format!("consumed-{platform}").as_bytes()),
                    visual_trace_hash: hash(format!("visual-{platform}").as_bytes()),
                    audio_meter_hash: hash(format!("audio-{platform}").as_bytes()),
                    route_terminal_hash: if matches!(platform, "windows" | "android-x86_64") {
                        hash(b"shared-e3-route")
                    } else {
                        hash(format!("route-{platform}").as_bytes())
                    },
                    lifecycle_steps: if platform == "android-arm64" {
                        vec!["package".into(), "native_manifest".into()]
                    } else {
                        ["create", "open", "step", "save", "restore", "shutdown"]
                            .into_iter()
                            .map(str::to_owned)
                            .collect()
                    },
                    evidence_level: level.into(),
                    status: "pass".into(),
                    diagnostic_codes: Vec::new(),
                },
            ));
        }
        let required_platforms = platforms
            .into_iter()
            .map(|(platform, ..)| platform.to_owned())
            .collect();
        sections.push(postcard_section(
            MANIFEST_SECTION,
            "astra.emu.release_manifest.v1",
            &EmuReleaseManifestV1 {
                schema: "astra.emu.release_manifest.v1".into(),
                runtime_provider_id: "astra.runtime.astraemu".into(),
                family_id: "fvp".into(),
                family_provider_id: "astra.emu.family.fvp".into(),
                ui_provider_id: "astra.emu.ui.slint".into(),
                required_platforms,
                evidence_sections,
            },
        ));
        PackageBuilder::build(PackageBuildRequest::fixture(
            "com.example.astraemu",
            "fvp-v1",
            sections,
        ))
        .unwrap()
        .into_bytes()
    }

    fn postcard_section<T: serde::Serialize>(id: &str, schema: &str, value: &T) -> SectionPayload {
        SectionPayload::postcard(id, schema, value).unwrap()
    }

    fn hash(bytes: &[u8]) -> Hash256 {
        Hash256::from_sha256(bytes)
    }
}
