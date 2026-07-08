use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::AssetError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, JsonSchema)]
pub struct VfsUri(String);

impl VfsUri {
    pub fn parse(value: &str) -> Result<Self, AssetError> {
        let normalized = value.replace('\\', "/");
        let Some((prefix, path)) = normalized.split_once(":/") else {
            return Err(AssetError::message("VfsUri must use provider:/path format"));
        };
        validate_vfs_prefix(prefix)?;
        let path = normalize_vfs_path(path)?;
        Ok(Self(format!("{prefix}:/{path}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn prefix(&self) -> &str {
        self.0
            .split_once(":/")
            .map(|(prefix, _)| prefix)
            .expect("validated VfsUri prefix")
    }

    pub fn path(&self) -> &str {
        self.0
            .split_once(":/")
            .map(|(_, path)| path)
            .expect("validated VfsUri path")
    }

    pub fn lookup_path(&self, policy: VfsCasePolicy) -> String {
        match policy {
            VfsCasePolicy::CaseSensitive => self.path().to_string(),
            VfsCasePolicy::CaseInsensitive | VfsCasePolicy::PreserveWithFoldedLookup => {
                self.path().to_ascii_lowercase()
            }
        }
    }
}

impl fmt::Display for VfsUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for VfsUri {
    type Err = AssetError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for VfsUri {
    type Error = AssetError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<VfsUri> for String {
    fn from(value: VfsUri) -> Self {
        value.0
    }
}

impl Serialize for VfsUri {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for VfsUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VfsCasePolicy {
    CaseSensitive,
    CaseInsensitive,
    PreserveWithFoldedLookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VfsBackendKind {
    Package,
    LocalAuthorized,
    Overlay,
    Memory,
    LegacyPack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VfsReadWriteMode {
    ReadOnly,
    WritableWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VfsPrefixDescriptor {
    pub prefix: String,
    pub provider_id: String,
    pub backend: VfsBackendKind,
    pub case_policy: VfsCasePolicy,
    pub mode: VfsReadWriteMode,
    pub redaction: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VfsLayerDescriptor {
    pub layer_id: String,
    pub prefix: String,
    pub priority: i32,
    pub source: VfsSourceRef,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VfsSourceRef {
    PackageSection { section_id: String },
    LocalAuthorized { alias: String },
    Overlay { base_layer_id: String },
    Memory { object_id: String },
    LegacyPack { pack_id: String, entry_id: String },
}

impl VfsSourceRef {
    pub fn package_section(section_id: impl Into<String>) -> Self {
        Self::PackageSection {
            section_id: section_id.into(),
        }
    }

    pub fn local_authorized(alias: impl Into<String>) -> Self {
        Self::LocalAuthorized {
            alias: alias.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VfsManifestEntry {
    #[serde(rename = "vfs_uri")]
    pub uri: VfsUri,
    pub layer_id: String,
    pub source: VfsSourceRef,
    pub offset: u64,
    pub size: u64,
    pub hash: Hash256,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    pub media_kind: String,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VfsWhiteoutEntry {
    #[serde(rename = "vfs_uri")]
    pub uri: VfsUri,
    pub layer_id: String,
    pub base_hash: Hash256,
    pub allowlist_id: String,
    pub reason: String,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VfsManifest {
    pub schema: String,
    #[serde(default)]
    pub prefixes: Vec<VfsPrefixDescriptor>,
    #[serde(default)]
    pub layers: Vec<VfsLayerDescriptor>,
    #[serde(default)]
    pub entries: Vec<VfsManifestEntry>,
    #[serde(default)]
    pub whiteouts: Vec<VfsWhiteoutEntry>,
}

impl VfsManifest {
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        if self.schema != "astra.asset_vfs_manifest.v1" {
            diagnostics.push(Diagnostic::blocking(
                "ASTRA_VFS_SCHEMA",
                "asset VFS manifest schema must be astra.asset_vfs_manifest.v1",
            ));
        }
        let mut prefixes = BTreeMap::new();
        for prefix in &self.prefixes {
            if let Err(err) = validate_vfs_prefix(&prefix.prefix) {
                diagnostics.push(
                    Diagnostic::blocking("ASTRA_VFS_PREFIX", err.to_string())
                        .with_field("prefix", &prefix.prefix),
                );
            }
            if prefix.provider_id.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VFS_PROVIDER_MISSING",
                        "VFS prefix provider missing",
                    )
                    .with_field("prefix", &prefix.prefix),
                );
            }
            prefixes.insert(prefix.prefix.as_str(), prefix);
        }
        let mut layer_priorities = BTreeMap::new();
        for layer in &self.layers {
            if !prefixes.contains_key(layer.prefix.as_str()) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VFS_LAYER_PREFIX",
                        "VFS layer prefix is not registered",
                    )
                    .with_field("prefix", &layer.prefix),
                );
            }
            layer_priorities.insert(layer.layer_id.as_str(), layer.priority);
        }
        for entry in &self.entries {
            if !prefixes.contains_key(entry.uri.prefix()) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VFS_ENTRY_PREFIX",
                        "VFS entry prefix is not registered",
                    )
                    .with_field("vfs_uri", entry.uri.as_str()),
                );
            }
            if !layer_priorities.contains_key(entry.layer_id.as_str()) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VFS_ENTRY_LAYER",
                        "VFS entry layer is not registered",
                    )
                    .with_field("layer_id", &entry.layer_id),
                );
            }
        }
        for whiteout in &self.whiteouts {
            if whiteout.allowlist_id.trim().is_empty() || whiteout.reason.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VFS_WHITEOUT_POLICY",
                        "VFS whiteout requires allowlist and reason",
                    )
                    .with_field("vfs_uri", whiteout.uri.as_str()),
                );
            }
            if !layer_priorities.contains_key(whiteout.layer_id.as_str()) {
                diagnostics.push(
                    Diagnostic::blocking(
                        "ASTRA_VFS_WHITEOUT_LAYER",
                        "VFS whiteout layer is not registered",
                    )
                    .with_field("layer_id", &whiteout.layer_id),
                );
            }
        }
        diagnostics
    }

    pub fn resolve(&self, uri: &VfsUri) -> Result<Option<&VfsManifestEntry>, Vec<Diagnostic>> {
        let diagnostics = self.validate();
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let priority_by_layer = self
            .layers
            .iter()
            .map(|layer| (layer.layer_id.as_str(), layer.priority))
            .collect::<BTreeMap<_, _>>();
        let whiteout_priority = self
            .whiteouts
            .iter()
            .filter(|whiteout| &whiteout.uri == uri)
            .filter_map(|whiteout| priority_by_layer.get(whiteout.layer_id.as_str()).copied())
            .max();
        let best = self
            .entries
            .iter()
            .filter(|entry| &entry.uri == uri)
            .max_by_key(|entry| {
                priority_by_layer
                    .get(entry.layer_id.as_str())
                    .copied()
                    .unwrap_or(i32::MIN)
            });
        if let (Some(entry), Some(whiteout_priority)) = (best, whiteout_priority) {
            let entry_priority = priority_by_layer
                .get(entry.layer_id.as_str())
                .copied()
                .unwrap_or(i32::MIN);
            if whiteout_priority >= entry_priority {
                return Ok(None);
            }
        }
        Ok(best)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssetCatalog {
    pub schema: String,
    #[serde(default)]
    pub assets: Vec<AssetCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssetCatalogEntry {
    pub asset_id: String,
    #[serde(rename = "vfs_uri")]
    pub uri: VfsUri,
    pub media_kind: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalMountRootSet {
    roots: BTreeMap<String, PathBuf>,
}

impl LocalMountRootSet {
    pub fn authorize(
        &mut self,
        prefix: impl Into<String>,
        root: impl AsRef<Path>,
    ) -> Result<(), AssetError> {
        let prefix = prefix.into();
        validate_vfs_prefix(&prefix)?;
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(AssetError::message("local VFS root must be a directory"));
        }
        let root = fs::canonicalize(root).map_err(|err| AssetError::message(err.to_string()))?;
        self.roots.insert(prefix, root);
        Ok(())
    }

    pub fn read_bounded(
        &self,
        uri: &VfsUri,
        max_len: usize,
        expected_hash: Option<Hash256>,
    ) -> Result<Vec<u8>, AssetError> {
        let root = self
            .roots
            .get(uri.prefix())
            .ok_or_else(|| AssetError::message("VFS local root is not authorized"))?;
        let relative = pathbuf_from_vfs_path(uri.path())?;
        let path = root.join(relative);
        let path = fs::canonicalize(path).map_err(|err| AssetError::message(err.to_string()))?;
        if !path.starts_with(root) {
            return Err(AssetError::message(
                "VFS local entry escapes authorized root",
            ));
        }
        if !path.is_file() {
            return Err(AssetError::message("VFS local entry is missing"));
        }
        let metadata = fs::metadata(&path).map_err(|err| AssetError::message(err.to_string()))?;
        if metadata.len() as usize > max_len {
            return Err(AssetError::message("VFS local entry exceeds read bound"));
        }
        let bytes = fs::read(path).map_err(|err| AssetError::message(err.to_string()))?;
        if let Some(expected_hash) = expected_hash {
            let actual = Hash256::from_sha256(&bytes);
            if actual != expected_hash {
                return Err(AssetError::message("VFS local entry hash mismatch"));
            }
        }
        Ok(bytes)
    }
}

fn validate_vfs_prefix(prefix: &str) -> Result<(), AssetError> {
    if prefix.is_empty()
        || !prefix
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
        || !prefix
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
    {
        return Err(AssetError::message("VFS prefix must be a safe symbol"));
    }
    Ok(())
}

fn normalize_vfs_path(path: &str) -> Result<String, AssetError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with("~/")
        || path.contains("://")
        || path
            .split('/')
            .next()
            .is_some_and(|part| part.ends_with(':'))
    {
        return Err(AssetError::message("VFS path must be provider-relative"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(AssetError::message("VFS path cannot escape provider root"));
        }
        if part.contains(':') || part.chars().any(|ch| ch.is_control()) {
            return Err(AssetError::message(
                "VFS path contains an invalid character",
            ));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(AssetError::message("VFS path cannot be empty"));
    }
    Ok(parts.join("/"))
}

fn pathbuf_from_vfs_path(path: &str) -> Result<PathBuf, AssetError> {
    let relative = PathBuf::from(path);
    for component in relative.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err(AssetError::message("VFS local path must stay inside root"));
            }
        }
    }
    Ok(relative)
}
