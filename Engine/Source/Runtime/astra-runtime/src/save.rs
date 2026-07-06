use astra_core::{Hash256, SchemaMigrationRegistry, SchemaVersion};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{RuntimeError, RuntimeSnapshot};

const MAGIC: &[u8; 8] = b"ASTRACT1";
const HEADER_LEN: usize = 32;
const ALIGNMENT: u32 = 8;
const CURRENT: SchemaVersion = SchemaVersion::new(1, 0, 0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveBlob(pub Vec<u8>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveRequest {
    pub minimum_supported_version: SchemaVersion,
}

impl Default for SaveRequest {
    fn default() -> Self {
        Self {
            minimum_supported_version: CURRENT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SectionEntry {
    pub id: String,
    pub schema: String,
    pub version: SchemaVersion,
    pub offset: u64,
    pub length: u64,
    pub hash: Hash256,
    pub codec: SectionCodec,
    pub migration: MigrationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SectionCodec {
    Postcard,
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MigrationPolicy {
    pub minimum_supported_version: SchemaVersion,
    pub current_version: SchemaVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MigrationManifest {
    pub sections: Vec<MigrationManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MigrationManifestEntry {
    pub schema: String,
    pub minimum_supported_version: SchemaVersion,
    pub current_version: SchemaVersion,
}

pub fn write_runtime_save(
    snapshot: RuntimeSnapshot,
    request: SaveRequest,
) -> Result<SaveBlob, RuntimeError> {
    let runtime_payload = postcard::to_allocvec(&snapshot)
        .map_err(|err| RuntimeError::message(format!("encode runtime save: {err}")))?;
    let manifest = MigrationManifest {
        sections: vec![MigrationManifestEntry {
            schema: "runtime.world".to_string(),
            minimum_supported_version: request.minimum_supported_version,
            current_version: CURRENT,
        }],
    };
    let manifest_payload = postcard::to_allocvec(&manifest)
        .map_err(|err| RuntimeError::message(format!("encode migration manifest: {err}")))?;
    write_container(vec![
        PendingSection::new(
            "runtime.world",
            "runtime.world",
            runtime_payload,
            request.minimum_supported_version,
        ),
        PendingSection::new(
            "migration.manifest",
            "migration.manifest",
            manifest_payload,
            CURRENT,
        ),
    ])
}

pub fn read_runtime_save(
    blob: &SaveBlob,
    registry: &SchemaMigrationRegistry,
) -> Result<RuntimeSnapshot, RuntimeError> {
    let sections = read_container(blob)?;
    let manifest_section = sections
        .iter()
        .find(|section| section.entry.id == "migration.manifest")
        .ok_or_else(|| RuntimeError::message("missing migration manifest"))?;
    let manifest: MigrationManifest = postcard::from_bytes(&manifest_section.payload)
        .map_err(|err| RuntimeError::message(format!("decode migration manifest: {err}")))?;
    for entry in manifest.sections {
        registry
            .validate_chain(
                &entry.schema,
                entry.minimum_supported_version,
                entry.current_version,
            )
            .map_err(|err| RuntimeError::message(err.to_string()))?;
    }
    let runtime = sections
        .iter()
        .find(|section| section.entry.id == "runtime.world")
        .ok_or_else(|| RuntimeError::message("missing runtime.world section"))?;
    postcard::from_bytes(&runtime.payload)
        .map_err(|err| RuntimeError::message(format!("decode runtime.world: {err}")))
}

struct PendingSection {
    id: String,
    schema: String,
    payload: Vec<u8>,
    minimum_supported_version: SchemaVersion,
}

impl PendingSection {
    fn new(
        id: impl Into<String>,
        schema: impl Into<String>,
        payload: Vec<u8>,
        minimum_supported_version: SchemaVersion,
    ) -> Self {
        Self {
            id: id.into(),
            schema: schema.into(),
            payload,
            minimum_supported_version,
        }
    }
}

struct DecodedSection {
    entry: SectionEntry,
    payload: Vec<u8>,
}

fn write_container(sections: Vec<PendingSection>) -> Result<SaveBlob, RuntimeError> {
    let mut entries: Vec<SectionEntry> = sections
        .iter()
        .map(|section| SectionEntry {
            id: section.id.clone(),
            schema: section.schema.clone(),
            version: CURRENT,
            offset: 0,
            length: section.payload.len() as u64,
            hash: Hash256::from_sha256(&section.payload),
            codec: SectionCodec::Postcard,
            migration: MigrationPolicy {
                minimum_supported_version: section.minimum_supported_version,
                current_version: CURRENT,
            },
        })
        .collect();

    let mut table = Vec::new();
    for _ in 0..8 {
        table = postcard::to_allocvec(&entries)
            .map_err(|err| RuntimeError::message(format!("encode section table: {err}")))?;
        let mut cursor = align((HEADER_LEN + table.len()) as u64, ALIGNMENT as u64);
        for (entry, section) in entries.iter_mut().zip(&sections) {
            cursor = align(cursor, ALIGNMENT as u64);
            entry.offset = cursor;
            entry.length = section.payload.len() as u64;
            cursor += entry.length;
        }
        let next = postcard::to_allocvec(&entries)
            .map_err(|err| RuntimeError::message(format!("encode section table: {err}")))?;
        if next.len() == table.len() {
            table = next;
            break;
        }
        table = next;
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&CURRENT.major.to_le_bytes());
    bytes.extend_from_slice(&CURRENT.minor.to_le_bytes());
    bytes.extend_from_slice(&CURRENT.patch.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(table.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&ALIGNMENT.to_le_bytes());
    bytes.resize(HEADER_LEN, 0);
    bytes.extend_from_slice(&table);
    for (entry, section) in entries.iter().zip(&sections) {
        bytes.resize(entry.offset as usize, 0);
        bytes.extend_from_slice(&section.payload);
    }
    let footer = Hash256::from_sha256(&bytes);
    bytes.extend_from_slice(footer.as_bytes());
    Ok(SaveBlob(bytes))
}

fn read_container(blob: &SaveBlob) -> Result<Vec<DecodedSection>, RuntimeError> {
    let bytes = &blob.0;
    if bytes.len() < HEADER_LEN + 32 || &bytes[..8] != MAGIC {
        return Err(RuntimeError::message("invalid Astra container magic"));
    }
    let version = SchemaVersion {
        major: u16::from_le_bytes(
            bytes[8..10]
                .try_into()
                .map_err(|_| RuntimeError::message("invalid major version"))?,
        ),
        minor: u16::from_le_bytes(
            bytes[10..12]
                .try_into()
                .map_err(|_| RuntimeError::message("invalid minor version"))?,
        ),
        patch: u16::from_le_bytes(
            bytes[12..14]
                .try_into()
                .map_err(|_| RuntimeError::message("invalid patch version"))?,
        ),
    };
    if version != CURRENT {
        return Err(RuntimeError::message("unsupported Astra container version"));
    }
    let section_count = u32::from_le_bytes(
        bytes[14..18]
            .try_into()
            .map_err(|_| RuntimeError::message("invalid section count"))?,
    ) as usize;
    let alignment = u32::from_le_bytes(
        bytes[26..30]
            .try_into()
            .map_err(|_| RuntimeError::message("invalid alignment"))?,
    );
    if alignment != ALIGNMENT {
        return Err(RuntimeError::message(
            "unsupported Astra container alignment",
        ));
    }
    let stored_footer = Hash256::from_bytes(
        bytes[bytes.len() - 32..]
            .try_into()
            .map_err(|_| RuntimeError::message("invalid footer length"))?,
    );
    let computed = Hash256::from_sha256(&bytes[..bytes.len() - 32]);
    if stored_footer != computed {
        return Err(RuntimeError::message("container footer hash mismatch"));
    }
    let table_len = u64::from_le_bytes(
        bytes[18..26]
            .try_into()
            .map_err(|_| RuntimeError::message("invalid table length"))?,
    ) as usize;
    let table_end = HEADER_LEN + table_len;
    if table_end > bytes.len() - 32 {
        return Err(RuntimeError::message("section table out of bounds"));
    }
    let entries: Vec<SectionEntry> = postcard::from_bytes(&bytes[HEADER_LEN..table_end])
        .map_err(|err| RuntimeError::message(format!("decode section table: {err}")))?;
    if entries.len() != section_count {
        return Err(RuntimeError::message("section count does not match table"));
    }
    let mut decoded = Vec::new();
    for entry in entries {
        if entry.offset % ALIGNMENT as u64 != 0 {
            return Err(RuntimeError::message(format!(
                "section {} is not aligned",
                entry.id
            )));
        }
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| RuntimeError::message("section length overflow"))?;
        if end > bytes.len() - 32 {
            return Err(RuntimeError::message(format!(
                "section {} out of bounds",
                entry.id
            )));
        }
        let payload = bytes[start..end].to_vec();
        if Hash256::from_sha256(&payload) != entry.hash {
            return Err(RuntimeError::message(format!(
                "section {} hash mismatch",
                entry.id
            )));
        }
        decoded.push(DecodedSection { entry, payload });
    }
    Ok(decoded)
}

fn align(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}
