use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

use astra_core::Hash256;
use lru::LruCache;
use thiserror::Error;

pub const DEFAULT_CACHE_LIMIT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const DEFAULT_CACHE_ENTRY_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIdentity {
    pub archive_hash: Hash256,
    pub entry_id: String,
    pub patch_hash: Hash256,
    pub decoder_id: String,
    pub codec_identity: String,
}

impl CacheIdentity {
    pub fn file_name(&self) -> String {
        let material = format!(
            "{}\0{}\0{}\0{}\0{}",
            self.archive_hash, self.entry_id, self.patch_hash, self.decoder_id, self.codec_identity
        );
        format!("{}.bin", Hash256::from_sha256(material.as_bytes()).to_hex())
    }
}

#[derive(Debug, Error)]
pub enum PlaintextCacheError {
    #[error("ASTRA_EMU_MINORI_CACHE_ENTRY_LIMIT: plaintext entry exceeds the configured limit")]
    EntryLimit,
    #[error("ASTRA_EMU_MINORI_CACHE_CORRUPT: plaintext cache entry metadata changed")]
    Corrupt,
    #[error("ASTRA_EMU_MINORI_CACHE_IO: cache operation failed")]
    Io(#[source] std::io::Error),
}

pub struct PlaintextCache {
    root: PathBuf,
    total_limit: u64,
    entry_limit: u64,
    state: Mutex<CacheState>,
}

struct CacheState {
    entries: LruCache<String, u64>,
    total: u64,
}

impl PlaintextCache {
    pub fn new(
        root: PathBuf,
        total_limit: u64,
        entry_limit: u64,
    ) -> Result<Self, PlaintextCacheError> {
        fs::create_dir_all(&root).map_err(PlaintextCacheError::Io)?;
        restrict_directory(&root).map_err(PlaintextCacheError::Io)?;
        let mut discovered = Vec::new();
        let mut names = HashSet::new();
        for entry in fs::read_dir(&root).map_err(PlaintextCacheError::Io)? {
            let entry = entry.map_err(PlaintextCacheError::Io)?;
            let metadata = entry.metadata().map_err(PlaintextCacheError::Io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if metadata.is_file() && valid_cache_name(&name) && names.insert(name.clone()) {
                discovered.push((
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    name,
                    metadata.len(),
                ));
            } else if metadata.is_file() && name.starts_with('.') && name.ends_with(".tmp") {
                fs::remove_file(entry.path()).map_err(PlaintextCacheError::Io)?;
            }
        }
        discovered.sort_by_key(|entry| entry.0);
        let mut entries = LruCache::unbounded();
        let mut total = 0u64;
        for (_, name, size) in discovered {
            total = total.saturating_add(size);
            entries.put(name, size);
        }
        let cache = Self {
            root,
            total_limit,
            entry_limit,
            state: Mutex::new(CacheState { entries, total }),
        };
        {
            let mut state = cache.state.lock().map_err(|_| {
                PlaintextCacheError::Io(std::io::Error::other("cache state poisoned"))
            })?;
            cache.evict_locked(&mut state, 0)?;
        }
        Ok(cache)
    }

    pub fn get(&self, identity: &CacheIdentity) -> Result<Option<Vec<u8>>, PlaintextCacheError> {
        let name = identity.file_name();
        let mut state = self
            .state
            .lock()
            .map_err(|_| PlaintextCacheError::Io(std::io::Error::other("cache state poisoned")))?;
        let Some(expected_size) = state.entries.get(&name).copied() else {
            return Ok(None);
        };
        let path = self.root.join(&name);
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(size) = state.entries.pop(&name) {
                    state.total = state.total.saturating_sub(size);
                }
                return Ok(None);
            }
            Err(error) => return Err(PlaintextCacheError::Io(error)),
        };
        let metadata = file.metadata().map_err(PlaintextCacheError::Io)?;
        if metadata.len() > self.entry_limit {
            return Err(PlaintextCacheError::EntryLimit);
        }
        if metadata.len() != expected_size {
            return Err(PlaintextCacheError::Corrupt);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)
            .map_err(PlaintextCacheError::Io)?;
        Ok(Some(bytes))
    }

    pub fn put(&self, identity: &CacheIdentity, bytes: &[u8]) -> Result<(), PlaintextCacheError> {
        if bytes.len() as u64 > self.entry_limit || bytes.len() as u64 > self.total_limit {
            return Err(PlaintextCacheError::EntryLimit);
        }
        let name = identity.file_name();
        let mut state = self
            .state
            .lock()
            .map_err(|_| PlaintextCacheError::Io(std::io::Error::other("cache state poisoned")))?;
        if state.entries.get(&name).is_some() {
            return Ok(());
        }
        self.evict_locked(&mut state, bytes.len() as u64)?;
        let destination = self.root.join(&name);
        let temporary = self.root.join(format!(".{name}.tmp"));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(PlaintextCacheError::Io)?;
        let write_result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &destination)
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result.map_err(PlaintextCacheError::Io)?;
        state.total = state.total.saturating_add(bytes.len() as u64);
        state.entries.put(name, bytes.len() as u64);
        Ok(())
    }

    fn evict_locked(
        &self,
        state: &mut CacheState,
        incoming: u64,
    ) -> Result<(), PlaintextCacheError> {
        while state.total.saturating_add(incoming) > self.total_limit {
            let Some((name, size)) = state.entries.pop_lru() else {
                return Err(PlaintextCacheError::EntryLimit);
            };
            match fs::remove_file(self.root.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(PlaintextCacheError::Io(error)),
            }
            state.total = state.total.saturating_sub(size);
        }
        Ok(())
    }
}

fn valid_cache_name(name: &str) -> bool {
    name.len() == 68
        && name.ends_with(".bin")
        && name[..64].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn restrict_directory(_path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_change_is_a_cache_miss() {
        let root = tempfile::tempdir().unwrap();
        let cache = PlaintextCache::new(root.path().to_path_buf(), 1024, 512).unwrap();
        let mut identity = CacheIdentity {
            archive_hash: Hash256::from_sha256(b"archive"),
            entry_id: "entry-1".into(),
            patch_hash: Hash256::from_sha256(b"patch-a"),
            decoder_id: "decoder".into(),
            codec_identity: "zlib".into(),
        };
        cache.put(&identity, b"plaintext").unwrap();
        assert_eq!(cache.get(&identity).unwrap().unwrap(), b"plaintext");
        identity.patch_hash = Hash256::from_sha256(b"patch-b");
        assert!(cache.get(&identity).unwrap().is_none());
    }

    #[test]
    fn lru_evicts_without_rescanning_the_directory() {
        let root = tempfile::tempdir().unwrap();
        let cache = PlaintextCache::new(root.path().to_path_buf(), 12, 8).unwrap();
        let identity = |entry_id: &str| CacheIdentity {
            archive_hash: Hash256::from_sha256(b"archive"),
            entry_id: entry_id.into(),
            patch_hash: Hash256::from_sha256(b"patch"),
            decoder_id: "decoder".into(),
            codec_identity: "raw".into(),
        };
        let first = identity("first");
        let second = identity("second");
        let third = identity("third");
        cache.put(&first, b"111111").unwrap();
        cache.put(&second, b"222222").unwrap();
        assert_eq!(cache.get(&first).unwrap().unwrap(), b"111111");
        cache.put(&third, b"333333").unwrap();
        assert!(cache.get(&second).unwrap().is_none());
        assert_eq!(cache.get(&first).unwrap().unwrap(), b"111111");
        assert_eq!(cache.get(&third).unwrap().unwrap(), b"333333");
    }

    #[test]
    fn externally_truncated_cache_entry_is_blocking() {
        let root = tempfile::tempdir().unwrap();
        let cache = PlaintextCache::new(root.path().to_path_buf(), 1024, 512).unwrap();
        let identity = CacheIdentity {
            archive_hash: Hash256::from_sha256(b"archive"),
            entry_id: "entry".into(),
            patch_hash: Hash256::from_sha256(b"patch"),
            decoder_id: "decoder".into(),
            codec_identity: "raw".into(),
        };
        cache.put(&identity, b"plaintext").unwrap();
        fs::write(root.path().join(identity.file_name()), b"short").unwrap();
        assert!(matches!(
            cache.get(&identity),
            Err(PlaintextCacheError::Corrupt)
        ));
    }
}
