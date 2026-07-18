use std::{collections::BTreeMap, io::Cursor};

use astra_core::Hash256;
use astra_emu_family_core::{
    LegacyCoreError, LegacyMountedVfs, LegacyPackManifest, LegacyVfsEntry, LegacyVfsNode,
    LegacyVfsNodeKind, LegacyVfsReadResult, LegacyVfsSource, LegacyVfsStat, LegacyVfsStream,
    LEGACY_PACK_MANIFEST_SCHEMA,
};

pub(crate) struct MemoryVfs {
    manifest: LegacyPackManifest,
    bytes: BTreeMap<String, Vec<u8>>,
}

impl MemoryVfs {
    pub(crate) fn new(entries: &[(&str, &[u8], &str)]) -> Self {
        let bytes = entries
            .iter()
            .map(|(uri, bytes, _)| ((*uri).to_owned(), bytes.to_vec()))
            .collect::<BTreeMap<_, _>>();
        let entries = entries
            .iter()
            .enumerate()
            .map(|(index, (uri, bytes, media_kind))| LegacyVfsEntry {
                uri: (*uri).to_owned(),
                entry_id: format!("entry-{index}"),
                source_id: "source".into(),
                source_offset: index as u64,
                stored_size: bytes.len() as u64,
                decoded_size: bytes.len() as u64,
                source_hash: Hash256::from_sha256(bytes),
                content_hash: Some(Hash256::from_sha256(bytes)),
                method: "raw".into(),
                media_kind: (*media_kind).to_owned(),
            })
            .collect::<Vec<_>>();
        let total_size = entries
            .iter()
            .map(|entry| entry.stored_size)
            .sum::<u64>()
            .max(1);
        Self {
            manifest: LegacyPackManifest {
                schema: LEGACY_PACK_MANIFEST_SCHEMA.into(),
                family_id: "test".into(),
                mount_id: "test-mount".into(),
                prefix: "test:/".into(),
                reader_id: "test-reader".into(),
                reader_hash: Hash256::from_sha256(b"reader"),
                decrypt_provider_id: "test-decrypt".into(),
                private_profile_hash: Hash256::from_sha256(b"profile"),
                mount_profile_hash: Hash256::from_sha256(b"mount"),
                sources: vec![LegacyVfsSource {
                    source_id: "source".into(),
                    archive_role: Some("test".into()),
                    byte_size: total_size,
                    part_count: 1,
                    source_hash: Hash256::from_sha256(b"source"),
                }],
                entries,
            },
            bytes,
        }
    }

    fn invalid() -> LegacyCoreError {
        LegacyCoreError::invalid("ASTRA_EMU_VFS_TEST_MISSING", "test entry is missing")
    }
}

impl LegacyMountedVfs for MemoryVfs {
    fn mount_id(&self) -> &str {
        &self.manifest.mount_id
    }

    fn manifest(&self) -> &LegacyPackManifest {
        &self.manifest
    }

    fn validate_sources(&self) -> Result<(), LegacyCoreError> {
        Ok(())
    }

    fn read_dir(&self, uri: &str) -> Result<Vec<LegacyVfsNode>, LegacyCoreError> {
        let prefix = if uri == self.manifest.prefix {
            self.manifest.prefix.clone()
        } else {
            format!("{}/", uri.trim_end_matches('/'))
        };
        let mut nodes = BTreeMap::new();
        for entry in &self.manifest.entries {
            let Some(rest) = entry.uri.strip_prefix(&prefix) else {
                continue;
            };
            let mut parts = rest.splitn(2, '/');
            let name = parts.next().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let directory = parts.next().is_some();
            nodes
                .entry(name.to_owned())
                .or_insert_with(|| LegacyVfsNode {
                    uri: format!("{prefix}{name}"),
                    name: name.to_owned(),
                    kind: if directory {
                        LegacyVfsNodeKind::Directory
                    } else {
                        LegacyVfsNodeKind::File
                    },
                });
        }
        Ok(nodes.into_values().collect())
    }

    fn stat(&self, uri: &str) -> Result<LegacyVfsStat, LegacyCoreError> {
        let entry = self
            .manifest
            .entries
            .iter()
            .find(|entry| entry.uri == uri)
            .ok_or_else(Self::invalid)?;
        Ok(LegacyVfsStat {
            uri: uri.to_owned(),
            entry_id: Some(entry.entry_id.clone()),
            kind: LegacyVfsNodeKind::File,
            size: entry.decoded_size,
            content_hash: entry.content_hash,
            archive_role: Some("test".into()),
            method: Some(entry.method.clone()),
        })
    }

    fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<LegacyVfsReadResult, LegacyCoreError> {
        let bytes = self.bytes.get(uri).ok_or_else(Self::invalid)?;
        let start = usize::try_from(offset).map_err(|_| Self::invalid())?;
        let end = usize::try_from(offset.checked_add(length).ok_or_else(Self::invalid)?)
            .map_err(|_| Self::invalid())?;
        if start > bytes.len() || end > bytes.len() {
            return Err(Self::invalid());
        }
        Ok(LegacyVfsReadResult {
            uri: uri.to_owned(),
            offset,
            bytes: bytes[start..end].to_vec(),
            eof: end == bytes.len(),
            cache_hit: true,
        })
    }

    fn open_stream(&self, uri: &str) -> Result<Box<dyn LegacyVfsStream>, LegacyCoreError> {
        Ok(Box::new(Cursor::new(
            self.bytes.get(uri).ok_or_else(Self::invalid)?.clone(),
        )))
    }
}
