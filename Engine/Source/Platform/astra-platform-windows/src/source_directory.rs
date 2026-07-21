use astra_platform::{
    AuthorizedSourceDirectory, AuthorizedSourceDirectoryBackend, AuthorizedSourceFileStat,
    PlatformError, PlatformErrorCode, UserAuthorizedSourceDirectoryProvider,
};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub struct WindowsSourceDirectoryProvider;

impl UserAuthorizedSourceDirectoryProvider for WindowsSourceDirectoryProvider {
    fn authorize_source_directory(&self) -> Result<AuthorizedSourceDirectory, PlatformError> {
        let root = pollster::block_on(rfd::AsyncFileDialog::new().pick_folder())
            .ok_or_else(|| {
                error(
                    PlatformErrorCode::Cancelled,
                    "user cancelled source selection",
                )
            })?
            .path()
            .to_path_buf();
        let root = root
            .canonicalize()
            .map_err(|_| error(PlatformErrorCode::Io, "selected source root is unreadable"))?;
        Ok(AuthorizedSourceDirectory::from_backend(
            WindowsSourceDirectory { root },
        ))
    }
}

struct WindowsSourceDirectory {
    root: PathBuf,
}

impl AuthorizedSourceDirectoryBackend for WindowsSourceDirectory {
    fn stat_relative(
        &self,
        relative_path: &str,
    ) -> Result<AuthorizedSourceFileStat, PlatformError> {
        let path = self.resolve(relative_path)?;
        let metadata = path
            .metadata()
            .map_err(|_| error(PlatformErrorCode::Io, "authorized source file is missing"))?;
        if !metadata.is_file() {
            return Err(error(
                PlatformErrorCode::IntegrityMismatch,
                "authorized source entry is not a file",
            ));
        }
        Ok(AuthorizedSourceFileStat {
            byte_length: metadata.len(),
        })
    }

    fn read_relative(&self, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, PlatformError> {
        let path = self.resolve(relative_path)?;
        let metadata = path
            .metadata()
            .map_err(|_| error(PlatformErrorCode::Io, "authorized source file is missing"))?;
        if !metadata.is_file() || metadata.len() > max_bytes {
            return Err(error(
                PlatformErrorCode::IntegrityMismatch,
                "authorized source file exceeds its read bound",
            ));
        }
        std::fs::read(path)
            .map_err(|_| error(PlatformErrorCode::Io, "authorized source file read failed"))
    }

    fn read_relative_range(
        &self,
        relative_path: &str,
        offset: u64,
        length: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PlatformError> {
        let path = self.resolve(relative_path)?;
        let metadata = path
            .metadata()
            .map_err(|_| error(PlatformErrorCode::Io, "authorized source file is missing"))?;
        let end = offset.checked_add(length).ok_or_else(|| {
            error(
                PlatformErrorCode::IntegrityMismatch,
                "authorized source range overflowed",
            )
        })?;
        if !metadata.is_file() || length == 0 || length > max_bytes || end > metadata.len() {
            return Err(error(
                PlatformErrorCode::IntegrityMismatch,
                "authorized source range exceeds its read bound",
            ));
        }
        let length: usize = length.try_into().map_err(|_| {
            error(
                PlatformErrorCode::IntegrityMismatch,
                "authorized source range exceeds addressable memory",
            )
        })?;
        let mut file = std::fs::File::open(path)
            .map_err(|_| error(PlatformErrorCode::Io, "authorized source file open failed"))?;
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| {
                let mut bytes = vec![0_u8; length];
                file.read_exact(&mut bytes).map(|_| bytes)
            })
            .map_err(|_| error(PlatformErrorCode::Io, "authorized source range read failed"))
    }
}

impl WindowsSourceDirectory {
    fn resolve(&self, relative_path: &str) -> Result<PathBuf, PlatformError> {
        let candidate = self.root.join(Path::new(relative_path));
        let canonical = candidate
            .canonicalize()
            .map_err(|_| error(PlatformErrorCode::Io, "authorized source file is missing"))?;
        if !canonical.starts_with(&self.root) {
            return Err(error(
                PlatformErrorCode::PermissionDenied,
                "authorized source path escaped its root",
            ));
        }
        Ok(canonical)
    }
}

fn error(code: PlatformErrorCode, message: &'static str) -> PlatformError {
    PlatformError::new(code, "source_directory.windows", message)
}
