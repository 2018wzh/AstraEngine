use std::collections::BTreeSet;

use astra_core::{SchemaMigrationRegistry, SchemaVersion};
use astra_package::{
    AstraContainerBuilder, AstraContainerReader, ContainerBlob, ContainerKind, MigrationPolicy,
    SectionCodec, SectionPayload, CURRENT_CONTAINER_VERSION,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{RuntimeError, RuntimeSnapshot};

const CURRENT: SchemaVersion = CURRENT_CONTAINER_VERSION;
const RUNTIME_WORLD_CURRENT: SchemaVersion = SchemaVersion::new(2, 0, 0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveBlob(pub Vec<u8>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveRequest {
    pub minimum_supported_version: SchemaVersion,
}

impl Default for SaveRequest {
    fn default() -> Self {
        Self {
            minimum_supported_version: RUNTIME_WORLD_CURRENT,
        }
    }
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
    write_runtime_save_with_sections(snapshot, request, Vec::new())
}

pub fn write_runtime_save_with_sections(
    snapshot: RuntimeSnapshot,
    request: SaveRequest,
    extra_sections: Vec<SectionPayload>,
) -> Result<SaveBlob, RuntimeError> {
    if request.minimum_supported_version != RUNTIME_WORLD_CURRENT {
        return Err(RuntimeError::message(
            "ASTRA_RUNTIME_SAVE_WORLD_VERSION_UNSUPPORTED",
        ));
    }
    let runtime_payload = postcard::to_allocvec(&snapshot)
        .map_err(|err| RuntimeError::message(format!("encode runtime save: {err}")))?;
    let mut section_ids = BTreeSet::new();
    section_ids.insert("runtime.world".to_string());
    section_ids.insert("migration.manifest".to_string());

    let mut migration_entries = vec![MigrationManifestEntry {
        schema: "runtime.world".to_string(),
        minimum_supported_version: RUNTIME_WORLD_CURRENT,
        current_version: RUNTIME_WORLD_CURRENT,
    }];
    for section in &extra_sections {
        if !section_ids.insert(section.id.clone()) {
            return Err(RuntimeError::message(format!(
                "duplicate save section {}",
                section.id
            )));
        }
        migration_entries.push(MigrationManifestEntry {
            schema: section.schema.clone(),
            minimum_supported_version: section.migration.minimum_supported_version,
            current_version: section.migration.current_version,
        });
    }
    let manifest = MigrationManifest {
        sections: migration_entries,
    };
    let manifest_payload = postcard::to_allocvec(&manifest)
        .map_err(|err| RuntimeError::message(format!("encode migration manifest: {err}")))?;

    let mut builder =
        AstraContainerBuilder::new(ContainerKind::Save).add_section(SectionPayload::new(
            "runtime.world",
            "runtime.world",
            RUNTIME_WORLD_CURRENT,
            SectionCodec::Postcard,
            runtime_payload,
            MigrationPolicy::current(),
        ));
    for section in extra_sections {
        builder = builder.add_section(section);
    }
    let blob = builder
        .add_section(SectionPayload::new(
            "migration.manifest",
            "migration.manifest",
            CURRENT,
            SectionCodec::Postcard,
            manifest_payload,
            MigrationPolicy::current(),
        ))
        .write()
        .map_err(|err| RuntimeError::message(err.to_string()))?;
    Ok(SaveBlob(blob.into_bytes()))
}

pub fn read_runtime_save(
    blob: &SaveBlob,
    registry: &SchemaMigrationRegistry,
) -> Result<RuntimeSnapshot, RuntimeError> {
    let container = ContainerBlob::new(blob.0.clone());
    let reader = AstraContainerReader::new(container.as_bytes())
        .map_err(|err| RuntimeError::message(err.to_string()))?;
    if reader.kind() != ContainerKind::Save {
        return Err(RuntimeError::message("container is not a runtime save"));
    }
    validate_runtime_world_contract(&reader)?;
    let manifest: MigrationManifest = reader
        .decode_postcard("migration.manifest")
        .map_err(|err| RuntimeError::message(err.to_string()))?;
    for entry in manifest.sections {
        registry
            .validate_chain(
                &entry.schema,
                entry.minimum_supported_version,
                entry.current_version,
            )
            .map_err(|err| RuntimeError::message(err.to_string()))?;
    }
    reader
        .decode_postcard("runtime.world")
        .map_err(|err| RuntimeError::message(format!("decode runtime.world: {err}")))
}

fn validate_runtime_world_contract(reader: &AstraContainerReader) -> Result<(), RuntimeError> {
    let entry = reader
        .entries()
        .iter()
        .find(|entry| entry.id == "runtime.world")
        .ok_or_else(|| RuntimeError::message("ASTRA_RUNTIME_SAVE_WORLD_MISSING"))?;
    if entry.schema != "runtime.world" || entry.version != RUNTIME_WORLD_CURRENT {
        return Err(RuntimeError::message(
            "ASTRA_RUNTIME_SAVE_WORLD_VERSION_UNSUPPORTED",
        ));
    }
    Ok(())
}

pub fn read_runtime_save_section<T: DeserializeOwned>(
    blob: &SaveBlob,
    section_id: &str,
    registry: &SchemaMigrationRegistry,
) -> Result<T, RuntimeError> {
    let reader = runtime_save_reader(blob, registry)?;
    reader
        .decode_postcard(section_id)
        .map_err(|err| RuntimeError::message(format!("decode {section_id}: {err}")))
}

pub fn runtime_save_section_ids(
    blob: &SaveBlob,
    registry: &SchemaMigrationRegistry,
) -> Result<Vec<String>, RuntimeError> {
    let reader = runtime_save_reader(blob, registry)?;
    Ok(reader
        .entries()
        .iter()
        .map(|entry| entry.id.clone())
        .collect())
}

fn runtime_save_reader(
    blob: &SaveBlob,
    registry: &SchemaMigrationRegistry,
) -> Result<AstraContainerReader, RuntimeError> {
    let container = ContainerBlob::new(blob.0.clone());
    let reader = AstraContainerReader::new(container.as_bytes())
        .map_err(|err| RuntimeError::message(err.to_string()))?;
    if reader.kind() != ContainerKind::Save {
        return Err(RuntimeError::message("container is not a runtime save"));
    }
    validate_runtime_world_contract(&reader)?;
    let manifest: MigrationManifest = reader
        .decode_postcard("migration.manifest")
        .map_err(|err| RuntimeError::message(err.to_string()))?;
    for entry in manifest.sections {
        registry
            .validate_chain(
                &entry.schema,
                entry.minimum_supported_version,
                entry.current_version,
            )
            .map_err(|err| RuntimeError::message(err.to_string()))?;
    }
    Ok(reader)
}
