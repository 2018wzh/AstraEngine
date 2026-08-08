use std::{
    any::Any,
    collections::BTreeMap,
    fmt,
    sync::{Arc, OnceLock},
};

use astra_core::{Hash128, SchemaId, SchemaVersion, StableId};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

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

trait RuntimeComponentValue: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn RuntimeComponentValue>;
    fn encode(&self) -> Result<Arc<[u8]>, RuntimeError>;
}

impl<T> RuntimeComponentValue for T
where
    T: Serialize + Clone + fmt::Debug + Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn RuntimeComponentValue> {
        Box::new(self.clone())
    }

    fn encode(&self) -> Result<Arc<[u8]>, RuntimeError> {
        postcard::to_allocvec(self)
            .map(Into::into)
            .map_err(|err| RuntimeError::message(format!("encode runtime component: {err}")))
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct RuntimeComponentPayloadWire {
    schema: SchemaId,
    version: SchemaVersion,
    revision: u64,
    bytes: Arc<[u8]>,
}

pub struct RuntimeComponentPayload {
    pub(crate) schema: SchemaId,
    pub(crate) version: SchemaVersion,
    pub(crate) revision: u64,
    typed: OnceLock<Box<dyn RuntimeComponentValue>>,
    bytes: OnceLock<Arc<[u8]>>,
}

impl fmt::Debug for RuntimeComponentPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeComponentPayload")
            .field("schema", &self.schema)
            .field("version", &self.version)
            .field("revision", &self.revision)
            .field("typed", &self.typed.get().is_some())
            .field("encoded", &self.bytes.get().is_some())
            .finish()
    }
}

impl Clone for RuntimeComponentPayload {
    fn clone(&self) -> Self {
        let typed = OnceLock::new();
        if let Some(value) = self.typed.get() {
            typed
                .set(value.clone_box())
                .expect("new component typed cell must be empty");
        }
        let bytes = OnceLock::new();
        if let Some(value) = self.bytes.get() {
            bytes
                .set(Arc::clone(value))
                .expect("new component byte cell must be empty");
        }
        Self {
            schema: self.schema.clone(),
            version: self.version,
            revision: self.revision,
            typed,
            bytes,
        }
    }
}

impl PartialEq for RuntimeComponentPayload {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema
            && self.version == other.version
            && self.revision == other.revision
            && self.postcard_bytes().ok() == other.postcard_bytes().ok()
    }
}

impl Eq for RuntimeComponentPayload {}

impl Serialize for RuntimeComponentPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RuntimeComponentPayloadWire {
            schema: self.schema.clone(),
            version: self.version,
            revision: self.revision,
            bytes: self.postcard_bytes().map_err(serde::ser::Error::custom)?,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RuntimeComponentPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RuntimeComponentPayloadWire::deserialize(deserializer)?;
        let bytes = OnceLock::new();
        bytes
            .set(wire.bytes)
            .expect("new component byte cell must be empty");
        Ok(Self {
            schema: wire.schema,
            version: wire.version,
            revision: wire.revision,
            typed: OnceLock::new(),
            bytes,
        })
    }
}

impl JsonSchema for RuntimeComponentPayload {
    fn schema_name() -> String {
        RuntimeComponentPayloadWire::schema_name()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::schema::Schema {
        RuntimeComponentPayloadWire::json_schema(generator)
    }
}

impl RuntimeComponentPayload {
    pub fn typed<T>(schema: impl Into<SchemaId>, version: SchemaVersion, value: T) -> Self
    where
        T: Serialize + Clone + fmt::Debug + Send + Sync + 'static,
    {
        let typed: OnceLock<Box<dyn RuntimeComponentValue>> = OnceLock::new();
        typed
            .set(Box::new(value))
            .expect("new component typed cell must be empty");
        Self {
            schema: schema.into(),
            version,
            revision: 1,
            typed,
            bytes: OnceLock::new(),
        }
    }

    pub fn decode<T>(&self) -> Result<T, RuntimeError>
    where
        T: Serialize + DeserializeOwned + Clone + fmt::Debug + Send + Sync + 'static,
    {
        if let Some(value) = self.typed.get() {
            return value.as_any().downcast_ref::<T>().cloned().ok_or_else(|| {
                RuntimeError::message("ASTRA_RUNTIME_COMPONENT_TYPE: typed component type mismatch")
            });
        }
        let bytes = self.bytes.get().ok_or_else(|| {
            RuntimeError::message("ASTRA_RUNTIME_COMPONENT_STORAGE: component has no value")
        })?;
        let value: T = postcard::from_bytes(bytes)
            .map_err(|err| RuntimeError::message(format!("decode runtime component: {err}")))?;
        let _ = self.typed.set(Box::new(value.clone()));
        Ok(value)
    }

    pub(crate) fn postcard_bytes(&self) -> Result<Arc<[u8]>, RuntimeError> {
        if let Some(bytes) = self.bytes.get() {
            return Ok(Arc::clone(bytes));
        }
        let bytes = self
            .typed
            .get()
            .ok_or_else(|| {
                RuntimeError::message("ASTRA_RUNTIME_COMPONENT_STORAGE: component has no value")
            })?
            .encode()?;
        let _ = self.bytes.set(Arc::clone(&bytes));
        Ok(bytes)
    }

    pub fn schema(&self) -> &SchemaId {
        &self.schema
    }

    pub fn version(&self) -> SchemaVersion {
        self.version
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ActorStore {
    actors: IndexMap<ActorId, ActorRecord>,
    components: IndexMap<ComponentId, ComponentRecord>,
    #[serde(skip)]
    transaction: Option<ActorStoreTransaction>,
}

impl Clone for ActorStore {
    fn clone(&self) -> Self {
        Self {
            actors: self.actors.clone(),
            components: self.components.clone(),
            transaction: self.transaction.clone(),
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
        let components = self.components.values().map(|component| {
            (
                component.component_id,
                component.actor_id,
                &component.payload.schema,
                component.payload.version,
                component.payload.revision,
            )
        });
        Hash128::from_blake3(
            &postcard::to_allocvec(&(&self.actors, components.collect::<Vec<_>>()))
                .expect("actor store metadata must serialize for deterministic fingerprinting"),
        )
    }

    pub fn insert_actor(&mut self, actor: ActorRecord) {
        self.record_actor_before(actor.actor_id);
        self.actors.insert(actor.actor_id, actor);
    }

    pub fn attach_component(&mut self, component: ComponentRecord) -> bool {
        self.record_actor_before(component.actor_id);
        self.record_component_before(component.component_id);
        let Some(actor) = self.actors.get_mut(&component.actor_id) else {
            return false;
        };
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
    fn actor_has_tag(&self, actor_id: ActorId, tag: &str) -> bool;
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

    fn actor_has_tag(&self, actor_id: ActorId, tag: &str) -> bool {
        self.actor(actor_id)
            .is_some_and(|actor| actor.tags.iter().any(|candidate| candidate == tag))
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

    fn actor_has_tag(&self, actor_id: ActorId, tag: &str) -> bool {
        self.actor(actor_id)
            .is_some_and(|actor| actor.tags.iter().any(|candidate| candidate == tag))
    }
}
