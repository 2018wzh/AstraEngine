use astra_vn_editor::{AstraSource, CompileAstraProjectOptions};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) struct ProjectFiles {
    pub sources: Vec<AstraSource>,
    pub content: Vec<String>,
    pub options: CompileAstraProjectOptions,
}

pub(crate) fn load(manifest: &Path) -> anyhow::Result<ProjectFiles> {
    let root = manifest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Project directory is missing"))?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(manifest)?)?;
    let vn = yaml
        .get("nativevn")
        .ok_or_else(|| anyhow::anyhow!("Project has no nativevn source configuration"))?;
    let mut result = ProjectFiles {
        sources: Vec::new(),
        content: Vec::new(),
        options: Default::default(),
    };
    for (field, default, ui) in [("sources", "Scripts", false), ("ui_sources", "UI", true)] {
        for path in paths(root, vn, field, default)? {
            if path.extension().and_then(|e| e.to_str()) != Some("astra") {
                continue;
            }
            let relative = relative(root, &path)?;
            let text = std::fs::read_to_string(&path)?;
            result.sources.push(if ui {
                AstraSource::ui(relative, text)
            } else {
                AstraSource::story(relative, text)
            });
        }
    }
    anyhow::ensure!(
        !result.sources.is_empty(),
        "Project contains no Astra source"
    );
    for path in paths(root, vn, "ui_themes", "Themes")? {
        if !matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("json" | "yaml" | "yml")
        ) {
            continue;
        }
        #[derive(serde::Deserialize)]
        struct ThemeSource {
            schema: String,
            id: String,
            #[serde(default)]
            parent: Option<String>,
            tokens: BTreeMap<String, astra_ui_core::UiThemeValue>,
            #[serde(default)]
            high_contrast_tokens: BTreeMap<String, astra_ui_core::UiThemeValue>,
        }
        let text = std::fs::read_to_string(&path)?;
        let source: ThemeSource =
            if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                serde_json::from_str(&text)?
            } else {
                serde_yaml::from_str(&text)?
            };
        result.options = result
            .options
            .with_ui_theme(astra_ui_core::UiThemeManifest {
                schema: source.schema,
                id: source.id,
                parent: source.parent,
                tokens: source.tokens,
                high_contrast_tokens: source.high_contrast_tokens,
                revision: 1,
            });
    }
    let controllers = paths(root, vn, "ui_controllers", "Controllers")?;
    if !controllers.is_empty() {
        let mut host = astra_vn_policy::LuauUiControllerHost::with_default_budget()?;
        for path in controllers {
            if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("lua" | "luau")
            ) {
                host.register_source(std::fs::read_to_string(path)?)?;
            }
        }
        for manifest in host.manifests() {
            let source = host
                .source(&manifest.id)
                .ok_or_else(|| anyhow::anyhow!("Controller source missing"))?;
            result.options = result
                .options
                .with_ui_controller_source(&manifest.id, source);
        }
    }
    result.content = paths(root, vn, "asset_roots", "AssetSidecars")?
        .iter()
        .map(|path| relative(root, path))
        .collect::<anyhow::Result<_>>()?;
    Ok(result)
}

fn relative(root: &Path, path: &Path) -> anyhow::Result<String> {
    Ok(path
        .strip_prefix(root)?
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 project path"))?
        .replace('\\', "/"))
}

fn paths(
    root: &Path,
    config: &serde_yaml::Value,
    field: &str,
    default: &str,
) -> anyhow::Result<Vec<PathBuf>> {
    let names = if let Some(value) = config.get(field) {
        value
            .as_sequence()
            .ok_or_else(|| anyhow::anyhow!("{field} must be a path list"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| anyhow::anyhow!("{field} path must be text"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    } else if root.join(default).exists() {
        vec![default]
    } else {
        Vec::new()
    };
    let mut files = Vec::new();
    let mut pending = Vec::new();
    for name in names {
        anyhow::ensure!(
            !name.contains(['\\', ':'])
                && Path::new(name)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
            "Project paths must be safe relative paths"
        );
        pending.push(root.join(name));
    }
    let mut visited = std::collections::BTreeSet::new();
    while let Some(path) = pending.pop() {
        let path = path.canonicalize()?;
        anyhow::ensure!(
            path.starts_with(root),
            "Project entry escapes the project directory"
        );
        if !visited.insert(path.clone()) {
            continue;
        }
        anyhow::ensure!(visited.len() <= 16_384, "Project entry limit exceeded");
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}
