use std::{
    fs,
    path::{Component, PathBuf},
};

use astra_core::Hash256;
use astra_package::SectionCodec;

use super::*;

pub(crate) fn asset_role_for_path(path: &str, asset_type: &str) -> String {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let parts = normalized.split('/').collect::<Vec<_>>();
    if asset_type.starts_with("font.") || parts.contains(&"fonts") {
        "font".to_string()
    } else if asset_type == "text" {
        "text".to_string()
    } else if parts.contains(&"backgrounds") {
        "background".to_string()
    } else if parts.contains(&"characters") {
        "character_sprite".to_string()
    } else if parts.contains(&"cg") {
        "cg".to_string()
    } else if parts.contains(&"ui") {
        "ui".to_string()
    } else if parts.contains(&"voice") {
        "voice".to_string()
    } else if parts
        .iter()
        .any(|part| matches!(*part, "audio" | "bgm" | "se"))
    {
        "audio".to_string()
    } else if parts.contains(&"movies") {
        "movie".to_string()
    } else if asset_type.starts_with("image.") {
        "image".to_string()
    } else if asset_type.starts_with("audio.") {
        "audio".to_string()
    } else {
        "binary".to_string()
    }
}

pub(crate) fn cook_scenario_ref_sections(
    scenario_refs: &[String],
    project_dir: &std::path::Path,
    out: &std::path::Path,
    existing: &[CookedArtifactRef],
) -> Result<Vec<CookedArtifactRef>, CliError> {
    if scenario_refs.is_empty() {
        return Ok(Vec::new());
    }
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::new();
    for (index, scenario_ref) in scenario_refs.iter().enumerate() {
        if existing
            .iter()
            .any(|artifact| artifact.section_id == *scenario_ref)
            || artifacts
                .iter()
                .any(|artifact: &CookedArtifactRef| artifact.section_id == *scenario_ref)
        {
            return Err(format!("scenario ref {scenario_ref} is declared more than once").into());
        }
        let source_path = resolve_scenario_ref_path(project_dir, scenario_ref)?;
        let payload = fs::read(source_path)
            .map_err(|err| format!("scenario ref {scenario_ref} is not readable: {err}"))?;
        let file_name = format!("scenario_ref_{:04}.bin", index + 1);
        fs::write(section_dir.join(&file_name), &payload)?;
        let normalized_path = normalize_vfs_path(scenario_ref)?;
        artifacts.push(CookedArtifactRef {
            section_id: scenario_section_id(&normalized_path),
            schema: scenario_ref_schema(&payload),
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
            scenario_path: Some(normalized_path),
        });
    }
    Ok(artifacts)
}

pub(crate) fn resolve_scenario_ref_path(
    project_dir: &std::path::Path,
    scenario_ref: &str,
) -> Result<PathBuf, CliError> {
    let relative = validate_project_relative_path(scenario_ref)?;
    let project_candidate = project_dir.join(&relative);
    let invocation_candidate = std::path::Path::new(".").join(&relative);
    let project_exists = project_candidate.is_file();
    let invocation_exists = invocation_candidate.is_file();
    match (project_exists, invocation_exists) {
        (true, true) if project_candidate != invocation_candidate => Err(format!(
            "scenario ref {scenario_ref} is ambiguous between project root and invocation root"
        )
        .into()),
        (true, _) => Ok(project_candidate),
        (false, true) => Ok(invocation_candidate),
        (false, false) => Err(format!(
            "scenario ref {scenario_ref} is not readable from project root or invocation root"
        )
        .into()),
    }
}

pub(crate) fn scenario_ref_schema(payload: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(payload) {
        if let Some(schema) = value.get("schema").and_then(serde_json::Value::as_str) {
            return schema.to_string();
        }
    }
    if let Ok(text) = std::str::from_utf8(payload) {
        if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(text) {
            if let Some(schema) = value.get("schema").and_then(serde_yaml::Value::as_str) {
                return schema.to_string();
            }
        }
    }
    "astra.scenario.v1".to_string()
}

pub(crate) fn cook_project_package_sections(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    out: &std::path::Path,
    profile: &str,
    target: &str,
    existing: &[CookedArtifactRef],
) -> Result<Vec<CookedArtifactRef>, CliError> {
    let Some(sections_value) = project.get("package_sections") else {
        return Ok(Vec::new());
    };
    let section_specs: Vec<ProjectPackageSection> = serde_yaml::from_value(sections_value.clone())?;
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::new();
    for spec in section_specs {
        if !spec.targets.is_empty() && !spec.targets.iter().any(|entry| entry == target) {
            continue;
        }
        if !spec.profiles.is_empty() && !spec.profiles.iter().any(|entry| entry == profile) {
            continue;
        }
        if existing
            .iter()
            .any(|artifact| artifact.section_id == spec.id)
            || artifacts
                .iter()
                .any(|artifact: &CookedArtifactRef| artifact.section_id == spec.id)
        {
            return Err(format!("package section {} is declared more than once", spec.id).into());
        }
        let source_path = validate_project_relative_path(&spec.path)?;
        let payload = fs::read(project_dir.join(source_path))?;
        let file_name = format!("{}.bin", spec.id.replace('.', "_"));
        fs::write(section_dir.join(&file_name), &payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: spec.id,
            schema: spec.schema,
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&payload).to_string(),
            codec: spec.codec,
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
    Ok(artifacts)
}

pub(crate) fn nativevn_source_paths(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    field: &str,
    default_directory: &str,
) -> Result<Vec<PathBuf>, CliError> {
    let mut sources = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get(field)),
    );
    if sources.is_empty() && field == "sources" {
        sources = string_list(project.get("scripts"));
    }
    if sources.is_empty() {
        let default = project_dir.join(default_directory);
        if default.is_dir() {
            sources.push(default_directory.to_string());
        }
    }

    let mut paths = Vec::new();
    for source in sources {
        let relative = PathBuf::from(&source);
        let absolute = project_dir.join(&relative);
        if absolute.is_dir() {
            collect_astra_sources(project_dir, &absolute, &mut paths)?;
        } else {
            paths.push(relative);
        }
    }
    paths.sort_by_key(|path| normalize_relative_path(path));
    paths.dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));
    Ok(paths)
}

pub(crate) fn nativevn_theme_paths(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
) -> Result<Vec<PathBuf>, CliError> {
    let mut sources = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("ui_themes")),
    );
    if sources.is_empty() && project_dir.join("Themes").is_dir() {
        sources.push("Themes".to_string());
    }
    let mut paths = Vec::new();
    for source in sources {
        let relative = validate_project_relative_path(&source)?;
        let absolute = project_dir.join(&relative);
        if absolute.is_dir() {
            collect_ui_theme_sources(project_dir, &absolute, &mut paths)?;
        } else {
            paths.push(relative);
        }
    }
    paths.sort_by_key(|path| normalize_relative_path(path));
    paths.dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));
    Ok(paths)
}

pub(crate) fn nativevn_controller_paths(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
) -> Result<Vec<PathBuf>, CliError> {
    let mut sources = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("ui_controllers")),
    );
    if sources.is_empty() && project_dir.join("Controllers").is_dir() {
        sources.push("Controllers".to_string());
    }
    let mut paths = Vec::new();
    for source in sources {
        let relative = validate_project_relative_path(&source)?;
        let absolute = project_dir.join(&relative);
        if absolute.is_dir() {
            collect_sources_with_extension(project_dir, &absolute, "luau", &mut paths)?;
        } else if relative
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("luau")
        {
            paths.push(relative);
        } else {
            return Err("ASTRA_UI_CONTROLLER_FORMAT: controller source must use .luau".into());
        }
    }
    paths.sort_by_key(|path| normalize_relative_path(path));
    paths.dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));
    Ok(paths)
}

pub(crate) fn collect_sources_with_extension(
    root: &std::path::Path,
    dir: &std::path::Path,
    extension: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_sources_with_extension(root, &path, extension, out)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

pub(crate) fn collect_ui_theme_sources(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_ui_theme_sources(root, &path, out)?;
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("json" | "yaml" | "yml")
        ) {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

pub(crate) fn collect_astra_sources(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_astra_sources(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("astra") {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

pub(crate) fn scenario_refs_from_project(project: &serde_yaml::Value) -> Vec<String> {
    let mut refs = string_list(project.get("scenario_refs"));
    refs.extend(string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("scenario_refs")),
    ));
    refs.sort();
    refs.dedup();
    refs
}

pub(crate) fn string_list(value: Option<&serde_yaml::Value>) -> Vec<String> {
    match value {
        Some(serde_yaml::Value::Sequence(values)) => values
            .iter()
            .filter_map(serde_yaml::Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(serde_yaml::Value::String(value)) => vec![value.clone()],
        _ => Vec::new(),
    }
}

pub(crate) fn normalize_relative_path(path: impl AsRef<std::path::Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

pub(crate) fn validate_project_relative_path(path: &str) -> Result<PathBuf, CliError> {
    let relative = PathBuf::from(path);
    if relative.is_absolute() {
        return Err("project package section path must be relative".into());
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("project package section path must stay inside the project".into());
            }
        }
    }
    Ok(relative)
}

pub(crate) fn validate_bundle_relative_path(path: &str) -> Result<PathBuf, CliError> {
    let relative = PathBuf::from(path);
    if relative.is_absolute() {
        return Err("bundle path must be relative".into());
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("bundle path must stay inside the bundle".into());
            }
        }
    }
    Ok(relative)
}
