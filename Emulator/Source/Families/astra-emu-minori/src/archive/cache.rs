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
const CACHE_MAGIC: &[u8; 8] = b"ASTRAC01";
const CACHE_HEADER_BYTES: usize = 8 + 8 + 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIdentity {
    pub family_id: String,
    pub source_hash: Hash256,
    pub entry_id: String,
    pub private_profile_hash: Hash256,
    pub decrypt_provider_id: String,
    pub descriptor_schema_hash: Hash256,
    pub codec_identity: String,
}

impl CacheIdentity {
    pub fn file_name(&self) -> String {
        let material = format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}",
            self.family_id,
            self.source_hash,
            self.entry_id,
            self.private_profile_hash,
            self.decrypt_provider_id,
            self.descriptor_schema_hash,
            self.codec_identity
        );
        format!("{}.bin", Hash256::from_sha256(material.as_bytes()).to_hex())
    }
}

#[derive(Debug, Error)]
pub enum PlaintextCacheError {
    #[error("ASTRA_EMU_CACHE_ENTRY_LIMIT: plaintext entry exceeds the configured limit")]
    EntryLimit,
    #[error("ASTRA_EMU_CACHE_CORRUPT: plaintext cache entry metadata changed")]
    Corrupt,
    #[error("ASTRA_EMU_CACHE_PERMISSION: cache privacy permission could not be enforced")]
    Permission(#[source] std::io::Error),
    #[error("ASTRA_EMU_CACHE_IO: cache operation failed")]
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
        if total_limit == 0 || entry_limit == 0 || entry_limit > total_limit {
            return Err(PlaintextCacheError::EntryLimit);
        }
        fs::create_dir_all(&root).map_err(PlaintextCacheError::Io)?;
        restrict_directory(&root).map_err(PlaintextCacheError::Permission)?;
        let mut discovered = Vec::new();
        let mut names = HashSet::new();
        for entry in fs::read_dir(&root).map_err(PlaintextCacheError::Io)? {
            let entry = entry.map_err(PlaintextCacheError::Io)?;
            let metadata = entry.metadata().map_err(PlaintextCacheError::Io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if metadata.is_file() && valid_cache_name(&name) && names.insert(name.clone()) {
                restrict_file(&entry.path()).map_err(PlaintextCacheError::Permission)?;
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
            total = total
                .checked_add(size)
                .ok_or(PlaintextCacheError::EntryLimit)?;
            entries.put(name, size);
        }
        let cache = Self {
            root,
            total_limit,
            entry_limit,
            state: Mutex::new(CacheState { entries, total }),
        };
        cache.with_state(|state| cache.evict_locked(state, 0))?;
        Ok(cache)
    }

    pub fn get(&self, identity: &CacheIdentity) -> Result<Option<Vec<u8>>, PlaintextCacheError> {
        let name = identity.file_name();
        self.with_state(|state| {
            let Some(expected_size) = state.entries.get(&name).copied() else {
                return Ok(None);
            };
            let path = self.root.join(&name);
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if let Some(size) = state.entries.pop(&name) {
                        state.total = state.total.saturating_sub(size);
                    }
                    return Ok(None);
                }
                Err(error) => return Err(PlaintextCacheError::Io(error)),
            };
            restrict_file(&path).map_err(PlaintextCacheError::Permission)?;
            let metadata = file.metadata().map_err(PlaintextCacheError::Io)?;
            if metadata.len() > self.entry_limit.saturating_add(CACHE_HEADER_BYTES as u64) {
                return Err(PlaintextCacheError::EntryLimit);
            }
            if metadata.len() != expected_size {
                return Err(PlaintextCacheError::Corrupt);
            }
            let mut stored = Vec::with_capacity(metadata.len() as usize);
            file.read_to_end(&mut stored)
                .map_err(PlaintextCacheError::Io)?;
            if stored.len() < CACHE_HEADER_BYTES || &stored[..8] != CACHE_MAGIC {
                return Err(PlaintextCacheError::Corrupt);
            }
            let declared = u64::from_le_bytes(
                stored[8..16]
                    .try_into()
                    .map_err(|_| PlaintextCacheError::Corrupt)?,
            );
            let payload = &stored[CACHE_HEADER_BYTES..];
            if declared != payload.len() as u64
                || declared > self.entry_limit
                || &stored[16..48] != Hash256::from_sha256(payload).as_bytes()
            {
                return Err(PlaintextCacheError::Corrupt);
            }
            Ok(Some(payload.to_vec()))
        })
    }

    pub fn put(&self, identity: &CacheIdentity, bytes: &[u8]) -> Result<(), PlaintextCacheError> {
        let payload_size =
            u64::try_from(bytes.len()).map_err(|_| PlaintextCacheError::EntryLimit)?;
        let size = payload_size
            .checked_add(CACHE_HEADER_BYTES as u64)
            .ok_or(PlaintextCacheError::EntryLimit)?;
        if payload_size > self.entry_limit || size > self.total_limit {
            return Err(PlaintextCacheError::EntryLimit);
        }
        let name = identity.file_name();
        self.with_state(|state| {
            if state.entries.get(&name).is_some() {
                return Ok(());
            }
            self.evict_locked(state, size)?;
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
            restrict_file(&temporary).map_err(PlaintextCacheError::Permission)?;
            let result = (|| {
                file.write_all(CACHE_MAGIC)?;
                file.write_all(&payload_size.to_le_bytes())?;
                file.write_all(Hash256::from_sha256(bytes).as_bytes())?;
                file.write_all(bytes)?;
                file.sync_all()?;
                drop(file);
                fs::rename(&temporary, &destination)?;
                restrict_file(&destination)
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            result.map_err(PlaintextCacheError::Io)?;
            state.total = state
                .total
                .checked_add(size)
                .ok_or(PlaintextCacheError::EntryLimit)?;
            state.entries.put(name, size);
            Ok(())
        })
    }

    fn with_state<T>(
        &self,
        operation: impl FnOnce(&mut CacheState) -> Result<T, PlaintextCacheError>,
    ) -> Result<T, PlaintextCacheError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PlaintextCacheError::Io(std::io::Error::other("cache state poisoned")))?;
        operation(&mut state)
    }

    fn evict_locked(
        &self,
        state: &mut CacheState,
        incoming: u64,
    ) -> Result<(), PlaintextCacheError> {
        while state
            .total
            .checked_add(incoming)
            .ok_or(PlaintextCacheError::EntryLimit)?
            > self.total_limit
        {
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

fn restrict_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        restrict_windows_path(path, true)?;
    }
    Ok(())
}

fn restrict_file(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    {
        restrict_windows_path(path, false)?;
    }
    Ok(())
}

pub fn enforce_private_file_permissions(path: &Path) -> Result<(), PlaintextCacheError> {
    restrict_file(path).map_err(PlaintextCacheError::Permission)
}

pub fn enforce_private_directory_permissions(path: &Path) -> Result<(), PlaintextCacheError> {
    restrict_directory(path).map_err(PlaintextCacheError::Permission)
}

#[cfg(windows)]
fn restrict_windows_path(path: &Path, _directory: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                },
                SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR,
            },
        },
    };

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Protected DACL, full access only for the file owner. New cache objects are
    // created by the current process, so OW resolves to the current user.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)".encode_utf16().chain(Some(0)).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        let result = SetFileSecurityW(
            PCWSTR(path.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
        .ok()
        .map_err(|error| std::io::Error::other(error.to_string()));
        let released = LocalFree(Some(HLOCAL(descriptor.0)));
        if !released.is_invalid() {
            return Err(std::io::Error::other("security descriptor release failed"));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(entry: &str) -> CacheIdentity {
        CacheIdentity {
            family_id: "fixture".into(),
            source_hash: Hash256::from_sha256(b"source"),
            entry_id: entry.into(),
            private_profile_hash: Hash256::from_sha256(b"private"),
            decrypt_provider_id: "fixture.decrypt.v1".into(),
            descriptor_schema_hash: Hash256::from_sha256(b"descriptor"),
            codec_identity: "raw".into(),
        }
    }

    #[test]
    fn identity_change_is_a_miss_and_corruption_is_blocking() {
        let root = tempfile::tempdir().unwrap();
        let cache = PlaintextCache::new(root.path().to_path_buf(), 4096, 1024).unwrap();
        let first = identity("one");
        cache.put(&first, b"plaintext").unwrap();
        assert_eq!(cache.get(&first).unwrap().unwrap(), b"plaintext");
        let mut changed = first.clone();
        changed.codec_identity = "zlib".into();
        assert!(cache.get(&changed).unwrap().is_none());
        std::fs::write(
            root.path().join(first.file_name()),
            vec![0; CACHE_HEADER_BYTES + 9],
        )
        .unwrap();
        assert!(matches!(
            cache.get(&first),
            Err(PlaintextCacheError::Corrupt)
        ));
    }

    #[test]
    fn lru_evicts_the_oldest_entry() {
        let root = tempfile::tempdir().unwrap();
        let per_file = CACHE_HEADER_BYTES as u64 + 4;
        let cache = PlaintextCache::new(root.path().to_path_buf(), per_file * 2, 16).unwrap();
        let one = identity("one");
        let two = identity("two");
        let three = identity("three");
        cache.put(&one, b"1111").unwrap();
        cache.put(&two, b"2222").unwrap();
        cache.get(&one).unwrap();
        cache.put(&three, b"3333").unwrap();
        assert!(cache.get(&two).unwrap().is_none());
        assert!(cache.get(&one).unwrap().is_some());
    }
}
