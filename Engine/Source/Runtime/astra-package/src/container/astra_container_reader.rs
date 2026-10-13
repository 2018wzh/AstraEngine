use super::*;

impl AstraContainerReader {
    pub fn new(bytes: &[u8]) -> Result<Self, ContainerError> {
        read_container(bytes)
    }

    pub fn open_source(
        source: Arc<dyn BoundedByteSource>,
        expected_content_root: Hash256,
    ) -> Result<Self, ContainerError> {
        read_container_source(source, expected_content_root)
    }

    pub fn open_storage_verified_source(
        source: Arc<dyn BoundedByteSource>,
        expected_storage_hash: Hash256,
    ) -> Result<Self, ContainerError> {
        let (reader, actual_storage_hash) = Self::open_storage_audited_source(source)?;
        if actual_storage_hash != expected_storage_hash {
            return Err(ContainerError::message(
                "container storage hash does not match launch identity",
            ));
        }
        Ok(reader)
    }

    pub fn open_storage_audited_source(
        source: Arc<dyn BoundedByteSource>,
    ) -> Result<(Self, Hash256), ContainerError> {
        let before = source
            .stat()
            .map_err(|error| ContainerError::message(error.to_string()))?;
        let actual_storage_hash = audit_source(source.as_ref())
            .map_err(|error| ContainerError::message(error.to_string()))?;
        let after = source
            .stat()
            .map_err(|error| ContainerError::message(error.to_string()))?;
        if before != after {
            return Err(ContainerError::message(
                "container byte source changed during storage verification",
            ));
        }
        if after.len < FOOTER_LEN as u64 {
            return Err(ContainerError::message("invalid Astra container length"));
        }
        let footer = read_source_range(
            source.as_ref(),
            after,
            ByteRange {
                offset: after.len - FOOTER_LEN as u64,
                len: FOOTER_LEN as u64,
            },
        )?;
        if &footer[..8] != FOOTER_MAGIC {
            return Err(ContainerError::message(
                "invalid Astra container v2 footer magic",
            ));
        }
        let expected_content_root =
            Hash256::from_bytes(footer[96..128].try_into().expect("content root"));
        let reader = read_container_source(source, expected_content_root)?;
        Ok((reader, actual_storage_hash))
    }

    pub fn with_crypto_provider(mut self, crypto: Arc<dyn ContainerCryptoProvider>) -> Self {
        self.crypto = Some(crypto);
        self
    }

    pub fn kind(&self) -> ContainerKind {
        self.kind
    }

    pub fn entries(&self) -> &[SectionEntry] {
        &self.entries
    }

    pub fn content_root(&self) -> Hash256 {
        self.content_root
    }

    pub fn section_entry(&self, id: &str) -> Option<&SectionEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn has_section(&self, id: &str) -> bool {
        self.section_entry(id).is_some()
    }

    pub fn read_bounded(&self, id: &str, max_len: usize) -> Result<Vec<u8>, ContainerError> {
        let entry = self
            .section_entry(id)
            .ok_or_else(|| ContainerError::message(format!("missing section {id}")))?;
        if entry.decoded_length as usize > max_len {
            return Err(ContainerError::message(format!(
                "section {id} exceeds bound"
            )));
        }
        self.read_section(id)
    }

    pub fn read_bounded_with_crypto(
        &self,
        id: &str,
        max_len: usize,
        crypto: &dyn ContainerCryptoProvider,
    ) -> Result<Vec<u8>, ContainerError> {
        let entry = self
            .section_entry(id)
            .ok_or_else(|| ContainerError::message(format!("missing section {id}")))?;
        if entry.decoded_length as usize > max_len {
            return Err(ContainerError::message(format!(
                "section {id} exceeds bound"
            )));
        }
        self.read_section_with_crypto(id, crypto)
    }

    pub fn read_section(&self, id: &str) -> Result<Vec<u8>, ContainerError> {
        let entry = self
            .section_entry(id)
            .ok_or_else(|| ContainerError::message(format!("missing section {id}")))?;
        if entry.encryption.is_some() {
            let crypto = self.crypto.as_deref().ok_or_else(|| {
                ContainerError::Crypto(format!("section {id} requires crypto provider"))
            })?;
            return self.read_section_with_crypto(id, crypto);
        }
        let stored = self.stored_payload(entry)?;
        decode_payload(entry, &stored)
    }

    pub fn read_section_with_crypto(
        &self,
        id: &str,
        crypto: &dyn ContainerCryptoProvider,
    ) -> Result<Vec<u8>, ContainerError> {
        let entry = self
            .section_entry(id)
            .ok_or_else(|| ContainerError::message(format!("missing section {id}")))?;
        let stored = self.stored_payload(entry)?;
        let decrypted = if let Some(descriptor) = &entry.encryption {
            if crypto.provider_id() != descriptor.provider_id {
                return Err(ContainerError::Crypto(format!(
                    "crypto provider {} does not match section provider {}",
                    crypto.provider_id(),
                    descriptor.provider_id
                )));
            }
            crypto.decrypt(descriptor, &stored)?
        } else {
            stored
        };
        decode_payload(entry, &decrypted)
    }

    pub fn decode_postcard<T: DeserializeOwned>(&self, id: &str) -> Result<T, ContainerError> {
        let bytes = self.read_section(id)?;
        decode_postcard_payload(id, &bytes)
    }

    pub fn decode_postcard_with_crypto<T: DeserializeOwned>(
        &self,
        id: &str,
        crypto: &dyn ContainerCryptoProvider,
    ) -> Result<T, ContainerError> {
        let bytes = self.read_section_with_crypto(id, crypto)?;
        decode_postcard_payload(id, &bytes)
    }

    pub(super) fn stored_payload(&self, entry: &SectionEntry) -> Result<Vec<u8>, ContainerError> {
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| ContainerError::message("section length overflow"))?;
        let stored = if let Some((source, stat)) = &self.source {
            read_source_range(
                source.as_ref(),
                *stat,
                ByteRange {
                    offset: entry.offset,
                    len: entry.length,
                },
            )?
        } else {
            self.bytes
                .get(start..end)
                .ok_or_else(|| ContainerError::message("section range out of bounds"))?
                .to_vec()
        };
        if Hash256::from_sha256(&stored) != entry.stored_hash {
            return Err(ContainerError::message(format!(
                "section {} stored hash mismatch",
                entry.id
            )));
        }
        Ok(stored)
    }
}

pub(super) fn decode_postcard_payload<T: DeserializeOwned>(
    id: &str,
    bytes: &[u8],
) -> Result<T, ContainerError> {
    postcard::from_bytes(bytes).map_err(|err| {
        ContainerError::PostcardDecode(format!(
            "{}; section={id}; len={}; prefix={}",
            err,
            bytes.len(),
            hex_prefix(bytes)
        ))
    })
}

pub(super) fn write_container(
    kind: ContainerKind,
    sections: Vec<SectionPayload>,
    crypto: Option<&dyn ContainerCryptoProvider>,
) -> Result<ContainerBlob, ContainerError> {
    validate_section_payloads(&sections)?;
    let mut stored_payloads = Vec::with_capacity(sections.len());
    let mut entries = Vec::with_capacity(sections.len());
    for section in &sections {
        let encoded = encode_payload(&section.codec, &section.payload)?;
        let decoded_hash = Hash256::from_sha256(&section.payload);
        let expected_aad = section_aad_hash_from_section(kind, section, decoded_hash)?;
        let stored = if let Some(descriptor) = &section.encryption {
            if descriptor.aad_hash != expected_aad {
                return Err(ContainerError::Crypto(format!(
                    "section {} encryption AAD hash does not match section metadata",
                    section.id
                )));
            }
            let provider = crypto.ok_or_else(|| {
                ContainerError::Crypto(format!(
                    "section {} requires crypto provider {}",
                    section.id, descriptor.provider_id
                ))
            })?;
            if provider.provider_id() != descriptor.provider_id {
                return Err(ContainerError::Crypto(format!(
                    "crypto provider {} does not match section provider {}",
                    provider.provider_id(),
                    descriptor.provider_id
                )));
            }
            provider.encrypt(descriptor, &encoded)?
        } else {
            encoded
        };
        entries.push(SectionEntry {
            id: section.id.clone(),
            schema: section.schema.clone(),
            version: section.version,
            offset: 0,
            length: stored.len() as u64,
            decoded_length: section.payload.len() as u64,
            hash: decoded_hash,
            stored_hash: Hash256::from_sha256(&stored),
            codec: section.codec.clone(),
            encryption: section.encryption.clone(),
            migration: section.migration.clone(),
        });
        stored_payloads.push(stored);
    }

    let mut table = Vec::new();
    for _ in 0..16 {
        table = postcard::to_allocvec(&entries)
            .map_err(|err| ContainerError::PostcardEncode(err.to_string()))?;
        let mut cursor = align((HEADER_LEN + table.len()) as u64, ALIGNMENT as u64);
        for (entry, stored) in entries.iter_mut().zip(&stored_payloads) {
            cursor = align(cursor, ALIGNMENT as u64);
            entry.offset = cursor;
            entry.length = stored.len() as u64;
            cursor += entry.length;
        }
        let next = postcard::to_allocvec(&entries)
            .map_err(|err| ContainerError::PostcardEncode(err.to_string()))?;
        if next.len() == table.len() {
            table = next;
            break;
        }
        table = next;
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&CURRENT_CONTAINER_VERSION.major.to_le_bytes());
    bytes.extend_from_slice(&CURRENT_CONTAINER_VERSION.minor.to_le_bytes());
    bytes.extend_from_slice(&CURRENT_CONTAINER_VERSION.patch.to_le_bytes());
    bytes.push(kind.tag());
    bytes.push(0);
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(table.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&ALIGNMENT.to_le_bytes());
    bytes.resize(HEADER_LEN, 0);
    bytes.extend_from_slice(&table);
    for (entry, stored) in entries.iter().zip(stored_payloads) {
        bytes.resize(entry.offset as usize, 0);
        bytes.extend_from_slice(&stored);
    }
    let header_hash = Hash256::from_sha256(&bytes[..HEADER_LEN]);
    let table_hash = Hash256::from_sha256(&table);
    let content_root = section_merkle_root(&entries)?;
    let final_len = bytes
        .len()
        .checked_add(FOOTER_LEN)
        .ok_or_else(|| ContainerError::message("container file length overflow"))?
        as u64;
    bytes.extend_from_slice(FOOTER_MAGIC);
    bytes.extend_from_slice(&final_len.to_le_bytes());
    bytes.push(kind.tag());
    bytes.extend_from_slice(&[0; 7]);
    bytes.extend_from_slice(&CURRENT_CONTAINER_VERSION.major.to_le_bytes());
    bytes.extend_from_slice(&CURRENT_CONTAINER_VERSION.minor.to_le_bytes());
    bytes.extend_from_slice(&CURRENT_CONTAINER_VERSION.patch.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(header_hash.as_bytes());
    bytes.extend_from_slice(table_hash.as_bytes());
    bytes.extend_from_slice(content_root.as_bytes());
    debug_assert_eq!(bytes.len() as u64, final_len);
    Ok(ContainerBlob(bytes))
}
