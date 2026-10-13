use std::{collections::BTreeMap, env, fs, path::Path};

use astra_core::Hash256;
use astra_media::FONT_PACKAGE_MANIFEST_SCHEMA;
use astra_package::{
    MigrationPolicy, PackageBuildRequest, PackageBuilder, ScenarioReference, ScenarioRefsManifest,
    SectionCodec, SectionPayload, SourceUnlockPolicy, CURRENT_CONTAINER_VERSION,
};
use astra_platform::PlatformId;
use astra_target::{validate_manifest, TargetManifest};

use super::*;

pub(crate) fn build_package_from_cooked(
    cooked: &std::path::Path,
    manifest: CookManifest,
    target: Option<&str>,
    source_lock: Option<&SourceLockBuildInputs>,
) -> Result<astra_package::ContainerBlob, CliError> {
    let target = target.unwrap_or(&manifest.target);
    if target != manifest.target {
        return Err(format!(
            "package target {target} does not match cooked target {}",
            manifest.target
        )
        .into());
    }
    let target_report = validate_manifest(&manifest.target_manifest, Some(target));
    if matches!(
        target_report.status,
        astra_target::TargetValidationStatus::Blocked
    ) {
        return Err(format!("target validation failed: {target}").into());
    }
    let package_target_manifest = package_target_manifest(&manifest.target_manifest, target)?;
    let mut artifacts = Vec::new();
    for artifact in &manifest.artifacts {
        let bytes = fs::read(cooked.join(&artifact.path))?;
        let hash = Hash256::from_sha256(&bytes).to_string();
        if hash != artifact.hash {
            return Err(format!("cooked artifact hash mismatch: {}", artifact.section_id).into());
        }
        artifacts.push(SectionPayload::new(
            artifact.section_id.clone(),
            artifact.schema.clone(),
            CURRENT_CONTAINER_VERSION,
            artifact.codec.clone(),
            bytes,
            MigrationPolicy::current(),
        ));
    }
    let asset_vfs_manifest = asset_vfs_manifest_from_cooked(&manifest, &artifacts)?;
    let asset_catalog = asset_catalog_from_cooked(&manifest, &artifacts)?;
    let mut request = production_package_request(
        manifest.package_id.clone(),
        manifest.profile.clone(),
        artifacts,
        &package_target_manifest,
        target,
    )?;
    request.cook_summary = serde_json::to_vec(&manifest.asset_cook)?;
    request.asset_vfs_manifest = asset_vfs_manifest;
    request.asset_catalog = asset_catalog;
    request.target_manifest = serde_json::to_vec(&package_target_manifest)?;
    request.platform_eligibility = platform_eligibility(&package_target_manifest, target)?;
    request.scenario_refs = serde_json::to_vec(&scenario_refs_manifest_from_cooked(
        &manifest,
        &request.cooked_assets,
    )?)?;
    request.release_summary = br#"{"schema":"astra.release_summary.v1","status":"built"}"#.to_vec();
    if let Some(source_lock) = source_lock {
        validate_source_lock_coverage(&request, &source_lock.policy)?;
        request.source_unlock_policy = Some(source_lock.policy.clone());
        PackageBuilder::build_with_crypto(request, &source_lock.crypto)
            .map_err(|err| err.to_string().into())
    } else {
        PackageBuilder::build(request).map_err(|err| err.to_string().into())
    }
}

pub(crate) fn validate_source_lock_coverage(
    request: &PackageBuildRequest,
    policy: &SourceUnlockPolicy,
) -> Result<(), CliError> {
    let required = request
        .cooked_assets
        .iter()
        .chain(request.extra_sections.iter())
        .map(|section| section.id.clone())
        .chain(
            [
                "cook.summary",
                "asset.vfs_manifest",
                "asset.catalog",
                "media.manifest",
                "scenario.refs",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .collect::<std::collections::BTreeSet<_>>();
    let missing = required
        .difference(&policy.protected_sections)
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "source unlock policy leaves product payload sections plaintext: {}",
            missing.join(",")
        )
        .into());
    }
    Ok(())
}

pub(crate) fn production_package_request(
    package_id: String,
    profile: String,
    cooked_assets: Vec<SectionPayload>,
    target_manifest: &TargetManifest,
    target_id: &str,
) -> Result<PackageBuildRequest, CliError> {
    let has_font_manifest = cooked_assets.iter().any(|section| {
        section.id == "media.font_manifest" && section.schema == FONT_PACKAGE_MANIFEST_SCHEMA
    });
    let target = target_manifest
        .targets
        .iter()
        .find(|target| target.id == target_id && target.kind == astra_target::TargetKind::Game)
        .ok_or_else(|| format!("package target {target_id} is not a game target"))?;
    let provider_specs = [
        ("presentation", "astra.renderer.wgpu", "renderer2d.wgpu"),
        ("vfs_provider", "astra.vfs.package", "vfs.backend.package"),
    ];
    let bindings = provider_specs
        .iter()
        .map(|(slot, provider_id, capability)| {
            astra_plugin_abi::ProviderBinding::new(
                *slot,
                *provider_id,
                astra_plugin_abi::ProviderBindingContext {
                    package_id: package_id.clone(),
                    target: target_id.to_string(),
                    profile: profile.clone(),
                    required_capability: capability.to_string(),
                    engine_version: env!("CARGO_PKG_VERSION").to_string(),
                    rustc_fingerprint: "rustc-stable".to_string(),
                    feature_fingerprint: "runtime-typed-v3".to_string(),
                    abi_fingerprint: "astra-plugin-abi-v3".to_string(),
                },
            )
            .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let provider_policy = serde_json::to_vec(&astra_plugin_abi::ProviderPolicy {
        schema: astra_plugin_abi::PROVIDER_POLICY_SCHEMA.to_string(),
        profile: profile.clone(),
        renderer: "astra.renderer.wgpu".to_string(),

        bindings: bindings.clone(),
    })?;
    let plugin_extension_registry =
        serde_json::to_vec(&astra_plugin_abi::PluginExtensionRegistrySnapshot {
            schema: astra_plugin_abi::PLUGIN_EXTENSION_REGISTRY_SCHEMA.to_string(),
            providers: provider_specs
                .iter()
                .map(
                    |(slot, provider_id, capability)| astra_plugin_abi::ProviderExtensionRecord {
                        slot: slot.to_string(),
                        provider_id: provider_id.to_string(),
                        capability: capability.to_string(),
                        phase: astra_plugin_abi::LoadPhase::Runtime,
                        packaged: true,
                        engine_version: env!("CARGO_PKG_VERSION").to_string(),
                        rustc_fingerprint: "rustc-stable".to_string(),
                        feature_fingerprint: "runtime-typed-v3".to_string(),
                        abi_fingerprint: "astra-plugin-abi-v3".to_string(),
                    },
                )
                .collect(),
            bindings,
            conflicts: vec![],
        })?;
    Ok(PackageBuildRequest {
        package_id,
        profile: profile.clone(),
        cooked_assets,
        cook_summary: vec![],
        asset_vfs_manifest: vec![],
        asset_catalog: vec![],
        media_manifest: serde_json::to_vec(&serde_json::json!({
            "schema": "astra.media_manifest.v1",
            "codecs": ["png", "jpeg", "webp", "wav", "ogg", "flac", "mp3"],
            "ffmpeg": "profile_bound",
            "font_manifest_required": has_font_manifest,
            "font_manifest_section": if has_font_manifest { "media.font_manifest" } else { "" }
        }))?,
        provider_policy,
        plugin_extension_registry,
        plugin_dependency_graph:
            br#"{"schema":"astra.plugin_dependency_graph.v1","dependencies":[]}"#.to_vec(),
        module_fingerprint: serde_json::to_vec(&serde_json::json!({
            "schema": "astra.module_fingerprint.v1",
            "modules": [{"crate": target.crate_name, "target": target.id}]
        }))?,
        target_manifest: serde_json::to_vec(target_manifest)?,
        release_summary: br#"{"schema":"astra.release_summary.v1","status":"built"}"#.to_vec(),
        scenario_refs: serde_json::to_vec(&ScenarioRefsManifest::empty())?,
        platform_eligibility: vec![],
        extra_sections: vec![],
        source_unlock_policy: None,
    })
}

pub(crate) fn asset_vfs_manifest_from_cooked(
    manifest: &CookManifest,
    sections: &[SectionPayload],
) -> Result<Vec<u8>, CliError> {
    let section_by_id = sections
        .iter()
        .map(|section| (section.id.as_str(), section))
        .collect::<BTreeMap<_, _>>();
    let entries = manifest
        .artifacts
        .iter()
        .map(|artifact| {
            let section = section_by_id
                .get(artifact.section_id.as_str())
                .ok_or_else(|| format!("missing cooked section {}", artifact.section_id))?;
            Ok(serde_json::json!({
                "vfs_uri": artifact_vfs_uri(artifact)?,
                "layer_id": "package.base",
                "source": {
                    "kind": "package_section",
                    "section_id": artifact.section_id
                },
                "offset": 0,
                "size": section.payload.len() as u64,
                "hash": Hash256::from_sha256(&section.payload).to_string(),
                "codec": section_codec_name(&section.codec),
                "media_kind": artifact_media_kind(artifact),
                "diagnostics": []
            }))
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": {
                "kind": "package_section",
                "section_id": "package.manifest"
            },
            "targets": [manifest.target],
            "profiles": [manifest.profile]
        }],
        "entries": entries,
        "whiteouts": []
    }))
    .map_err(Into::into)
}

pub(crate) fn scenario_refs_manifest_from_cooked(
    manifest: &CookManifest,
    sections: &[SectionPayload],
) -> Result<ScenarioRefsManifest, CliError> {
    let section_by_id = sections
        .iter()
        .map(|section| (section.id.as_str(), section))
        .collect::<BTreeMap<_, _>>();
    let mut refs_by_path = manifest
        .artifacts
        .iter()
        .filter_map(|artifact| {
            artifact
                .scenario_path
                .as_deref()
                .map(|path| (path, artifact))
        })
        .collect::<BTreeMap<_, _>>();
    if refs_by_path.len() != manifest.scenario_refs.len() {
        return Err("cook manifest scenario path bindings are incomplete or duplicated".into());
    }
    let mut scenarios = Vec::with_capacity(manifest.scenario_refs.len());
    for path in &manifest.scenario_refs {
        let normalized = normalize_vfs_path(path)?;
        let artifact = refs_by_path
            .remove(normalized.as_str())
            .ok_or_else(|| format!("scenario ref {normalized} has no cooked section binding"))?;
        let section = section_by_id
            .get(artifact.section_id.as_str())
            .ok_or_else(|| format!("scenario ref {normalized} section is missing"))?;
        scenarios.push(ScenarioReference {
            path: normalized,
            section_id: artifact.section_id.clone(),
            hash: Hash256::from_sha256(&section.payload),
            byte_size: section.payload.len() as u64,
        });
    }
    scenarios.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ScenarioRefsManifest {
        schema: "astra.scenario_refs.v2".to_string(),
        scenarios,
    })
}

pub(crate) fn asset_catalog_from_cooked(
    manifest: &CookManifest,
    sections: &[SectionPayload],
) -> Result<Vec<u8>, CliError> {
    let section_ids = sections
        .iter()
        .map(|section| section.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let assets = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.asset_path.is_some())
        .map(|artifact| {
            if !section_ids.contains(artifact.section_id.as_str()) {
                return Err(format!("missing cooked asset section {}", artifact.section_id).into());
            }
            Ok(serde_json::json!({
                "asset_id": artifact.asset_id.as_deref().unwrap_or(&artifact.section_id),
                "vfs_uri": artifact_vfs_uri(artifact)?,
                "media_kind": artifact_media_kind(artifact),
                "tags": asset_tags_for_artifact(artifact),
                "bundle_id": manifest.profile,
                "chunk_id": "base",
                "profiles": [manifest.profile]
            }))
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_catalog.v1",
        "assets": assets
    }))
    .map_err(Into::into)
}

pub(crate) fn artifact_vfs_uri(artifact: &CookedArtifactRef) -> Result<String, CliError> {
    let path = artifact
        .asset_path
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| artifact.section_id.replace('.', "/"));
    Ok(format!("package:/{}", normalize_vfs_path(&path)?))
}

pub(crate) fn normalize_vfs_path(path: &str) -> Result<String, CliError> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.starts_with("~/")
        || normalized.contains("://")
        || normalized
            .split('/')
            .next()
            .is_some_and(|part| part.ends_with(':'))
    {
        return Err(format!("invalid VFS path {path}").into());
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') || part.chars().any(|ch| ch.is_control()) {
            return Err(format!("invalid VFS path {path}").into());
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(format!("invalid VFS path {path}").into());
    }
    Ok(parts.join("/"))
}

pub(crate) fn scenario_section_id(path: &str) -> String {
    format!(
        "scenario.ref.{}",
        Hash256::from_sha256(path.as_bytes()).to_hex()
    )
}

pub(crate) fn section_codec_name(codec: &SectionCodec) -> &'static str {
    match codec {
        SectionCodec::Postcard => "postcard",
        SectionCodec::Raw => "raw",
        SectionCodec::Zstd => "zstd",
    }
}

pub(crate) fn artifact_media_kind(artifact: &CookedArtifactRef) -> String {
    if artifact.font.is_some()
        || artifact
            .asset_type
            .as_deref()
            .is_some_and(|asset_type| asset_type.starts_with("font."))
    {
        "font".to_string()
    } else {
        artifact
            .asset_role
            .as_deref()
            .or(artifact.asset_type.as_deref())
            .unwrap_or("data")
            .to_string()
    }
}

pub(crate) fn asset_tags_for_artifact(artifact: &CookedArtifactRef) -> Vec<&str> {
    artifact
        .asset_role
        .as_deref()
        .into_iter()
        .chain(artifact.asset_type.as_deref())
        .collect()
}

pub(crate) fn build_standalone_bundle(
    package: &std::path::Path,
    out: &std::path::Path,
    target: &str,
    profile: &str,
    platform: PlatformId,
    artifacts: &BundleArtifactInputs,
) -> Result<StandaloneBundleManifest, CliError> {
    if out.exists() {
        return Err("ASTRA_BUNDLE_OUTPUT_EXISTS: refusing to replace existing output".into());
    }
    let parent = out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".astra-bundle-staging-")
        .tempdir_in(parent)?;
    let manifest = build_standalone_bundle_into(
        package,
        staging.path(),
        target,
        profile,
        platform,
        artifacts,
    )?;
    let staging_path = staging.keep();
    if let Err(error) = rename_directory_with_transient_retry(&staging_path, out, "bundle_commit") {
        let cleanup = fs::remove_dir_all(&staging_path);
        return Err(format!(
            "ASTRA_BUNDLE_COMMIT_FAILED: {error}; staging_cleanup={}",
            if cleanup.is_ok() {
                "complete"
            } else {
                "failed"
            }
        )
        .into());
    }
    Ok(manifest)
}
