use astra_core::{SchemaMigrationRegistry, SchemaVersion};
use astra_package::{
    AstraContainerBuilder, AstraContainerReader, ContainerBlob, ContainerKind, MigrationPolicy,
    SectionCodec, SectionPayload, CURRENT_CONTAINER_VERSION,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{RuntimeError, RuntimeSnapshot};

const CURRENT: SchemaVersion = CURRENT_CONTAINER_VERSION;

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

    let blob = AstraContainerBuilder::new(ContainerKind::Save)
        .add_section(SectionPayload::new(
            "runtime.world",
            "runtime.world",
            CURRENT,
            SectionCodec::Postcard,
            runtime_payload,
            MigrationPolicy::from_minimum(request.minimum_supported_version),
        ))
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
