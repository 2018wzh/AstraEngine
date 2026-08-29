use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use astra_core::Hash256;

use crate::{LegacyCoreError, LegacyMountedVfs};

#[derive(Debug, Clone)]
pub struct LegacyOpaqueFamilyConfig {
    pub schema_id: String,
    pub schema_hash: Hash256,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LegacyVfsMountContext {
    pub game_root: PathBuf,
    pub profile_id: String,
    pub profile_hash: Hash256,
    pub mount_id: String,
    pub prefix: String,
    pub family_config: LegacyOpaqueFamilyConfig,
}

/// Reads one bounded, read-only family-private file below the game root.
///
/// This is intentionally a function rather than a provider trait: the family
/// owns the key format and calls the API during mount, while the core owns the
/// path and size boundary. The bytes never enter the public VFS manifest.
pub fn read_private_file(
    game_root: &Path,
    relative_path: &Path,
    max_bytes: u64,
) -> Result<Vec<u8>, LegacyCoreError> {
    if max_bytes == 0
        || max_bytes > usize::MAX as u64
        || relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_PATH",
            "private file path must be a normalized relative path",
        ));
    }
    let root = game_root.canonicalize().map_err(|_| {
        LegacyCoreError::invalid("ASTRA_EMU_VFS_GAME_ROOT", "game root could not be resolved")
    })?;
    let candidate = root.join(relative_path);
    let metadata = fs::symlink_metadata(&candidate).map_err(|_| {
        LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_FILE",
            "private file could not be opened",
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_FILE",
            "private file is not a regular file",
        ));
    }
    let resolved = candidate.canonicalize().map_err(|_| {
        LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_FILE",
            "private file could not be resolved",
        )
    })?;
    if !resolved.starts_with(&root) {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_PATH",
            "private file resolves outside the game root",
        ));
    }
    if metadata.len() > max_bytes {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_SIZE",
            "private file exceeds its configured byte bound",
        ));
    }
    let file = fs::File::open(resolved).map_err(|_| {
        LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_FILE",
            "private file could not be opened",
        )
    })?;
    let read_limit = max_bytes.checked_add(1).ok_or_else(|| {
        LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_SIZE",
            "private file read bound overflowed",
        )
    })?;
    let mut bounded = file.take(read_limit);
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    bounded.read_to_end(&mut bytes).map_err(|_| {
        LegacyCoreError::invalid("ASTRA_EMU_VFS_PRIVATE_FILE", "private file read failed")
    })?;
    if bytes.len() as u64 > max_bytes || bytes.len() as u64 != metadata.len() {
        return Err(LegacyCoreError::invalid(
            "ASTRA_EMU_VFS_PRIVATE_FILE",
            "private file changed or exceeded its bound during the read",
        ));
    }
    Ok(bytes)
}

pub trait LegacyVfsFamilyFactory: Send + Sync {
    fn family_id(&self) -> &str;
    fn family_options_schema_id(&self) -> &str;
    fn family_options_schema_hash(&self) -> Hash256;
    fn mount(
        &self,
        context: &LegacyVfsMountContext,
    ) -> Result<Arc<dyn LegacyMountedVfs>, LegacyCoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_file_reader_is_bounded_and_rejects_unsafe_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("key.toml"), b"bounded-private-data").unwrap();

        assert_eq!(
            read_private_file(root.path(), Path::new("key.toml"), 64).unwrap(),
            b"bounded-private-data"
        );
        for path in ["", "../key.toml", "nested/../key.toml"] {
            assert!(read_private_file(root.path(), Path::new(path), 64).is_err());
        }
        assert!(read_private_file(root.path(), Path::new("key.toml"), 4).is_err());
        assert!(read_private_file(root.path(), Path::new("key.toml"), 0).is_err());
    }

    #[test]
    fn private_file_reader_rejects_directories() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("key.toml")).unwrap();
        assert!(read_private_file(root.path(), Path::new("key.toml"), 64).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn private_file_reader_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.path().join("key.toml")).unwrap();
        assert!(read_private_file(root.path(), Path::new("key.toml"), 64).is_err());
    }
}
