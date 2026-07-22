use std::sync::Arc;

use astra_core::{Hash128, Hash256, SchemaId, SchemaVersion, StableId};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

use crate::RuntimeError;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ActorId(pub StableId);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ComponentId(pub StableId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActorSnapshot {
    pub actor_id: ActorId,
    pub name: String,
    pub tags: Vec<String>,
    pub components: Vec<ComponentId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ComponentSnapshot {
    pub component_id: ComponentId,
    pub actor_id: ActorId,
    pub payload: RuntimeComponentPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActorRecord {
    pub actor_id: ActorId,
    pub name: String,
    pub tags: Vec<String>,
    pub components: Vec<ComponentId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ComponentRecord {
    pub component_id: ComponentId,
    pub actor_id: ActorId,
    pub payload: RuntimeComponentPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePayloadCodec {
    Postcard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RuntimeComponentPayload {
    pub(crate) schema: SchemaId,
    pub(crate) version: SchemaVersion,
    pub(crate) codec: RuntimePayloadCodec,
    pub(crate) hash: Hash256,
    pub(crate) bytes: Arc<[u8]>,
}

#[derive(Deserialize)]
struct RuntimeComponentPayloadWire {
    schema: SchemaId,
    version: SchemaVersion,
    codec: RuntimePayloadCodec,
    hash: Hash256,
    bytes: Arc<[u8]>,
}

impl<'de> Deserialize<'de> for RuntimeComponentPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RuntimeComponentPayloadWire::deserialize(deserializer)?;
        if Hash256::from_sha256(&wire.bytes) != wire.hash {
            return Err(D::Error::custom(
                "ASTRA_RUNTIME_COMPONENT_HASH: runtime component payload hash does not match its bytes",
            ));
        }
        Ok(Self {
            schema: wire.schema,
            version: wire.version,
            codec: wire.codec,
            hash: wire.hash,
            bytes: wire.bytes,
        })
    }
}

impl RuntimeComponentPayload {
    pub fn postcard<T: Serialize>(
        schema: impl Into<SchemaId>,
        version: SchemaVersion,
        value: &T,
    ) -> Result<Self, RuntimeError> {
        let bytes = postcard::to_allocvec(value)
            .map_err(|err| RuntimeError::message(format!("encode runtime component: {err}")))?;
        Ok(Self {
            schema: schema.into(),
            version,
            codec: RuntimePayloadCodec::Postcard,
            hash: Hash256::from_sha256(&bytes),
            bytes: bytes.into(),
        })
    }

    pub(crate) fn encoded_postcard(
        schema: impl Into<SchemaId>,
        version: SchemaVersion,
        bytes: Arc<[u8]>,
    ) -> Self {
        Self {
            schema: schema.into(),
            version,
            codec: RuntimePayloadCodec::Postcard,
            hash: Hash256::from_sha256(&bytes),
            bytes,
        }
    }

    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, RuntimeError> {
        let bytes = self.validated_postcard_bytes()?;
        postcard::from_bytes(&bytes)
            .map_err(|err| RuntimeError::message(format!("decode runtime component: {err}")))
    }

    pub fn validated_postcard_bytes(&self) -> Result<Arc<[u8]>, RuntimeError> {
        match self.codec {
            RuntimePayloadCodec::Postcard => Ok(Arc::clone(&self.bytes)),
        }
    }

    pub fn schema(&self) -> &SchemaId {
        &self.schema
    }

    pub fn version(&self) -> SchemaVersion {
        self.version
    }

    pub fn codec(&self) -> RuntimePayloadCodec {
        self.codec
    }

    pub fn hash(&self) -> Hash256 {
        self.hash
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActorStore {
    actors: IndexMap<ActorId, ActorRecord>,
    components: IndexMap<ComponentId, ComponentRecord>,
}

impl ActorStore {
    pub(crate) fn deterministic_fingerprint(&self) -> Hash128 {
        let components = self.components.values().map(|component| {
            (
                component.component_id,
                component.actor_id,
                &component.payload.schema,
                component.payload.version,
                component.payload.codec,
                component.payload.hash,
            )
        });
        Hash128::from_blake3(
            &postcard::to_allocvec(&(&self.actors, components.collect::<Vec<_>>()))
                .expect("actor store metadata must serialize for deterministic fingerprinting"),
        )
    }

    pub fn insert_actor(&mut self, actor: ActorRecord) {
        self.actors.insert(actor.actor_id, actor);
    }

    pub fn attach_component(&mut self, component: ComponentRecord) -> bool {
        let Some(actor) = self.actors.get_mut(&component.actor_id) else {
            return false;
        };
        actor.components.push(component.component_id);
        self.components.insert(component.component_id, component);
        true
    }

    pub fn remove_actor(&mut self, actor_id: ActorId) -> Option<ActorRecord> {
        let actor = self.actors.shift_remove(&actor_id)?;
        for component_id in &actor.components {
            self.components.shift_remove(component_id);
        }
        Some(actor)
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> Option<ComponentRecord> {
        let component = self.components.shift_remove(&component_id)?;
        if let Some(actor) = self.actors.get_mut(&component.actor_id) {
            actor.components.retain(|id| *id != component_id);
        }
        Some(component)
    }

    pub fn actor(&self, actor_id: ActorId) -> Option<&ActorRecord> {
        self.actors.get(&actor_id)
    }

    pub fn component(&self, component_id: ComponentId) -> Option<&ComponentRecord> {
        self.components.get(&component_id)
    }

    pub fn component_mut(&mut self, component_id: ComponentId) -> Option<&mut ComponentRecord> {
        self.components.get_mut(&component_id)
    }

    pub fn component_ids_for_actor_schema(
        &self,
        actor_id: ActorId,
        schema: &SchemaId,
    ) -> Vec<ComponentId> {
        self.components
            .values()
            .filter(|component| {
                component.actor_id == actor_id && &component.payload.schema == schema
            })
            .map(|component| component.component_id)
            .collect()
    }

    pub fn actor_snapshots(&self) -> Vec<ActorSnapshot> {
        let mut actors: Vec<_> = self
            .actors
            .values()
            .map(|actor| ActorSnapshot {
                actor_id: actor.actor_id,
                name: actor.name.clone(),
                tags: actor.tags.clone(),
                components: actor.components.clone(),
            })
            .collect();
        actors.sort_by_key(|actor| actor.actor_id);
        actors
    }

    pub fn component_snapshots(&self, actor_id: ActorId) -> Vec<ComponentSnapshot> {
        let mut components: Vec<_> = self
            .components
            .values()
            .filter(|component| component.actor_id == actor_id)
            .map(|component| ComponentSnapshot {
                component_id: component.component_id,
                actor_id: component.actor_id,
                payload: component.payload.clone(),
            })
            .collect();
        components.sort_by_key(|component| component.component_id);
        components
    }
}
