#![cfg(not(target_arch = "wasm32"))]

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use astra_core::Hash256;
use astra_platform::{PackageCachePolicy, PlatformError, PlatformErrorCode};
use serde::{Deserialize, Serialize};

use crate::FilePackageSource;

const CACHE_INDEX_SCHEMA: &str = "astra.platform_verified_package_cache.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    bytes: u64,
    last_access: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheIndex {
    schema: String,
    clock: u64,
    entries: BTreeMap<String, CacheEntry>,
}

impl Default for CacheIndex {
    fn default() -> Self {
        Self {
            schema: CACHE_INDEX_SCHEMA.to_string(),
            clock: 0,
            entries: BTreeMap::new(),
        }
    }
}

pub struct VerifiedPackageCache {
    root: PathBuf,
    policy: PackageCachePolicy,
    index: CacheIndex,
}

impl VerifiedPackageCache {
    pub fn open(root: impl AsRef<Path>, policy: PackageCachePolicy) -> Result<Self, PlatformError> {
        validate_policy(&policy)?;
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|_| io_error("package.cache.open"))?;
        let index_path = root.join("index.json");
        let index = if index_path.exists() {
            let bytes = fs::read(&index_path).map_err(|_| io_error("package.cache.open"))?;
            let index: CacheIndex = serde_json::from_slice(&bytes).map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "package.cache.open",
                    "verified package cache index is invalid",
                )
            })?;
            if index.schema != CACHE_INDEX_SCHEMA {
                return Err(PlatformError::new(
                    PlatformErrorCode::IntegrityMismatch,
                    "package.cache.open",
                    "verified package cache index schema is unsupported",
                ));
            }
            index
        } else {
            CacheIndex::default()
        };
        Ok(Self {
            root,
            policy,
            index,
        })
    }

    pub fn platform_cache_root(app_id: &str) -> Result<PathBuf, PlatformError> {
        if app_id.is_empty()
            || !app_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidProfile,
                "package.cache.root",
                "package cache app id is unsafe",
            ));
        }
        let dirs =
            directories::ProjectDirs::from("com", "AstraEngine", app_id).ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::Io,
                    "package.cache.root",
                    "platform cache directory is unavailable",
                )
            })?;
        Ok(dirs.cache_dir().join("packages"))
    }

    pub fn store_verified(
        &mut self,
        expected_hash: &str,
        bytes: &[u8],
    ) -> Result<(), PlatformError> {
        let key = cache_key(expected_hash)?;
        let byte_len = u64::try_from(bytes.len()).map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.cache.store",
                "package byte length overflows",
            )
        })?;
        if byte_len > self.policy.max_entry_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.cache.store",
                "package exceeds cache entry limit",
            ));
        }
        if Hash256::from_sha256(bytes).to_string() != expected_hash {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.cache.store",
                "package source hash does not match declared identity",
            ));
        }
        self.evict_for(byte_len, Some(&key))?;
        let destination = self.path_for(&key);
        if !destination.exists() {
            let mut staging = tempfile::NamedTempFile::new_in(&self.root)
                .map_err(|_| io_error("package.cache.store"))?;
            staging
                .write_all(bytes)
                .map_err(|_| io_error("package.cache.store"))?;
            staging
                .as_file()
                .sync_all()
                .map_err(|_| io_error("package.cache.store"))?;
            match staging.persist_noclobber(&destination) {
                Ok(_) => {}
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(io_error("package.cache.store")),
            }
        }
        self.record_access(key, byte_len)?;
        Ok(())
    }

    pub fn open_source(&mut self, expected_hash: &str) -> Result<FilePackageSource, PlatformError> {
        let key = cache_key(expected_hash)?;
        let path = self.path_for(&key);
        let source = FilePackageSource::open(&path, expected_hash)?;
        self.record_access(key, source.len())?;
        Ok(source)
    }

    pub fn contains(&mut self, expected_hash: &str) -> Result<bool, PlatformError> {
        let key = cache_key(expected_hash)?;
        if !self.index.entries.contains_key(&key) || !self.path_for(&key).exists() {
            return Ok(false);
        }
        let source = FilePackageSource::open(self.path_for(&key), expected_hash)?;
        self.record_access(key, source.len())?;
        Ok(true)
    }

    pub fn entry_count(&self) -> usize {
        self.index.entries.len()
    }

    fn evict_for(&mut self, required: u64, keep: Option<&str>) -> Result<(), PlatformError> {
        while self.total_bytes().checked_add(required).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.cache.evict",
                "cache size overflows",
            )
        })? > self.policy.max_total_bytes
        {
            let candidate = self
                .index
                .entries
                .iter()
                .filter(|(key, _)| Some(key.as_str()) != keep)
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(key, _)| key.clone())
                .ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::InvalidState,
                        "package.cache.evict",
                        "cache limit cannot accommodate the verified package",
                    )
                })?;
            fs::remove_file(self.path_for(&candidate))
                .map_err(|_| io_error("package.cache.evict"))?;
            self.index.entries.remove(&candidate);
        }
        self.persist_index()
    }

    fn record_access(&mut self, key: String, bytes: u64) -> Result<(), PlatformError> {
        self.index.clock = self.index.clock.checked_add(1).ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.cache.index",
                "cache clock overflowed",
            )
        })?;
        self.index.entries.insert(
            key,
            CacheEntry {
                bytes,
                last_access: self.index.clock,
            },
        );
        self.persist_index()
    }

    fn total_bytes(&self) -> u64 {
        self.index.entries.values().map(|entry| entry.bytes).sum()
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.astrapkg"))
    }

    fn persist_index(&self) -> Result<(), PlatformError> {
        let bytes = serde_json::to_vec(&self.index).map_err(|_| io_error("package.cache.index"))?;
        let mut staging = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(|_| io_error("package.cache.index"))?;
        staging
            .write_all(&bytes)
            .map_err(|_| io_error("package.cache.index"))?;
        staging
            .as_file()
            .sync_all()
            .map_err(|_| io_error("package.cache.index"))?;
        staging
            .persist(self.root.join("index.json"))
            .map_err(|_| io_error("package.cache.index"))?;
        Ok(())
    }
}

fn validate_policy(policy: &PackageCachePolicy) -> Result<(), PlatformError> {
    if policy.max_entry_bytes == 0
        || policy.max_total_bytes == 0
        || policy.max_entry_bytes > policy.max_total_bytes
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "package.cache.open",
            "package cache policy is invalid",
        ));
    }
    Ok(())
}

fn cache_key(expected_hash: &str) -> Result<String, PlatformError> {
    let Some(key) = expected_hash.strip_prefix("sha256:") else {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.cache.key",
            "package hash is not sha256",
        ));
    };
    if key.len() != 64 || !key.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.cache.key",
            "package hash is invalid",
        ));
    }
    Ok(key.to_ascii_lowercase())
}

fn io_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Io,
        operation,
        "verified package cache I/O failed",
    )
}
