use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use astra_core::Hash256;
use thiserror::Error;

use crate::{CancellationToken, Library, LibraryError, ScanCandidate, ScanReport};

const FINGERPRINT_SCHEMA: &str = "astra.emu.discovery_fingerprint.v1";
const LARGE_MARKER_PREFIX_BYTES: u64 = 64 * 1024;

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

    fn read_prefix(
        &self,
        relative_path: &str,
        byte_size: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, SourceScanError> {
        if byte_size > max_bytes {
            return Err(SourceScanError::ScriptBounds);
        }
        self.read_file(relative_path, max_bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryMarker {
    FileName(&'static str),
    Extension(&'static str),
}

impl DiscoveryMarker {
    fn matches(self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        match self {
            Self::FileName(expected) => name.eq_ignore_ascii_case(expected),
            Self::Extension(extension) => name
                .to_ascii_lowercase()
                .ends_with(&format!(".{extension}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyDiscoveryDescriptor {
    pub family_id: &'static str,
    pub entry_markers: &'static [DiscoveryMarker],
    pub supporting_markers: &'static [DiscoveryMarker],
    pub max_markers_per_root: usize,
}

const KRKR_ENTRY: &[DiscoveryMarker] = &[
    DiscoveryMarker::FileName("data.xp3"),
    DiscoveryMarker::Extension("tjs"),
];
const KRKR_SUPPORT: &[DiscoveryMarker] = &[DiscoveryMarker::Extension("xp3")];
const ARTEMIS_ENTRY: &[DiscoveryMarker] = &[DiscoveryMarker::Extension("pfs")];
const ARTEMIS_SUPPORT: &[DiscoveryMarker] = &[DiscoveryMarker::FileName("system.ini")];
const BGI_ENTRY: &[DiscoveryMarker] = &[
    DiscoveryMarker::FileName("data01000.arc"),
    DiscoveryMarker::FileName("sysgrp.arc"),
];
const BGI_SUPPORT: &[DiscoveryMarker] = &[DiscoveryMarker::Extension("arc")];
const SIGLUS_ENTRY: &[DiscoveryMarker] = &[
    DiscoveryMarker::FileName("gameexe.dat"),
    DiscoveryMarker::FileName("scene.pck"),
];
const SIGLUS_SUPPORT: &[DiscoveryMarker] = &[DiscoveryMarker::Extension("pck")];
const SOFTPAL_ENTRY: &[DiscoveryMarker] = &[
    DiscoveryMarker::FileName("data.pac"),
    DiscoveryMarker::FileName("archive.dat"),
    DiscoveryMarker::FileName("script.src"),
];
const SOFTPAL_SUPPORT: &[DiscoveryMarker] = &[
    DiscoveryMarker::Extension("pac"),
    DiscoveryMarker::Extension("src"),
];
const FVP_ENTRY: &[DiscoveryMarker] = &[DiscoveryMarker::Extension("hcb")];
const FVP_SUPPORT: &[DiscoveryMarker] = &[DiscoveryMarker::Extension("hcb")];
const MINORI_ENTRY: &[DiscoveryMarker] = &[DiscoveryMarker::FileName("scr.paz")];
const MINORI_SUPPORT: &[DiscoveryMarker] = &[DiscoveryMarker::Extension("paz")];

pub const DEFAULT_DISCOVERY_DESCRIPTORS: [FamilyDiscoveryDescriptor; 7] = [
    FamilyDiscoveryDescriptor {
        family_id: "krkr",
        entry_markers: KRKR_ENTRY,
        supporting_markers: KRKR_SUPPORT,
        max_markers_per_root: 32,
    },
    FamilyDiscoveryDescriptor {
        family_id: "artemis",
        entry_markers: ARTEMIS_ENTRY,
        supporting_markers: ARTEMIS_SUPPORT,
        max_markers_per_root: 16,
    },
    FamilyDiscoveryDescriptor {
        family_id: "bgi",
        entry_markers: BGI_ENTRY,
        supporting_markers: BGI_SUPPORT,
        max_markers_per_root: 32,
    },
    FamilyDiscoveryDescriptor {
        family_id: "siglus",
        entry_markers: SIGLUS_ENTRY,
        supporting_markers: SIGLUS_SUPPORT,
        max_markers_per_root: 16,
    },
    FamilyDiscoveryDescriptor {
        family_id: "softpal",
        entry_markers: SOFTPAL_ENTRY,
        supporting_markers: SOFTPAL_SUPPORT,
        max_markers_per_root: 32,
    },
    FamilyDiscoveryDescriptor {
        family_id: "fvp",
        entry_markers: FVP_ENTRY,
        supporting_markers: FVP_SUPPORT,
        max_markers_per_root: 64,
    },
    FamilyDiscoveryDescriptor {
        family_id: "minori",
        entry_markers: MINORI_ENTRY,
        supporting_markers: MINORI_SUPPORT,
        max_markers_per_root: 32,
    },
];

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
    #[error("ASTRA_EMU_SCAN_DISCOVERY_BOUNDS")]
    DiscoveryBounds,
    #[error("ASTRA_EMU_SCAN_CANCELLED")]
    Cancelled,
    #[error(transparent)]
    Library(#[from] LibraryError),
}

pub struct LibraryScanner {
    limits: ScanLimits,
    descriptors: Vec<FamilyDiscoveryDescriptor>,
}

#[derive(Default)]
struct RootEvidence {
    entries: BTreeMap<String, GrantedSourceEntry>,
    families: BTreeSet<&'static str>,
    primary: BTreeSet<String>,
    marker_limit: usize,
}

impl LibraryScanner {
    pub fn new(limits: ScanLimits) -> Result<Self, SourceScanError> {
        Self::with_descriptors(limits, DEFAULT_DISCOVERY_DESCRIPTORS)
    }

    pub fn with_descriptors(
        limits: ScanLimits,
        descriptors: impl IntoIterator<Item = FamilyDiscoveryDescriptor>,
    ) -> Result<Self, SourceScanError> {
        if limits.max_entries == 0 || limits.max_script_bytes == 0 {
            return Err(SourceScanError::EntryBounds);
        }
        let descriptors = descriptors.into_iter().collect::<Vec<_>>();
        if descriptors.is_empty()
            || descriptors.iter().any(|descriptor| {
                descriptor.family_id.is_empty()
                    || descriptor.entry_markers.is_empty()
                    || descriptor.max_markers_per_root == 0
            })
        {
            return Err(SourceScanError::DiscoveryBounds);
        }
        Ok(Self {
            limits,
            descriptors,
        })
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
        let mut normalized_entries = Vec::with_capacity(entries.len());
        for mut entry in entries {
            let path = normalize_source_path(&entry.relative_path)?;
            if !paths.insert(path.clone()) {
                return Err(SourceScanError::DuplicatePath);
            }
            entry.relative_path = path;
            normalized_entries.push(entry);
        }

        let mut roots = BTreeMap::<String, RootEvidence>::new();
        for entry in normalized_entries.iter().filter(|entry| entry.is_file) {
            if cancellation.is_cancelled() {
                return Err(SourceScanError::Cancelled);
            }
            for descriptor in &self.descriptors {
                let is_primary = descriptor
                    .entry_markers
                    .iter()
                    .any(|marker| marker.matches(&entry.relative_path));
                let is_support = descriptor
                    .supporting_markers
                    .iter()
                    .any(|marker| marker.matches(&entry.relative_path));
                if !is_primary && !is_support {
                    continue;
                }
                let root = installation_root(&entry.relative_path).to_owned();
                let evidence = roots.entry(root).or_default();
                evidence.families.insert(descriptor.family_id);
                evidence.marker_limit = evidence.marker_limit.max(descriptor.max_markers_per_root);
                evidence
                    .entries
                    .insert(entry.relative_path.clone(), entry.clone());
                if is_primary {
                    evidence.primary.insert(entry.relative_path.clone());
                }
            }
        }

        let mut candidates = Vec::new();
        for (root, evidence) in roots {
            if evidence.primary.is_empty() {
                continue;
            }
            if evidence.entries.len() > evidence.marker_limit {
                return Err(SourceScanError::DiscoveryBounds);
            }
            let primary_path = evidence
                .primary
                .iter()
                .next()
                .expect("checked above")
                .clone();
            let primary = evidence
                .entries
                .get(&primary_path)
                .expect("same evidence map");
            let primary_modified_ns = primary.modified_ns;
            let primary_byte_size = primary.byte_size;
            let mut material = Vec::new();
            append_field(&mut material, FINGERPRINT_SCHEMA.as_bytes());
            append_field(&mut material, root.as_bytes());
            for family in evidence.families {
                append_field(&mut material, family.as_bytes());
            }
            for (path, marker) in evidence.entries {
                append_field(&mut material, path.as_bytes());
                append_field(&mut material, &marker.byte_size.to_le_bytes());
                append_field(&mut material, &marker.modified_ns.to_le_bytes());
                if marker.byte_size <= self.limits.max_script_bytes {
                    let bytes = source.read_file(&path, self.limits.max_script_bytes)?;
                    if bytes.len() as u64 != marker.byte_size {
                        return Err(SourceScanError::Read);
                    }
                    append_field(&mut material, Hash256::from_sha256(&bytes).as_bytes());
                } else {
                    let prefix_len = marker.byte_size.min(LARGE_MARKER_PREFIX_BYTES);
                    let bytes = source.read_prefix(&path, marker.byte_size, prefix_len)?;
                    if bytes.len() as u64 != prefix_len {
                        return Err(SourceScanError::Read);
                    }
                    append_field(&mut material, Hash256::from_sha256(&bytes).as_bytes());
                }
            }
            let content_hash = Hash256::from_sha256(&material).to_string();
            let identity = Hash256::from_sha256(format!("{source_id}\0{root}").as_bytes()).to_hex();
            let title = if root.is_empty() {
                primary_path
                    .rsplit('/')
                    .next()
                    .unwrap_or("Legacy Game")
                    .rsplit_once('.')
                    .map_or_else(|| primary_path.clone(), |(stem, _)| stem.to_owned())
            } else {
                root.rsplit('/').next().unwrap_or("Legacy Game").to_owned()
            };
            candidates.push(ScanCandidate {
                source_id: source_id.to_owned(),
                relative_path: primary_path,
                case_identity: format!("case-{}", &identity[..32]),
                content_hash,
                modified_ns: primary_modified_ns,
                byte_size: i64::try_from(primary_byte_size)
                    .map_err(|_| SourceScanError::ScriptBounds)?,
                title: if title.is_empty() {
                    "Legacy Game".into()
                } else {
                    title
                },
            });
        }
        library
            .apply_scan(source_id, &candidates, cancellation)
            .map_err(Into::into)
    }
}

fn append_field(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_le_bytes());
    output.extend_from_slice(value);
}

fn installation_root(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
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
    fn scan_merges_family_markers_per_installation_root() {
        let source = Arc::new(MemorySource {
            files: Mutex::new(BTreeMap::from([
                ("a/start.hcb".into(), b"one".to_vec()),
                ("a/second.hcb".into(), b"two".to_vec()),
                ("b/data.xp3".into(), b"archive".to_vec()),
                ("b/startup.tjs".into(), b"script".to_vec()),
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
        assert_eq!(
            (second.updated, second.unchanged, second.removed),
            (1, 1, 0)
        );
    }

    #[test]
    fn discovery_does_not_execute_or_parse_candidate_content() {
        let source = Arc::new(MemorySource {
            files: Mutex::new(BTreeMap::from([(
                "game/startup.tjs".into(),
                b"System.exec('commercial payload')".to_vec(),
            )])),
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
        let report = LibraryScanner::new(ScanLimits::default())
            .unwrap()
            .scan(
                &mut library,
                "root-1",
                source,
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(report.inserted, 1);
    }
}
