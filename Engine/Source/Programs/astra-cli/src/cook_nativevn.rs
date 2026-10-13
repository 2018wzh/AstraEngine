use std::{collections::BTreeMap, fs, path::PathBuf};

use astra_asset::{AssetSidecar, VfsUri};
use astra_cook::{
    CookBatchExecutor, CookBatchLimits, CookBatchRequest, CookCancellationToken, CookNode,
    CookProcessorRegistry, CookRequest, DefaultCookProcessor, FileCookCache,
};
use astra_core::Hash256;
use astra_media::{
    FontPackageEntry, FontPackageManifest, UnicodeRange, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_package::{CookSummaryManifest, SectionCodec};
use astra_platform::{migrate_host_profile_json, validate_host_profile};
use astra_vn_package::{
    package_sections_for_project, PlayerLocaleConfig, PLAYER_LOCALE_CONFIG_SCHEMA,
};
use astra_vn_policy::LuauUiControllerHost;
use astra_vn_script::{compile_astra_project, AstraSource, CompileAstraProjectOptions};

use super::*;

pub(crate) fn cook_platform_profiles(
    project: &serde_yaml::Value,
    package_id: &str,
    target: &str,
    target_platforms: &[String],
) -> Result<Option<CookedPlatformProfiles>, CliError> {
    let Some(value) = project.get("platform_profiles") else {
        return Ok(None);
    };
    let raw_profiles: BTreeMap<String, serde_json::Value> =
        serde_json::from_value(serde_json::to_value(value.clone())?)?;
    let mut selected = Vec::new();
    for (id, raw_profile) in raw_profiles {
        let profile = migrate_host_profile_json(raw_profile)?;
        if profile.id != id {
            return Err(format!("platform profile key {id} does not match profile id").into());
        }
        if profile.target != target || profile.package_id != package_id {
            return Err(
                format!("platform profile {id} is not bound to the cooked target/package").into(),
            );
        }
        if !target_platforms
            .iter()
            .any(|platform| platform == profile.platform.as_str())
        {
            return Err(format!("platform profile {id} is not declared by target {target}").into());
        }
        validate_host_profile(&profile)?;
        selected.push(profile);
    }
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    if selected.is_empty() {
        return Err("platform_profiles must contain at least one selected profile".into());
    }
    Ok(Some(CookedPlatformProfiles {
        schema: "astra.platform_profiles.v3".to_string(),
        profiles: selected,
    }))
}

pub(crate) fn read_cook_manifest(cooked: &std::path::Path) -> Result<CookManifest, CliError> {
    let text = fs::read_to_string(cooked.join("cook_manifest.yaml"))?;
    let manifest: CookManifest = serde_yaml::from_str(&text)?;
    if manifest.schema != "astra.cook_manifest.v2"
        || manifest.asset_cook.schema != "astra.cook_batch_summary.v1"
    {
        return Err("ASTRA_COOK_MANIFEST_VERSION: unsupported cook manifest schema".into());
    }
    let asset_count = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.asset_id.is_some())
        .count() as u64;
    if asset_count != manifest.asset_cook.artifact_count
        || manifest.asset_cook.cache_hit_count + manifest.asset_cook.cooked_count
            != manifest.asset_cook.artifact_count
        || manifest.asset_cook.max_concurrency == 0
    {
        return Err("ASTRA_COOK_MANIFEST_IDENTITY: asset cook summary is inconsistent".into());
    }
    Ok(manifest)
}

pub(crate) fn project_uses_nativevn(project: &serde_yaml::Value) -> bool {
    project.get("nativevn").is_some()
}

pub(crate) fn cook_nativevn_sections(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    out: &std::path::Path,
    profile: &str,
    target: &str,
    cancellation: &CookCancellationToken,
) -> Result<(Vec<CookedArtifactRef>, CookSummaryManifest), CliError> {
    cancellation.check_cancelled()?;
    let source_paths = nativevn_source_paths(project, project_dir, "sources", "Scripts")?;
    if source_paths.is_empty() {
        return Err("nativevn project must declare at least one .astra source".into());
    }
    let mut sources = Vec::with_capacity(source_paths.len());
    for source in source_paths {
        let source_text = fs::read_to_string(project_dir.join(&source))?;
        sources.push(AstraSource::story(
            normalize_relative_path(&source),
            source_text,
        ));
    }
    let ui_source_paths = nativevn_source_paths(project, project_dir, "ui_sources", "UI")?;
    if ui_source_paths.is_empty() {
        return Err("nativevn project must declare at least one UI .astra source".into());
    }
    for source in ui_source_paths {
        let source_text = fs::read_to_string(project_dir.join(&source))?;
        sources.push(AstraSource::ui(
            normalize_relative_path(&source),
            source_text,
        ));
    }
    let theme_paths = nativevn_theme_paths(project, project_dir)?;
    if theme_paths.is_empty() {
        return Err("nativevn project must declare at least one UI theme manifest".into());
    }
    let mut compile_options = CompileAstraProjectOptions::default();
    for theme_path in theme_paths {
        let text = fs::read_to_string(project_dir.join(&theme_path))?;
        #[derive(serde::Deserialize)]
        struct UiThemeSource {
            schema: String,
            id: String,
            #[serde(default)]
            parent: Option<String>,
            tokens: std::collections::BTreeMap<String, astra_ui_core::UiThemeValue>,
            #[serde(default)]
            high_contrast_tokens: std::collections::BTreeMap<String, astra_ui_core::UiThemeValue>,
        }
        let source: UiThemeSource = match theme_path.extension().and_then(std::ffi::OsStr::to_str) {
            Some("json") => serde_json::from_str(&text)?,
            Some("yaml" | "yml") => serde_yaml::from_str(&text)?,
            _ => {
                return Err(format!(
                    "ASTRA_UI_THEME_FORMAT: {} must use .json, .yaml, or .yml",
                    normalize_relative_path(&theme_path)
                )
                .into())
            }
        };
        let theme = astra_ui_core::UiThemeManifest {
            schema: source.schema,
            id: source.id,
            parent: source.parent,
            tokens: source.tokens,
            high_contrast_tokens: source.high_contrast_tokens,
            revision: 1,
        };
        compile_options = compile_options.with_ui_theme(theme);
    }
    let controller_paths = nativevn_controller_paths(project, project_dir)?;
    if controller_paths.is_empty() {
        return Err("nativevn project must declare at least one UI controller source".into());
    }
    let mut controller_host = LuauUiControllerHost::with_default_budget()?;
    for controller_path in controller_paths {
        let source = fs::read_to_string(project_dir.join(&controller_path))?;
        controller_host.register_source(source)?;
    }
    for manifest in controller_host.manifests() {
        let source = controller_host
            .source(&manifest.id)
            .ok_or("ASTRA_UI_CONTROLLER_SOURCE_IDENTITY: validated controller source is missing")?;
        compile_options =
            compile_options.with_ui_controller_source(manifest.id.clone(), source.to_string());
    }
    let policy_actions = nativevn_actions::load(project, project_dir, cancellation)?;
    for binding in &policy_actions {
        compile_options = compile_options.bind_extension(binding.descriptor.clone());
    }
    let compiled = compile_astra_project(sources, compile_options)?;
    nativevn_actions::validate_commands(&compiled, &policy_actions)?;
    let mut profiles = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("profiles")),
    );
    if profiles.is_empty() {
        profiles.push(profile.to_string());
    }
    if !profiles.iter().any(|entry| entry == profile) {
        profiles.push(profile.to_string());
    }

    let mut sections = package_sections_for_project(&compiled, &profiles, target)?;
    if !policy_actions.is_empty() {
        sections.push(astra_vn_package::policy_action_section(&policy_actions)?);
    }
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::new();
    for section in sections {
        if section.id == "scenario.refs" {
            continue;
        }
        let file_name = format!("{}.bin", section.id.replace('.', "_"));
        fs::write(section_dir.join(&file_name), &section.payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: section.id,
            schema: section.schema,
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&section.payload).to_string(),
            codec: section.codec,
            asset_path: None,
            asset_role: None,
            asset_sha256: None,
            asset_byte_size: None,
            asset_type: None,
            asset_id: None,
            font: None,
            scenario_path: None,
        });
    }
    let (asset_artifacts, asset_cook) =
        cook_nativevn_asset_sections(project, project_dir, out, profile, cancellation)?;
    artifacts.extend(asset_artifacts);
    nativevn_actions::validate_resources(&policy_actions, &artifacts, out, cancellation)?;
    Ok((artifacts, asset_cook))
}

pub(crate) fn cook_nativevn_asset_sections(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    out: &std::path::Path,
    profile: &str,
    cancellation: &CookCancellationToken,
) -> Result<(Vec<CookedArtifactRef>, CookSummaryManifest), CliError> {
    cancellation.check_cancelled()?;
    let mut sidecar_paths = Vec::new();
    for root in nativevn_asset_roots(project) {
        let relative_root = validate_project_relative_path(&root)?;
        let absolute_root = project_dir.join(&relative_root);
        if !absolute_root.is_dir() {
            return Err(format!("nativevn asset root is missing: {root}").into());
        }
        collect_asset_sidecars(project_dir, &absolute_root, &mut sidecar_paths)?;
    }
    sidecar_paths.sort_by_key(|path| normalize_relative_path(path));
    sidecar_paths
        .dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));

    let mut nodes = Vec::with_capacity(sidecar_paths.len());
    let mut prepared = BTreeMap::new();
    let mut processor_ids = std::collections::BTreeSet::new();
    for sidecar_path in sidecar_paths {
        let sidecar_text = fs::read_to_string(project_dir.join(&sidecar_path))?;
        let sidecar = AssetSidecar::from_yaml(&sidecar_text)?;
        let diagnostics = sidecar.validate();
        if !diagnostics.is_empty() {
            return Err(format!("nativevn asset sidecar is invalid: {}", sidecar.source).into());
        }
        let source_path = validate_project_relative_path(&sidecar.source)?;
        let source_bytes = fs::read(project_dir.join(&source_path))?;
        let source_hash = Hash256::from_sha256(&source_bytes);
        if sidecar.source_hash.as_ref() != Some(&source_hash) {
            return Err(format!("nativevn asset hash mismatch: {}", sidecar.source).into());
        }

        let source_byte_size = source_bytes.len() as u64;
        let asset_id = sidecar.id.to_string();
        if prepared
            .insert(
                asset_id.clone(),
                PreparedAssetCook {
                    asset_id: asset_id.clone(),
                    source_path,
                    source_hash,
                    source_byte_size,
                    asset_role: asset_role_for_path(&sidecar.source, &sidecar.asset_type),
                    asset_type: sidecar.asset_type.clone(),
                    font: sidecar.font.as_ref().map(|font| CookedFontMetadata {
                        family: font.family.clone(),
                        face_index: font.face_index,
                        license_id: sidecar
                            .license
                            .clone()
                            .expect("validated asset sidecar has a license"),
                        subset: font.subset.clone(),
                        coverage: font
                            .coverage
                            .iter()
                            .map(|range| UnicodeRange {
                                start: range.start,
                                end: range.end,
                            })
                            .collect(),
                    }),
                },
            )
            .is_some()
        {
            return Err(format!("nativevn asset id is duplicated: {asset_id}").into());
        }
        processor_ids.insert(sidecar.cook.processor.clone());
        nodes.push(CookNode {
            request: CookRequest {
                sidecar: sidecar.clone(),
                source_bytes,
                target_profile: profile.to_string(),
                processor_version: "1.0.0".to_string(),
                dependency_artifacts: Default::default(),
            },
        });
    }
    let mut registry = CookProcessorRegistry::default();
    for processor_id in processor_ids {
        registry.register(DefaultCookProcessor::new(processor_id, "1.0.0"))?;
    }
    let cache = FileCookCache::new(project_dir.join(".astra-cache").join("cook"));
    let executor = CookBatchExecutor::new(&registry, Some(&cache));
    let max_concurrency = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8);
    let batch = executor.execute(
        CookBatchRequest {
            nodes,
            max_concurrency,
            limits: CookBatchLimits {
                max_node_count: 65_536,
                max_source_bytes_per_node: 1024 * 1024 * 1024,
                max_total_source_bytes: 8 * 1024 * 1024 * 1024,
                max_concurrency: 8,
            },
        },
        cancellation,
    )?;

    let summary = CookSummaryManifest {
        schema: "astra.cook_batch_summary.v1".to_string(),
        graph_hash: batch.graph_hash,
        artifact_count: batch.artifacts.len() as u64,
        cache_hit_count: batch.cache_hit_count,
        cooked_count: batch.cooked_count,
        max_concurrency: max_concurrency as u64,
    };
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::with_capacity(batch.artifacts.len());
    for cooked in batch.artifacts {
        let metadata = prepared
            .remove(&cooked.asset_id)
            .ok_or_else(|| format!("cooked asset has no prepared metadata: {}", cooked.asset_id))?;
        let section = cooked.to_section();
        let file_name = format!("{}.bin", section.id.replace('.', "_"));
        fs::write(section_dir.join(&file_name), &section.payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: section.id,
            schema: section.schema,
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&section.payload).to_string(),
            codec: section.codec,
            asset_path: Some(normalize_relative_path(&metadata.source_path)),
            asset_role: Some(metadata.asset_role),
            asset_sha256: Some(metadata.source_hash.to_string()),
            asset_byte_size: Some(metadata.source_byte_size),
            asset_type: Some(metadata.asset_type),
            asset_id: Some(metadata.asset_id),
            font: metadata.font,
            scenario_path: None,
        });
    }
    if !prepared.is_empty() {
        return Err("cook batch did not produce every prepared asset".into());
    }
    Ok((artifacts, summary))
}

pub(crate) fn nativevn_asset_roots(project: &serde_yaml::Value) -> Vec<String> {
    string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("asset_roots")),
    )
}

pub(crate) fn cook_font_manifest_section(
    artifacts: &[CookedArtifactRef],
    target: &str,
    profile: &str,
    out: &std::path::Path,
) -> Result<CookedArtifactRef, CliError> {
    if artifacts
        .iter()
        .any(|artifact| artifact.section_id == "media.font_manifest")
    {
        return Err(
            "ASTRA_COOK_FONT_MANIFEST_DUPLICATE: media.font_manifest is generated from font assets"
                .into(),
        );
    }
    let mut fonts = artifacts
        .iter()
        .filter_map(|artifact| artifact.font.as_ref().map(|font| (artifact, font)))
        .map(|(artifact, font)| {
            let asset_id = artifact
                .asset_id
                .clone()
                .ok_or("ASTRA_COOK_FONT_IDENTITY: font artifact is missing its asset id")?;
            let asset_path = artifact
                .asset_path
                .as_deref()
                .ok_or("ASTRA_COOK_FONT_IDENTITY: font artifact is missing its package path")?;
            let hash: Hash256 = artifact
                .hash
                .parse()
                .map_err(|_| "ASTRA_COOK_FONT_HASH: font artifact hash is invalid")?;
            Ok(FontPackageEntry {
                asset_id,
                uri: VfsUri::parse(&format!("package:/{}", normalize_vfs_path(asset_path)?))?,
                family: font.family.clone(),
                face_index: font.face_index,
                hash,
                license_id: font.license_id.clone(),
                subset: font.subset.clone(),
                coverage: font.coverage.clone(),
                targets: vec![target.to_string()],
                profiles: vec![profile.to_string()],
            })
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    fonts.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
    if fonts.is_empty() {
        return Err(
            "ASTRA_COOK_FONT_MANIFEST_MISSING: NativeVN projects require at least one packaged font asset"
                .into(),
        );
    }
    let manifest = FontPackageManifest {
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.to_string(),
        target: target.to_string(),
        profile: profile.to_string(),
        provider_binding: "astra.vfs.package".to_string(),
        fonts,
    };
    let payload = serde_json::to_vec_pretty(&manifest)?;
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let file_name = "media_font_manifest.json";
    fs::write(section_dir.join(file_name), &payload)?;
    Ok(CookedArtifactRef {
        section_id: "media.font_manifest".to_string(),
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.to_string(),
        path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
        hash: Hash256::from_sha256(&payload).to_string(),
        codec: SectionCodec::Raw,
        asset_path: None,
        asset_role: None,
        asset_sha256: None,
        asset_byte_size: None,
        asset_type: None,
        asset_id: None,
        font: None,
        scenario_path: None,
    })
}

pub(crate) fn cook_player_locale_config(
    project: &serde_yaml::Value,
    artifacts: &[CookedArtifactRef],
    out: &std::path::Path,
) -> Result<CookedArtifactRef, CliError> {
    let default_locale = project
        .get("nativevn")
        .and_then(|nativevn| nativevn.get("default_locale"))
        .and_then(serde_yaml::Value::as_str)
        .ok_or("ASTRA_COOK_LOCALE_DEFAULT: nativevn.default_locale is required")?;
    if default_locale.is_empty()
        || default_locale.len() > 64
        || !default_locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("ASTRA_COOK_LOCALE_DEFAULT: default locale id is unsafe".into());
    }
    let mut available_locales = artifacts
        .iter()
        .filter(|artifact| artifact.schema == "astra.vn.localization_table.v1")
        .filter_map(|artifact| artifact.section_id.strip_prefix("vn.localization."))
        .map(str::to_string)
        .collect::<Vec<_>>();
    available_locales.sort();
    available_locales.dedup();
    if available_locales.is_empty()
        || !available_locales
            .iter()
            .any(|locale| locale == default_locale)
    {
        return Err(
            "ASTRA_COOK_LOCALE_COVERAGE: default locale has no profile-eligible localization section"
                .into(),
        );
    }
    let config = PlayerLocaleConfig {
        schema: PLAYER_LOCALE_CONFIG_SCHEMA.to_string(),
        default_locale: default_locale.to_string(),
        available_locales,
    };
    let payload = serde_json::to_vec_pretty(&config)?;
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let file_name = "player_locale_config.json";
    fs::write(section_dir.join(file_name), &payload)?;
    Ok(CookedArtifactRef {
        section_id: "player.locale_config".to_string(),
        schema: PLAYER_LOCALE_CONFIG_SCHEMA.to_string(),
        path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
        hash: Hash256::from_sha256(&payload).to_string(),
        codec: SectionCodec::Raw,
        asset_path: None,
        asset_role: None,
        asset_sha256: None,
        asset_byte_size: None,
        asset_type: None,
        asset_id: None,
        font: None,
        scenario_path: None,
    })
}

pub(crate) fn collect_asset_sidecars(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_asset_sidecars(root, &path, out)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".astra-asset.yaml"))
        {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}
