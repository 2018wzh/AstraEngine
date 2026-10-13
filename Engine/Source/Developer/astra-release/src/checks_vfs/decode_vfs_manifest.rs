use super::*;

pub(crate) fn decode_vfs_manifest(
    package: &PackageReader,
) -> Result<VfsManifest, Box<ReleaseCheckRecord>> {
    let bytes = package
        .container()
        .read_bounded("asset.vfs_manifest", 1024 * 1024)
        .map_err(|err| {
            Box::new(package_blocked(
                "vfs.prefix_registry",
                "ASTRA_VFS_MANIFEST_MISSING",
                format!("asset.vfs_manifest section is required: {err}"),
                vec![evidence("section", "asset.vfs_manifest")],
            ))
        })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        Box::new(package_blocked(
            "vfs.prefix_registry",
            "ASTRA_VFS_MANIFEST_JSON",
            format!("asset.vfs_manifest is not valid JSON: {err}"),
            vec![evidence("section", "asset.vfs_manifest")],
        ))
    })
}

pub(crate) fn decode_asset_catalog(
    package: &PackageReader,
) -> Result<AssetCatalog, Box<ReleaseCheckRecord>> {
    let bytes = package
        .container()
        .read_bounded("asset.catalog", 1024 * 1024)
        .map_err(|err| {
            Box::new(package_blocked(
                "vfs.catalog",
                "ASTRA_VFS_CATALOG_MISSING",
                format!("asset.catalog section is required: {err}"),
                vec![evidence("section", "asset.catalog")],
            ))
        })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        Box::new(package_blocked(
            "vfs.catalog",
            "ASTRA_VFS_CATALOG_JSON",
            format!("asset.catalog is not valid JSON: {err}"),
            vec![evidence("section", "asset.catalog")],
        ))
    })
}

#[derive(Debug, Clone)]
pub(crate) struct RegisteredVfsProvider {
    pub(super) capability: String,
    pub(super) packaged: bool,
}

pub(crate) fn vfs_registered_providers(
    package: &PackageReader,
) -> Result<BTreeMap<String, RegisteredVfsProvider>, Box<ReleaseCheckRecord>> {
    let registry = read_typed_json_section::<PluginExtensionRegistrySnapshot>(
        package,
        "plugin.extension_registry",
    )
    .map_err(|(code, message)| {
        Box::new(plugin_blocked(
            "vfs.prefix_registry",
            code,
            message,
            vec![evidence("section", "plugin.extension_registry")],
        ))
    })?;
    let policy = read_typed_json_section::<ProviderPolicy>(package, "provider.policy").map_err(
        |(code, message)| {
            Box::new(plugin_blocked(
                "vfs.prefix_registry",
                code,
                message,
                vec![evidence("section", "provider.policy")],
            ))
        },
    )?;
    let manifest = package
        .container()
        .decode_postcard::<PackageManifest>("package.manifest")
        .map_err(|error| {
            Box::new(plugin_blocked(
                "vfs.prefix_registry",
                "ASTRA_PACKAGE_MANIFEST_INVALID",
                error.to_string(),
                vec![evidence("section", "package.manifest")],
            ))
        })?;
    registry
        .validate_embedded_package(&policy, &manifest.package_id, &manifest.profile)
        .map_err(|diagnostic| {
            Box::new(plugin_blocked(
                "vfs.prefix_registry",
                diagnostic.code,
                diagnostic.message,
                vec![evidence("section", "plugin.extension_registry")],
            ))
        })?;
    let mut providers = BTreeMap::new();
    for binding in registry
        .bindings
        .iter()
        .filter(|binding| binding.slot == "vfs_provider")
    {
        let Some(provider) = registry.providers.iter().find(|provider| {
            provider.slot == binding.slot && provider.provider_id == binding.provider_id
        }) else {
            continue;
        };
        providers.insert(
            provider.provider_id.clone(),
            RegisteredVfsProvider {
                capability: provider.capability.clone(),
                packaged: provider.packaged,
            },
        );
    }
    Ok(providers)
}

pub(crate) fn vfs_capability_matches(actual: &str, required: &str) -> bool {
    actual == required
}

pub(crate) fn forbidden_vfs_report_field(package: &PackageReader, section: &str) -> Option<String> {
    let bytes = package
        .container()
        .read_bounded(section, 1024 * 1024)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    forbidden_vfs_field_path(&value)
}

pub(crate) fn forbidden_vfs_field_path(value: &serde_json::Value) -> Option<String> {
    fn walk(value: &serde_json::Value, path: &mut Vec<String>, found: &mut Option<String>) {
        if found.is_some() {
            return;
        }
        match value {
            serde_json::Value::Object(object) => {
                for (key, child) in object {
                    path.push(key.clone());
                    if is_forbidden_vfs_key(key, path, child) {
                        *found = Some(path.join("."));
                        return;
                    }
                    walk(child, path, found);
                    path.pop();
                }
            }
            serde_json::Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    path.push(index.to_string());
                    walk(child, path, found);
                    path.pop();
                }
            }
            serde_json::Value::String(value) if looks_like_host_absolute_path(value) => {
                *found = Some(path.join("."));
            }
            _ => {}
        }
    }

    let mut path = Vec::new();
    let mut found = None;
    walk(value, &mut path, &mut found);
    found
}

pub(crate) fn is_forbidden_vfs_key(key: &str, path: &[String], value: &serde_json::Value) -> bool {
    if key == "payload" {
        return !(path.len() == 2
            && path[0] == "redaction"
            && path[1] == "payload"
            && value.as_str() == Some("omitted"));
    }
    matches!(
        key,
        "root"
            | "host_root"
            | "local_root"
            | "absolute_path"
            | "text"
            | "script_text"
            | "source_text"
            | "content"
            | "payload_bytes"
            | "raw_payload"
            | "source_payload"
            | "commercial_text"
            | "bytecode"
            | "bytes"
    )
}

pub(crate) fn looks_like_host_absolute_path(value: &str) -> bool {
    let normalized = value.replace('\\', "/");
    normalized.starts_with('/')
        || normalized.starts_with("~/")
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':')
}

pub(crate) fn package_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Package,
        status: CheckStatus::Blocked,
        diagnostic: Some(Diagnostic::blocking(code, summary.clone())),
        summary,
        evidence: evidence_values,
    }
}

pub(crate) fn media_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Media,
        status: CheckStatus::Blocked,
        diagnostic: Some(Diagnostic::blocking(code, summary.clone())),
        summary,
        evidence: evidence_values,
    }
}

pub(super) trait ReleaseCheckRecordExt {
    fn with_id(self, id: &'static str) -> Self;
}

impl ReleaseCheckRecordExt for ReleaseCheckRecord {
    fn with_id(mut self, id: &'static str) -> Self {
        self.id = id.to_string();
        self
    }
}

pub(crate) fn read_json_section(
    package: &PackageReader,
    section: &str,
) -> Result<serde_json::Value, (&'static str, String)> {
    let bytes = package
        .container()
        .read_bounded(section, 256 * 1024)
        .map_err(|err| ("ASTRA_PLUGIN_SECTION_MISSING", err.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|err| ("ASTRA_PLUGIN_SECTION_JSON", err.to_string()))
}

pub(crate) fn read_typed_json_section<T: serde::de::DeserializeOwned>(
    package: &PackageReader,
    section: &str,
) -> Result<T, (&'static str, String)> {
    let bytes = package
        .container()
        .read_bounded(section, 256 * 1024)
        .map_err(|err| ("ASTRA_PLUGIN_SECTION_MISSING", err.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|err| {
        (
            "ASTRA_PLUGIN_SECTION_CONTRACT",
            format!("section {section} does not satisfy its typed contract: {err}"),
        )
    })
}

pub(crate) fn plugin_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Plugin,
        status: CheckStatus::Blocked,
        diagnostic: Some(Diagnostic::blocking(code, summary.clone())),
        summary,
        evidence: evidence_values,
    }
}

pub(crate) fn runtime_blocked(
    id: impl Into<String>,
    code: &'static str,
    summary: impl Into<String>,
    evidence_values: Vec<ReleaseEvidence>,
) -> ReleaseCheckRecord {
    let summary = summary.into();
    ReleaseCheckRecord {
        id: id.into(),
        domain: ReleaseDomain::Runtime,
        status: CheckStatus::Blocked,
        diagnostic: Some(Diagnostic::blocking(code, summary.clone())),
        summary,
        evidence: evidence_values,
    }
}
