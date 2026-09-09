use rfvp::host_api::{RfvpError, RfvpFile, RfvpFileInfo, RfvpFileKind, RfvpFileSystem, RfvpResult};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

pub(crate) struct NativeFileSystem {
    root: PathBuf,
    root_canonical: PathBuf,
    temp_counter: u64,
}

impl NativeFileSystem {
    pub(crate) fn new(root: &str) -> RfvpResult<Self> {
        let root = PathBuf::from(root);
        if !root.is_dir() {
            return Err(RfvpError::NotFound);
        }
        let root_canonical = fs::canonicalize(&root).map_err(|_| RfvpError::Io)?;
        Ok(Self {
            root,
            root_canonical,
            temp_counter: 0,
        })
    }
    fn relative(path: &str) -> RfvpResult<PathBuf> {
        if path.is_empty() || path.contains('\0') || path.contains('\\') {
            return Err(RfvpError::InvalidArgument);
        }
        let candidate = Path::new(path);
        if candidate.is_absolute() {
            return Err(RfvpError::InvalidArgument);
        }
        let mut clean = PathBuf::new();
        for component in candidate.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => clean.push(part),
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(RfvpError::InvalidArgument)
                }
            }
        }
        if clean.as_os_str().is_empty() {
            return Ok(PathBuf::from("."));
        }
        Ok(clean)
    }
    fn resolve_existing(&self, path: &str) -> RfvpResult<PathBuf> {
        let joined = self.root.join(Self::relative(path)?);
        let canonical = fs::canonicalize(joined).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RfvpError::NotFound
            } else {
                RfvpError::Io
            }
        })?;
        if !canonical.starts_with(&self.root_canonical) {
            return Err(RfvpError::InvalidArgument);
        }
        Ok(canonical)
    }
    fn resolve_existing_lexical(&self, path: &str) -> RfvpResult<PathBuf> {
        let relative = Self::relative(path)?;
        let joined = self.root.join(relative);
        let canonical = fs::canonicalize(&joined).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RfvpError::NotFound
            } else {
                RfvpError::Io
            }
        })?;
        if !canonical.starts_with(&self.root_canonical) {
            return Err(RfvpError::InvalidArgument);
        }
        let parent = joined.parent().ok_or(RfvpError::InvalidArgument)?;
        let canonical_parent = fs::canonicalize(parent).map_err(|_| RfvpError::Io)?;
        if !canonical_parent.starts_with(&self.root_canonical) {
            return Err(RfvpError::InvalidArgument);
        }
        Ok(joined)
    }
    fn resolve_for_write(&self, path: &str) -> RfvpResult<PathBuf> {
        let relative = Self::relative(path)?;
        let joined = self.root.join(&relative);
        let parent = joined.parent().ok_or(RfvpError::InvalidArgument)?;
        // Save slots may be nested and are created on first use. Walk one
        // component at a time so an existing symlink is checked before any
        // missing child is created; `create_dir_all` could otherwise create
        // directories through a symlink outside the writable root.
        let mut current = self.root.clone();
        if let Some(relative_parent) = relative.parent() {
            for component in relative_parent.components() {
                let Component::Normal(part) = component else {
                    continue;
                };
                current.push(part);
                match fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.is_dir() => {}
                    Ok(_) => return Err(RfvpError::InvalidArgument),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        match fs::create_dir(&current) {
                            Ok(()) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                            Err(_) => return Err(RfvpError::Io),
                        }
                    }
                    Err(_) => return Err(RfvpError::Io),
                }
                let canonical = fs::canonicalize(&current).map_err(|_| RfvpError::Io)?;
                if !canonical.starts_with(&self.root_canonical) {
                    return Err(RfvpError::InvalidArgument);
                }
            }
        }
        let canonical_parent = fs::canonicalize(parent).map_err(|_| RfvpError::Io)?;
        if !canonical_parent.starts_with(&self.root_canonical) {
            return Err(RfvpError::InvalidArgument);
        }

        // `fs::copy` follows a destination symlink and an atomic rename may
        // otherwise publish through a path whose final component escaped the
        // writable root. Resolve an existing final component as well as its
        // parent before returning the path to a mutating operation.
        match fs::symlink_metadata(&joined) {
            Ok(_) => {
                let canonical_target =
                    fs::canonicalize(&joined).map_err(|_| RfvpError::InvalidArgument)?;
                if !canonical_target.starts_with(&self.root_canonical) {
                    return Err(RfvpError::InvalidArgument);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(RfvpError::Io),
        }
        Ok(joined)
    }

    fn next_temp_path(&mut self, target: &Path) -> RfvpResult<PathBuf> {
        let file_name = target
            .file_name()
            .ok_or(RfvpError::InvalidArgument)?
            .to_os_string();
        let pid = std::process::id();
        for _ in 0..64 {
            let counter = self.temp_counter;
            self.temp_counter = self.temp_counter.checked_add(1).ok_or(RfvpError::Io)?;
            let mut temp_name = file_name.clone();
            temp_name.push(format!(".astra-tmp-{pid}-{counter}"));
            let path = target.with_file_name(temp_name);
            if !path.exists() {
                return Ok(path);
            }
        }
        Err(RfvpError::Io)
    }
    fn info(path: &Path) -> RfvpResult<RfvpFileInfo> {
        let metadata = fs::metadata(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RfvpError::NotFound
            } else {
                RfvpError::Io
            }
        })?;
        let kind = if metadata.is_file() {
            RfvpFileKind::File
        } else if metadata.is_dir() {
            RfvpFileKind::Directory
        } else {
            RfvpFileKind::Other
        };
        Ok(RfvpFileInfo {
            len: metadata.len(),
            kind,
        })
    }
    fn walk(
        &self,
        relative: &Path,
        extension: Option<&str>,
        visitor: &mut dyn FnMut(&str, RfvpFileInfo) -> RfvpResult<()>,
    ) -> RfvpResult<()> {
        let relative_text = relative.to_str().ok_or(RfvpError::InvalidArgument)?;
        let directory = self.resolve_existing(relative_text)?;
        let mut entries = fs::read_dir(&directory)
            .map_err(|_| RfvpError::Io)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RfvpError::Io)?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let child_relative = relative.join(entry.file_name());
            let child_text = child_relative
                .to_str()
                .ok_or(RfvpError::InvalidArgument)?
                .replace(std::path::MAIN_SEPARATOR, "/");
            let child = self.resolve_existing(&child_text)?;
            let info = Self::info(&child)?;
            if info.kind == RfvpFileKind::Directory {
                self.walk(&child_relative, extension, visitor)?;
            } else if info.kind == RfvpFileKind::File
                && extension.is_none_or(|ext| {
                    child_relative.extension().and_then(|v| v.to_str()) == Some(ext)
                })
            {
                visitor(&child_text, info)?;
            }
        }
        Ok(())
    }
}

pub(crate) struct NativeFile {
    file: fs::File,
    length: u64,
}
impl RfvpFile for NativeFile {
    fn len(&mut self) -> RfvpResult<u64> {
        Ok(self.length)
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RfvpResult<usize> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| RfvpError::Io)?;
        self.file.read(buf).map_err(|_| RfvpError::Io)
    }
}

impl RfvpFileSystem for NativeFileSystem {
    type File = NativeFile;
    fn open(&mut self, path: &str) -> RfvpResult<Self::File> {
        let resolved = self.resolve_existing(path)?;
        let file = fs::File::open(&resolved).map_err(|_| RfvpError::Io)?;
        let length = file.metadata().map_err(|_| RfvpError::Io)?.len();
        Ok(NativeFile { file, length })
    }
    fn metadata(&mut self, path: &str) -> RfvpResult<RfvpFileInfo> {
        Self::info(&self.resolve_existing(path)?)
    }
    fn write_all(&mut self, path: &str, bytes: &[u8]) -> RfvpResult<()> {
        let target = self.resolve_for_write(path)?;
        let tmp = self.next_temp_path(&target)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|_| RfvpError::Io)?;
        let result = (|| {
            file.write_all(bytes).map_err(|_| RfvpError::Io)?;
            file.sync_all().map_err(|_| RfvpError::Io)?;
            drop(file);
            atomic_replace(&tmp, &target).map_err(|_| RfvpError::Io)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
    fn remove(&mut self, path: &str) -> RfvpResult<()> {
        // Use the validated lexical path so removing an in-root symlink
        // removes the link itself instead of deleting its target. The target
        // was still canonicalized above, so a link escaping the game root is
        // rejected before any mutation.
        fs::remove_file(self.resolve_existing_lexical(path)?).map_err(|_| RfvpError::Io)
    }
    fn copy(&mut self, source: &str, destination: &str) -> RfvpResult<()> {
        let source = self.resolve_existing(source)?;
        let destination = self.resolve_for_write(destination)?;
        let temporary = self.next_temp_path(&destination)?;
        let result = (|| {
            fs::copy(&source, &temporary).map_err(|_| RfvpError::Io)?;
            let file = fs::OpenOptions::new()
                .read(true)
                .open(&temporary)
                .map_err(|_| RfvpError::Io)?;
            file.sync_all().map_err(|_| RfvpError::Io)?;
            atomic_replace(&temporary, &destination).map_err(|_| RfvpError::Io)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
    fn list(
        &mut self,
        root: &str,
        visitor: &mut dyn FnMut(&str, RfvpFileInfo) -> RfvpResult<()>,
    ) -> RfvpResult<()> {
        let relative = Self::relative(root)?;
        match self.resolve_existing(root) {
            Ok(_) => self.walk(&relative, None, visitor),
            // A fresh game has no save directory yet. Treat that save root as
            // an empty collection so SaveData(RefreshAll) is harmless before
            // the first save is written; other missing roots remain errors.
            Err(RfvpError::NotFound) if relative == Path::new("save") => Ok(()),
            Err(error) => Err(error),
        }
    }
    fn enumerate_by_extension(
        &mut self,
        root: &str,
        extension: &str,
        visitor: &mut dyn FnMut(&str, RfvpFileInfo) -> RfvpResult<()>,
    ) -> RfvpResult<()> {
        self.walk(
            Path::new(root),
            Some(extension.trim_start_matches('.')),
            visitor,
        )
    }
}

#[cfg(not(windows))]
fn atomic_replace(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn atomic_replace(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::GetLastError,
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
    };

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    // SAFETY: both paths are valid, NUL-terminated UTF-16 buffers for the
    // duration of the call.
    let result = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        // SAFETY: GetLastError has no preconditions.
        return Err(std::io::Error::from_raw_os_error(
            unsafe { GetLastError() } as i32
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_all_creates_nested_save_directories() {
        let root = tempfile::tempdir().expect("temporary game root");
        let mut fs = NativeFileSystem::new(root.path().to_str().expect("UTF-8 test path"))
            .expect("native VFS opens the game root");

        fs.write_all("save/slot-1/state.bin", b"state")
            .expect("nested save path is writable");

        assert_eq!(
            std::fs::read(root.path().join("save/slot-1/state.bin")).expect("save exists"),
            b"state"
        );
    }

    #[test]
    fn failed_replace_preserves_existing_destination() {
        let root = tempfile::tempdir().expect("temporary game root");
        let target = root.path().join("save.bin");
        std::fs::create_dir(&target).expect("create a replacement blocker");
        let mut fs = NativeFileSystem::new(root.path().to_str().expect("UTF-8 test path"))
            .expect("native VFS opens the game root");

        assert!(fs.write_all("save.bin", b"new state").is_err());
        assert!(target.is_dir(), "a failed replace must keep the old target");
    }

    #[test]
    fn copy_uses_atomic_replace_and_preserves_a_blocking_destination() {
        let root = tempfile::tempdir().expect("temporary game root");
        let source = root.path().join("source.bin");
        let destination = root.path().join("save.bin");
        std::fs::write(&source, b"source").expect("source exists");
        std::fs::create_dir(&destination).expect("create a replacement blocker");
        let mut fs = NativeFileSystem::new(root.path().to_str().expect("UTF-8 test path"))
            .expect("native VFS opens the game root");

        assert!(fs.copy("source.bin", "save.bin").is_err());
        assert!(
            destination.is_dir(),
            "a failed copy must keep the old target"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remove_deletes_an_in_root_symlink_without_deleting_its_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary game root");
        let target = root.path().join("target.bin");
        let link = root.path().join("link.bin");
        std::fs::write(&target, b"target").expect("target exists");
        symlink(&target, &link).expect("in-root symlink created");
        let mut fs = NativeFileSystem::new(root.path().to_str().expect("UTF-8 test path"))
            .expect("native VFS opens the game root");

        fs.remove("link.bin").expect("in-root link is removable");
        assert!(!link.exists());
        assert_eq!(std::fs::read(target).expect("target remains"), b"target");
    }

    #[test]
    fn listing_a_fresh_save_root_is_empty() {
        let root = tempfile::tempdir().expect("temporary game root");
        let mut fs = NativeFileSystem::new(root.path().to_str().expect("UTF-8 test path"))
            .expect("native VFS opens the game root");
        let mut entries = Vec::new();

        fs.list("save", &mut |path, _| {
            entries.push(path.to_owned());
            Ok(())
        })
        .expect("missing save root is an empty listing");
        assert!(entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn copy_rejects_destination_symlink_outside_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary game root");
        let outside = tempfile::tempdir().expect("temporary outside root");
        std::fs::write(root.path().join("source.bin"), b"source").expect("source exists");
        std::fs::write(outside.path().join("escape.bin"), b"old").expect("outside target exists");
        symlink(
            outside.path().join("escape.bin"),
            root.path().join("link.bin"),
        )
        .expect("destination symlink created");
        let mut fs = NativeFileSystem::new(root.path().to_str().expect("UTF-8 test path"))
            .expect("native VFS opens the game root");

        assert_eq!(
            fs.copy("source.bin", "link.bin")
                .expect_err("escape is blocked"),
            RfvpError::InvalidArgument
        );
        assert_eq!(
            std::fs::read(outside.path().join("escape.bin")).expect("outside target remains"),
            b"old"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_rejects_parent_symlink_before_creating_children() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary game root");
        let outside = tempfile::tempdir().expect("temporary outside root");
        symlink(outside.path(), root.path().join("link")).expect("parent symlink created");
        let mut fs = NativeFileSystem::new(root.path().to_str().expect("UTF-8 test path"))
            .expect("native VFS opens the game root");

        assert_eq!(
            fs.write_all("link/new/save.bin", b"state")
                .expect_err("parent escape is blocked"),
            RfvpError::InvalidArgument
        );
        assert!(!outside.path().join("new").exists());
    }
}
