use astra_core::{SchemaId, SchemaVersion, StableId};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::BlackboardValue;

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
    pub schema: SchemaId,
    pub version: SchemaVersion,
    pub data: BlackboardValue,
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
    pub schema: SchemaId,
    pub version: SchemaVersion,
    pub data: BlackboardValue,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActorStore {
    actors: IndexMap<ActorId, ActorRecord>,
    components: IndexMap<ComponentId, ComponentRecord>,
}

impl ActorStore {
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
                schema: component.schema.clone(),
                version: component.version,
                data: component.data.clone(),
            })
            .collect();
        components.sort_by_key(|component| component.component_id);
        components
    }
}
