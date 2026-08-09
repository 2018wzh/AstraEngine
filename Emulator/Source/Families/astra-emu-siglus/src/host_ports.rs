use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use astra_byte_source::{ByteRange, OwnedByteBuffer, SourceRevision};
use astra_emu_family_api::{
    LegacyPrivateMaterialHostV8, LegacyPrivateMaterialRequestV8, LegacyRuntimeSessionId,
    LegacySaveReadResultV8, LegacySaveSlotV8, LegacySaveStoreHostV8, LegacySaveWriteRequestV8,
    LegacySaveWriteResultV8, LegacyVfsReader,
};
use siglus_hosted::hosted_port::{
    normalize_resource_id, HostedPrivateMaterialPort, HostedResourceEntry, HostedResourcePort,
    HostedResourceStat, HostedSaveRead, HostedSaveSlot, HostedSaveStorePort, HostedSaveWrite,
    HostedSaveWriteResult,
};
use zeroize::Zeroizing;

const DEFAULT_ENUMERATED_EXTENSIONS: [&str; 15] = [
    "dat", "pck", "chs", "ini", "g00", "nwa", "ovk", "omv", "mpg", "mpeg", "wmv", "ogg", "wav",
    "dbs", "cgm",
];

fn port_error(code: &'static str) -> anyhow::Error {
    anyhow!(code)
}

fn checked_usize(value: u64, code: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| port_error(code))
}

/// Instance-bound Family ABI VFS adapter. It carries only a mount-set identity
/// and a logical URI prefix; no host path enters the fork.
pub struct SiglusResourcePortV8 {
    reader: Arc<dyn LegacyVfsReader>,
    mount_set_id: String,
    uri_prefix: String,
    enumerated_extensions: Vec<String>,
}

impl SiglusResourcePortV8 {
    pub fn new(
        reader: Arc<dyn LegacyVfsReader>,
        mount_set_id: impl Into<String>,
        uri_prefix: impl Into<String>,
    ) -> Result<Self> {
        Self::with_extensions(
            reader,
            mount_set_id,
            uri_prefix,
            DEFAULT_ENUMERATED_EXTENSIONS,
        )
    }

    pub fn with_extensions<I, S>(
        reader: Arc<dyn LegacyVfsReader>,
        mount_set_id: impl Into<String>,
        uri_prefix: impl Into<String>,
        extensions: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mount_set_id = mount_set_id.into();
        let uri_prefix = uri_prefix.into().trim_end_matches('/').to_string();
        if mount_set_id.is_empty()
            || uri_prefix.is_empty()
            || uri_prefix.contains('\\')
            || !uri_prefix.contains("://")
        {
            bail!("ASTRA_SIGLUS_HOST_VFS_BINDING_INVALID");
        }
        let mut enumerated_extensions = extensions
            .into_iter()
            .map(|value| value.as_ref().trim_start_matches('.').to_ascii_lowercase())
            .collect::<Vec<_>>();
        if enumerated_extensions.iter().any(|value| {
            value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }) {
            bail!("ASTRA_SIGLUS_HOST_VFS_EXTENSION_INVALID");
        }
        enumerated_extensions.sort();
        enumerated_extensions.dedup();
        Ok(Self {
            reader,
            mount_set_id,
            uri_prefix,
            enumerated_extensions,
        })
    }

    fn uri(&self, resource_id: &str) -> Result<String> {
        let resource_id = normalize_resource_id(resource_id)
            .map_err(|_| port_error("ASTRA_SIGLUS_RESOURCE_ID_INVALID"))?;
        Ok(format!("{}/{resource_id}", self.uri_prefix))
    }

    fn resource_id(&self, uri: &str) -> Result<String> {
        let prefix = format!("{}/", self.uri_prefix);
        let resource_id = uri
            .strip_prefix(&prefix)
            .ok_or_else(|| port_error("ASTRA_SIGLUS_ENUMERATED_URI_OUTSIDE_BINDING"))?;
        normalize_resource_id(resource_id)
            .map_err(|_| port_error("ASTRA_SIGLUS_ENUMERATED_RESOURCE_ID_INVALID"))
    }
}

impl HostedResourcePort for SiglusResourcePortV8 {
    fn stat(&self, resource_id: &str) -> Result<HostedResourceStat> {
        let stat = self
            .reader
            .stat_file(&self.mount_set_id, &self.uri(resource_id)?)
            .map_err(|_| port_error("ASTRA_SIGLUS_VFS_STAT_FAILED"))?;
        if stat.revision.0 == 0 {
            bail!("ASTRA_SIGLUS_VFS_REVISION_INVALID");
        }
        Ok(HostedResourceStat {
            revision: stat.revision.0,
            byte_len: stat.len,
        })
    }

    fn list(&self, root: &str, max_entries: usize) -> Result<Vec<HostedResourceEntry>> {
        if max_entries == 0 || max_entries > u32::MAX as usize {
            bail!("ASTRA_SIGLUS_VFS_LIST_BOUNDS");
        }
        let root = if root.is_empty() {
            self.uri_prefix.clone()
        } else {
            self.uri(root)?
        };
        let mut entries = BTreeMap::new();
        for extension in &self.enumerated_extensions {
            let remaining = max_entries.saturating_sub(entries.len());
            if remaining == 0 {
                bail!("ASTRA_SIGLUS_VFS_LIST_TRUNCATED");
            }
            let listed = self
                .reader
                .enumerate_by_extension(&self.mount_set_id, &root, extension, remaining as u32)
                .map_err(|_| port_error("ASTRA_SIGLUS_VFS_LIST_FAILED"))?;
            for file in listed {
                let resource_id = self.resource_id(&file.uri)?;
                if file.stat.revision.0 == 0 {
                    bail!("ASTRA_SIGLUS_VFS_REVISION_INVALID");
                }
                let entry = HostedResourceEntry {
                    resource_id: resource_id.clone(),
                    revision: file.stat.revision.0,
                    byte_len: file.stat.len,
                };
                if entries.insert(resource_id, entry).is_some() {
                    bail!("ASTRA_SIGLUS_VFS_LIST_DUPLICATE");
                }
                if entries.len() > max_entries {
                    bail!("ASTRA_SIGLUS_VFS_LIST_TRUNCATED");
                }
            }
        }
        Ok(entries.into_values().collect())
    }

    fn read_range(
        &self,
        resource_id: &str,
        expected_revision: u64,
        offset: u64,
        length: usize,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        if expected_revision == 0 || length > max_bytes {
            bail!("ASTRA_SIGLUS_VFS_RANGE_BOUNDS");
        }
        let result = self
            .reader
            .read_file_range(
                &self.mount_set_id,
                &self.uri(resource_id)?,
                SourceRevision(expected_revision),
                ByteRange {
                    offset,
                    len: length as u64,
                },
                max_bytes as u64,
            )
            .map_err(|_| port_error("ASTRA_SIGLUS_VFS_RANGE_FAILED"))?;
        if result.revision.0 != expected_revision
            || result.range.offset != offset
            || checked_usize(result.range.len, "ASTRA_SIGLUS_VFS_RANGE_SIZE")? != length
            || result.bytes.len() != length
        {
            bail!("ASTRA_SIGLUS_VFS_RANGE_RESULT_INVALID");
        }
        result
            .bytes
            .try_into_vec()
            .map_err(|_| port_error("ASTRA_SIGLUS_VFS_RANGE_OWNER_NOT_MOVABLE"))
    }
}

pub struct SiglusPrivateMaterialPortV8 {
    host: Arc<dyn LegacyPrivateMaterialHostV8>,
    session: LegacyRuntimeSessionId,
}

impl SiglusPrivateMaterialPortV8 {
    pub fn new(
        host: Arc<dyn LegacyPrivateMaterialHostV8>,
        session: LegacyRuntimeSessionId,
    ) -> Self {
        Self { host, session }
    }
}

impl HostedPrivateMaterialPort for SiglusPrivateMaterialPortV8 {
    fn read_secret(&self, secret_id: &str, exact_len: usize) -> Result<Zeroizing<Vec<u8>>> {
        let exact_len = u32::try_from(exact_len)
            .map_err(|_| port_error("ASTRA_SIGLUS_PRIVATE_MATERIAL_BOUNDS"))?;
        let secret = self
            .host
            .read_private_material(
                &self.session,
                LegacyPrivateMaterialRequestV8 {
                    secret_id: secret_id.to_string(),
                    exact_len,
                },
            )
            .map_err(|_| port_error("ASTRA_SIGLUS_PRIVATE_MATERIAL_READ_FAILED"))?;
        if secret.len() != exact_len as usize {
            bail!("ASTRA_SIGLUS_PRIVATE_MATERIAL_LENGTH_MISMATCH");
        }
        Ok(Zeroizing::new(secret.as_slice().to_vec()))
    }
}

pub struct SiglusSaveStorePortV8 {
    host: Arc<dyn LegacySaveStoreHostV8>,
    session: LegacyRuntimeSessionId,
}

impl SiglusSaveStorePortV8 {
    pub fn new(host: Arc<dyn LegacySaveStoreHostV8>, session: LegacyRuntimeSessionId) -> Self {
        Self { host, session }
    }
}

impl HostedSaveStorePort for SiglusSaveStorePortV8 {
    fn list_slots(&self, max_slots: usize) -> Result<Vec<HostedSaveSlot>> {
        let max_slots =
            u32::try_from(max_slots).map_err(|_| port_error("ASTRA_SIGLUS_SAVE_SLOT_BOUNDS"))?;
        self.host
            .list_slots(&self.session, max_slots)
            .map_err(|_| port_error("ASTRA_SIGLUS_SAVE_LIST_FAILED"))?
            .into_iter()
            .map(|slot: LegacySaveSlotV8| {
                Ok(HostedSaveSlot {
                    slot_id: slot.slot_id,
                    revision: slot.revision,
                    byte_len: slot.byte_len,
                })
            })
            .collect()
    }

    fn read_slot(
        &self,
        slot_id: &str,
        expected_revision: Option<u64>,
        max_bytes: usize,
    ) -> Result<HostedSaveRead> {
        let result: LegacySaveReadResultV8 = self
            .host
            .read_slot(&self.session, slot_id, expected_revision, max_bytes as u64)
            .map_err(|_| port_error("ASTRA_SIGLUS_SAVE_READ_FAILED"))?;
        if result.payload.len() > max_bytes {
            bail!("ASTRA_SIGLUS_SAVE_READ_BOUNDS");
        }
        Ok(HostedSaveRead {
            slot_id: result.slot_id,
            revision: result.revision,
            payload: result.payload.as_slice().to_vec(),
        })
    }

    fn atomic_write_slot(&self, request: HostedSaveWrite) -> Result<HostedSaveWriteResult> {
        if request.payload.len() > request.max_bytes {
            bail!("ASTRA_SIGLUS_SAVE_WRITE_BOUNDS");
        }
        let result: LegacySaveWriteResultV8 = self
            .host
            .atomic_write_slot(
                &self.session,
                LegacySaveWriteRequestV8 {
                    slot_id: request.slot_id,
                    expected_revision: request.expected_revision,
                    max_bytes: request.max_bytes as u64,
                    payload: OwnedByteBuffer::from_vec(request.payload),
                },
            )
            .map_err(|_| port_error("ASTRA_SIGLUS_SAVE_WRITE_FAILED"))?;
        Ok(HostedSaveWriteResult {
            slot_id: result.slot_id,
            revision: result.revision,
            byte_len: result.byte_len,
            verified: result.verified,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use astra_byte_source::{ByteSourceStat, RangeReadResult};
    use astra_emu_family_api::{LegacyProviderError, LegacySecretBufferV8, LegacyVfsListedFile};
    use siglus_hosted::hosted_port::{atomic_write_first_empty_slot, read_exact_private_material};

    use super::*;

    struct MemoryVfs {
        files: BTreeMap<String, (u64, Vec<u8>)>,
    }

    impl LegacyVfsReader for MemoryVfs {
        fn stat_file(
            &self,
            _mount_set_id: &str,
            uri: &str,
        ) -> std::result::Result<ByteSourceStat, LegacyProviderError> {
            let (revision, bytes) = self.files.get(uri).ok_or_else(|| {
                LegacyProviderError::invalid("TEST_MISSING", "fixture resource is missing")
            })?;
            Ok(ByteSourceStat {
                len: bytes.len() as u64,
                revision: SourceRevision(*revision),
            })
        }

        fn read_file_range(
            &self,
            _mount_set_id: &str,
            uri: &str,
            expected_revision: SourceRevision,
            range: ByteRange,
            max_bytes: u64,
        ) -> std::result::Result<RangeReadResult, LegacyProviderError> {
            let stat = self.stat_file("fixture", uri)?;
            if stat.revision != expected_revision || range.len > max_bytes {
                return Err(LegacyProviderError::invalid(
                    "TEST_RANGE",
                    "fixture range validation failed",
                ));
            }
            let bytes = &self.files[uri].1;
            let start = range.offset as usize;
            let end = start + range.len as usize;
            let selected = bytes.get(start..end).ok_or_else(|| {
                LegacyProviderError::invalid("TEST_BOUNDS", "fixture range is out of bounds")
            })?;
            Ok(RangeReadResult {
                range,
                revision: expected_revision,
                bytes: OwnedByteBuffer::from_vec(selected.to_vec()),
            })
        }

        fn enumerate_by_extension(
            &self,
            _mount_set_id: &str,
            root: &str,
            extension_without_dot: &str,
            max_entries: u32,
        ) -> std::result::Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
            let suffix = format!(".{extension_without_dot}");
            let mut listed = self
                .files
                .iter()
                .filter(|(uri, _)| uri.starts_with(root) && uri.ends_with(&suffix))
                .map(|(uri, (revision, bytes))| LegacyVfsListedFile {
                    uri: uri.clone(),
                    stat: ByteSourceStat {
                        len: bytes.len() as u64,
                        revision: SourceRevision(*revision),
                    },
                })
                .collect::<Vec<_>>();
            listed.sort_by(|left, right| left.uri.cmp(&right.uri));
            if listed.len() > max_entries as usize {
                return Err(LegacyProviderError::invalid(
                    "TEST_ENUM_BOUNDS",
                    "fixture enumeration exceeds its bound",
                ));
            }
            Ok(listed)
        }
    }

    struct PrivateHostFixture;

    impl LegacyPrivateMaterialHostV8 for PrivateHostFixture {
        fn read_private_material(
            &self,
            _session: &LegacyRuntimeSessionId,
            request: LegacyPrivateMaterialRequestV8,
        ) -> std::result::Result<LegacySecretBufferV8, LegacyProviderError> {
            Ok(LegacySecretBufferV8::new(vec![
                7;
                request.exact_len as usize
            ]))
        }
    }

    struct SaveHostFixture {
        slots: Mutex<BTreeMap<String, (u64, Vec<u8>)>>,
    }

    impl LegacySaveStoreHostV8 for SaveHostFixture {
        fn list_slots(
            &self,
            _session: &LegacyRuntimeSessionId,
            max_slots: u32,
        ) -> std::result::Result<Vec<LegacySaveSlotV8>, LegacyProviderError> {
            let slots = self.slots.lock().unwrap();
            if slots.len() > max_slots as usize {
                return Err(LegacyProviderError::invalid(
                    "TEST_SAVE_LIST",
                    "fixture has too many slots",
                ));
            }
            Ok(slots
                .iter()
                .map(|(slot_id, (revision, bytes))| LegacySaveSlotV8 {
                    slot_id: slot_id.clone(),
                    revision: *revision,
                    byte_len: bytes.len() as u64,
                })
                .collect())
        }

        fn read_slot(
            &self,
            _session: &LegacyRuntimeSessionId,
            slot_id: &str,
            expected_revision: Option<u64>,
            max_bytes: u64,
        ) -> std::result::Result<LegacySaveReadResultV8, LegacyProviderError> {
            let slots = self.slots.lock().unwrap();
            let (revision, bytes) = slots.get(slot_id).ok_or_else(|| {
                LegacyProviderError::invalid("TEST_SAVE_MISSING", "fixture slot is missing")
            })?;
            if expected_revision.is_some_and(|expected| expected != *revision)
                || bytes.len() as u64 > max_bytes
            {
                return Err(LegacyProviderError::invalid(
                    "TEST_SAVE_READ",
                    "fixture save read validation failed",
                ));
            }
            Ok(LegacySaveReadResultV8 {
                slot_id: slot_id.to_string(),
                revision: *revision,
                payload: OwnedByteBuffer::from_vec(bytes.clone()),
            })
        }

        fn atomic_write_slot(
            &self,
            _session: &LegacyRuntimeSessionId,
            request: LegacySaveWriteRequestV8,
        ) -> std::result::Result<LegacySaveWriteResultV8, LegacyProviderError> {
            let mut slots = self.slots.lock().unwrap();
            let (revision, bytes) = slots.get_mut(&request.slot_id).ok_or_else(|| {
                LegacyProviderError::invalid("TEST_SAVE_MISSING", "fixture slot is missing")
            })?;
            if !bytes.is_empty()
                || request.expected_revision != Some(*revision)
                || request.payload.len() as u64 > request.max_bytes
            {
                return Err(LegacyProviderError::invalid(
                    "TEST_SAVE_WRITE",
                    "fixture save write validation failed",
                ));
            }
            *revision += 1;
            *bytes = request.payload.as_slice().to_vec();
            Ok(LegacySaveWriteResultV8 {
                slot_id: request.slot_id,
                revision: *revision,
                byte_len: bytes.len() as u64,
                verified: true,
            })
        }
    }

    #[test]
    fn v8_resource_adapter_preserves_revision_range_and_logical_identity() {
        let vfs = Arc::new(MemoryVfs {
            files: [
                ("legacy://case/Gameexe.dat".into(), (3, vec![1, 2, 3])),
                ("legacy://case/Scene.pck".into(), (5, vec![4, 5, 6, 7])),
            ]
            .into_iter()
            .collect(),
        });
        let port = SiglusResourcePortV8::with_extensions(
            vfs,
            "mount-main",
            "legacy://case",
            ["dat", "pck"],
        )
        .unwrap();
        assert_eq!(port.stat("Scene.pck").unwrap().revision, 5);
        assert_eq!(port.read_range("Scene.pck", 5, 1, 2, 2).unwrap(), [5, 6]);
        assert!(port.read_range("Scene.pck", 4, 1, 2, 2).is_err());
        let listed = port.list("", 4).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].resource_id, "Gameexe.dat");
        assert_eq!(listed[1].resource_id, "Scene.pck");
    }

    #[test]
    fn v8_private_material_and_save_ports_preserve_bounds() {
        let session = LegacyRuntimeSessionId("siglus-session".into());
        let private =
            SiglusPrivateMaterialPortV8::new(Arc::new(PrivateHostFixture), session.clone());
        let secret = read_exact_private_material(&private, "siglus/scene-key", 16).unwrap();
        assert_eq!(secret.len(), 16);

        let save_host = Arc::new(SaveHostFixture {
            slots: Mutex::new(
                [
                    ("slot-a".into(), (1, vec![9])),
                    ("slot-b".into(), (2, Vec::new())),
                ]
                .into_iter()
                .collect(),
            ),
        });
        let save = SiglusSaveStorePortV8::new(save_host.clone(), session);
        let result = atomic_write_first_empty_slot(&save, vec![1, 2, 3], 2, 16).unwrap();
        assert_eq!(result.slot_id, "slot-b");
        assert_eq!(result.revision, 3);
        assert_eq!(save_host.slots.lock().unwrap()["slot-a"].1, vec![9]);
    }
}
