use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use astra_core::Hash256;
use astra_emu_manager_core::{
    AstraEmuEvidenceBundleV1, EmuPlatformRunEvidenceV1, EmuProviderBindingEvidenceV1,
    EmuReleaseManifestV1, FvpParityEvidence, FvpSyscallCoverageEvidence, MetadataEvidenceV1,
    TranslationEvidence, TrustedLuauEvidenceV1, UiHostIdentityEvidence, EMU_RELEASE_PLATFORMS,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

const MAX_INPUT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageSectionDescriptor {
    id: String,
    schema: String,
    path: String,
    codec: String,
    targets: Vec<String>,
    profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageSectionsFragment {
    package_sections: Vec<PackageSectionDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedFile {
    section_id: String,
    schema: String,
    file_name: String,
    sha256: Hash256,
    byte_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSummary {
    schema: String,
    target: String,
    profile: String,
    files: Vec<EncodedFile>,
    redaction_status: String,
}

pub fn encode_bundle(
    input_dir: &Path,
    project_root: &Path,
    relative_output: &Path,
    target: &str,
    profile: &str,
) -> Result<(), String> {
    validate_symbol(target, "ASTRA_EMU_EVIDENCE_TARGET")?;
    validate_symbol(profile, "ASTRA_EMU_EVIDENCE_PROFILE")?;
    validate_relative_path(relative_output)?;
    let project_root = project_root
        .canonicalize()
        .map_err(|_| "ASTRA_EMU_EVIDENCE_PROJECT_ROOT")?;
    let input_dir = input_dir
        .canonicalize()
        .map_err(|_| "ASTRA_EMU_EVIDENCE_INPUT_DIRECTORY")?;
    let output_dir = project_root.join(relative_output);
    fs::create_dir_all(&output_dir).map_err(|_| "ASTRA_EMU_EVIDENCE_OUTPUT_CREATE")?;
    let canonical_output = output_dir
        .canonicalize()
        .map_err(|_| "ASTRA_EMU_EVIDENCE_OUTPUT_CREATE")?;
    if !canonical_output.starts_with(&project_root) {
        return Err("ASTRA_EMU_EVIDENCE_OUTPUT_ESCAPE".into());
    }

    let mut section_ids = BTreeMap::from([
        ("provider_binding".into(), "emu.evidence.binding".into()),
        ("ui_host_identity".into(), "emu.evidence.ui".into()),
        ("fvp_coverage".into(), "emu.evidence.coverage".into()),
        ("fvp_parity".into(), "emu.evidence.parity".into()),
        ("trusted_luau".into(), "emu.evidence.luau".into()),
        ("translation".into(), "emu.evidence.translation".into()),
        ("metadata".into(), "emu.evidence.metadata".into()),
    ]);
    for platform in EMU_RELEASE_PLATFORMS {
        section_ids.insert(
            format!("platform.{platform}"),
            format!("emu.evidence.platform.{platform}"),
        );
    }
    let manifest = EmuReleaseManifestV1 {
        schema: "astra.emu.release_manifest.v1".into(),
        runtime_provider_id: "astra.runtime.astraemu".into(),
        family_id: "fvp".into(),
        family_provider_id: "astra.emu.family.fvp".into(),
        ui_provider_id: "astra.emu.ui.slint".into(),
        required_platforms: EMU_RELEASE_PLATFORMS
            .into_iter()
            .map(str::to_owned)
            .collect(),
        evidence_sections: section_ids,
    };
    let provider_binding =
        read_json::<EmuProviderBindingEvidenceV1>(&input_dir.join("provider-binding.json"))?;
    let ui_host_identity =
        read_json::<UiHostIdentityEvidence>(&input_dir.join("ui-host-identity.json"))?;
    let fvp_coverage =
        read_json::<FvpSyscallCoverageEvidence>(&input_dir.join("fvp-coverage.json"))?;
    let fvp_parity = read_json::<FvpParityEvidence>(&input_dir.join("fvp-parity.json"))?;
    let trusted_luau = read_json::<TrustedLuauEvidenceV1>(&input_dir.join("trusted-luau.json"))?;
    let translation = read_json::<TranslationEvidence>(&input_dir.join("translation.json"))?;
    let metadata = read_json::<MetadataEvidenceV1>(&input_dir.join("metadata.json"))?;
    let mut platforms = BTreeMap::new();
    for platform in EMU_RELEASE_PLATFORMS {
        platforms.insert(
            platform.to_owned(),
            read_json::<EmuPlatformRunEvidenceV1>(
                &input_dir.join(format!("platform-{platform}.json")),
            )?,
        );
    }
    let bundle = AstraEmuEvidenceBundleV1 {
        manifest,
        provider_binding,
        ui_host_identity,
        fvp_coverage,
        fvp_parity,
        trusted_luau,
        translation,
        metadata,
        platforms,
    };
    bundle.validate(target, profile)?;

    reject_output_collisions(&output_dir)?;
    let mut files = Vec::new();
    encode(
        &output_dir,
        "emu.release_manifest",
        "astra.emu.release_manifest.v1",
        "emu-release-manifest.bin",
        &bundle.manifest,
        &mut files,
    )?;
    encode(
        &output_dir,
        "emu.evidence.metadata",
        "astra.emu.metadata_evidence.v1",
        "metadata.bin",
        &bundle.metadata,
        &mut files,
    )?;
    encode(
        &output_dir,
        "emu.evidence.binding",
        "astra.emu.provider_binding.v1",
        "provider-binding.bin",
        &bundle.provider_binding,
        &mut files,
    )?;
    encode(
        &output_dir,
        "emu.evidence.ui",
        "astra.emu.ui_host_identity.v1",
        "ui-host-identity.bin",
        &bundle.ui_host_identity,
        &mut files,
    )?;
    encode(
        &output_dir,
        "emu.evidence.coverage",
        "astra.emu.fvp_syscall_coverage.v1",
        "fvp-coverage.bin",
        &bundle.fvp_coverage,
        &mut files,
    )?;
    encode(
        &output_dir,
        "emu.evidence.parity",
        "astra.emu.fvp_parity.v1",
        "fvp-parity.bin",
        &bundle.fvp_parity,
        &mut files,
    )?;
    encode(
        &output_dir,
        "emu.evidence.luau",
        "astra.emu.trusted_luau_evidence.v1",
        "trusted-luau.bin",
        &bundle.trusted_luau,
        &mut files,
    )?;
    encode(
        &output_dir,
        "emu.evidence.translation",
        "astra.emu.translation_evidence.v1",
        "translation.bin",
        &bundle.translation,
        &mut files,
    )?;
    for (platform, evidence) in &bundle.platforms {
        encode(
            &output_dir,
            &format!("emu.evidence.platform.{platform}"),
            "astra.emu.platform_run_evidence.v1",
            &format!("platform-{platform}.bin"),
            evidence,
            &mut files,
        )?;
    }
    files.sort_by(|left, right| left.section_id.cmp(&right.section_id));
    let prefix = relative_output.to_string_lossy().replace('\\', "/");
    let fragment = PackageSectionsFragment {
        package_sections: files
            .iter()
            .map(|file| PackageSectionDescriptor {
                id: file.section_id.clone(),
                schema: file.schema.clone(),
                path: format!("{prefix}/{}", file.file_name),
                codec: "postcard".into(),
                targets: vec![target.to_owned()],
                profiles: vec![profile.to_owned()],
            })
            .collect(),
    };
    write_atomic(
        &output_dir.join("package-sections.yaml"),
        serde_yaml::to_string(&fragment)
            .map_err(|_| "ASTRA_EMU_EVIDENCE_FRAGMENT_ENCODE")?
            .as_bytes(),
    )?;
    let summary = BundleSummary {
        schema: "astra.emu.evidence_bundle.v1".into(),
        target: target.into(),
        profile: profile.into(),
        files,
        redaction_status: "pass".into(),
    };
    write_atomic(
        &output_dir.join("evidence-bundle.json"),
        &serde_json::to_vec_pretty(&summary).map_err(|_| "ASTRA_EMU_EVIDENCE_SUMMARY_ENCODE")?,
    )?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let metadata = fs::metadata(path).map_err(|_| "ASTRA_EMU_EVIDENCE_INPUT_MISSING")?;
    if metadata.len() == 0 || metadata.len() > MAX_INPUT_BYTES {
        return Err("ASTRA_EMU_EVIDENCE_INPUT_BOUNDS".into());
    }
    let bytes = fs::read(path).map_err(|_| "ASTRA_EMU_EVIDENCE_INPUT_READ")?;
    let generic: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "ASTRA_EMU_EVIDENCE_JSON_INVALID")?;
    validate_redaction(&generic)?;
    serde_json::from_value(generic).map_err(|_| "ASTRA_EMU_EVIDENCE_CONTRACT_INVALID".into())
}

fn validate_redaction(value: &serde_json::Value) -> Result<(), String> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if matches!(
                    key.as_str(),
                    "payload"
                        | "payload_bytes"
                        | "bytes"
                        | "content"
                        | "text"
                        | "source_text"
                        | "script_text"
                        | "commercial_text"
                        | "local_path"
                        | "absolute_path"
                        | "secret"
                        | "api_key"
                ) {
                    return Err("ASTRA_EMU_EVIDENCE_REDACTION_FIELD".into());
                }
                validate_redaction(value)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_redaction(value)?;
            }
        }
        serde_json::Value::String(value) => {
            let bytes = value.as_bytes();
            let drive_absolute = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\');
            if drive_absolute
                || value.starts_with('/')
                || value.starts_with("\\\\")
                || value.starts_with("file://")
                || value.starts_with("content://")
            {
                return Err("ASTRA_EMU_EVIDENCE_LOCAL_PATH".into());
            }
        }
        _ => {}
    }
    Ok(())
}

fn encode<T: Serialize>(
    output_dir: &Path,
    section_id: &str,
    schema: &str,
    file_name: &str,
    value: &T,
    files: &mut Vec<EncodedFile>,
) -> Result<(), String> {
    let bytes = postcard::to_allocvec(value).map_err(|_| "ASTRA_EMU_EVIDENCE_POSTCARD_ENCODE")?;
    write_atomic(&output_dir.join(file_name), &bytes)?;
    files.push(EncodedFile {
        section_id: section_id.into(),
        schema: schema.into(),
        file_name: file_name.into(),
        sha256: Hash256::from_sha256(&bytes),
        byte_size: bytes.len() as u64,
    });
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension(format!(
        "{}.partial",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("bin")
    ));
    fs::write(&partial, bytes).map_err(|_| "ASTRA_EMU_EVIDENCE_OUTPUT_WRITE")?;
    fs::rename(&partial, path).map_err(|_| "ASTRA_EMU_EVIDENCE_OUTPUT_COMMIT".to_owned())
}

fn reject_output_collisions(output_dir: &Path) -> Result<(), String> {
    let entries = fs::read_dir(output_dir).map_err(|_| "ASTRA_EMU_EVIDENCE_OUTPUT_READ")?;
    if entries
        .filter_map(Result::ok)
        .any(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
    {
        return Err("ASTRA_EMU_EVIDENCE_OUTPUT_NOT_EMPTY".into());
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("ASTRA_EMU_EVIDENCE_OUTPUT_PATH".into());
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("ASTRA_EMU_EVIDENCE_OUTPUT_PATH".into());
    }
    Ok(())
}

fn validate_symbol(value: &str, code: &'static str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(code.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(value: &str) -> Hash256 {
        Hash256::from_sha256(value.as_bytes())
    }

    fn write_json<T: Serialize>(directory: &Path, name: &str, value: &T) {
        fs::write(
            directory.join(name),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn redaction_rejects_payload_fields_and_absolute_paths() {
        assert!(validate_redaction(&serde_json::json!({"payload": "omitted"})).is_err());
        assert!(
            validate_redaction(&serde_json::json!({"diagnostic": "C:\\private\\game"})).is_err()
        );
        assert!(validate_redaction(&serde_json::json!({"hash": "sha256:00"})).is_ok());
    }

    #[test]
    fn output_path_must_remain_project_relative() {
        assert!(validate_relative_path(Path::new("Evidence/AstraEMU")).is_ok());
        assert!(validate_relative_path(Path::new("../outside")).is_err());
        assert!(validate_relative_path(&std::path::PathBuf::from("C:\\outside")).is_err());
    }

    #[test]
    fn complete_bundle_is_validated_encoded_and_round_trips() {
        let project = tempfile::tempdir().unwrap();
        let input = project.path().join("input");
        fs::create_dir(&input).unwrap();
        write_complete_inputs(&input);

        encode_bundle(
            &input,
            project.path(),
            Path::new("Evidence/AstraEMU"),
            "astra-emu-case",
            "fvp-v1",
        )
        .unwrap();

        let output = project.path().join("Evidence/AstraEMU");
        let manifest: EmuReleaseManifestV1 =
            postcard::from_bytes(&fs::read(output.join("emu-release-manifest.bin")).unwrap())
                .unwrap();
        assert_eq!(manifest.required_platforms.len(), 6);
        let binding: EmuProviderBindingEvidenceV1 =
            postcard::from_bytes(&fs::read(output.join("provider-binding.bin")).unwrap()).unwrap();
        assert_eq!(binding.family_id, "fvp");
        let fragment: PackageSectionsFragment =
            serde_yaml::from_slice(&fs::read(output.join("package-sections.yaml")).unwrap())
                .unwrap();
        assert_eq!(fragment.package_sections.len(), 14);
        assert!(fragment.package_sections.iter().all(|section| {
            section.path.starts_with("Evidence/AstraEMU/")
                && section.targets == ["astra-emu-case"]
                && section.profiles == ["fvp-v1"]
                && output
                    .join(section.path.rsplit('/').next().unwrap())
                    .is_file()
        }));
        let summary: BundleSummary =
            serde_json::from_slice(&fs::read(output.join("evidence-bundle.json")).unwrap())
                .unwrap();
        assert_eq!(summary.files.len(), 14);
        for file in summary.files {
            let bytes = fs::read(output.join(&file.file_name)).unwrap();
            assert_eq!(file.sha256, Hash256::from_sha256(&bytes));
            assert_eq!(file.byte_size, bytes.len() as u64);
        }
    }

    fn write_complete_inputs(input: &Path) {
        write_json(
            input,
            "provider-binding.json",
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
                binary_hash: hash("binary"),
                package_eligible: true,
                signer_identity_hash: hash("signer"),
                status: "pass".into(),
                diagnostic_codes: Vec::new(),
            },
        );
        write_json(
            input,
            "ui-host-identity.json",
            &UiHostIdentityEvidence {
                schema: "astra.emu.ui_host_identity.v1".into(),
                program_target_id: "astra-emu-manager".into(),
                platform: "windows".into(),
                slint_version: "1.17.1".into(),
                slint_license_mode: "royalty-free-2.0".into(),
                wgpu_version: "29.0.4".into(),
                winit_version: "0.30.13".into(),
                renderer_adapter_hash: hash("adapter"),
                shared_device_queue: true,
                cpu_frame_readback: false,
                cross_device_texture_copy: false,
                build_identity_hash: hash("build-windows"),
                package_hash: hash("package"),
                session_id_hash: hash("session-windows"),
            },
        );
        write_json(
            input,
            "fvp-coverage.json",
            &FvpSyscallCoverageEvidence {
                schema: "astra.emu.fvp_syscall_coverage.v1".into(),
                rfvp_revision: astra_emu_manager_core::RFVP_REFERENCE_REVISION.into(),
                release_syscall_total: 148,
                covered_syscall_ids: astra_emu_fvp::release_syscall_ids(),
                missing_syscall_ids: Vec::new(),
                opcode_counts: astra_emu_fvp::release_opcode_ids()
                    .into_iter()
                    .map(|id| (id, 1))
                    .collect(),
                full_flow_hash: Some(hash("full-flow")),
                status: "pass".into(),
                diagnostic_codes: Vec::new(),
            },
        );
        let parity_hash = hash("parity-trace");
        write_json(
            input,
            "fvp-parity.json",
            &FvpParityEvidence {
                schema: "astra.emu.fvp_parity.v1".into(),
                rfvp_revision: astra_emu_manager_core::RFVP_REFERENCE_REVISION.into(),
                fixture_id: "fvp.synthetic.vm.reference.v1".into(),
                fixture_hash: hash("fixture"),
                astra_trace_hash: parity_hash,
                reference_trace_hash: parity_hash,
                compared_event_count: 1,
                first_divergence_sequence: None,
                status: "pass".into(),
                diagnostic_codes: Vec::new(),
            },
        );
        write_json(
            input,
            "trusted-luau.json",
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
                evaluated_script_hashes: vec![hash("patch")],
                violation_codes: Vec::new(),
                commercial_payload_present: false,
                local_path_present: false,
                deterministic_effect_hash: hash("effects"),
                status: "pass".into(),
            },
        );
        write_json(
            input,
            "translation.json",
            &TranslationEvidence {
                schema: "astra.emu.translation_evidence.v1".into(),
                provider_identity: "ecnu-openai-compatible".into(),
                endpoint_identity_hash: hash("endpoint"),
                model_identity_hash: hash("model"),
                consent_present: true,
                persistent_cache_enabled: false,
                request_count: 1,
                source_hashes: vec![hash("source")],
                latency_ms_total: 1,
                error_codes: Vec::new(),
                redaction_status: "pass".into(),
            },
        );
        write_json(
            input,
            "metadata.json",
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
        );

        let platforms = [
            ("windows", "x86_64", "native", "E3"),
            ("android-x86_64", "x86_64", "android-native", "E3"),
            ("android-arm64", "arm64-v8a", "android-native", "E2"),
            ("linux", "x86_64", "native", "E2"),
            ("macos", "universal", "native", "E2"),
            ("ios", "arm64", "ios-static-registry", "E2"),
        ];
        for (platform, architecture, host_kind, evidence_level) in platforms {
            let shared_e3 = matches!(platform, "windows" | "android-x86_64");
            write_json(
                input,
                &format!("platform-{platform}.json"),
                &EmuPlatformRunEvidenceV1 {
                    schema: "astra.emu.platform_run_evidence.v1".into(),
                    platform: platform.into(),
                    architecture: architecture.into(),
                    host_kind: host_kind.into(),
                    build_identity_hash: hash(&format!("build-{platform}")),
                    profile_hash: hash(&format!("profile-{platform}")),
                    package_hash: hash("package"),
                    session_id_hash: hash(&format!("session-{platform}")),
                    input_sequence_hash: hash(if shared_e3 { "shared-input" } else { platform }),
                    consumed_input_trace_hash: hash(&format!("consumed-{platform}")),
                    visual_trace_hash: hash(&format!("visual-{platform}")),
                    audio_meter_hash: hash(&format!("audio-{platform}")),
                    route_terminal_hash: hash(if shared_e3 { "shared-route" } else { platform }),
                    lifecycle_steps: if platform == "android-arm64" {
                        vec!["package".into(), "native_manifest".into()]
                    } else {
                        ["create", "open", "step", "save", "restore", "shutdown"]
                            .into_iter()
                            .map(str::to_owned)
                            .collect()
                    },
                    evidence_level: evidence_level.into(),
                    status: "pass".into(),
                    diagnostic_codes: Vec::new(),
                },
            );
        }
    }
}
