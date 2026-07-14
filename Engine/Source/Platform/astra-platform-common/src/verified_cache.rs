#![cfg(not(target_arch = "wasm32"))]

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use astra_platform::{PackageCachePolicy, PlatformError, PlatformErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

/// A single verified-cache write.  The staging file is deleted on every error
/// path, so callers can stream untrusted transport bytes without ever exposing
/// them through a package handle.
pub struct VerifiedPackageStaging<'a> {
    cache: &'a mut VerifiedPackageCache,
    expected_hash: String,
    key: String,
    staging: tempfile::NamedTempFile,
    hasher: Sha256,
    written: u64,
}

/// A verified cache entry that keeps an in-process lease for its complete host
/// lifetime.  Eviction observes that lease, so a live package handle cannot be
/// unlinked by an unrelated cache write.
pub struct CachedPackageSource {
    source: FilePackageSource,
    root: PathBuf,
    key: String,
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
        let mut staging = self.begin_staging(expected_hash)?;
        staging.write(bytes)?;
        staging.commit()
    }

    pub fn begin_staging(
        &mut self,
        expected_hash: &str,
    ) -> Result<VerifiedPackageStaging<'_>, PlatformError> {
        let key = cache_key(expected_hash)?;
        let staging = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(|_| io_error("package.cache.stage"))?;
        Ok(VerifiedPackageStaging {
            cache: self,
            expected_hash: expected_hash.to_string(),
            key,
            staging,
            hasher: Sha256::new(),
            written: 0,
        })
    }

    pub fn open_source(
        &mut self,
        expected_hash: &str,
    ) -> Result<CachedPackageSource, PlatformError> {
        let key = cache_key(expected_hash)?;
        let path = self.path_for(&key);
        let source = FilePackageSource::open(&path, expected_hash)?;
        self.record_access(key.clone(), source.len())?;
        acquire_lease(&self.root, &key)?;
        Ok(CachedPackageSource {
            source,
            root: self.root.clone(),
            key,
        })
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

    pub fn max_entry_bytes(&self) -> u64 {
        self.policy.max_entry_bytes
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
                .filter(|(key, _)| Some(key.as_str()) != keep && !is_leased(&self.root, key))
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

impl CachedPackageSource {
    pub fn len(&self) -> u64 {
        self.source.len()
    }

    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }

    pub fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        self.source.read_range(offset, length)
    }
}

impl Drop for CachedPackageSource {
    fn drop(&mut self) {
        if let Ok(mut leases) = lease_table().lock() {
            let lease_key = LeaseKey::new(&self.root, &self.key);
            if let Some(count) = leases.get_mut(&lease_key) {
                *count -= 1;
                if *count == 0 {
                    leases.remove(&lease_key);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct LeaseKey {
    root: PathBuf,
    key: String,
}

impl LeaseKey {
    fn new(root: &Path, key: &str) -> Self {
        Self {
            root: root.to_path_buf(),
            key: key.to_string(),
        }
    }
}

fn lease_table() -> &'static Mutex<BTreeMap<LeaseKey, usize>> {
    static TABLE: OnceLock<Mutex<BTreeMap<LeaseKey, usize>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn acquire_lease(root: &Path, key: &str) -> Result<(), PlatformError> {
    let mut leases = lease_table().lock().map_err(|_| {
        PlatformError::new(
            PlatformErrorCode::InvalidState,
            "package.cache.lease",
            "verified package cache lease table is unavailable",
        )
    })?;
    let count = leases.entry(LeaseKey::new(root, key)).or_default();
    *count = count.checked_add(1).ok_or_else(|| {
        PlatformError::new(
            PlatformErrorCode::InvalidState,
            "package.cache.lease",
            "verified package cache lease counter overflowed",
        )
    })?;
    Ok(())
}

fn is_leased(root: &Path, key: &str) -> bool {
    lease_table()
        .lock()
        .map(|leases| leases.contains_key(&LeaseKey::new(root, key)))
        .unwrap_or(true)
}

impl VerifiedPackageStaging<'_> {
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PlatformError> {
        let next = self
            .written
            .checked_add(u64::try_from(bytes.len()).map_err(|_| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "package.cache.stage",
                    "package byte length overflows",
                )
            })?)
            .ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "package.cache.stage",
                    "package byte length overflows",
                )
            })?;
        if next > self.cache.policy.max_entry_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.cache.stage",
                "package exceeds cache entry limit",
            ));
        }
        self.staging
            .write_all(bytes)
            .map_err(|_| io_error("package.cache.stage"))?;
        self.hasher.update(bytes);
        self.written = next;
        Ok(())
    }

    pub fn commit(self) -> Result<(), PlatformError> {
        let actual_hash = format!("sha256:{:x}", self.hasher.finalize());
        if actual_hash != self.expected_hash {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.cache.commit",
                "package source hash does not match declared identity",
            ));
        }
        self.cache.evict_for(self.written, Some(&self.key))?;
        let destination = self.cache.path_for(&self.key);
        if destination.exists() {
            // A concurrent or previous request may already have populated this
            // content-addressed entry.  Verify it again before treating it as a
            // cache hit; an orphaned/corrupt file must never become readable.
            FilePackageSource::open(&destination, &self.expected_hash)?;
        } else {
            self.staging
                .as_file()
                .sync_all()
                .map_err(|_| io_error("package.cache.commit"))?;
            match self.staging.persist_noclobber(&destination) {
                Ok(_) => {}
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(io_error("package.cache.commit")),
            }
        }
        self.cache.record_access(self.key.clone(), self.written)
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
