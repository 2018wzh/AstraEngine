use super::*;
use crate::AssetError;
use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

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

pub(super) fn validate_vfs_prefix(prefix: &str) -> Result<(), AssetError> {
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

pub(super) fn normalize_vfs_path(path: &str) -> Result<String, AssetError> {
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
