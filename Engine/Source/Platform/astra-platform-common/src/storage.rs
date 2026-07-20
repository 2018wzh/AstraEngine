use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use astra_core::Hash256;
use astra_platform::{PlatformError, PlatformErrorCode};
use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
pub struct AtomicSaveStore {
    root: PathBuf,
}

impl AtomicSaveStore {
    pub fn new(base: impl AsRef<Path>, package_id: &str) -> Result<Self, PlatformError> {
        if !is_safe_package_id(package_id) {
            return Err(PlatformError::new(
                PlatformErrorCode::PermissionDenied,
                "save.store.open",
                "save package id is unsafe",
            ));
        }
        let root = base.as_ref().join(package_id).join("Saved");
        fs::create_dir_all(&root).map_err(|_| io_error("save.store.open"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn begin(&self, slot: &str) -> Result<SaveTransaction, PlatformError> {
        validate_slot(slot)?;
        let staging = NamedTempFile::new_in(&self.root).map_err(|_| io_error("save.begin"))?;
        Ok(SaveTransaction {
            staging: Some(staging),
            destination: self.root.join(format!("{slot}.astrasave")),
            wrote_bytes: false,
        })
    }

    pub fn read(&self, slot: &str) -> Result<Vec<u8>, PlatformError> {
        validate_slot(slot)?;
        fs::read(self.root.join(format!("{slot}.astrasave"))).map_err(|_| io_error("save.read"))
    }

    pub fn list(&self) -> Result<Vec<String>, PlatformError> {
        let mut slots = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|_| io_error("save.list"))? {
            let entry = entry.map_err(|_| io_error("save.list"))?;
            if !entry
                .file_type()
                .map_err(|_| io_error("save.list"))?
                .is_file()
            {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| io_error("save.list"))?;
            let Some(slot) = name.strip_suffix(".astrasave") else {
                continue;
            };
            validate_slot(slot)?;
            slots.push(slot.to_string());
        }
        slots.sort();
        if slots.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "save.list",
                "save store contains duplicate slot identities",
            ));
        }
        Ok(slots)
    }

    pub fn delete(&self, slot: &str) -> Result<(), PlatformError> {
        validate_slot(slot)?;
        fs::remove_file(self.root.join(format!("{slot}.astrasave")))
            .map_err(|_| io_error("save.delete"))
    }
}

#[derive(Debug)]
pub struct SaveTransaction {
    staging: Option<NamedTempFile>,
    destination: PathBuf,
    wrote_bytes: bool,
}

impl SaveTransaction {
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PlatformError> {
        if bytes.is_empty() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "save.write",
                "save transaction cannot write an empty payload",
            ));
        }
        let staging = self.staging.as_mut().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "save.write",
                "save transaction is already closed",
            )
        })?;
        staging
            .as_file_mut()
            .write_all(bytes)
            .map_err(|_| io_error("save.write"))?;
        self.wrote_bytes = true;
        Ok(())
    }

    pub fn commit(mut self) -> Result<String, PlatformError> {
        if !self.wrote_bytes {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "save.commit",
                "save transaction has no payload",
            ));
        }
        let mut staging = self.staging.take().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "save.commit",
                "save transaction is already closed",
            )
        })?;
        staging
            .as_file_mut()
            .flush()
            .and_then(|_| staging.as_file().sync_all())
            .map_err(|_| io_error("save.commit"))?;
        let bytes = fs::read(staging.path()).map_err(|_| io_error("save.commit"))?;
        let hash = Hash256::from_sha256(&bytes).to_string();
        staging
            .persist(&self.destination)
            .map_err(|_| io_error("save.commit"))?;
        Ok(hash)
    }

    pub fn abort(mut self) -> Result<(), PlatformError> {
        let staging = self.staging.take().ok_or_else(|| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "save.abort",
                "save transaction is already closed",
            )
        })?;
        staging.close().map_err(|_| io_error("save.abort"))
    }
}

#[derive(Debug)]
pub struct FilePackageSource {
    file: File,
    len: u64,
    hash: String,
}

impl FilePackageSource {
    pub fn open(path: impl AsRef<Path>, expected_hash: &str) -> Result<Self, PlatformError> {
        let mut file = File::open(path.as_ref()).map_err(|_| io_error("package.open"))?;
        let len = file.metadata().map_err(|_| io_error("package.open"))?.len();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| io_error("package.open"))?;
        let hash = Hash256::from_sha256(&bytes).to_string();
        if hash != expected_hash {
            return Err(PlatformError::new(
                PlatformErrorCode::IntegrityMismatch,
                "package.open",
                "package source hash does not match its declared identity",
            ));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| io_error("package.open"))?;
        Ok(Self { file, len, hash })
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, PlatformError> {
        if length == 0 || offset > self.len {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.read_range",
                "package range is invalid",
            ));
        }
        let available = self.len.saturating_sub(offset);
        let read_len = usize::try_from(available.min(length as u64)).map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "package.read_range",
                "package range cannot be represented on this host",
            )
        })?;
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| io_error("package.read_range"))?;
        let mut bytes = vec![0; read_len];
        self.file
            .read_exact(&mut bytes)
            .map_err(|_| io_error("package.read_range"))?;
        Ok(bytes)
    }
}

fn validate_slot(slot: &str) -> Result<(), PlatformError> {
    if slot.is_empty()
        || slot.len() > 128
        || !slot.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(PlatformError::new(
            PlatformErrorCode::PermissionDenied,
            "save.slot.validate",
            "save slot is not a safe symbol",
        ));
    }
    Ok(())
}

fn is_safe_package_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn io_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Io,
        operation,
        "platform storage operation failed",
    )
}
