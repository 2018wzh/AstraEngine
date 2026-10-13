use std::{
    fs,
    path::{Path, PathBuf},
};

use astra_cook::CookCancellationToken;
use astra_core::Hash256;
use astra_package::{CookSummaryManifest, SectionCodec};
use astra_target::{validate_manifest, TargetManifest};

use super::*;

pub(crate) fn project_player_display_config(
    project: &serde_yaml::Value,
) -> Result<Option<PlayerDisplayConfig>, CliError> {
    let Some(display) = project
        .get("nativevn")
        .and_then(|nativevn| nativevn.get("display"))
    else {
        return Ok(None);
    };
    let Some(resolution) = display.get("original_resolution") else {
        return Err(
            "nativevn.display.original_resolution is required when display is declared".into(),
        );
    };
    let width = yaml_u32(resolution.get("width"))
        .ok_or("nativevn.display.original_resolution.width must be a positive integer")?;
    let height = yaml_u32(resolution.get("height"))
        .ok_or("nativevn.display.original_resolution.height must be a positive integer")?;
    if !(1..=16_384).contains(&width) || !(1..=16_384).contains(&height) {
        return Err("nativevn.display.original_resolution dimensions are out of range".into());
    }
    let scale_filter = display
        .get("scale_filter")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("linear");
    if !matches!(scale_filter, "nearest" | "linear") {
        return Err("nativevn.display.scale_filter must be nearest or linear".into());
    }
    let preview_layers = project_display_preview_layers(display)?;
    Ok(Some(PlayerDisplayConfig {
        schema: "astra.player_display_config.v1".to_string(),
        original_resolution: PlayerResolution { width, height },
        scale_filter: scale_filter.to_string(),
        preview_layers,
    }))
}

pub(crate) fn project_display_preview_layers(
    display: &serde_yaml::Value,
) -> Result<Vec<PlayerDisplayLayer>, CliError> {
    let Some(raw_layers) = display.get("preview_layers") else {
        return Ok(Vec::new());
    };
    let layers = raw_layers
        .as_sequence()
        .ok_or("nativevn.display.preview_layers must be a list")?;
    let mut parsed = Vec::new();
    for layer in layers {
        let vfs_uri = layer
            .get("vfs_uri")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("nativevn.display.preview_layers entries require vfs_uri")?;
        validate_player_display_layer_uri(vfs_uri)?;
        let x = yaml_non_negative_u32(layer.get("x"))
            .ok_or("nativevn.display.preview_layers entries require non-negative x")?;
        let y = yaml_non_negative_u32(layer.get("y"))
            .ok_or("nativevn.display.preview_layers entries require non-negative y")?;
        if x > 16_384 || y > 16_384 {
            return Err("nativevn.display.preview_layers coordinates are out of range".into());
        }
        parsed.push(PlayerDisplayLayer {
            vfs_uri: vfs_uri.to_string(),
            x,
            y,
        });
    }
    Ok(parsed)
}

pub(crate) fn validate_player_display_layer_uri(vfs_uri: &str) -> Result<(), CliError> {
    if !vfs_uri.starts_with("package:/")
        || vfs_uri.contains('\\')
        || vfs_uri.contains("..")
        || vfs_uri.contains("://")
        || vfs_uri.trim() != vfs_uri
    {
        return Err("player display preview layer vfs_uri must be a package VFS URI".into());
    }
    Ok(())
}

pub(crate) fn yaml_u32(value: Option<&serde_yaml::Value>) -> Option<u32> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return u32::try_from(number).ok().filter(|number| *number > 0);
    }
    value
        .as_i64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| *number > 0)
}

pub(crate) fn yaml_non_negative_u32(value: Option<&serde_yaml::Value>) -> Option<u32> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return u32::try_from(number).ok();
    }
    value.as_i64().and_then(|number| u32::try_from(number).ok())
}

pub(crate) fn read_target_manifest(project: &std::path::Path) -> Result<TargetManifest, CliError> {
    let text = fs::read_to_string(project)?;
    TargetManifest::from_project_yaml(&text).map_err(|err| err.to_string().into())
}

pub(crate) fn cook_project(
    project: PathBuf,
    profile: &str,
    target: Option<&str>,
    out: PathBuf,
    cancellation: &CookCancellationToken,
) -> Result<CookManifest, CliError> {
    cancellation.check_cancelled()?;
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".astra-cook-stage-")
        .tempdir_in(parent)?;
    let manifest = cook_project_into(
        project,
        profile,
        target,
        staging.path().to_path_buf(),
        cancellation,
    )?;
    cancellation.check_cancelled()?;
    atomic_replace_directory(staging, &out)?;
    Ok(manifest)
}

pub(crate) fn atomic_replace_directory(
    staging: tempfile::TempDir,
    destination: &Path,
) -> Result<(), CliError> {
    if destination.exists() && !destination.is_dir() {
        return Err("ASTRA_COOK_COMMIT_PATH: cook output exists and is not a directory".into());
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let staging_path = staging.keep();
    if !destination.exists() {
        if let Err(error) =
            rename_directory_with_transient_retry(&staging_path, destination, "commit")
        {
            let _ = fs::remove_dir_all(&staging_path);
            return Err(format!("ASTRA_COOK_COMMIT_SWAP: {error}").into());
        }
        return Ok(());
    }
    let staging_name = staging_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("ASTRA_COOK_COMMIT_PATH: staging directory name is invalid")?;
    let backup = parent.join(format!("{staging_name}.backup"));
    rename_directory_with_transient_retry(destination, &backup, "backup")
        .map_err(|error| format!("ASTRA_COOK_COMMIT_BACKUP: {error}"))?;
    if let Err(error) = rename_directory_with_transient_retry(&staging_path, destination, "commit")
    {
        let rollback = rename_directory_with_transient_retry(&backup, destination, "rollback");
        let _ = fs::remove_dir_all(&staging_path);
        return match rollback {
            Ok(()) => Err(format!("ASTRA_COOK_COMMIT_SWAP: {error}").into()),
            Err(rollback_error) => Err(format!(
                "ASTRA_COOK_COMMIT_ROLLBACK: swap failed ({error}) and rollback failed ({rollback_error})"
            )
            .into()),
        };
    }
    if let Err(error) = fs::remove_dir_all(&backup) {
        let failed_output = parent.join(format!("{staging_name}.rollback"));
        let rollback =
            fs::rename(destination, &failed_output).and_then(|_| fs::rename(&backup, destination));
        let _ = fs::remove_dir_all(&failed_output);
        return match rollback {
            Ok(()) => Err(format!(
                "ASTRA_COOK_COMMIT_CLEANUP: {error}; previous output restored"
            )
            .into()),
            Err(rollback_error) => Err(format!(
                "ASTRA_COOK_COMMIT_ROLLBACK: cleanup failed ({error}) and rollback failed ({rollback_error})"
            )
            .into()),
        };
    }
    Ok(())
}

pub(crate) fn rename_directory_with_transient_retry(
    source: &Path,
    destination: &Path,
    operation: &'static str,
) -> std::io::Result<()> {
    const RETRY_DELAYS_MS: [u64; 5] = [10, 20, 40, 80, 160];
    for (index, delay_ms) in RETRY_DELAYS_MS.into_iter().enumerate() {
        match fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
                ) =>
            {
                tracing::warn!(
                    event = "filesystem.atomic_directory_rename.retry",
                    operation,
                    attempt = index + 1,
                    error_kind = ?error.kind(),
                    "atomic directory rename encountered a transient filesystem lock"
                );
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }
    fs::rename(source, destination)
}

pub(crate) fn cook_project_into(
    project: PathBuf,
    profile: &str,
    target: Option<&str>,
    out: PathBuf,
    cancellation: &CookCancellationToken,
) -> Result<CookManifest, CliError> {
    cancellation.check_cancelled()?;
    tracing::info!(
        target: "astra_cook",
        event = "cook.run.start",
        profile,
        has_target = target.is_some(),
        "cook run started"
    );
    let project_text = fs::read_to_string(&project)?;
    let project_yaml: serde_yaml::Value = serde_yaml::from_str(&project_text)?;
    let target_manifest = TargetManifest::from_project_value(&project_yaml)?;
    let target = target
        .or_else(|| {
            target_manifest
                .targets
                .first()
                .map(|target| target.id.as_str())
        })
        .ok_or("project has no targets")?;
    let target_report = validate_manifest(&target_manifest, Some(target));
    if matches!(
        target_report.status,
        astra_target::TargetValidationStatus::Blocked
    ) {
        return Err(format!("target validation failed: {target}").into());
    }
    cancellation.check_cancelled()?;
    let target_descriptor = target_manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    let package_id = project_yaml
        .get("id")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("com.example.nativevn")
        .to_string();
    let project_hash = Hash256::from_sha256(project_text.as_bytes()).to_string();
    fs::create_dir_all(&out)?;

    let project_dir = project
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let artifact = serde_json::json!({
        "schema": "astra.cooked_project.v1",
        "package_id": package_id,
        "profile": profile,
        "target": target,
        "project_hash": project_hash,
    })
    .to_string()
    .into_bytes();
    let artifact_path = "compiled_project.json";
    fs::write(out.join(artifact_path), &artifact)?;
    let mut artifacts = vec![CookedArtifactRef {
        section_id: "compiled.project".to_string(),
        schema: "astra.cooked_project.v1".to_string(),
        path: artifact_path.to_string(),
        hash: Hash256::from_sha256(&artifact).to_string(),
        codec: SectionCodec::Raw,
        asset_path: None,
        asset_role: None,
        asset_sha256: None,
        asset_byte_size: None,
        asset_type: None,
        asset_id: None,
        font: None,
        scenario_path: None,
    }];
    if let Some(platform_profiles) = cook_platform_profiles(
        &project_yaml,
        &package_id,
        target,
        &target_descriptor.platforms,
    )? {
        let profile_bytes = serde_json::to_vec_pretty(&platform_profiles)?;
        let profile_path = "platform_profiles.json";
        fs::write(out.join(profile_path), &profile_bytes)?;
        artifacts.push(CookedArtifactRef {
            section_id: "platform.profiles".to_string(),
            schema: "astra.platform_profiles.v3".to_string(),
            path: profile_path.to_string(),
            hash: Hash256::from_sha256(&profile_bytes).to_string(),
            codec: SectionCodec::Raw,
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
    if let Some(display_config) = project_player_display_config(&project_yaml)? {
        let display_bytes = serde_json::to_vec_pretty(&display_config)?;
        let display_path = "player_display_config.json";
        fs::write(out.join(display_path), &display_bytes)?;
        artifacts.push(CookedArtifactRef {
            section_id: "player.display_config".to_string(),
            schema: "astra.player_display_config.v1".to_string(),
            path: display_path.to_string(),
            hash: Hash256::from_sha256(&display_bytes).to_string(),
            codec: SectionCodec::Raw,
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
    let mut asset_cook = CookSummaryManifest::empty();
    if project_uses_nativevn(&project_yaml) {
        let (nativevn_artifacts, summary) = cook_nativevn_sections(
            &project_yaml,
            project_dir,
            &out,
            profile,
            target,
            cancellation,
        )?;
        artifacts.extend(nativevn_artifacts);
        asset_cook = summary;
    }
    artifacts.extend(cook_project_package_sections(
        &project_yaml,
        project_dir,
        &out,
        profile,
        target,
        &artifacts,
    )?);
    if project_uses_nativevn(&project_yaml) {
        artifacts.push(cook_player_locale_config(&project_yaml, &artifacts, &out)?);
    }
    let scenario_refs = scenario_refs_from_project(&project_yaml);
    artifacts.extend(cook_scenario_ref_sections(
        &scenario_refs,
        project_dir,
        &out,
        &artifacts,
    )?);
    if project_uses_nativevn(&project_yaml) {
        artifacts.push(cook_font_manifest_section(
            &artifacts, target, profile, &out,
        )?);
    }
    let manifest = CookManifest {
        schema: "astra.cook_manifest.v2".to_string(),
        package_id,
        profile: profile.to_string(),
        target: target.to_string(),
        project_hash,
        target_manifest,
        scenario_refs,
        artifacts,
        asset_cook,
    };
    cancellation.check_cancelled()?;
    fs::write(
        out.join("cook_manifest.yaml"),
        serde_yaml::to_string(&manifest)?,
    )?;
    tracing::info!(
        target: "astra_cook",
        event = "cook.run.complete",
        profile,
        target = %manifest.target,
        artifact_count = manifest.artifacts.len(),
        scenario_count = manifest.scenario_refs.len(),
        "cook run completed"
    );
    Ok(manifest)
}
