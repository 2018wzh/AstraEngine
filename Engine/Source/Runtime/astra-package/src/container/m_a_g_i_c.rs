use super::*;

pub(super) const MAGIC: &[u8; 8] = b"ASTRACT2";

pub(super) const FOOTER_MAGIC: &[u8; 8] = b"ASTRAF2\0";

pub(super) const HEADER_LEN: usize = 40;

pub(super) const FOOTER_LEN: usize = 128;

pub(super) const ALIGNMENT: u32 = 8;

pub(super) const MAX_SECTION_COUNT: usize = 65_536;

pub(super) const MAX_TABLE_LEN: usize = 64 * 1024 * 1024;

pub(super) const MAX_DECODED_SECTION_LEN: u64 = 1024 * 1024 * 1024;

pub const CURRENT_CONTAINER_VERSION: SchemaVersion = SchemaVersion::new(2, 0, 0);

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
    pub(super) fn tag(self) -> u8 {
        match self {
            Self::Save => 1,
            Self::Package => 2,
        }
    }

    pub(super) fn from_tag(value: u8) -> Result<Self, ContainerError> {
        match value {
            1 => Ok(Self::Save),
            2 => Ok(Self::Package),
            _ => Err(ContainerError::message("unsupported Astra container kind")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainerBlob(pub(super) Vec<u8>);

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

pub trait ContainerCryptoProvider: Send + Sync {
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
    pub(super) kind: ContainerKind,
    pub(super) sections: Vec<SectionPayload>,
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

pub struct AstraContainerReader {
    pub(super) kind: ContainerKind,
    pub(super) entries: Vec<SectionEntry>,
    pub(super) bytes: Arc<[u8]>,
    pub(super) source: Option<(Arc<dyn BoundedByteSource>, ByteSourceStat)>,
    pub(super) content_root: Hash256,
    pub(super) crypto: Option<Arc<dyn ContainerCryptoProvider>>,
}

impl Clone for AstraContainerReader {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            entries: self.entries.clone(),
            bytes: self.bytes.clone(),
            source: self.source.clone(),
            content_root: self.content_root,
            crypto: self.crypto.clone(),
        }
    }
}

impl std::fmt::Debug for AstraContainerReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AstraContainerReader")
            .field("kind", &self.kind)
            .field("entries", &self.entries)
            .field("content_root", &self.content_root)
            .finish_non_exhaustive()
    }
}
