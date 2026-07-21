use crate::{PlatformError, PlatformErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizedSourceFileStat {
    pub byte_length: u64,
}

pub trait AuthorizedSourceDirectoryBackend: Send {
    fn stat_relative(&self, relative_path: &str)
        -> Result<AuthorizedSourceFileStat, PlatformError>;
    fn read_relative(&self, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, PlatformError>;
    fn read_relative_range(
        &self,
        relative_path: &str,
        offset: u64,
        length: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PlatformError>;
}

pub struct AuthorizedSourceDirectory {
    backend: Box<dyn AuthorizedSourceDirectoryBackend>,
}

impl AuthorizedSourceDirectory {
    pub fn from_backend(backend: impl AuthorizedSourceDirectoryBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    pub fn stat_relative(
        &self,
        relative_path: &str,
    ) -> Result<AuthorizedSourceFileStat, PlatformError> {
        validate_safe_relative_path(relative_path)?;
        self.backend.stat_relative(relative_path)
    }

    pub fn read_relative(
        &self,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PlatformError> {
        validate_safe_relative_path(relative_path)?;
        if max_bytes == 0 {
            return Err(source_error(
                "authorized source read bound must be positive",
            ));
        }
        let bytes = self.backend.read_relative(relative_path, max_bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "source_directory.read",
                "authorized source backend exceeded the requested byte bound",
            ));
        }
        Ok(bytes)
    }

    pub fn read_relative_range(
        &self,
        relative_path: &str,
        offset: u64,
        length: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PlatformError> {
        validate_safe_relative_path(relative_path)?;
        if length == 0 || length > max_bytes || offset.checked_add(length).is_none() {
            return Err(source_error(
                "authorized source range must be non-empty and bounded",
            ));
        }
        let bytes = self
            .backend
            .read_relative_range(relative_path, offset, length, max_bytes)?;
        if bytes.len() as u64 != length {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "source_directory.read_range",
                "authorized source backend returned a short or oversized range",
            ));
        }
        Ok(bytes)
    }
}

pub trait UserAuthorizedSourceDirectoryProvider {
    fn authorize_source_directory(&self) -> Result<AuthorizedSourceDirectory, PlatformError>;
}

pub fn validate_safe_relative_path(relative_path: &str) -> Result<(), PlatformError> {
    if relative_path.is_empty()
        || relative_path.len() > 512
        || relative_path.starts_with('/')
        || relative_path.starts_with('\\')
        || relative_path.contains(':')
        || relative_path.split(['/', '\\']).any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.len() > 128
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        return Err(source_error(
            "authorized source path must be a bounded safe relative path",
        ));
    }
    Ok(())
}

fn source_error(message: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::PermissionDenied,
        "source_directory.validate",
        message,
    )
}
