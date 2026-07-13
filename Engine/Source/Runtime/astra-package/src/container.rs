use astra_core::{Hash256, SchemaVersion};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

const MAGIC: &[u8; 8] = b"ASTRACT2";
const HEADER_LEN: usize = 40;
const FOOTER_LEN: usize = 32;
const ALIGNMENT: u32 = 8;
const MAX_SECTION_COUNT: usize = 65_536;
const MAX_TABLE_LEN: usize = 64 * 1024 * 1024;
const MAX_DECODED_SECTION_LEN: u64 = 1024 * 1024 * 1024;
pub const CURRENT_CONTAINER_VERSION: SchemaVersion = SchemaVersion::new(1, 0, 0);

#[derive(Debug, Error)]
pub enum ContainerError {
    #[error("{0}")]
    Message(String),
    #[error("postcard encode failed: {0}")]
    PostcardEncode(String),
    #[error("postcard decode failed: {0}")]
    PostcardDecode(String),
    #[error("zstd codec failed: {0}")]
    Zstd(String),
    #[error("crypto provider failed: {0}")]
    Crypto(String),
}

impl ContainerError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContainerKind {
    Save,
    Package,
}

impl ContainerKind {
    fn tag(self) -> u8 {
        match self {
            Self::Save => 1,
            Self::Package => 2,
        }
    }

    fn from_tag(value: u8) -> Result<Self, ContainerError> {
        match value {
            1 => Ok(Self::Save),
            2 => Ok(Self::Package),
            _ => Err(ContainerError::message("unsupported Astra container kind")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainerBlob(Vec<u8>);

impl ContainerBlob {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SectionEntry {
    pub id: String,
    pub schema: String,
    pub version: SchemaVersion,
    pub offset: u64,
    pub length: u64,
    pub decoded_length: u64,
    pub hash: Hash256,
    pub stored_hash: Hash256,
    pub codec: SectionCodec,
    #[serde(default)]
    pub encryption: Option<EncryptionDescriptor>,
    pub migration: MigrationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SectionCodec {
    Postcard,
    Raw,
    Zstd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MigrationPolicy {
    pub minimum_supported_version: SchemaVersion,
    pub current_version: SchemaVersion,
}

impl MigrationPolicy {
    pub fn current() -> Self {
        Self {
            minimum_supported_version: CURRENT_CONTAINER_VERSION,
            current_version: CURRENT_CONTAINER_VERSION,
        }
    }

    pub fn from_minimum(minimum_supported_version: SchemaVersion) -> Self {
        Self {
            minimum_supported_version,
            current_version: CURRENT_CONTAINER_VERSION,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EncryptionDescriptor {
    pub provider_id: String,
    pub method: String,
    pub key_ref: ExternalKeyRef,
    pub aad_hash: Hash256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalKeyRef {
    pub uri: String,
}

pub trait ContainerCryptoProvider {
    fn provider_id(&self) -> &str;

    fn decrypt(
        &self,
        descriptor: &EncryptionDescriptor,
        stored_payload: &[u8],
    ) -> Result<Vec<u8>, ContainerError>;

    fn encrypt(
        &self,
        descriptor: &EncryptionDescriptor,
        plain_payload: &[u8],
    ) -> Result<Vec<u8>, ContainerError>;
}

pub struct NoCryptoProvider;

impl ContainerCryptoProvider for NoCryptoProvider {
    fn provider_id(&self) -> &str {
        "astra.crypto.none"
    }

    fn decrypt(
        &self,
        descriptor: &EncryptionDescriptor,
        _stored_payload: &[u8],
    ) -> Result<Vec<u8>, ContainerError> {
        Err(ContainerError::Crypto(format!(
            "crypto provider {} is not available",
            descriptor.provider_id
        )))
    }

    fn encrypt(
        &self,
        descriptor: &EncryptionDescriptor,
        _plain_payload: &[u8],
    ) -> Result<Vec<u8>, ContainerError> {
        Err(ContainerError::Crypto(format!(
            "crypto provider {} is not available",
            descriptor.provider_id
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionPayload {
    pub id: String,
    pub schema: String,
    pub version: SchemaVersion,
    pub codec: SectionCodec,
    pub payload: Vec<u8>,
    pub migration: MigrationPolicy,
    pub encryption: Option<EncryptionDescriptor>,
}

impl SectionPayload {
    pub fn new(
        id: impl Into<String>,
        schema: impl Into<String>,
        version: SchemaVersion,
        codec: SectionCodec,
        payload: Vec<u8>,
        migration: MigrationPolicy,
    ) -> Self {
        Self {
            id: id.into(),
            schema: schema.into(),
            version,
            codec,
            payload,
            migration,
            encryption: None,
        }
    }

    pub fn raw(id: impl Into<String>, schema: impl Into<String>, payload: Vec<u8>) -> Self {
        Self::new(
            id,
            schema,
            CURRENT_CONTAINER_VERSION,
            SectionCodec::Raw,
            payload,
            MigrationPolicy::current(),
        )
    }

    pub fn postcard<T: Serialize>(
        id: impl Into<String>,
        schema: impl Into<String>,
        value: &T,
    ) -> Result<Self, ContainerError> {
        let payload = postcard::to_allocvec(value)
            .map_err(|err| ContainerError::PostcardEncode(err.to_string()))?;
        Ok(Self::new(
            id,
            schema,
            CURRENT_CONTAINER_VERSION,
            SectionCodec::Postcard,
            payload,
            MigrationPolicy::current(),
        ))
    }

    pub fn with_encryption_descriptor(mut self, descriptor: EncryptionDescriptor) -> Self {
        self.encryption = Some(descriptor);
        self
    }
}

#[derive(Debug, Clone)]
pub struct AstraContainerBuilder {
    kind: ContainerKind,
    sections: Vec<SectionPayload>,
}

impl AstraContainerBuilder {
    pub fn new(kind: ContainerKind) -> Self {
        Self {
            kind,
            sections: Vec::new(),
        }
    }

    pub fn add_section(mut self, section: SectionPayload) -> Self {
        self.sections.push(section);
        self
    }

    pub fn write(self) -> Result<ContainerBlob, ContainerError> {
        if self.sections.is_empty() {
            return Err(ContainerError::message("Astra container requires sections"));
        }
        write_container(self.kind, self.sections, None)
    }

    pub fn write_with_crypto(
        self,
        crypto: &dyn ContainerCryptoProvider,
    ) -> Result<ContainerBlob, ContainerError> {
        if self.sections.is_empty() {
            return Err(ContainerError::message("Astra container requires sections"));
        }
        write_container(self.kind, self.sections, Some(crypto))
    }
}

#[derive(Debug, Clone)]
pub struct AstraContainerReader {
    kind: ContainerKind,
    entries: Vec<SectionEntry>,
    bytes: Vec<u8>,
}

impl AstraContainerReader {
    pub fn new(bytes: &[u8]) -> Result<Self, ContainerError> {
        read_container(bytes)
    }

    pub fn kind(&self) -> ContainerKind {
        self.kind
    }

    pub fn entries(&self) -> &[SectionEntry] {
        &self.entries
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
            return Err(ContainerError::Crypto(format!(
                "section {id} requires crypto provider"
            )));
        }
        let stored = self.stored_payload(entry)?;
        decode_payload(entry, stored)
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
            crypto.decrypt(descriptor, stored)?
        } else {
            stored.to_vec()
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

    fn stored_payload(&self, entry: &SectionEntry) -> Result<&[u8], ContainerError> {
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| ContainerError::message("section length overflow"))?;
        self.bytes
            .get(start..end)
            .ok_or_else(|| ContainerError::message("section range out of bounds"))
    }
}

fn decode_postcard_payload<T: DeserializeOwned>(
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

fn write_container(
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
    let footer = Hash256::from_sha256(&bytes);
    bytes.extend_from_slice(footer.as_bytes());
    Ok(ContainerBlob(bytes))
}

fn read_container(bytes: &[u8]) -> Result<AstraContainerReader, ContainerError> {
    if bytes.len() < HEADER_LEN + FOOTER_LEN || &bytes[..8] != MAGIC {
        return Err(ContainerError::message("invalid Astra container magic"));
    }
    let version = SchemaVersion {
        major: u16::from_le_bytes(bytes[8..10].try_into().expect("header major")),
        minor: u16::from_le_bytes(bytes[10..12].try_into().expect("header minor")),
        patch: u16::from_le_bytes(bytes[12..14].try_into().expect("header patch")),
    };
    if version != CURRENT_CONTAINER_VERSION {
        return Err(ContainerError::message(
            "unsupported Astra container version",
        ));
    }
    let kind = ContainerKind::from_tag(bytes[14])?;
    let section_count =
        u32::from_le_bytes(bytes[16..20].try_into().expect("section count")) as usize;
    let table_len = u64::from_le_bytes(bytes[20..28].try_into().expect("table len")) as usize;
    if section_count == 0 || section_count > MAX_SECTION_COUNT {
        return Err(ContainerError::message(
            "container section count is outside the supported bound",
        ));
    }
    if table_len == 0 || table_len > MAX_TABLE_LEN {
        return Err(ContainerError::message(
            "container section table exceeds the supported bound",
        ));
    }
    let alignment = u32::from_le_bytes(bytes[28..32].try_into().expect("alignment"));
    if alignment != ALIGNMENT {
        return Err(ContainerError::message(
            "unsupported Astra container alignment",
        ));
    }
    let footer_start = bytes.len() - FOOTER_LEN;
    let stored_footer = Hash256::from_bytes(
        bytes[footer_start..]
            .try_into()
            .map_err(|_| ContainerError::message("invalid footer length"))?,
    );
    let computed_footer = Hash256::from_sha256(&bytes[..footer_start]);
    if stored_footer != computed_footer {
        return Err(ContainerError::message("container footer hash mismatch"));
    }
    let table_end = HEADER_LEN
        .checked_add(table_len)
        .ok_or_else(|| ContainerError::message("section table overflow"))?;
    if table_end > footer_start {
        return Err(ContainerError::message("section table out of bounds"));
    }
    let entries: Vec<SectionEntry> =
        postcard::from_bytes(&bytes[HEADER_LEN..table_end]).map_err(|err| {
            ContainerError::PostcardDecode(format!(
                "{}; table_len={table_len}; prefix={}",
                err,
                hex_prefix(&bytes[HEADER_LEN..table_end])
            ))
        })?;
    if entries.len() != section_count {
        return Err(ContainerError::message(
            "section count does not match table",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut ranges = Vec::with_capacity(entries.len());
    for entry in &entries {
        validate_section_descriptor(
            &entry.id,
            &entry.schema,
            entry.decoded_length,
            &entry.migration,
        )?;
        if !ids.insert(entry.id.as_str()) {
            return Err(ContainerError::message(format!(
                "duplicate section id {}",
                entry.id
            )));
        }
        if entry.offset % ALIGNMENT as u64 != 0 {
            return Err(ContainerError::message(format!(
                "section {} is not aligned",
                entry.id
            )));
        }
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| ContainerError::message("section length overflow"))?;
        if end > footer_start {
            return Err(ContainerError::message(format!(
                "section {} out of bounds",
                entry.id
            )));
        }
        if start < table_end {
            return Err(ContainerError::message(format!(
                "section {} overlaps the container header or table",
                entry.id
            )));
        }
        ranges.push((start, end, entry.id.as_str()));
        let stored = &bytes[start..end];
        if Hash256::from_sha256(stored) != entry.stored_hash {
            return Err(ContainerError::message(format!(
                "section {} stored hash mismatch",
                entry.id
            )));
        }
        if let Some(descriptor) = &entry.encryption {
            let expected_aad = section_aad_hash_from_entry(kind, entry)?;
            if descriptor.aad_hash != expected_aad {
                return Err(ContainerError::Crypto(format!(
                    "section {} encryption AAD hash mismatch",
                    entry.id
                )));
            }
        }
        if entry.encryption.is_none() {
            let decoded = decode_payload(entry, stored)?;
            if Hash256::from_sha256(&decoded) != entry.hash {
                return Err(ContainerError::message(format!(
                    "section {} decoded hash mismatch",
                    entry.id
                )));
            }
        }
    }
    ranges.sort_unstable_by_key(|(start, _, _)| *start);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(ContainerError::message(format!(
                "section {} overlaps section {}",
                pair[0].2, pair[1].2
            )));
        }
    }
    Ok(AstraContainerReader {
        kind,
        entries,
        bytes: bytes.to_vec(),
    })
}

#[derive(Serialize)]
struct SectionAad<'a> {
    kind: ContainerKind,
    id: &'a str,
    schema: &'a str,
    version: SchemaVersion,
    codec: &'a SectionCodec,
    decoded_length: u64,
    decoded_hash: Hash256,
    migration: &'a MigrationPolicy,
}

pub fn section_aad_hash(
    kind: ContainerKind,
    section: &SectionPayload,
) -> Result<Hash256, ContainerError> {
    section_aad_hash_from_section(kind, section, Hash256::from_sha256(&section.payload))
}

fn section_aad_hash_from_section(
    kind: ContainerKind,
    section: &SectionPayload,
    decoded_hash: Hash256,
) -> Result<Hash256, ContainerError> {
    hash_section_aad(SectionAad {
        kind,
        id: &section.id,
        schema: &section.schema,
        version: section.version,
        codec: &section.codec,
        decoded_length: section.payload.len() as u64,
        decoded_hash,
        migration: &section.migration,
    })
}

fn section_aad_hash_from_entry(
    kind: ContainerKind,
    entry: &SectionEntry,
) -> Result<Hash256, ContainerError> {
    hash_section_aad(SectionAad {
        kind,
        id: &entry.id,
        schema: &entry.schema,
        version: entry.version,
        codec: &entry.codec,
        decoded_length: entry.decoded_length,
        decoded_hash: entry.hash,
        migration: &entry.migration,
    })
}

fn hash_section_aad(aad: SectionAad<'_>) -> Result<Hash256, ContainerError> {
    let bytes = postcard::to_allocvec(&aad)
        .map_err(|err| ContainerError::PostcardEncode(err.to_string()))?;
    Ok(Hash256::from_sha256(&bytes))
}

fn encode_payload(codec: &SectionCodec, payload: &[u8]) -> Result<Vec<u8>, ContainerError> {
    match codec {
        SectionCodec::Postcard | SectionCodec::Raw => Ok(payload.to_vec()),
        SectionCodec::Zstd => {
            zstd::bulk::compress(payload, 3).map_err(|err| ContainerError::Zstd(err.to_string()))
        }
    }
}

fn decode_payload(entry: &SectionEntry, stored: &[u8]) -> Result<Vec<u8>, ContainerError> {
    let decoded_len = usize::try_from(entry.decoded_length)
        .map_err(|_| ContainerError::message("section decoded length exceeds address space"))?;
    let decoded = match entry.codec {
        SectionCodec::Postcard | SectionCodec::Raw => stored.to_vec(),
        SectionCodec::Zstd => zstd::bulk::decompress(stored, decoded_len)
            .map_err(|err| ContainerError::Zstd(err.to_string()))?,
    };
    if decoded.len() as u64 != entry.decoded_length {
        return Err(ContainerError::message(format!(
            "section {} decoded length mismatch",
            entry.id
        )));
    }
    if Hash256::from_sha256(&decoded) != entry.hash {
        return Err(ContainerError::message(format!(
            "section {} decoded hash mismatch",
            entry.id
        )));
    }
    Ok(decoded)
}

fn validate_section_payloads(sections: &[SectionPayload]) -> Result<(), ContainerError> {
    if sections.len() > MAX_SECTION_COUNT {
        return Err(ContainerError::message(
            "container section count exceeds the supported bound",
        ));
    }
    let mut ids = BTreeSet::new();
    for section in sections {
        validate_section_descriptor(
            &section.id,
            &section.schema,
            section.payload.len() as u64,
            &section.migration,
        )?;
        if !ids.insert(section.id.as_str()) {
            return Err(ContainerError::message(format!(
                "duplicate section id {}",
                section.id
            )));
        }
    }
    Ok(())
}

fn validate_section_descriptor(
    id: &str,
    schema: &str,
    decoded_length: u64,
    migration: &MigrationPolicy,
) -> Result<(), ContainerError> {
    if !is_safe_section_symbol(id) {
        return Err(ContainerError::message("section id is empty or invalid"));
    }
    if !is_safe_section_symbol(schema) {
        return Err(ContainerError::message(
            "section schema is empty or invalid",
        ));
    }
    if decoded_length > MAX_DECODED_SECTION_LEN {
        return Err(ContainerError::message(
            "section decoded length exceeds the supported bound",
        ));
    }
    if migration.minimum_supported_version > migration.current_version
        || migration.current_version.major == 0
    {
        return Err(ContainerError::message(
            "section migration policy is invalid for this container version",
        ));
    }
    Ok(())
}

fn is_safe_section_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn align(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}
