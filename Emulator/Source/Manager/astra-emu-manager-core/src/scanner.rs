use std::{collections::BTreeSet, sync::Arc};

use astra_core::Hash256;
use thiserror::Error;

use crate::{CancellationToken, Library, LibraryError, ScanCandidate, ScanReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantedSourceEntry {
    pub relative_path: String,
    pub modified_ns: i64,
    pub byte_size: u64,
    pub is_file: bool,
}

pub trait GrantedSourceReader: Send + Sync {
    fn enumerate(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<GrantedSourceEntry>, SourceScanError>;

    fn read_file(&self, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, SourceScanError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    pub max_entries: usize,
    pub max_script_bytes: u64,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_script_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Error)]
pub enum SourceScanError {
    #[error("ASTRA_EMU_SCAN_SOURCE_ENUMERATION")]
    Enumeration,
    #[error("ASTRA_EMU_SCAN_SOURCE_READ")]
    Read,
    #[error("ASTRA_EMU_SCAN_ENTRY_BOUNDS")]
    EntryBounds,
    #[error("ASTRA_EMU_SCAN_SCRIPT_BOUNDS")]
    ScriptBounds,
    #[error("ASTRA_EMU_SCAN_PATH_INVALID")]
    InvalidPath,
    #[error("ASTRA_EMU_SCAN_DUPLICATE_PATH")]
    DuplicatePath,
    #[error("ASTRA_EMU_SCAN_CANCELLED")]
    Cancelled,
    #[error(transparent)]
    Library(#[from] LibraryError),
}

pub struct LibraryScanner {
    limits: ScanLimits,
}

impl LibraryScanner {
    pub fn new(limits: ScanLimits) -> Result<Self, SourceScanError> {
        if limits.max_entries == 0 || limits.max_script_bytes == 0 {
            return Err(SourceScanError::EntryBounds);
        }
        Ok(Self { limits })
    }

    pub fn scan(
        &self,
        library: &mut Library,
        source_id: &str,
        source: Arc<dyn GrantedSourceReader>,
        cancellation: &CancellationToken,
    ) -> Result<ScanReport, SourceScanError> {
        if cancellation.is_cancelled() {
            return Err(SourceScanError::Cancelled);
        }
        let mut entries = source.enumerate(cancellation)?;
        if entries.len() > self.limits.max_entries {
            return Err(SourceScanError::EntryBounds);
        }
        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut paths = BTreeSet::new();
        let mut candidates = Vec::new();
        for entry in entries {
            if cancellation.is_cancelled() {
                return Err(SourceScanError::Cancelled);
            }
            let relative_path = normalize_source_path(&entry.relative_path)?;
            if !paths.insert(relative_path.clone()) {
                return Err(SourceScanError::DuplicatePath);
            }
            if !entry.is_file || !relative_path.to_ascii_lowercase().ends_with(".hcb") {
                continue;
            }
            if entry.byte_size > self.limits.max_script_bytes || entry.byte_size > i64::MAX as u64 {
                return Err(SourceScanError::ScriptBounds);
            }
            let bytes = source.read_file(&relative_path, self.limits.max_script_bytes)?;
            if bytes.len() as u64 != entry.byte_size {
                return Err(SourceScanError::Read);
            }
            let content_hash = Hash256::from_sha256(&bytes).to_string();
            let identity_material = format!("{source_id}\0{relative_path}");
            let identity = Hash256::from_sha256(identity_material.as_bytes()).to_hex();
            let title = relative_path
                .rsplit_once('/')
                .and_then(|(parent, _)| parent.rsplit('/').next())
                .filter(|value| !value.is_empty())
                .unwrap_or("FVP Game")
                .to_owned();
            candidates.push(ScanCandidate {
                source_id: source_id.to_owned(),
                relative_path,
                case_identity: format!("case-{}", &identity[..32]),
                content_hash,
                modified_ns: entry.modified_ns,
                byte_size: entry.byte_size as i64,
                title,
            });
        }
        library
            .apply_scan(source_id, &candidates, cancellation)
            .map_err(Into::into)
    }
}

fn normalize_source_path(value: &str) -> Result<String, SourceScanError> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.len() > 4096
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(SourceScanError::InvalidPath);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Mutex};

    use super::*;
    use crate::SourceGrant;

    struct MemorySource {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl GrantedSourceReader for MemorySource {
        fn enumerate(
            &self,
            _cancellation: &CancellationToken,
        ) -> Result<Vec<GrantedSourceEntry>, SourceScanError> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .map(|(path, bytes)| GrantedSourceEntry {
                    relative_path: path.clone(),
                    modified_ns: 1,
                    byte_size: bytes.len() as u64,
                    is_file: true,
                })
                .collect())
        }

        fn read_file(
            &self,
            relative_path: &str,
            _max_bytes: u64,
        ) -> Result<Vec<u8>, SourceScanError> {
            self.files
                .lock()
                .unwrap()
                .get(relative_path)
                .cloned()
                .ok_or(SourceScanError::Read)
        }
    }

    #[test]
    fn scan_is_deterministic_and_removes_disappeared_cases() {
        let source = Arc::new(MemorySource {
            files: Mutex::new(BTreeMap::from([
                ("a/start.hcb".into(), b"one".to_vec()),
                ("b/start.hcb".into(), b"two".to_vec()),
                ("notes/readme.txt".into(), b"ignored".to_vec()),
            ])),
        });
        let mut library = Library::in_memory().unwrap();
        library
            .upsert_grant(&SourceGrant {
                source_id: "root-1".into(),
                alias: "Games".into(),
                platform_token: "opaque".into(),
                token_kind: "test".into(),
                active: true,
            })
            .unwrap();
        let scanner = LibraryScanner::new(ScanLimits::default()).unwrap();
        let cancellation = CancellationToken::default();
        let first = scanner
            .scan(&mut library, "root-1", source.clone(), &cancellation)
            .unwrap();
        assert_eq!(first.inserted, 2);
        source.files.lock().unwrap().remove("a/start.hcb");
        let second = scanner
            .scan(&mut library, "root-1", source, &cancellation)
            .unwrap();
        assert_eq!((second.unchanged, second.removed), (1, 1));
    }
}
