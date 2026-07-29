use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use astra_core::{Hash128, Hash256, SchemaId, SchemaVersion, StableId};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

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
    /// Postcard bytes authenticated with BLAKE3-256. This is intended for
    /// high-frequency authoritative components whose deterministic state hash
    /// is the first 128 bits of the same digest.
    PostcardBlake3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RuntimeComponentPayload {
    pub(crate) schema: SchemaId,
    pub(crate) version: SchemaVersion,
    pub(crate) codec: RuntimePayloadCodec,
    pub(crate) hash: Hash256,
    pub(crate) bytes: Arc<[u8]>,
}

/// An encoded postcard component whose storage and deterministic hashes were
/// computed together from the owned byte sequence.
///
/// Keeping the bytes and hashes in one opaque value prevents callers from
/// pairing a digest with different bytes while allowing hot reducers to avoid
/// scanning a growing component once per hash algorithm.
#[derive(Debug, Clone)]
pub struct ValidatedRuntimeComponentEncoding {
    bytes: Arc<[u8]>,
    storage_hash: Hash256,
    state_hash: Hash128,
    codec: RuntimePayloadCodec,
}

impl ValidatedRuntimeComponentEncoding {
    pub fn postcard(bytes: Arc<[u8]>) -> Self {
        let mut storage = Sha256::new();
        let mut state = blake3::Hasher::new();
        for chunk in bytes.chunks(64 * 1024) {
            storage.update(chunk);
            state.update(chunk);
        }
        let storage_hash = Hash256::from_bytes(storage.finalize().into());
        let mut state_bytes = [0_u8; 16];
        state_bytes.copy_from_slice(&state.finalize().as_bytes()[..16]);
        Self {
            bytes,
            storage_hash,
            state_hash: Hash128::from_bytes(state_bytes),
            codec: RuntimePayloadCodec::Postcard,
        }
    }

    pub fn postcard_blake3(bytes: Arc<[u8]>) -> Self {
        let digest = blake3::hash(&bytes);
        let mut state_bytes = [0_u8; 16];
        state_bytes.copy_from_slice(&digest.as_bytes()[..16]);
        Self {
            bytes,
            storage_hash: Hash256::from_bytes(*digest.as_bytes()),
            state_hash: Hash128::from_bytes(state_bytes),
            codec: RuntimePayloadCodec::PostcardBlake3,
        }
    }

    pub fn storage_hash(&self) -> Hash256 {
        self.storage_hash
    }

    pub fn state_hash(&self) -> Hash128 {
        self.state_hash
    }
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
        let actual_hash = match wire.codec {
            RuntimePayloadCodec::Postcard => Hash256::from_sha256(&wire.bytes),
            RuntimePayloadCodec::PostcardBlake3 => {
                Hash256::from_bytes(*blake3::hash(&wire.bytes).as_bytes())
            }
        };
        if actual_hash != wire.hash {
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

    pub(crate) fn validated_encoded_postcard(
        schema: impl Into<SchemaId>,
        version: SchemaVersion,
        encoding: ValidatedRuntimeComponentEncoding,
    ) -> Self {
        Self {
            schema: schema.into(),
            version,
            codec: encoding.codec,
            hash: encoding.storage_hash,
            bytes: encoding.bytes,
        }
    }

    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, RuntimeError> {
        let bytes = self.validated_postcard_bytes()?;
        postcard::from_bytes(&bytes)
            .map_err(|err| RuntimeError::message(format!("decode runtime component: {err}")))
    }

    pub fn validated_postcard_bytes(&self) -> Result<Arc<[u8]>, RuntimeError> {
        match self.codec {
            RuntimePayloadCodec::Postcard | RuntimePayloadCodec::PostcardBlake3 => {
                Ok(Arc::clone(&self.bytes))
            }
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ActorStore {
    actors: IndexMap<ActorId, ActorRecord>,
    components: IndexMap<ComponentId, ComponentRecord>,
    #[serde(skip)]
    transaction: Option<ActorStoreTransaction>,
    #[serde(skip)]
    fingerprint: Mutex<Option<Hash128>>,
}

impl Clone for ActorStore {
    fn clone(&self) -> Self {
        Self {
            actors: self.actors.clone(),
            components: self.components.clone(),
            transaction: self.transaction.clone(),
            fingerprint: Mutex::new(
                *self
                    .fingerprint
                    .lock()
                    .expect("actor fingerprint cache lock must not be poisoned"),
            ),
        }
    }
}

impl PartialEq for ActorStore {
    fn eq(&self, other: &Self) -> bool {
        self.actors == other.actors && self.components == other.components
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct ActorStoreTransaction {
    actors: BTreeMap<ActorId, IndexedActorUndo>,
    components: BTreeMap<ComponentId, IndexedComponentUndo>,
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedActorUndo {
    index: Option<usize>,
    value: Option<ActorRecord>,
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedComponentUndo {
    index: Option<usize>,
    value: Option<ComponentRecord>,
}

fn restore_indexed_actor(
    actors: &mut IndexMap<ActorId, ActorRecord>,
    actor_id: ActorId,
    undo: IndexedActorUndo,
) {
    actors.shift_remove(&actor_id);
    if let Some(value) = undo.value {
        actors.shift_insert(
            undo.index.unwrap_or(actors.len()).min(actors.len()),
            actor_id,
            value,
        );
    }
}

fn restore_indexed_component(
    components: &mut IndexMap<ComponentId, ComponentRecord>,
    component_id: ComponentId,
    undo: IndexedComponentUndo,
) {
    components.shift_remove(&component_id);
    if let Some(value) = undo.value {
        components.shift_insert(
            undo.index.unwrap_or(components.len()).min(components.len()),
            component_id,
            value,
        );
    }
}

impl ActorStore {
    pub(crate) fn begin_transaction(&mut self) -> Result<(), RuntimeError> {
        if self.transaction.is_some() {
            return Err(RuntimeError::message(
                "ASTRA_RUNTIME_ACTOR_TRANSACTION_NESTED: actor transaction is already active",
            ));
        }
        self.transaction = Some(ActorStoreTransaction::default());
        Ok(())
    }

    pub(crate) fn commit_transaction(&mut self) {
        self.transaction = None;
    }

    pub(crate) fn rollback_transaction(&mut self) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        for (component_id, undo) in transaction.components.into_iter().rev() {
            restore_indexed_component(&mut self.components, component_id, undo);
        }
        for (actor_id, undo) in transaction.actors.into_iter().rev() {
            restore_indexed_actor(&mut self.actors, actor_id, undo);
        }
        *self
            .fingerprint
            .get_mut()
            .expect("actor fingerprint cache lock must not be poisoned") = None;
    }

    fn record_actor_before(&mut self, actor_id: ActorId) {
        let Some(transaction) = self.transaction.as_ref() else {
            return;
        };
        if transaction.actors.contains_key(&actor_id) {
            return;
        }
        let undo = IndexedActorUndo {
            index: self.actors.get_index_of(&actor_id),
            value: self.actors.get(&actor_id).cloned(),
        };
        self.transaction
            .as_mut()
            .expect("transaction presence was checked")
            .actors
            .insert(actor_id, undo);
    }

    fn record_component_before(&mut self, component_id: ComponentId) {
        let Some(transaction) = self.transaction.as_ref() else {
            return;
        };
        if transaction.components.contains_key(&component_id) {
            return;
        }
        let undo = IndexedComponentUndo {
            index: self.components.get_index_of(&component_id),
            value: self.components.get(&component_id).cloned(),
        };
        self.transaction
            .as_mut()
            .expect("transaction presence was checked")
            .components
            .insert(component_id, undo);
    }

    pub(crate) fn deterministic_fingerprint(&self) -> Hash128 {
        if let Some(fingerprint) = *self
            .fingerprint
            .lock()
            .expect("actor fingerprint cache lock must not be poisoned")
        {
            return fingerprint;
        }
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
        let fingerprint = Hash128::from_blake3(
            &postcard::to_allocvec(&(&self.actors, components.collect::<Vec<_>>()))
                .expect("actor store metadata must serialize for deterministic fingerprinting"),
        );
        *self
            .fingerprint
            .lock()
            .expect("actor fingerprint cache lock must not be poisoned") = Some(fingerprint);
        fingerprint
    }

    pub fn insert_actor(&mut self, actor: ActorRecord) {
        *self
            .fingerprint
            .get_mut()
            .expect("actor fingerprint cache lock must not be poisoned") = None;
        self.record_actor_before(actor.actor_id);
        self.actors.insert(actor.actor_id, actor);
    }

    pub fn attach_component(&mut self, component: ComponentRecord) -> bool {
        self.record_actor_before(component.actor_id);
        self.record_component_before(component.component_id);
        let Some(actor) = self.actors.get_mut(&component.actor_id) else {
            return false;
        };
        *self
            .fingerprint
            .get_mut()
            .expect("actor fingerprint cache lock must not be poisoned") = None;
        actor.components.push(component.component_id);
        self.components.insert(component.component_id, component);
        true
    }

    pub fn remove_actor(&mut self, actor_id: ActorId) -> Option<ActorRecord> {
        self.record_actor_before(actor_id);
        let component_ids = self.actors.get(&actor_id)?.components.clone();
        for component_id in &component_ids {
            self.record_component_before(*component_id);
        }
        let actor = self.actors.shift_remove(&actor_id)?;
        *self
            .fingerprint
            .get_mut()
            .expect("actor fingerprint cache lock must not be poisoned") = None;
        for component_id in &actor.components {
            self.components.shift_remove(component_id);
        }
        Some(actor)
    }

    pub fn detach_component(&mut self, component_id: ComponentId) -> Option<ComponentRecord> {
        let actor_id = self.components.get(&component_id)?.actor_id;
        self.record_actor_before(actor_id);
        self.record_component_before(component_id);
        let component = self.components.shift_remove(&component_id)?;
        *self
            .fingerprint
            .get_mut()
            .expect("actor fingerprint cache lock must not be poisoned") = None;
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
        self.record_component_before(component_id);
        *self
            .fingerprint
            .get_mut()
            .expect("actor fingerprint cache lock must not be poisoned") = None;
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

pub(crate) trait ActorStoreAccess {
    fn insert_actor(&mut self, actor: ActorRecord);
    fn attach_component(&mut self, component: ComponentRecord) -> bool;
    fn remove_actor(&mut self, actor_id: ActorId) -> Option<ActorRecord>;
    fn detach_component(&mut self, component_id: ComponentId) -> Option<ComponentRecord>;
    fn component(&self, component_id: ComponentId) -> Option<&ComponentRecord>;
    fn component_mut(&mut self, component_id: ComponentId) -> Option<&mut ComponentRecord>;
    fn component_ids_for_actor_schema(
        &self,
        actor_id: ActorId,
        schema: &SchemaId,
    ) -> Vec<ComponentId>;
    fn actor_has_tag(&self, actor_id: ActorId, tag: &str) -> bool;
    fn deterministic_fingerprint(&self) -> Hash128;
}

impl ActorStoreAccess for ActorStore {
    fn insert_actor(&mut self, actor: ActorRecord) {
        ActorStore::insert_actor(self, actor);
    }

    fn attach_component(&mut self, component: ComponentRecord) -> bool {
        ActorStore::attach_component(self, component)
    }

    fn remove_actor(&mut self, actor_id: ActorId) -> Option<ActorRecord> {
        ActorStore::remove_actor(self, actor_id)
    }

    fn detach_component(&mut self, component_id: ComponentId) -> Option<ComponentRecord> {
        ActorStore::detach_component(self, component_id)
    }

    fn component(&self, component_id: ComponentId) -> Option<&ComponentRecord> {
        ActorStore::component(self, component_id)
    }

    fn component_mut(&mut self, component_id: ComponentId) -> Option<&mut ComponentRecord> {
        ActorStore::component_mut(self, component_id)
    }

    fn component_ids_for_actor_schema(
        &self,
        actor_id: ActorId,
        schema: &SchemaId,
    ) -> Vec<ComponentId> {
        ActorStore::component_ids_for_actor_schema(self, actor_id, schema)
    }

    fn actor_has_tag(&self, actor_id: ActorId, tag: &str) -> bool {
        self.actor(actor_id)
            .is_some_and(|actor| actor.tags.iter().any(|candidate| candidate == tag))
    }

    fn deterministic_fingerprint(&self) -> Hash128 {
        ActorStore::deterministic_fingerprint(self)
    }
}

pub(crate) struct ActorStoreOverlay<'a> {
    base: &'a ActorStore,
    actors: BTreeMap<ActorId, Option<ActorRecord>>,
    components: BTreeMap<ComponentId, Option<ComponentRecord>>,
}

pub(crate) struct ActorStoreDelta {
    actors: BTreeMap<ActorId, Option<ActorRecord>>,
    components: BTreeMap<ComponentId, Option<ComponentRecord>>,
}

impl<'a> ActorStoreOverlay<'a> {
    pub(crate) fn new(base: &'a ActorStore) -> Self {
        Self {
            base,
            actors: BTreeMap::new(),
            components: BTreeMap::new(),
        }
    }

    pub(crate) fn into_delta(self) -> ActorStoreDelta {
        ActorStoreDelta {
            actors: self.actors,
            components: self.components,
        }
    }

    fn actor(&self, actor_id: ActorId) -> Option<&ActorRecord> {
        match self.actors.get(&actor_id) {
            Some(actor) => actor.as_ref(),
            None => self.base.actor(actor_id),
        }
    }

    fn actor_mut(&mut self, actor_id: ActorId) -> Option<&mut ActorRecord> {
        if !self.actors.contains_key(&actor_id) {
            self.actors
                .insert(actor_id, self.base.actor(actor_id).cloned());
        }
        self.actors.get_mut(&actor_id)?.as_mut()
    }
}

impl ActorStoreDelta {
    pub(crate) fn commit(self, target: &mut ActorStore) {
        if !self.actors.is_empty() || !self.components.is_empty() {
            *target
                .fingerprint
                .get_mut()
                .expect("actor fingerprint cache lock must not be poisoned") = None;
        }
        for (actor_id, actor) in self.actors {
            target.record_actor_before(actor_id);
            match actor {
                Some(actor) => {
                    target.actors.insert(actor_id, actor);
                }
                None => {
                    target.actors.shift_remove(&actor_id);
                }
            }
        }
        for (component_id, component) in self.components {
            target.record_component_before(component_id);
            match component {
                Some(component) => {
                    target.components.insert(component_id, component);
                }
                None => {
                    target.components.shift_remove(&component_id);
                }
            }
        }
    }
}

impl ActorStoreAccess for ActorStoreOverlay<'_> {
    fn insert_actor(&mut self, actor: ActorRecord) {
        self.actors.insert(actor.actor_id, Some(actor));
    }

    fn attach_component(&mut self, component: ComponentRecord) -> bool {
        let Some(actor) = self.actor_mut(component.actor_id) else {
            return false;
        };
        actor.components.push(component.component_id);
        self.components
            .insert(component.component_id, Some(component));
        true
    }

    fn remove_actor(&mut self, actor_id: ActorId) -> Option<ActorRecord> {
        let actor = self.actor(actor_id)?.clone();
        self.actors.insert(actor_id, None);
        for component_id in &actor.components {
            self.components.insert(*component_id, None);
        }
        Some(actor)
    }

    fn detach_component(&mut self, component_id: ComponentId) -> Option<ComponentRecord> {
        let component = self.component(component_id)?.clone();
        self.components.insert(component_id, None);
        if let Some(actor) = self.actor_mut(component.actor_id) {
            actor.components.retain(|id| *id != component_id);
        }
        Some(component)
    }

    fn component(&self, component_id: ComponentId) -> Option<&ComponentRecord> {
        match self.components.get(&component_id) {
            Some(component) => component.as_ref(),
            None => self.base.component(component_id),
        }
    }

    fn component_mut(&mut self, component_id: ComponentId) -> Option<&mut ComponentRecord> {
        if !self.components.contains_key(&component_id) {
            self.components
                .insert(component_id, self.base.component(component_id).cloned());
        }
        self.components.get_mut(&component_id)?.as_mut()
    }

    fn component_ids_for_actor_schema(
        &self,
        actor_id: ActorId,
        schema: &SchemaId,
    ) -> Vec<ComponentId> {
        let mut ids = self
            .base
            .components
            .values()
            .filter(|component| {
                !self.components.contains_key(&component.component_id)
                    && component.actor_id == actor_id
                    && &component.payload.schema == schema
            })
            .map(|component| component.component_id)
            .collect::<Vec<_>>();
        ids.extend(self.components.values().filter_map(|component| {
            component.as_ref().and_then(|component| {
                (component.actor_id == actor_id && &component.payload.schema == schema)
                    .then_some(component.component_id)
            })
        }));
        ids.sort();
        ids
    }

    fn actor_has_tag(&self, actor_id: ActorId, tag: &str) -> bool {
        self.actor(actor_id)
            .is_some_and(|actor| actor.tags.iter().any(|candidate| candidate == tag))
    }

    fn deterministic_fingerprint(&self) -> Hash128 {
        let component_delta = self.components.iter().map(|(component_id, component)| {
            (
                component_id,
                component.as_ref().map(|component| {
                    (
                        component.actor_id,
                        &component.payload.schema,
                        component.payload.version,
                        component.payload.codec,
                        component.payload.hash,
                    )
                }),
            )
        });
        Hash128::from_blake3(
            &postcard::to_allocvec(&(
                "astra.runtime.actor_overlay_fingerprint.v1",
                ActorStoreAccess::deterministic_fingerprint(self.base),
                &self.actors,
                component_delta.collect::<Vec<_>>(),
            ))
            .expect("actor overlay metadata must serialize for deterministic fingerprinting"),
        )
    }
}
