use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use astra_emu_family_api::{
    LegacyProviderError, LegacyWritableFileEntryV1, LegacyWritableFileHostV1,
    LegacyWritableFileRequestV1, LegacyWritableFileResultV1,
};

const DEFAULT_MAX_IO_BYTES: u64 = 64 * 1024 * 1024;

/// A session-scoped filesystem port rooted in a caller-selected private data
/// directory. Every operation rejects symlinks and remains beneath that root.
pub struct LocalPrivateWritableFileHostV1 {
    root: PathBuf,
    max_io_bytes: u64,
}

impl LocalPrivateWritableFileHostV1 {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, LegacyProviderError> {
        Self::with_max_io_bytes(root, DEFAULT_MAX_IO_BYTES)
    }

    pub fn with_max_io_bytes(
        root: impl AsRef<Path>,
        max_io_bytes: u64,
    ) -> Result<Self, LegacyProviderError> {
        if max_io_bytes == 0 || max_io_bytes > usize::MAX as u64 {
            return Err(invalid(
                "ASTRA_EMU_WRITABLE_LIMITS",
                "writable file I/O limit is invalid",
            ));
        }
        fs::create_dir_all(root.as_ref()).map_err(io_error)?;
        set_private_directory_permissions(root.as_ref())?;
        let root = fs::canonicalize(root.as_ref()).map_err(io_error)?;
        if !fs::metadata(&root).map_err(io_error)?.is_dir() {
            return Err(invalid(
                "ASTRA_EMU_WRITABLE_ROOT",
                "writable file root is not a directory",
            ));
        }
        reject_symlink(&root)?;
        Ok(Self { root, max_io_bytes })
    }

    fn resolve(&self, relative: &str) -> Result<PathBuf, LegacyProviderError> {
        astra_emu_family_api::validate_relative_writable_path(relative)?;
        let mut path = self.root.clone();
        for component in relative.split(['/', '\\']) {
            path.push(component);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(invalid(
                        "ASTRA_EMU_WRITABLE_SYMLINK",
                        "writable file paths may not cross symlinks",
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        Ok(path)
    }

    fn require_parent(&self, path: &Path) -> Result<(), LegacyProviderError> {
        let parent = path.parent().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_WRITABLE_PARENT",
                "writable file path has no parent",
            )
        })?;
        let canonical = fs::canonicalize(parent).map_err(io_error)?;
        if !canonical.starts_with(&self.root)
            || !fs::metadata(&canonical).map_err(io_error)?.is_dir()
        {
            return Err(invalid(
                "ASTRA_EMU_WRITABLE_PARENT",
                "writable file parent is outside the private root",
            ));
        }
        reject_symlink(parent)
    }

    fn stat_result(&self, path: &Path) -> Result<LegacyWritableFileResultV1, LegacyProviderError> {
        match fs::metadata(path) {
            Ok(metadata) => Ok(result(
                true,
                metadata.is_file(),
                if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                },
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(result(false, false, 0))
            }
            Err(error) => Err(io_error(error)),
        }
    }
}

impl LegacyWritableFileHostV1 for LocalPrivateWritableFileHostV1 {
    fn execute(
        &self,
        _session_id: &str,
        request: LegacyWritableFileRequestV1,
    ) -> Result<LegacyWritableFileResultV1, LegacyProviderError> {
        request.validate()?;
        match request {
            LegacyWritableFileRequestV1::Stat { path } => self.stat_result(&self.resolve(&path)?),
            LegacyWritableFileRequestV1::List { path } => {
                let path = self.resolve(&path)?;
                let mut entries = Vec::new();
                for entry in fs::read_dir(path).map_err(io_error)? {
                    let entry = entry.map_err(io_error)?;
                    let metadata = entry.file_type().map_err(io_error)?;
                    if metadata.is_symlink() {
                        return Err(invalid(
                            "ASTRA_EMU_WRITABLE_SYMLINK",
                            "writable file directory contains a symlink",
                        ));
                    }
                    let name = entry.file_name().into_string().map_err(|_| {
                        invalid(
                            "ASTRA_EMU_WRITABLE_NAME",
                            "writable file name is not valid UTF-8",
                        )
                    })?;
                    astra_emu_family_api::validate_relative_writable_path(&name)?;
                    let metadata = entry.metadata().map_err(io_error)?;
                    entries.push(LegacyWritableFileEntryV1 {
                        name,
                        is_file: metadata.is_file(),
                        length: if metadata.is_file() {
                            metadata.len()
                        } else {
                            0
                        },
                    });
                }
                entries.sort_by(|left, right| left.name.cmp(&right.name));
                let mut output = result(true, false, 0);
                output.entries = entries;
                Ok(output)
            }
            LegacyWritableFileRequestV1::CreateDir { path } => {
                let path = self.resolve(&path)?;
                fs::create_dir_all(&path).map_err(io_error)?;
                set_private_directory_permissions(&path)?;
                reject_tree_symlinks(&self.root, &path)?;
                Ok(result(true, false, 0))
            }
            LegacyWritableFileRequestV1::ReadRange {
                path,
                offset,
                length,
            } => {
                if length > self.max_io_bytes {
                    return Err(invalid(
                        "ASTRA_EMU_WRITABLE_RANGE",
                        "writable file read exceeds the configured limit",
                    ));
                }
                let path = self.resolve(&path)?;
                let metadata = fs::metadata(&path).map_err(io_error)?;
                let end = offset.checked_add(length).ok_or_else(|| {
                    invalid("ASTRA_EMU_WRITABLE_RANGE", "writable file read overflows")
                })?;
                if !metadata.is_file() || end > metadata.len() {
                    return Err(invalid(
                        "ASTRA_EMU_WRITABLE_RANGE",
                        "writable file read is outside the file",
                    ));
                }
                let mut file = File::open(path).map_err(io_error)?;
                file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
                let mut bytes = vec![0; length as usize];
                file.read_exact(&mut bytes).map_err(io_error)?;
                let mut output = result(true, true, metadata.len());
                output.bytes = bytes.into();
                Ok(output)
            }
            LegacyWritableFileRequestV1::WriteRange {
                path,
                offset,
                bytes,
            } => {
                if bytes.len() as u64 > self.max_io_bytes {
                    return Err(invalid(
                        "ASTRA_EMU_WRITABLE_RANGE",
                        "writable file write exceeds the configured limit",
                    ));
                }
                let path = self.resolve(&path)?;
                self.require_parent(&path)?;
                let mut options = OpenOptions::new();
                options.create(true).write(true);
                set_private_create_mode(&mut options);
                let mut file = options.open(&path).map_err(io_error)?;
                set_private_file_permissions(&path)?;
                file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
                file.write_all(&bytes).map_err(io_error)?;
                file.sync_data().map_err(io_error)?;
                let written = bytes.len() as u64;
                let mut output = result(true, true, file.metadata().map_err(io_error)?.len());
                output.written = written;
                Ok(output)
            }
            LegacyWritableFileRequestV1::SetLength { path, length } => {
                let path = self.resolve(&path)?;
                self.require_parent(&path)?;
                let mut options = OpenOptions::new();
                options.create(true).write(true);
                set_private_create_mode(&mut options);
                let file = options.open(&path).map_err(io_error)?;
                set_private_file_permissions(&path)?;
                file.set_len(length).map_err(io_error)?;
                file.sync_data().map_err(io_error)?;
                Ok(result(true, true, length))
            }
            LegacyWritableFileRequestV1::Remove { path } => {
                let path = self.resolve(&path)?;
                let metadata = fs::metadata(&path).map_err(io_error)?;
                if metadata.is_file() {
                    fs::remove_file(path).map_err(io_error)?;
                } else if metadata.is_dir() {
                    fs::remove_dir(path).map_err(io_error)?;
                } else {
                    return Err(invalid(
                        "ASTRA_EMU_WRITABLE_TYPE",
                        "writable path is neither a file nor directory",
                    ));
                }
                Ok(result(false, false, 0))
            }
            LegacyWritableFileRequestV1::AtomicReplace {
                temporary_path,
                destination_path,
            } => {
                let temporary = self.resolve(&temporary_path)?;
                let destination = self.resolve(&destination_path)?;
                self.require_parent(&temporary)?;
                self.require_parent(&destination)?;
                if temporary.parent() != destination.parent()
                    || !fs::metadata(&temporary).map_err(io_error)?.is_file()
                {
                    return Err(invalid(
                        "ASTRA_EMU_WRITABLE_ATOMIC_REPLACE",
                        "atomic replace requires a temporary file in the destination directory",
                    ));
                }
                atomic_replace(&temporary, &destination)?;
                set_private_file_permissions(&destination)?;
                self.stat_result(&destination)
            }
        }
    }
}

fn result(exists: bool, is_file: bool, length: u64) -> LegacyWritableFileResultV1 {
    LegacyWritableFileResultV1 {
        exists,
        is_file,
        length,
        entries: Vec::new(),
        bytes: Vec::new().into(),
        written: 0,
    }
}

fn reject_symlink(path: &Path) -> Result<(), LegacyProviderError> {
    if fs::symlink_metadata(path)
        .map_err(io_error)?
        .file_type()
        .is_symlink()
    {
        return Err(invalid(
            "ASTRA_EMU_WRITABLE_SYMLINK",
            "writable file paths may not cross symlinks",
        ));
    }
    Ok(())
}

fn reject_tree_symlinks(root: &Path, leaf: &Path) -> Result<(), LegacyProviderError> {
    let relative = leaf.strip_prefix(root).map_err(|_| {
        invalid(
            "ASTRA_EMU_WRITABLE_PATH",
            "writable directory escaped its private root",
        )
    })?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component);
        reject_symlink(&cursor)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), LegacyProviderError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), LegacyProviderError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_create_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_create_mode(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), LegacyProviderError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), LegacyProviderError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn atomic_replace(temporary: &Path, destination: &Path) -> Result<(), LegacyProviderError> {
    fs::rename(temporary, destination).map_err(io_error)
}

#[cfg(target_os = "windows")]
fn atomic_replace(temporary: &Path, destination: &Path) -> Result<(), LegacyProviderError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    let source = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|_| invalid("ASTRA_EMU_WRITABLE_IO", "atomic replace failed"))
}

fn io_error(_error: std::io::Error) -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_WRITABLE_IO",
        "private writable file operation failed",
    )
}

fn invalid(code: &'static str, message: &'static str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_store_round_trips_atomic_file_and_rejects_escape() {
        let root = tempfile::tempdir().unwrap();
        let host = LocalPrivateWritableFileHostV1::new(root.path()).unwrap();
        host.execute(
            "session.test",
            LegacyWritableFileRequestV1::CreateDir {
                path: "minori".into(),
            },
        )
        .unwrap();
        host.execute(
            "session.test",
            LegacyWritableFileRequestV1::WriteRange {
                path: "minori/progress.tmp".into(),
                offset: 0,
                bytes: vec![1, 2, 3],
            },
        )
        .unwrap();
        host.execute(
            "session.test",
            LegacyWritableFileRequestV1::AtomicReplace {
                temporary_path: "minori/progress.tmp".into(),
                destination_path: "minori/progress.bin".into(),
            },
        )
        .unwrap();
        let read = host
            .execute(
                "session.test",
                LegacyWritableFileRequestV1::ReadRange {
                    path: "minori/progress.bin".into(),
                    offset: 0,
                    length: 3,
                },
            )
            .unwrap();
        assert_eq!(read.bytes.as_slice(), &[1, 2, 3]);
        assert!(host
            .execute(
                "session.test",
                LegacyWritableFileRequestV1::Stat {
                    path: "../escape".into(),
                },
            )
            .is_err());
    }
}
