use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{CancellationToken, GrantedSourceEntry, GrantedSourceReader, SourceScanError};
use astra_emu_family_api::{LegacyProviderError, LegacyVfsReader};
use sha2::{Digest, Sha256};

const MAX_ENUMERATED_ENTRIES: usize = 100_001;

#[derive(Default)]
pub struct DesktopVfsRegistry {
    mounts: Mutex<BTreeMap<String, DesktopVfsMount>>,
}

struct DesktopVfsMount {
    root: PathBuf,
    files: BTreeMap<String, BoundFile>,
    overlays: BTreeMap<String, Arc<[u8]>>,
}

#[derive(Clone)]
struct BoundFile {
    relative: PathBuf,
    byte_size: u64,
    sha256: [u8; 32],
}

impl DesktopVfsRegistry {
    pub fn bind(&self, mount_set_id: &str, platform_token: &str) -> Result<(), String> {
        let root = fs::canonicalize(PathBuf::from(platform_token))
            .map_err(|_| "ASTRA_EMU_VFS_ROOT_INVALID")?;
        if !root.is_dir() {
            return Err("ASTRA_EMU_VFS_ROOT_INVALID".into());
        }
        let mut files = BTreeMap::new();
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            let entries = fs::read_dir(&directory).map_err(|_| "ASTRA_EMU_VFS_ENUMERATION")?;
            for entry in entries {
                let entry = entry.map_err(|_| "ASTRA_EMU_VFS_ENUMERATION")?;
                let kind = entry.file_type().map_err(|_| "ASTRA_EMU_VFS_ENUMERATION")?;
                if kind.is_symlink() {
                    return Err("ASTRA_EMU_VFS_SYMLINK_UNSUPPORTED".into());
                }
                if kind.is_dir() {
                    pending.push(entry.path());
                } else if kind.is_file() {
                    let relative = entry
                        .path()
                        .strip_prefix(&root)
                        .map_err(|_| "ASTRA_EMU_VFS_PATH_INVALID")?
                        .to_path_buf();
                    let key = relative_path_string(&relative)
                        .map_err(|_| "ASTRA_EMU_VFS_PATH_INVALID")?
                        .to_ascii_lowercase();
                    let path = entry.path();
                    let metadata = entry.metadata().map_err(|_| "ASTRA_EMU_VFS_METADATA")?;
                    let sha256 = hash_file(&path)?;
                    if files
                        .insert(
                            key,
                            BoundFile {
                                relative,
                                byte_size: metadata.len(),
                                sha256,
                            },
                        )
                        .is_some()
                    {
                        return Err("ASTRA_EMU_VFS_CASE_COLLISION".into());
                    }
                    if files.len() > MAX_ENUMERATED_ENTRIES {
                        return Err("ASTRA_EMU_VFS_ENTRY_BOUNDS".into());
                    }
                }
            }
        }
        let mut mounts = self
            .mounts
            .lock()
            .map_err(|_| "ASTRA_EMU_VFS_REGISTRY_LOCK")?;
        if mounts
            .insert(
                mount_set_id.to_owned(),
                DesktopVfsMount {
                    root,
                    files,
                    overlays: BTreeMap::new(),
                },
            )
            .is_some()
        {
            return Err("ASTRA_EMU_VFS_MOUNT_DUPLICATE".into());
        }
        Ok(())
    }

    pub fn install_overlays(
        &self,
        mount_set_id: &str,
        overlays: BTreeMap<String, Vec<u8>>,
    ) -> Result<(), String> {
        if overlays.len() > 4096 {
            return Err("ASTRA_EMU_VFS_OVERLAY_COUNT".into());
        }
        let mut normalized = BTreeMap::new();
        let mut total_bytes = 0_usize;
        for (path, bytes) in overlays {
            validate_relative_path(&path).map_err(|_| "ASTRA_EMU_VFS_OVERLAY_PATH")?;
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| "ASTRA_EMU_VFS_OVERLAY_BOUNDS".to_owned())?;
            if total_bytes > 64 * 1024 * 1024 {
                return Err("ASTRA_EMU_VFS_OVERLAY_BOUNDS".into());
            }
            if normalized
                .insert(path.to_ascii_lowercase(), Arc::from(bytes))
                .is_some()
            {
                return Err("ASTRA_EMU_VFS_OVERLAY_COLLISION".into());
            }
        }
        let mut mounts = self
            .mounts
            .lock()
            .map_err(|_| "ASTRA_EMU_VFS_REGISTRY_LOCK")?;
        let mount = mounts
            .get_mut(mount_set_id)
            .ok_or("ASTRA_EMU_VFS_MOUNT_MISSING")?;
        if !mount.overlays.is_empty() {
            return Err("ASTRA_EMU_VFS_OVERLAY_ALREADY_INSTALLED".into());
        }
        mount.overlays = normalized;
        Ok(())
    }

    pub fn unbind(&self, mount_set_id: &str) {
        if let Ok(mut mounts) = self.mounts.lock() {
            mounts.remove(mount_set_id);
        }
    }
}

impl LegacyVfsReader for DesktopVfsRegistry {
    fn read_file(
        &self,
        mount_set_id: &str,
        uri: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, LegacyProviderError> {
        validate_relative_path(uri).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_PATH_INVALID", "VFS URI is unsafe")
        })?;
        let mounts = self.mounts.lock().map_err(|_| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_REGISTRY_LOCK",
                "desktop VFS registry lock is poisoned",
            )
        })?;
        let mount = mounts.get(mount_set_id).ok_or_else(|| {
            LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MOUNT_MISSING",
                "desktop VFS mount is not active",
            )
        })?;
        if let Some(bytes) = mount.overlays.get(&uri.to_ascii_lowercase()) {
            if bytes.len() as u64 > max_bytes {
                return Err(LegacyProviderError::invalid(
                    "ASTRA_EMU_VFS_BOUNDS",
                    "VFS overlay exceeds the requested byte bound",
                ));
            }
            return Ok(bytes.to_vec());
        }
        let root = mount.root.clone();
        let bound = mount
            .files
            .get(&uri.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                LegacyProviderError::invalid("ASTRA_EMU_VFS_NOT_FOUND", "VFS entry is not present")
            })?;
        drop(mounts);
        let path = root.join(&bound.relative);
        let link_metadata = fs::symlink_metadata(&path).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_READ", "VFS metadata read failed")
        })?;
        if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MUTATED",
                "VFS entry type changed after the mount was bound",
            ));
        }
        let canonical = fs::canonicalize(&path).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_READ", "VFS path resolution failed")
        })?;
        if !canonical.starts_with(&root) || link_metadata.len() != bound.byte_size {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MUTATED",
                "VFS entry identity changed after the mount was bound",
            ));
        }
        if link_metadata.len() > max_bytes {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_BOUNDS",
                "VFS entry exceeds the requested byte bound",
            ));
        }
        let bytes = fs::read(canonical).map_err(|_| {
            LegacyProviderError::invalid("ASTRA_EMU_VFS_READ", "VFS entry read failed")
        })?;
        if bytes.len() as u64 != link_metadata.len() {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_CHANGED_DURING_READ",
                "VFS entry changed while it was being read",
            ));
        }
        let observed: [u8; 32] = Sha256::digest(&bytes).into();
        if observed != bound.sha256 {
            return Err(LegacyProviderError::invalid(
                "ASTRA_EMU_VFS_MUTATED",
                "VFS entry content changed after the mount was bound",
            ));
        }
        Ok(bytes)
    }
}

pub struct DesktopGrantedSource {
    root: PathBuf,
}

impl DesktopGrantedSource {
    pub fn new(platform_token: &str) -> Result<Self, SourceScanError> {
        let root = fs::canonicalize(PathBuf::from(platform_token))
            .map_err(|_| SourceScanError::Enumeration)?;
        if !root.is_dir() {
            return Err(SourceScanError::Enumeration);
        }
        Ok(Self { root })
    }

    fn resolved_file(&self, relative_path: &str) -> Result<PathBuf, SourceScanError> {
        validate_relative_path(relative_path)?;
        let candidate = self
            .root
            .join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let resolved = fs::canonicalize(candidate).map_err(|_| SourceScanError::Read)?;
        if !resolved.starts_with(&self.root) || !resolved.is_file() {
            return Err(SourceScanError::InvalidPath);
        }
        Ok(resolved)
    }
}

impl GrantedSourceReader for DesktopGrantedSource {
    fn enumerate(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<GrantedSourceEntry>, SourceScanError> {
        let mut pending = vec![self.root.clone()];
        let mut entries = Vec::new();
        while let Some(directory) = pending.pop() {
            if cancellation.is_cancelled() {
                return Err(SourceScanError::Cancelled);
            }
            let children = fs::read_dir(directory).map_err(|_| SourceScanError::Enumeration)?;
            for child in children {
                let child = child.map_err(|_| SourceScanError::Enumeration)?;
                let file_type = child
                    .file_type()
                    .map_err(|_| SourceScanError::Enumeration)?;
                if file_type.is_symlink() {
                    return Err(SourceScanError::InvalidPath);
                }
                let path = child.path();
                if file_type.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let metadata = child.metadata().map_err(|_| SourceScanError::Enumeration)?;
                let relative = path
                    .strip_prefix(&self.root)
                    .map_err(|_| SourceScanError::InvalidPath)?;
                let relative = relative_path_string(relative)?;
                let modified_ns = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
                    .unwrap_or(0);
                entries.push(GrantedSourceEntry {
                    relative_path: relative,
                    modified_ns,
                    byte_size: metadata.len(),
                    is_file: true,
                });
                if entries.len() >= MAX_ENUMERATED_ENTRIES {
                    return Err(SourceScanError::EntryBounds);
                }
            }
        }
        Ok(entries)
    }

    fn read_file(&self, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, SourceScanError> {
        let path = self.resolved_file(relative_path)?;
        let metadata = fs::metadata(&path).map_err(|_| SourceScanError::Read)?;
        if metadata.len() > max_bytes {
            return Err(SourceScanError::ScriptBounds);
        }
        let bytes = fs::read(path).map_err(|_| SourceScanError::Read)?;
        if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > max_bytes {
            return Err(SourceScanError::Read);
        }
        Ok(bytes)
    }
}

fn relative_path_string(path: &Path) -> Result<String, SourceScanError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(SourceScanError::InvalidPath);
        };
        let part = part.to_str().ok_or(SourceScanError::InvalidPath)?;
        if part.is_empty() || part == "." || part == ".." {
            return Err(SourceScanError::InvalidPath);
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(SourceScanError::InvalidPath);
    }
    Ok(parts.join("/"))
}

fn validate_relative_path(path: &str) -> Result<(), SourceScanError> {
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || path
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(SourceScanError::InvalidPath);
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<[u8; 32], String> {
    use std::io::Read;

    let mut file = fs::File::open(path).map_err(|_| "ASTRA_EMU_VFS_HASH_READ".to_owned())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "ASTRA_EMU_VFS_HASH_READ".to_owned())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_mount_rejects_same_size_content_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("SCRIPT.BIN");
        fs::write(&path, b"before").unwrap();
        let registry = DesktopVfsRegistry::default();
        registry
            .bind("mount.test", directory.path().to_str().unwrap())
            .unwrap();
        assert_eq!(
            registry
                .read_file("mount.test", "script.bin", 1024)
                .unwrap(),
            b"before"
        );
        fs::write(path, b"mutate").unwrap();
        let error = registry
            .read_file("mount.test", "script.bin", 1024)
            .unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_VFS_MUTATED");
    }

    #[test]
    fn trusted_overlay_is_mount_scoped_and_case_insensitive() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("script.bin"), b"base").unwrap();
        let registry = DesktopVfsRegistry::default();
        registry
            .bind("mount.test", directory.path().to_str().unwrap())
            .unwrap();
        registry
            .install_overlays(
                "mount.test",
                [("SCRIPT.BIN".into(), b"patched".to_vec())]
                    .into_iter()
                    .collect(),
            )
            .unwrap();
        assert_eq!(
            registry
                .read_file("mount.test", "script.bin", 1024)
                .unwrap(),
            b"patched"
        );
        registry.unbind("mount.test");
        assert!(registry
            .read_file("mount.test", "script.bin", 1024)
            .is_err());
    }
}
